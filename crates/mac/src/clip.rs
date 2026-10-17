use crate::*;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

/// 最後に Windows と同期したクリップボードの changeCount
pub(crate) static LAST_SYNC_COUNT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);

/// 同一 changeCount の画像 DIB 変換を 1 回に集約する単一スロットのキャッシュ。
/// mac_clipboard_image_dib() は NSBitmapImageRep の BMP エンコード(4K で 33MB の
/// 直列化)を含み、同じ 1 回のコピーに対して履歴経路(record_local_copy)と
/// 越境経路(sync_clipboard_to_win)がそれぞれ呼ぶため 2 回走っていた。
/// changeCount が一致する間だけ変換結果(Arc)を共有して 1 回にする。
/// 鍵は changeCount のみ。Knit 自身がクリップボードへ書く経路(Windows からの
/// 受信・履歴からの復元)は必ず changeCount を進めるため古い実体は返らない
/// (changeCount が単調増加である前提は claim_history / LAST_SYNC_COUNT と同じ)。
pub(crate) struct DibCache {
    slot: Mutex<Option<(isize, std::sync::Arc<Vec<u8>>)>>,
}

impl DibCache {
    pub(crate) const fn new() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }

    /// cnt が前回と同じなら変換せず前回の実体を共有して返す。違えば convert を
    /// 呼んで差し替える。変換失敗(None)はキャッシュしない(次回に再試行できる)。
    /// 変換はロック内で行う: 履歴スレッドと越境スレッドが同じ cnt を同時に処理しても
    /// 変換が重複しない(convert はクリップボード読み出しだけで再入しない)。
    pub(crate) fn get_or_convert(
        &self,
        cnt: isize,
        convert: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<std::sync::Arc<Vec<u8>>> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached, dib)) = slot.as_ref() {
            if *cached == cnt {
                return Some(std::sync::Arc::clone(dib));
            }
        }
        let dib = std::sync::Arc::new(convert()?);
        *slot = Some((cnt, std::sync::Arc::clone(&dib)));
        Some(dib)
    }
}

/// 製品コードが使う単一スロット。保持は「最後に変換した 1 枚」だけで、次の
/// クリップボード変化(changeCount の更新)で必ず差し替わる。サイズ上限は設けて
/// いないため、8K 等の大きい画像では変換結果(例: 7680x4320x4 ≒ 132MB)が次の
/// コピーまで残る。上限で切ると同じ画像を履歴経路と越境経路が 2 回変換して
/// 遅くなるため、ここでは速度を優先している(保持量を抑えたくなったら、この
/// スロットにサイズ上限を足すか、アイドル時に None へ戻す)。
static DIB_CACHE: DibCache = DibCache::new();

/// 同一 changeCount の間は変換結果を共有する(mac_clipboard_image_dib の薄い包み)
fn clipboard_image_dib_cached(cnt: isize) -> Option<std::sync::Arc<Vec<u8>>> {
    DIB_CACHE.get_or_convert(cnt, || unsafe { mac_clipboard_image_dib() })
}

// ---------- クリップボード履歴(Universal Clipboard History) ----------
/// 送信・受信したテキストの履歴。メニューバーから選んで Mac のクリップボードへ
/// 復元できる。機密判定(smartguard)と秘匿指定は送信側で弾くため、履歴へは届かない
pub static HISTORY: Mutex<knit_common::history::History> =
    Mutex::new(knit_common::history::History::new(50));
/// GUI の履歴メニュー再構築用の世代(push で更新、gui.rs が変化検知に使う)
pub static HISTORY_LAST_ID: AtomicU64 = AtomicU64::new(0);
/// 履歴の保存先(env と同じ ~/.config/knit/)。アプリバンドルには書かない
pub fn history_path() -> Option<std::path::PathBuf> {
    knit_common::envutil::config_dir().map(|dir| dir.join("history.json"))
}

pub fn history_save() {
    let Some(path) = history_path() else { return };
    // push スレッド同士の同時保存で tmp の書き途中を rename しない(壊れた JSON の防止)
    static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // ループ9: 直列化だけ HISTORY のロック内で行い、ファイル書き込み(tmp へ書いて
    // rename、最大 50×8KB 前後)はロックを離した後に実行する。ロック保持中に
    // ディスク I/O をしないため、クリップボード操作(push)の待ち時間が短くなる。
    // 保存形式・権限・アトミック性は従来(save_to)と同一。
    // ロックが毒されている場合は従来どおり保存しない。
    // 監査3: ロックが毒されていても保存を続ける(以前は無言でスキップし、
    // 以後その実行の間ずっと履歴が保存されなくなっていた)
    let bytes = match HISTORY.lock() {
        Ok(h) => h.to_bytes(),
        Err(poisoned) => {
            eprintln!("[clip] 履歴のロックが壊れていますが保存を続けます");
            poisoned.into_inner().to_bytes()
        }
    };
    if let Some(bytes) = bytes {
        knit_common::history::save_bytes_to(&path, &bytes);
    }
}

pub fn history_load() {
    let Some(path) = history_path() else { return };
    if let Ok(mut h) = HISTORY.lock() {
        h.load_from(&path);
        HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
    }
}

/// 保存件数の上限を変更する(設定画面)。超過分は古い方から除去して保存する。
pub fn history_set_limit(limit: usize) {
    if let Ok(mut h) = HISTORY.lock() {
        h.set_limit(limit);
    }
    history_save();
}

/// 履歴の全消去(メニューバー)。保存ファイルも消して次回起動に残さない。
/// 画像の実体(images/)も一緒に消す: 実体は履歴から参照されなくなっても
/// IMAGE_KEEP(60 件)/512MiB まで残り続けるため、全消去では残さない。
/// 戻り値は (消した履歴の件数, 消した画像の件数, そのバイト数)。
/// 注意: 保存ワーカーが処理中の 1 枚はこの後に書き込まれ得る(flush すると
/// ワーカーが永久に停止するため待たない)。次に全消去すれば消える
pub fn history_clear() -> (usize, usize, u64) {
    let entries = match HISTORY.lock() {
        Ok(mut h) => {
            let n = h.entries().len();
            h.clear();
            n
        }
        Err(poisoned) => {
            // ロックが毒されていても消去は続ける(保存側と同じ方針)
            let mut h = poisoned.into_inner();
            let n = h.entries().len();
            h.clear();
            n
        }
    };
    if let Some(path) = history_path() {
        let _ = std::fs::remove_file(path);
    }
    let (images, bytes) = image_store_dir()
        .map(|dir| knit_common::history::clear_image_store(&dir))
        .unwrap_or((0, 0));
    eprintln!("[clip] 履歴を消しました(履歴 {entries} 件・画像 {images} 件/{bytes} バイト)");
    (entries, images, bytes)
}

