use crate::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// 最後に Windows と同期したクリップボードの changeCount
pub(crate) static LAST_SYNC_COUNT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);

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
    if let Ok(h) = HISTORY.lock() {
        h.save_to(&path);
    }
}

pub fn history_load() {
    let Some(path) = history_path() else { return };
    if let Ok(mut h) = HISTORY.lock() {
        h.load_from(&path);
        HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
    }
}

/// 履歴の全消去(メニューバー)。保存ファイルも消して次回起動に残さない
pub fn history_clear() {
    if let Ok(mut h) = HISTORY.lock() {
        h.clear();
    }
    if let Some(path) = history_path() {
        let _ = std::fs::remove_file(path);
    }
    eprintln!("[clip] 履歴を消しました");
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

/// 画像を履歴へ載せる(ビジョン§10 の Image 分類)。本体は BMP バイトを
/// 内容ハッシュ名で images/ へ保存し、履歴には「ファイル名\tバイト数」だけ
/// 残す。同名=同一内容のため二重保存は起きない
pub(crate) fn history_push_image(bmp: &[u8], device: &str) {
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
    knit_common::history::prune_image_store(
        &dir,
        IMAGE_KEEP,
        knit_common::history::MAX_IMAGE_STORE_BYTES,
    );
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
            history_push_image(&bmp, "Mac");
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
        if let Some(dib) = mac_clipboard_image_dib() {
            if claim_history(&HISTORY_CLAIM_COUNT, cnt) {
                history_push_image(&dib_to_bmp(&dib), "Mac");
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
                } else if let Some(dib) = mac_clipboard_image_dib() {
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
                                        history_push_image(&dib_to_bmp(&dib), "Mac");
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
                                    history_push_image(&dib_to_bmp(&dib), "Mac");
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