/// 履歴の件数(全消去の確認文に「何件消えるか」を出すため。ロックが毒されて
/// いても読める)
pub fn history_len() -> usize {
    match HISTORY.lock() {
        Ok(h) => h.entries().len(),
        Err(poisoned) => poisoned.into_inner().entries().len(),
    }
}

/// images/ の使用量 (件数, 合計バイト)。確認文・設定画面の表示用
pub fn image_store_usage() -> (usize, u64) {
    image_store_dir()
        .map(|dir| knit_common::history::image_store_usage(&dir))
        .unwrap_or((0, 0))
}

pub(crate) fn history_push(text: &str, device: &str) {
    let ts = knit_common::history::now_epoch_ms();
    if let Ok(mut h) = HISTORY.lock() {
        if h.push_text(text, device, ts).is_some() {
            HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
            drop(h);
            history_save();
        }
    }
}

/// ファイル群を履歴へ載せる(ビジョン§10 の File 分類)。送信・受信・掴み投げの
/// すべてのファイル移動で呼ぶ
pub(crate) fn history_push_files(paths: &[std::path::PathBuf], device: &str) {
    let ts = knit_common::history::now_epoch_ms();
    if let Ok(mut h) = HISTORY.lock() {
        if h.push_files(paths, device, ts).is_some() {
            HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
            drop(h);
            history_save();
        }
    }
}

/// Windows への送信結果を履歴へ反映する。履歴は「渡った」事実の記録のため、
/// 送信スレッドの開始ではなく完了(Ok)のときだけ載せる: 失敗・中止まで載せると
/// 履歴からの復元が実在しないパスを指すことになる
pub(crate) fn push_files_history_on_result<T>(r: &Result<T, std::io::Error>, paths: &[std::path::PathBuf]) {
    if r.is_ok() {
        history_push_files(paths, "Mac");
    }
}

/// 画像履歴の本体を置くディレクトリ(env と同じ ~/.config/knit/images/)
fn image_store_dir() -> Option<std::path::PathBuf> {
    knit_common::envutil::config_dir().map(|dir| dir.join("images"))
}

/// images/ に残す実体の上限。履歴 cap(50)より少し余裕を持たせた件数
const IMAGE_KEEP: usize = 60;

/// 画像保存ワーカーの残りを書き切ってから終了する(終了経路から呼ぶ)。
/// ワーカーが起動していなければ何もしない。プロセス終了前に呼ばないと、
/// 直前にコピーした画像が実体・履歴の両方で失われる(レビューで判明)
pub(crate) fn flush_image_store() {
    if let Some(worker) = IMAGE_STORE_WORKER.get() {
        worker.flush();
    }
}

/// 画像を履歴へ載せる(ビジョン§10 の Image 分類)。本体は BMP バイトを
/// 内容ハッシュ名で images/ へ保存し、履歴には「ファイル名\tバイト数」だけ
/// 残す。同名=同一内容のため二重保存は起きない
/// 履歴画像の背景保存ワークスレッドの句柄。起動時に1回だけ生成し、
/// 終了時にフラッシュして取りこぼしを防止する。
pub static IMAGE_STORE_WORKER: std::sync::OnceLock<ImageStoreWorker> = std::sync::OnceLock::new();

/// 1枚の委譲タスク。dib(33MB 生データ)と端末名だけを持つ。
pub struct ImageStoreJob {
    dib: Vec<u8>,
    device: String,
}

/// 履歴画像を背景スレッドへ委譲するワークスレッド。
/// 1本のワーカーに FIFO で流し、上限(最大4枚)を超えた場合は古いタスクを破棄して警告する。
/// 終了時にフラッシュして残りのタスクを完了する。
pub struct ImageStoreWorker {
    sender: Mutex<Option<mpsc::Sender<ImageStoreJob>>>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// 未処理タスク数(投入側とワーカー側で共有。上限4枚)
    depth: std::sync::Arc<AtomicUsize>,
}

impl ImageStoreWorker {
    /// 非同期な保存を開始する。キュー上限(4枚)を超えたら**新しいタスクを捨てる**
    /// (古いタスクを捨てるにはチャネルを空にする必要があり、その間ホットパスが
    /// 止まるため。捨てたことはログに残す)。ホットパスはここで終わる。
    pub fn enqueue(&self, dib: Vec<u8>, device: &str) {
        let depth_before = self.depth.fetch_add(1, Ordering::SeqCst);
        if depth_before >= 4 {
            let _ = self.depth.fetch_sub(1, Ordering::SeqCst);
            eprintln!("[clip] 画像保存キューが上限(4枚)に達したため新しいタスクを破棄");
            return;
        }
        let sender_guard = self.sender.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tx) = sender_guard.as_ref() else {
            let _ = self.depth.fetch_sub(1, Ordering::SeqCst);
            eprintln!("[clip] 画像保存ワークスレッドが停止しているため保存をスキップ");
            return;
        };
        if let Err(e) = tx.send(ImageStoreJob {
            dib,
            device: device.to_string(),
        }) {
            let _ = self.depth.fetch_sub(1, Ordering::SeqCst);
            eprintln!("[clip] 画像保存ワークスレッドへの送信用失敗: {e}");
        }
    }

    /// ワーカースレッドを停止し、残りのタスクをすべて実行して完了を待つ。
    /// Sender を落とすとチャネルが閉じ、キューに残ったタスクを処理し終えた
    /// ワーカーが終了するため join が返る(取りこぼしなし)。
    pub fn flush(&self) {
        {
            let mut sender = self.sender.lock().unwrap_or_else(|e| e.into_inner());
            drop(sender.take()); // ここでチャネルを閉じる
        }
        let handle = self
            .handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = handle {
            if let Err(e) = h.join() {
                eprintln!("[clip] 画像保存ワークスレッドがパニックしました: {e:?}");
            }
            eprintln!("[clip] 画像保存ワークスレッドをフラッシュし完了");
        }
    }
}

/// ワーカースレッドの初期化(1回だけ)。保存先は起動時に解決する。
fn ensure_image_store_worker() -> Option<&'static ImageStoreWorker> {
    let dir = image_store_dir()?;
    Some(IMAGE_STORE_WORKER.get_or_init(|| ImageStoreWorker::start(dir, true)))
}

impl ImageStoreWorker {
    /// 保存先ディレクトリを指定してワーカーを起動する。
    /// `push_history` は履歴(HISTORY + history_save)へも載せるかどうか。
    /// テストでは false にして実ユーザーの履歴ファイルを触らない。
    fn start(store_dir: std::path::PathBuf, push_history: bool) -> ImageStoreWorker {
        let (tx, rx) = mpsc::channel::<ImageStoreJob>();
        // 深さカウンタは投入側とワーカー側で共有する(処理し終えたら減らす。
        // 減らさないと上限4枚に達した後は永久に投入が捨てられる)
        let depth = std::sync::Arc::new(AtomicUsize::new(0));
        let depth_worker = std::sync::Arc::clone(&depth);
        let handle = std::thread::spawn(move || {
            for job in rx {
                // BMP 変換とファイル書き込みをワーカー側で行う(ホットパスから追い出した処理)
                let bmp = objc::dib_to_bmp(&job.dib);
                let dir = store_dir.clone();
                let name = knit_common::history::image_file_name(knit_common::history::fnv1a64(&bmp), "bmp");
                let _ = std::fs::create_dir_all(&dir);
                knit_common::history::restrict_dir(&dir);
                let path = dir.join(&name);
                if !path.exists() {
                    if knit_common::history::write_private_or_remove(&path, &bmp).is_err() {
                        eprintln!("[clip] 画像の履歴保存に失敗しました(ディスク容量等)");
                        let _ = depth_worker.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                } else {
                    knit_common::history::touch_mtime(&path);
                }
                let removed = knit_common::history::prune_image_store(
                    &dir,
                    IMAGE_KEEP,
                    knit_common::history::MAX_IMAGE_STORE_BYTES,
                );
                // 履歴への追加はワーカー側で行う(順序保証と排他維持)。
                // テスト(push_history=false)では実行しない
                let ts = knit_common::history::now_epoch_ms();
                if push_history {
                    if let Ok(mut h) = HISTORY.lock().map_err(|e| e.into_inner()) {
                        // 監査1: 実体を消した画像を参照する履歴項目も一緒に消す。
                        // 残すと「押しても復元できない項目」が履歴に溜まる
                        let mut changed = h.remove_image_entries(&removed) > 0;
                        if h.push_image(&name, bmp.len(), &job.device, ts).is_some() {
                            changed = true;
                        }
                        if changed {
                            HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
                            drop(h);
                            history_save();
                        }
                    }
                }
                // 1枚処理し終えたので深さを戻す(次の投入を受け入れる)
                let _ = depth_worker.fetch_sub(1, Ordering::SeqCst);
            }
        });
        ImageStoreWorker {
            sender: Mutex::new(Some(tx)),
            handle: Mutex::new(Some(handle)),
            depth,
        }
    }
}

/// 画像を履歴へ載せる(ビジョン§10 の Image 分類)。本体は BMP バイトを
/// 内容ハッシュ名で images/ へ保存し、履歴には「ファイル名\tバイト数」だけ
/// 残す。同名=同一内容のため二重保存は起きない。
/// ループ11: DIB を受ける版は**ホットパスではキュー投入だけ**行い、dib_to_bmp と
/// 33MB のファイル書き込み(実測 26ms/枚)は背景ワーカーが担う。
pub(crate) fn history_push_image(dib: &[u8], device: &str) {
    if let Some(worker) = ensure_image_store_worker() {
        worker.enqueue(dib.to_vec(), device);
    }
}

/// BMP バイトをそのまま履歴へ載せる同期版(履歴からの復元で既に BMP を
/// 持っている経路用。変換が不要なため従来どおり同期で書く)。
pub(crate) fn history_push_image_bmp(bmp: &[u8], device: &str) {
    let Some(dir) = image_store_dir() else { return };
    let name = knit_common::history::image_file_name(knit_common::history::fnv1a64(bmp), "bmp");
    let _ = std::fs::create_dir_all(&dir);
    knit_common::history::restrict_dir(&dir);
    let path = dir.join(&name);
    if !path.exists() {
        // 失敗時は部分書き込みの実体を消す(exists チェックで壊れた実体が
        // 固定化され、復元だけが恒久失敗するのを防ぐ)
        if knit_common::history::write_private_or_remove(&path, bmp).is_err() {
            eprintln!("[clip] 画像の履歴保存に失敗しました(ディスク容量等)");
            return;
        }
    } else {
        // 同じ画像の再コピー: 実体は既に有るため書き直さず mtime だけ現在へ
        // 更新する。刈り込み(prune)が mtime 順に残すため、更新しないと
        // 「履歴の最新」として参照中の実体が古い順に消える
        knit_common::history::touch_mtime(&path);
    }
    // 刈り込みは if の外: 同じ画像の再コピー(実体の新規保存が無い)の間も
    // バイト総量上限(512MiB)へ収め続ける。件数とバイトの両方で頭打ちにする
    knit_common::history::prune_image_store(
        &dir,
        IMAGE_KEEP,
        knit_common::history::MAX_IMAGE_STORE_BYTES,
    );
    let ts = knit_common::history::now_epoch_ms();
    if let Ok(mut h) = HISTORY.lock() {
        if h.push_image(&name, bmp.len(), device, ts).is_some() {
            HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
            drop(h);
            history_save();
        }
    }
}

/// 起動時に画像履歴の実体を刈り込む(履歴 load の後に 1 回)。保存時の刈り込みは
/// 画像が届いた時しか走らないため、起動をまたぐと総量超過が残り続くのを防ぐ
pub(crate) fn prune_image_store_now() {
    let Some(dir) = image_store_dir() else { return };
    let removed = knit_common::history::prune_image_store(
        &dir,
        IMAGE_KEEP,
        knit_common::history::MAX_IMAGE_STORE_BYTES,
    );
    // 監査1: 起動時の刈り込みでも、消した実体を参照する履歴項目を取り除く
    if removed.is_empty() {
        return;
    }
    if let Ok(mut h) = HISTORY.lock().map_err(|e| e.into_inner()) {
        if h.remove_image_entries(&removed) > 0 {
            HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
            drop(h);
            history_save();
        }
    }
}

/// 履歴から Mac のクリップボードへ復元する。受信ループ防止のため
/// 同期基準を先に進め、自分の履歴へ「Mac」のコピーとして載せる。
/// 本文が画像保存名(img-….bmp\tサイズ)なら images/ から本体を、
/// 絶対パスの並び(ファイル参照)なら Finder の ⌘C 相当へ載せ直す
pub fn history_restore(text: String) {
    // 画像の履歴(ビジョン§10): images/ から本体を読み戻してクリップボードへ
    if let Some((name, _size)) = knit_common::history::parse_image_entry(&text) {
        let path = image_store_dir().map(|d| d.join(&name));
        let bmp = path.and_then(|p| std::fs::read(p).ok());
        let Some(bmp) = bmp else {
            eprintln!("[clip] 履歴の画像本体が見つからないため復元しません");
            notify(
                "Knit",
                "履歴の画像が見つからないため復元できませんでした(削除済み)",
            );
            return;
        };
        let ok = with_pool(|| unsafe { mac_set_clipboard_image_bmp(&bmp) });
        if ok {
            *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = None;
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            // 受信画像と同じく、載せ直しをきっかけにした送り返しを二重に防ぐ
            LAST_IMG_RX_MS.store(now_ms(), Ordering::Relaxed);
            history_push_image_bmp(&bmp, "Mac");
            eprintln!(
                "[clip] 履歴から Mac のクリップボードへ画像を復元({}KB)",
                bmp.len() / 1024
            );
        } else {
            // 復元に失敗したのに黙っていると、通知を見た利用者が ⌘V して
            // 何も貼られない状態になる(Windows 側と同じく失敗を伝える)
            eprintln!("[clip] 履歴画像のクリップボード載せに失敗しました");
            notify(
                "Knit",
                "画像を復元できませんでした(クリップボードが他アプリで使用中)。もう一度お試しください",
            );
        }
        return;
    }
    if knit_common::history::looks_like_file_paths(&text) {
        let paths: Vec<std::path::PathBuf> = text
            .lines()
            .map(|l| std::path::PathBuf::from(l.trim()))
            .collect();
        // 一部が移動・削除されていても、存在する分は復元する(全部拒否すると
        // 履歴が使い物にならなくなる。Windows 側と同じ挙動に揃える)
        let found: Vec<std::path::PathBuf> =
            paths.iter().filter(|p| p.exists()).cloned().collect();
        let missing = paths.len() - found.len();
        if found.is_empty() {
            eprintln!("[clip] 履歴のファイル参照が全て見つからないため復元しません");
            notify(
                "Knit",
                "履歴のファイルが見つからないため復元できませんでした(移動・削除済み)",
            );
            return;
        }
        if missing > 0 {
            notify(
                "Knit",
                &format!("履歴のファイル {}/{} 件を復元しました(残り {missing} 件は移動・削除済み)", found.len(), paths.len()),
            );
        }
        let ok = with_pool(|| unsafe { mac_clipboard_write_files(&found) });
        if ok {
            *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = None;
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            history_push_files(&found, "Mac");
            eprintln!(
                "[clip] 履歴から Mac のクリップボードへファイル {} 件を復元(欠け {missing} 件)",
                found.len()
            );
            if missing > 0 {
                eprintln!("[clip] 復元時に {missing} 件が移動・削除済みでした");
            }
        } else {
            // 上の部分欠け通知が先に出ているため、載せ失敗を明示しないと
            // 「復元された」と誤解したまま ⌘V しても何も貼られない
            eprintln!("[clip] 履歴ファイルのクリップボード載せに失敗しました");
            notify(
                "Knit",
                "ファイルを復元できませんでした(クリップボードが他アプリで使用中)。もう一度お試しください",
            );
        }
        return;
    }
    let ok = with_pool(|| unsafe { mac_set_clipboard(&text) });
    if ok {
        *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
        LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
        history_push(&text, "Mac");
        eprintln!(
            "[clip] 履歴から Mac のクリップボードへ復元({} bytes)",
            text.len()
        );
    } else {
        eprintln!("[clip] 履歴テキストのクリップボード載せに失敗しました");
        notify(
            "Knit",
            "テキストを復元できませんでした(クリップボードが他アプリで使用中)。もう一度お試しください",
        );
    }
}
/// Windows へ入る時に Mac のクリップボードを渡す(Deskflow と同じ「画面を離れる時に
/// 同期」方式)。コピーのたびに送る旧方式は、Mac 内だけのコピペでも最大 200MB の
/// ファイルを流し、パスワード等も即座に相手へ渡っていた
/// Mac でのコピーを、画面を越えなくても履歴へ載せる(Clipy のような履歴アプリが無くても、
/// Mac のコピーをメニューバーの「クリップボード履歴」から選び直せる)。設定「Macのコピーを履歴に残す」
pub static LOCAL_HISTORY: AtomicBool = AtomicBool::new(true);
static RECORD_DONE_COUNT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);
static RECORD_SEEN_COUNT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);
/// ローカルコピー 1 回分(changeCount)の履歴記録権。record_local_copy(常時の
/// 監視)と sync_clipboard_to_win(越境時の同期)が同じコピーを扱えるため、
/// 片方が読み出しから history_push までの間に他方の印(LAST_SYNC_COUNT)を
/// 読み違えると二重に積まれる。CAS で一方のみが積めるようにする
static HISTORY_CLAIM_COUNT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);

/// changeCount への履歴記録権を取る。true=初めての記録として通す、false=他の
/// 経路が既に記録済み(またはより新しいコピーを記録済み)のため積まない。
/// changeCount は単調増加するため、CAS ループで現在値より大きい時だけ書き換える
fn claim_history(claimed: &std::sync::atomic::AtomicIsize, cnt: isize) -> bool {
    let mut cur = claimed.load(Ordering::Relaxed);
    loop {
        if cur >= cnt {
            return false;
        }
        match claimed.compare_exchange(cur, cnt, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(actual) => cur = actual,
        }
    }
}

/// ペーストボードの変化を見張る。書き込み直後は読まず、次の確認でも同じ値なら読む(遅延提供の途中を避ける)
pub(crate) fn start_local_history() {
    if envutil::get("KNIT_LOCAL_HISTORY").as_deref() == Some("0") || !knit_common::share::env_cap().clip {
        return;
    }
    std::thread::spawn(|| {
        // 起動時点にすでにあるコピーは記録しない(いつのものか分からないため)
        let start = with_pool(clipboard_change_count);
        RECORD_DONE_COUNT.store(start, Ordering::Relaxed);
        loop {
            std::thread::sleep(Duration::from_millis(600));
            with_pool(|| unsafe { record_local_copy() });
        }
    });
}

unsafe fn record_local_copy() {
    if !LOCAL_HISTORY.load(Ordering::Relaxed) {
        // オフの間の変化は記録せず、オンに戻した時に古いコピーを拾わない
        RECORD_DONE_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
        return;
    }
    let cnt = clipboard_change_count();
    if cnt == RECORD_DONE_COUNT.load(Ordering::Relaxed) {
        return;
    }
    if RECORD_SEEN_COUNT.swap(cnt, Ordering::Relaxed) != cnt {
        return; // 変化を見つけた直後。次の確認でも同じなら読む
    }
    RECORD_DONE_COUNT.store(cnt, Ordering::Relaxed);
    // Knit 自身の書き込み(Windows からの受信・履歴からの復元)は、その経路で記録済み
    if cnt == LAST_SYNC_COUNT.load(Ordering::Relaxed) {
        return;
    }
    // パスワード管理アプリなどが「機密」と印を付けたコピーは記録しない
    if pb_is_concealed(general_pasteboard()) {
        return;
    }
    if let Some(text) = mac_get_clipboard().filter(|t| !t.trim().is_empty()) {
        // 1MB 超のテキストは履歴にも載らない。黙っていると「コピーしたのに履歴に
        // 現れない」状態になるため、サイズを伝える(通知は 60 秒に 1 回)
        if text.len() > CLIP_MAX_BYTES {
            eprintln!(
                "[clip] ローカルコピーのテキストが大きすぎます({} bytes > 上限 1MB)",
                text.len()
            );
            big_text_notify(&text);
            return;
        }
        if LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(text.as_str()) {
            return;
        }
        if knit_common::smartguard::looks_secret(&text) == Some(true) {
            return;
        }
        // 越境同期(sync_clipboard_to_win)が同じ changeCount を記録済みなら
        // 二重に積まない(⌘C 直後の越境で片方が読み違える競合の窓を塞ぐ)
        if claim_history(&HISTORY_CLAIM_COUNT, cnt) {
            history_push(&text, "Mac");
        }
    } else if let Some(files) = mac_clipboard_files() {
        if claim_history(&HISTORY_CLAIM_COUNT, cnt) {
            history_push_files(&files, "Mac");
        }
    } else if now_ms().saturating_sub(LAST_IMG_RX_MS.load(Ordering::Relaxed)) >= 1_000 {
        // 越境経路と同じ changeCount なら変換は共有される(2 回目の変換をしない)
        if let Some(dib) = clipboard_image_dib_cached(cnt) {
            if claim_history(&HISTORY_CLAIM_COUNT, cnt) {
                history_push_image(&dib, "Mac");
            }
        }
    }
}

/// Busy 等で同期を保留するとき、同期済みの印(LAST_SYNC_COUNT)を自分が書いた
/// 値のままの時だけ prev へ戻す。間に別の書き込みが入っていれば上書きしない。
/// 戻さないとこのコピーは二度と同期されない(次の変更検知・切替で再送する)
fn defer_sync_rollback(prev: isize, cnt: isize) {
    LAST_SYNC_COUNT
        .compare_exchange(cnt, prev, Ordering::Relaxed, Ordering::Relaxed)
        .ok();
}

pub(crate) fn sync_clipboard_to_win() {
    try_app_handoff();
    if !CLIP_SHARE.load(Ordering::Relaxed)
        || !knit_common::share::allow_clip()
        || !CONNECTED.load(Ordering::Relaxed)
    {
        eprintln!(
            "[clip] 同期しません(共有={} 接続={})",
            CLIP_SHARE.load(Ordering::Relaxed) && knit_common::share::allow_clip(),
            CONNECTED.load(Ordering::Relaxed)
        );
        return;
    }
    // 貼り付け元アプリの遅延提供データ読み出しでタップを止めないよう別スレッドで行う
    std::thread::spawn(move || {
        with_pool(|| unsafe {
            let cnt = clipboard_change_count();
            let prev = LAST_SYNC_COUNT.swap(cnt, Ordering::Relaxed);
            if prev == cnt {
                eprintln!("[clip] 変化なしのため同期しません(cnt={cnt})");
                return;
            }
            if pb_is_concealed(general_pasteboard()) {
                eprintln!("[clip] 秘匿指定のコピー(パスワード等)のため送りません");
                return;
            }
            let text = mac_get_clipboard().filter(|t| !t.is_empty());
            let Some(text) = text else {
                if let Some(files) = mac_clipboard_files() {
                    if active_peer_is_android() && !active_peer_is_android_app() {
                        // ADB中継への画像・ファイルのクリップボード共有は未対応
                        //(端末のクリップボードへ入れる公開手段が無い。明示送信は
                        // 設定画面のボタンから行う)
                        eprintln!("[file] Android 接続先へのファイルの自動送信はしません");
                    } else {
                        let key = mac_files_key(&files);
                        let dup =
                            *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) == key;
                        if !dup {
                            let n = files.len();
                            match send_files_to_win(files) {
                                SendFilesOutcome::Started => {
                                    *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) =
                                        key;
                                    eprintln!("[file] クリップボードのファイル {n} 件を渡します");
                                }
                                // 他の転送中で送れなかった。同期済みの印を戻さないと
                                // このコピーは二度と同期されない(次回の切替時に再試行する)。
                                // 戻すのは自分が書いた値のままの時だけにする(間に別の
                                // 書き込みが入っていれば上書きしない)
                                SendFilesOutcome::Busy => {
                                    defer_sync_rollback(prev, cnt);
                                    eprintln!(
                                        "[file] 他の転送中のためファイル同期を保留します(次の切替で再試行します)"
                                    );
                                }
                                // 共有設定での拒否は通知済み。印を戻すと切替のたびに
                                // 同じ通知が繰り返されるため、このコピーは同期済み扱いにする
                                SendFilesOutcome::Denied => {}
                            }
                        }
                    }
                } else if now_ms().saturating_sub(LAST_IMG_RX_MS.load(Ordering::Relaxed)) < 1_000 {
                    // 受信画像の載せ直後に来た同期: 送り返しの恐れがあるため見送る
                    eprintln!("[clip] 画像受信直後のため同期を控えます");
                    return;
                } else if let Some(dib) = clipboard_image_dib_cached(cnt) {
                    // 履歴経路が同じ changeCount を先に変換していればその実体を共有する
                    if active_peer_is_android() && !active_peer_is_android_app() {
                        // 端末のクリップボードへ画像を載せる: 画像は MediaStore の
                        // Pictures/Knit へ保存され、その URI がクリップボードに入る
                        // (scrcpy と同じ app_process 方式のヘルパー経由)
                        match dib_to_png(&dib) {
                            Some(png) => {
                                let stamp = now_ms();
                                if android::set_clipboard_image(
                                    "image/png",
                                    &format!("Knit-{stamp}.png"),
                                    &png,
                                ) {
                                    eprintln!(
                                        "[clip] mac->android image(クリップボード) {}KB",
                                        png.len() / 1024
                                    );
                                    if claim_history(&HISTORY_CLAIM_COUNT, cnt) {
                                        history_push_image(&dib, "Mac");
                                    }
                                } else {
                                    eprintln!("[clip] タブレットのクリップボードへ画像を載せられませんでした");
                                }
                            }
                            None => {
                                let (w, h, bpp, comp) = if dib.len() >= 20 {
                                    (
                                        i32::from_le_bytes(dib[4..8].try_into().unwrap()),
                                        i32::from_le_bytes(dib[8..12].try_into().unwrap()),
                                        u16::from_le_bytes(dib[14..16].try_into().unwrap()),
                                        u32::from_le_bytes(dib[16..20].try_into().unwrap()),
                                    )
                                } else {
                                    (0, 0, 0, 0)
                                };
                                eprintln!(
                                    "[clip] 画像を PNG へ変換できません(未対応の形式: w={w} h={h} bpp={bpp} comp={comp})"
                                );
                            }
                        }
                    } else if dib.len() > bulk::MAX_IMAGE {
                        // 受信側は上限超の画像をイベントも出さず破棄するため、
                        // 送る前にここで止めて利用者へ伝える(Win→Mac 方向と対称)
                        eprintln!(
                            "[clip] mac->win image が大きすぎます({}MB > 上限 {}MB)",
                            dib.len() / 1024 / 1024,
                            bulk::MAX_IMAGE / 1024 / 1024
                        );
                        notify(
                            "Knit",
                            &format!(
                                "画像が大きすぎて送信できません({}MB。上限は {}MB です)",
                                dib.len() / 1024 / 1024,
                                bulk::MAX_IMAGE / 1024 / 1024
                            ),
                        );
                    } else if FILE_TX_BUSY.load(Ordering::Relaxed) {
                        // ファイル転送中に画像を送ると、bulk の送信口(転送が終わる
                        // まで ロックを保持)を待ってこのスレッドが留まり、転送完了後に
                        // 古い画像が上書いてしまう。ファイル同期と同じ Busy 扱いで
                        // 今回はスキップし、印を戻して次の切替(変更検知)で再送する
                        defer_sync_rollback(prev, cnt);
                        eprintln!(
                            "[clip] 他の転送中のため画像同期を保留します(次の切替で再試行します)"
                        );
                    } else {
                        match BULK_LINK.send(|w| bulk::send_image(w, &dib)) {
                            Ok(()) => {
                                eprintln!("[clip] mac->win image {}KB", dib.len() / 1024);
                                if claim_history(&HISTORY_CLAIM_COUNT, cnt) {
                                    history_push_image(&dib, "Mac");
                                }
                            }
                            Err(e) => eprintln!("[clip] mac->win image 送信失敗: {e}"),
                        }
                    }
                } else {
                    // 診断: 3 種のどれにも該当しない(クリップボードに共有対象が無い)
                    eprintln!("[clip] 共有するテキスト・ファイル・画像がありません");
                }
                return;
            };
            // 1MB 超のテキストは Windows へ送れない。黙って落とすと「コピーしたのに
            // 貼り付けられない」状態になるため、サイズ・文字数を伝える(60 秒に 1 回)
            if text.len() > CLIP_MAX_BYTES {
                eprintln!(
                    "[clip] mac->win テキストが大きすぎます({} bytes > 上限 1MB)",
                    text.len()
                );
                big_text_notify(&text);
                return;
            }
            // 自分が Windows から受信して書き込んだ内容は送り返さない(ループ防止)
            if LAST_RECV_CLIP
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_deref()
                == Some(text.as_str())
            {
                return;
            }
            // 実験的ガード: ローカル ollaya が動いていれば機密テキストを検査する。
            // 無し・失敗は None=現行どおり送る。判定はこの同期スレッド内で完結し
            // 入力経路(タップ)は塞がない
            if knit_common::smartguard::looks_secret(&text) == Some(true) {
                eprintln!("[clip] smartguard: 機密の可能性が高いため Windows へ送りません");
                smart_secret_notify(&format!("{} へは送りませんでした", active_peer_label()));
                return;
            }
            eprintln!("[clip] mac->win {} bytes", text.len());
            // 履歴記録はローカル監視(record_local_copy)と同じ changeCount の
            // 記録権を取り合う: どちらが先に通っても一方だけが積む
            if claim_history(&HISTORY_CLAIM_COUNT, cnt) {
                history_push(&text, "Mac");
            }
            send_msg(&Msg::Clip { text });
        })
    });
}

/// smartguard の通知間引き(誤検知の連打防止。60 秒に 1 回)
static SMART_SECRET_NOTIFY_MS: AtomicU64 = AtomicU64::new(0);
fn smart_secret_notify(detail: &str) {
    let now = now_ms();
    if now.saturating_sub(SMART_SECRET_NOTIFY_MS.swap(now, Ordering::Relaxed)) < 60_000 {
        return;
    }
    notify(
        "Knit",
        &format!("クリップボードに機密の可能性があるため {detail}、履歴にも載せません(KNIT_SMART_SECRET=0 で無効化)"),
    );
}

/// 1MB 上限超過テキストの通知間引き(同じ巨大コピーが切替のたびに通知するのを
/// 防ぐ。60 秒に 1 回)。文言は実際の文字数・サイズを含む共通の生成器を使う
static BIG_TEXT_NOTIFY_MS: AtomicU64 = AtomicU64::new(0);
fn big_text_notify(text: &str) {
    let now = now_ms();
    if now.saturating_sub(BIG_TEXT_NOTIFY_MS.swap(now, Ordering::Relaxed)) < 60_000 {
        return;
    }
    notify(
        "Knit",
        &knit_common::history::too_large_clip_message(text),
    );
}

/// 最後に Windows から受信して書き込んだテキスト(エコーバック送信防止)
pub(crate) static LAST_RECV_CLIP: Mutex<Option<String>> = Mutex::new(None);
/// 最後の画像受信時刻(ms)。受信画像のクリップボード載せ→changeCount 更新の
/// 間に切替同期が走ると画像を送り返してしまう(最大 64MB の無駄転送)ため、
/// 直近の受信では画像送信を 1 回控える(changeCount 保護の二重ガード)
pub(crate) static LAST_IMG_RX_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) const CLIP_MAX_BYTES: usize = 1024 * 1024; // 1MB(Win側と同じ上限)

/// Busy による画像同期の保留: 同期済みの印が巻き戻ること(次の変更検知で再送される)
#[cfg(test)]
mod defer_sync_rollback_tests {
    use super::{defer_sync_rollback, LAST_SYNC_COUNT};
    use std::sync::atomic::Ordering;

    /// LAST_SYNC_COUNT はプロセス共有の static。入れ替えて必ず元へ戻す
    #[test]
    fn rollback_restores_prev_only_when_untouched() {
        let saved = LAST_SYNC_COUNT.load(Ordering::Relaxed);
        // prev=5 → cnt=7 へ進めた状態(同期スレッドが swap で書いた直後を再現)
        LAST_SYNC_COUNT.store(7, Ordering::Relaxed);
        defer_sync_rollback(5, 7);
        assert_eq!(
            LAST_SYNC_COUNT.load(Ordering::Relaxed),
            5,
            "誰も触っていなければ prev へ巻き戻る(次の切替で再送される)"
        );
        // 間に別の書き込み(=別のコピーの同期)が入っていれば上書きしない
        LAST_SYNC_COUNT.store(8, Ordering::Relaxed);
        defer_sync_rollback(5, 7);
        assert_eq!(
            LAST_SYNC_COUNT.load(Ordering::Relaxed),
            8,
            "自分の書いた値のまま以外は触らない"
        );
        LAST_SYNC_COUNT.store(saved, Ordering::Relaxed);
    }
}

/// 画像 DIB 変換の単一スロットキャッシュ(同一 changeCount の 2 経路で 1 回に集約)
#[cfg(test)]
mod dib_cache_tests {
    use super::DibCache;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 履歴経路と越境経路が同じ changeCount を処理しても変換は 1 回で、
    /// 同じ実体(Arc)を共有する = 33MB の再変換も再確保もしない
    #[test]
    fn same_change_count_converts_once_and_shares_one_buffer() {
        let cache = DibCache::new();
        let calls = AtomicUsize::new(0);
        let first = cache
            .get_or_convert(7, || {
                calls.fetch_add(1, Ordering::SeqCst);
                Some(vec![1u8, 2, 3])
            })
            .expect("1回目は変換できる");
        let second = cache
            .get_or_convert(7, || {
                calls.fetch_add(1, Ordering::SeqCst);
                Some(vec![9u8, 9, 9])
            })
            .expect("2回目はキャッシュから返る");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "同一 changeCount の変換は 1 回だけ(現行は 2 回走っていた)"
        );
        assert_eq!(
            Arc::as_ptr(&first),
            Arc::as_ptr(&second),
            "同じ実体を共有する(複製しない)"
        );
        assert_eq!(&*second, &vec![1u8, 2, 3], "2回目の変換結果は使われない");
    }

    /// changeCount が進めば作り直し、戻ってきた count では古い実体を返さない
    /// (Knit 自身のクリップボード書き込みは必ず count を進める)
    #[test]
    fn change_count_change_reconverts_and_replaces_the_slot() {
        let cache = DibCache::new();
        let a = cache.get_or_convert(1, || Some(vec![1])).unwrap();
        assert_eq!(&*a, &vec![1u8]);
        let b = cache.get_or_convert(2, || Some(vec![2])).unwrap();
        assert_eq!(&*b, &vec![2u8], "changeCount が変われば作り直す");
        let back = cache.get_or_convert(1, || Some(vec![3])).unwrap();
        assert_eq!(&*back, &vec![3u8], "戻った count では古い実体を返さない");
    }

    /// 変換失敗(None)はキャッシュしない。キャッシュすると非対応形式の画像が
    /// 以後ずっと「変換できない」まま固定される
    #[test]
    fn conversion_failure_is_not_cached_so_it_is_retried() {
        let cache = DibCache::new();
        assert!(cache.get_or_convert(5, || None).is_none());
        let calls = AtomicUsize::new(0);
        let retried = cache.get_or_convert(5, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(vec![4u8])
        });
        assert!(retried.is_some(), "失敗はキャッシュされず次回に再試行される");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "再試行では変換が走る");
    }

    /// 履歴スレッドと越境スレッドが同時に同じ count を処理しても変換は 1 回
    /// (結果は実行順に依存せず、常に「変換 1 回・全員が同じ実体」になる)
    #[test]
    fn concurrent_callers_with_the_same_change_count_convert_once() {
        let cache = Arc::new(DibCache::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let hs: Vec<_> = (0..4)
            .map(|_| {
                let (cache, calls) = (Arc::clone(&cache), Arc::clone(&calls));
                std::thread::spawn(move || {
                    cache.get_or_convert(11, || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Some(vec![7u8; 8])
                    })
                })
            })
            .collect();
        let ptrs: Vec<_> = hs
            .into_iter()
            .map(|h| Arc::as_ptr(&h.join().unwrap().unwrap()))
            .collect();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "同時でも変換は 1 回");
        assert!(ptrs.windows(2).all(|w| w[0] == w[1]), "全員が同じ実体を得る");
    }
}

/// 履歴記録権の CAS(⌘C 直後の越境で二重記録される回帰の防止)
#[cfg(test)]
mod claim_history_tests {
    use super::claim_history;
    use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

    #[test]
    fn only_one_side_wins_the_same_count() {
        let claimed = AtomicIsize::new(-1);
        assert!(claim_history(&claimed, 7), "最初の要求は通る");
        assert!(!claim_history(&claimed, 7), "同じ changeCount の 2 回目は通らない");
    }

    #[test]
    fn newer_copy_claims_after_older_one() {
        let claimed = AtomicIsize::new(3);
        assert!(!claim_history(&claimed, 3));
        assert!(claim_history(&claimed, 4), "新しいコピーは記録できる");
        assert!(!claim_history(&claimed, 4));
    }

    #[test]
    fn stale_count_never_displaces_a_newer_claim() {
        let claimed = AtomicIsize::new(9);
        assert!(!claim_history(&claimed, 5), "古い changeCount は記録しない");
        assert_eq!(claimed.load(Ordering::Relaxed), 9, "値を巻き戻さない");
    }

    /// 並走を模して 8 スレッドが同じ changeCount を要求した時、通るのは 1 つだけ
    #[test]
    fn concurrent_claims_admit_exactly_one() {
        let claimed = AtomicIsize::new(-1);
        let wins = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    if claim_history(&claimed, 42) {
                        wins.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(wins.load(Ordering::Relaxed), 1, "記録権を取れるのは 1 スレッドだけ");
    }
}

/// ループ11: 履歴画像の背景保存ワーカーのテスト。
/// 実ユーザーの保存先と履歴ファイルを触らないよう、temp ディレクトリと
/// `push_history=false` を使う。
#[cfg(test)]
mod image_store_worker_tests {
    use super::ImageStoreWorker;

    /// 8×8 32bpp の合成 DIB(BITMAPINFOHEADER 40 + 画素。下から上)
    fn small_dib(fill: u8) -> Vec<u8> {
        let (w, h) = (8usize, 8usize);
        let mut dib = Vec::with_capacity(40 + w * h * 4);
        let mut hdr = [0u8; 40];
        hdr[0..4].copy_from_slice(&40u32.to_le_bytes());
        hdr[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        hdr[8..12].copy_from_slice(&(h as i32).to_le_bytes());
        hdr[12..16].copy_from_slice(&1u32.to_le_bytes());
        hdr[14..16].copy_from_slice(&32u16.to_le_bytes());
        hdr[20..24].copy_from_slice(&((w * h * 4) as u32).to_le_bytes());
        dib.extend_from_slice(&hdr);
        for i in 0..w * h {
            dib.extend_from_slice(&[fill, fill.wrapping_add(1), fill.wrapping_add(2), 255]);
            let _ = i;
        }
        dib
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("knit-imgstore-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 委譲した画像が保存され、内容が BMP(14バイトヘッダ + DIB)になっている。
    /// flush 後は必ず完了している(取りこぼしなし)
    #[test]
    fn worker_writes_bmp_and_flush_completes_it() {
        let dir = temp_dir("one");
        let worker = ImageStoreWorker::start(dir.clone(), false);
        let dib = small_dib(0x10);
        worker.enqueue(dib.clone(), "Mac");
        worker.flush();
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        assert_eq!(entries.len(), 1, "1枚だけ保存される");
        let bytes = std::fs::read(&entries[0]).unwrap();
        assert_eq!(&bytes[..2], b"BM", "BMP ファイルヘッダが付く");
        assert_eq!(bytes.len(), 14 + dib.len(), "BMP = 14 + DIB のサイズ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 複数枚を続けて委譲しても取りこぼさず、すべて保存される(FIFO 処理)
    #[test]
    fn worker_keeps_all_jobs_in_order() {
        let dir = temp_dir("many");
        let worker = ImageStoreWorker::start(dir.clone(), false);
        for i in 0..3u8 {
            worker.enqueue(small_dib(0x20 + i), "Mac");
        }
        worker.flush();
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, 3, "3枚すべて保存される");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ホットパスのコスト計測(キュー投入のみ)。旧実装は 33MB の書き込みを含み
    /// 実測 26ms だった。ここでは投入が桁違いに軽いことを数値で残す
    #[test]
    fn enqueue_is_far_cheaper_than_a_disk_write() {
        let dir = temp_dir("bench");
        let worker = ImageStoreWorker::start(dir.clone(), false);
        // 4K 相当(3840×2160×4 ≈ 33MB)
        let dib = vec![0x7Fu8; 3840 * 2160 * 4 + 40];
        let mut samples = Vec::new();
        for _ in 0..20 {
            let t = std::time::Instant::now();
            worker.enqueue(dib.clone(), "Mac");
            samples.push(t.elapsed().as_micros());
        }
        samples.sort_unstable();
        let p50 = samples[samples.len() / 2];
        println!("[perf] enqueue(33MB DIB, キュー投入のみ): p50={p50}µs");
        assert!(p50 < 20_000, "投入は 20ms 未満(実測 {p50}µs)");
        worker.flush();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
