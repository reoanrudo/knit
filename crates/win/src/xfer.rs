use crate::clipboard::{
    clipboard_seq, clipboard_write_dib, clipboard_write_files, CLIP_SHARE_W, history_push_files,
    history_push_image, LAST_RECV_FILES, LAST_SYNC_SEQ,
};
use crate::dragdrop;
use crate::input::BTN_W;
use crate::state::send_main_msg;
use crate::tray;
use knit_common::bulk;
use knit_common::proto::Msg;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// 進行中の Mac へのファイル送信(トレイ表示と Esc キャンセルで使う)
struct XferTx {
    id: u64,
    sent: u64,
    total: u64,
    label: String,
    began: Instant,
}
static TX: std::sync::Mutex<Option<XferTx>> = std::sync::Mutex::new(None);
/// 送信の識別ID(キャンセル要求の宛先)
static NEXT_TX_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// 送信中(多重送信の抑制)。クリップボード検出のたびにスレッドを起こす呼び出し
/// 元のため、2つの送信が並走すると単一スロットの XFER_ACK を取り違ける。
/// panic でも残らないよう TxGuard で解く
static TX_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// ファイル送信中か。クリップボード画像の同期を Busy 設計に乗せるために
/// clipboard.rs から読む(送信中の画像送信は bulk の送信口を待って後から
/// 古い内容が流れるのを防ぐ)
pub(crate) fn tx_busy() -> bool {
    TX_BUSY.load(std::sync::atomic::Ordering::Relaxed)
}
struct TxGuard(std::cell::Cell<Option<u64>>);
impl TxGuard {
    fn new() -> Self {
        TxGuard(std::cell::Cell::new(None))
    }
    fn arm(&self, id: u64) {
        self.0.set(Some(id));
    }
}
impl Drop for TxGuard {
    fn drop(&mut self) {
        TX_BUSY.store(false, Ordering::Relaxed);
        if let Some(id) = self.0.get() {
            end_tx(id);
            knit_common::xfer::discard(id);
        }
    }
}
/// 進行中の Mac からの受信(掴みドラッグ)。Esc で相手へ中止を頼める
pub(crate) static RX_DRAG: std::sync::Mutex<Option<(u64, u64)>> = std::sync::Mutex::new(None); // (id, total)
pub(crate) static RX_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 累計ファイル受信数(ステータス窓の表示用)
pub static FILES_RX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 相手(Mac)の転送受理結果(本線の XferAck で受信ループから届く)
static XFER_ACK: std::sync::Mutex<Option<(usize, usize, bool)>> = std::sync::Mutex::new(None);
static XFER_ACK_CV: std::sync::Condvar = std::sync::Condvar::new();

/// 本線受信ループから呼ぶ: 相手の受理結果を記録し、待っている送信スレッドへ通知
pub(crate) fn note_xfer_ack(accepted: usize, rejected: usize, scope: bool) {
    let mut g = XFER_ACK.lock().unwrap_or_else(|e| e.into_inner());
    *g = Some((accepted, rejected, scope));
    XFER_ACK_CV.notify_all();
}

/// 送信開始前に前回の受理結果を捨てる
fn clear_xfer_ack() {
    *XFER_ACK.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// 相手の受理結果を待つ。旧版相手は応答しないためタイムアウトで None
fn wait_xfer_ack(timeout: Duration) -> Option<(usize, usize, bool)> {
    let mut g = XFER_ACK.lock().unwrap_or_else(|e| e.into_inner());
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = g.take() {
            return Some(v);
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let (ng, _) = XFER_ACK_CV
            .wait_timeout(g, deadline - now)
            .unwrap_or_else(|e| e.into_inner());
        g = ng;
    }
}

/// スキップ(読めなかった項目・シンボリックリンク)の通知用の追記部分。
/// 無ければ空文字列
fn skip_note(skipped: &[std::path::PathBuf]) -> String {
    if skipped.is_empty() {
        return String::new();
    }
    let name = skipped[0]
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".into());
    let extra = skipped.len().saturating_sub(1);
    let more = if extra > 0 { format!(" ほか{extra}件") } else { String::new() };
    format!(" ※読めない・リンク等の {} 件をスキップ: {name}{more}", skipped.len())
}

/// バイト数を通知・表示用に整形する(2048→"2 KB"、3_500_000→"3.3 MB")
pub fn human_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", n as f64 / (1024 * 1024 * 1024) as f64)
    } else if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024 * 1024) as f64)
    } else if n >= 1024 {
        format!("{} KB", n.div_ceil(1024))
    } else {
        format!("{n} B")
    }
}

pub(crate) fn begin_tx(id: u64, total: u64, label: &str) {
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(XferTx {
        id,
        sent: 0,
        total,
        label: label.to_string(),
        began: Instant::now(),
    });
}

pub(crate) fn update_tx(id: u64, sent: u64, total: u64, label: &str) {
    if let Some(x) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        if x.id == id {
            x.sent = sent;
            x.total = total;
            x.label = label.to_string();
        }
    }
}

pub(crate) fn end_tx(id: u64) {
    let mut g = TX.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|x| x.id == id) {
        *g = None;
    }
}

/// トレイのツールチップ・ステータス窓へ出す転送の一行(進行中のみ)
pub fn xfer_line() -> Option<String> {
    if let Ok(g) = RX_DRAG.lock() {
        if let Some((_, total)) = g.as_ref() {
            let total = *total;
            let sent = RX_BYTES.load(Ordering::Relaxed);
            let pct = (sent * 100).checked_div(total).unwrap_or(100);
            return Some(format!("受信中 {pct}%"));
        }
    }
    let g = TX.lock().unwrap_or_else(|e| e.into_inner());
    let x = g.as_ref()?;
    let pct = (x.sent * 100).checked_div(x.total).unwrap_or(100);
    let speed = x.began.elapsed().as_secs_f64().max(0.001);
    Some(format!(
        "転送中 {pct}%・{}/s",
        human_bytes((x.sent as f64 / speed) as u64)
    ))
}

/// Esc キーで進行中の転送を中止する(Esc=ドラッグ中止という両 OS 共通の慣習に合わせる)。
/// 100ms ごとのポーリングで、物理キーと Mac から注入されたキーの両方を拾う。
/// ただし Mac がこの PC を操作中(CONTROLLED)の間だけ反応する: Windows を
/// 直接操作している利用者の Esc(ゲーム・全画面アプリの取消)で転送を
/// 勝手に止めないため
pub(crate) fn spawn_esc_cancel_watcher() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(100));
        let esc = unsafe {
            windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x1B) as u32
                & 0x8000
                != 0
        };
        if !esc {
            continue;
        }
        if !crate::dragdrop::edge::CONTROLLED.load(Ordering::Relaxed) {
            continue; // この PC を直接操作中: 転送とは無関係の Esc とみなす
        }
        // 送信中なら自分の転送を止める
        let tx_id = TX
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|x| x.id);
        if let Some(id) = tx_id {
            println!("[file] Esc で転送を中止します");
            knit_common::xfer::request(id);
        }
        // 受信中の掴みドラッグなら相手(Mac)へ中止を頼む
        let rx = RX_DRAG.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some((id, _)) = rx {
            println!("[drag] Esc で受信ドラッグの中止を Mac へ要求します");
            let _ = send_main_msg(&Msg::DragCancel { id });
        }
    });
}

/// ファイル群の指紋(パス+サイズ)。形式は common::files::key に統一
pub(crate) fn files_key(paths: &[String]) -> String {
    bulk::files_key(paths)
}

/// ファイル群を Mac へ送る(大容量経路。本線の入力・ping を詰まらせない)。
/// フォルダは相手が版 14 以降のときだけ展開して送れる。Esc で中止できる。
/// 呼び出し元がコピー検出のたびにスレッドを起こすため、多重送信はここで弾く
pub(crate) fn send_files_to_mac(paths: &[String]) {
    if TX_BUSY.swap(true, Ordering::Relaxed) {
        println!("[file] win->mac 送信中のため要求を無視しました");
        tray::notify(
            "Knit",
            "前のファイルを転送中のため開始できませんでした。完了後にもう一度お試しください",
        );
        return;
    }
    let guard = TxGuard::new();
    let paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
    // 相手の版が持つ機能(フォルダ=版 14 以降・空フォルダ=版 15 以降)は
    // proto::peer_features に集約した判定を使う
    let f = knit_common::proto::peer_features(dragdrop::edge::PEER_VERSION.load(Ordering::Relaxed));
    let cr = match bulk::collect_with_skips(&paths, false, f.dirs, f.empty_dirs) {
        Ok(e) => e,
        Err(e) => {
            println!("[file] win->mac skip: {e}");
            // ラベル側に実際の値と上限(例: 「展開後 640 件以上(上限 512 件)」)が
            // 入るため、ここでは上限の再掲をしない
            tray::notify(
                "Knit",
                &format!("ファイルを送信できません({})", bulk::send_error_label(&e)),
            );
            return;
        }
    };
    let entries = cr.entries;
    if entries.is_empty() {
        println!("[file] win->mac skip(送れるものが無い: {} 件)", paths.len());
        tray::notify("Knit", "送れるものがありません(フォルダが空か、読み取りできない項目です)");
        return;
    }
    let total = bulk::entries_total(&entries);
    let id = NEXT_TX_ID.fetch_add(1, Ordering::Relaxed) + 1;
    guard.arm(id);
    // 大容量(1GiB 以上)は開始時にも規模を知らせる。相手(Mac)側の保存先の
    // 空き容量を送る前に確認する手がかりにする(小さい転送で通知を増やさない閾値)
    if total >= 1024 * 1024 * 1024 {
        tray::notify(
            "Knit",
            &format!(
                "{} 件・合計 {} を Mac へ送信します(相手の保存先 Downloads/Knit の空き容量をご確認ください)",
                entries.len(),
                human_bytes(total)
            ),
        );
    }
    begin_tx(id, total, &entries[0].name);
    clear_xfer_ack();
    let send = || {
        BULK_LINK.send(|w| {
            bulk::send_entries(
                w,
                &entries,
                false,
                None,
                &mut || knit_common::xfer::take(id),
                &mut |sent, total, name| update_tx(id, sent, total, name),
            )
        })
    };
    // 未接続(NotConnected)は本線再接続直後の bulk 張り直しの窓で起きる。
    // 中止(Esc)は接続の切断として伝播する設計のため、その直後の再送もこの窓に
    // 入りやすい。接続の回復を最大60秒待って再試行し、待ちの間に中止要求が来たら
    // 即座に中断する(呼び出し元はすべてバックグラウンドスレッド)
    let mut r = send();
    {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut attempt = 0;
        while r.as_ref().is_err_and(|e| e.kind() == std::io::ErrorKind::NotConnected)
            && Instant::now() < deadline
        {
            if knit_common::xfer::take(id) {
                r = Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cancelled while waiting for the transfer link",
                ));
                break;
            }
            attempt += 1;
            println!("[file] bulk 経路の再接続を待って再試行します({attempt})");
            std::thread::sleep(Duration::from_millis(2500));
            r = send();
        }
    }
    match &r {
        Ok(rep) => {
            let n = rep.sent;
            println!("[file] win->mac {n} 件送信完了");
            // collect 時(フォルダ走査)と送信時(open 失敗)の両方の読み取り不可を通知に反映
            let mut all_skipped = cr.skipped.clone();
            all_skipped.extend(rep.skipped.iter().cloned());
            let note = skip_note(&all_skipped);
            // 相手の受理結果を待つ(版 15 以降の相手だけ返す)。
            // scope=true は設定による拒否、rejected>0 は相手側での保存失敗
            match wait_xfer_ack(Duration::from_millis(2500)) {
                Some((_, _, true)) => {
                    tray::notify(
                        "Knit",
                        &format!("Mac の設定でファイルの受け取りが拒否されています(相手側アプリの「共有」設定を確認してください){note}"),
                    );
                }
                Some((_, rejected, false)) if rejected > 0 => {
                    tray::notify(
                        "Knit",
                        &format!("{n} 件を Mac へ送信しましたが、{rejected} 件は相手側で保存できませんでした(相手側の通知と受信フォルダを確認してください){note}"),
                    );
                }
                _ => {
                    tray::notify(
                        "Knit",
                        &format!("{n} 件({})を Mac へ送信しました{note}", human_bytes(total)),
                    );
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
            println!("[file] win->mac 送信を中止しました");
            tray::notify("Knit", "ファイル転送を中止しました");
        }
        Err(e) => {
            println!("[file] win->mac 送信失敗: {e}");
            tray::notify(
                "Knit",
                &format!(
                    "Mac へファイルを送れませんでした({}: {e})",
                    bulk::send_error_label(e)
                ),
            );
        }
    }
    // 履歴は実際に送信に回った分だけ(読めずスキップした項目は載せない)。
    // skipped は collect 段(走査)と送信段(open)の両方
    let mut skipped = cr.skipped.clone();
    skipped.extend(r.as_ref().map(|rep| rep.skipped.clone()).unwrap_or_default());
    let recorded: Vec<std::path::PathBuf> = entries
        .iter()
        .map(|e| e.src.clone())
        .filter(|p| !skipped.contains(p))
        .collect();
    if r.is_ok() {
        history_push_files(&recorded, "Windows");
    }
    // end_tx/discard/TX_BUSY の解錠は TxGuard の drop が担う
}

/// 受信側で保存できなかったファイルを通知する(黙って欠けるのを防ぐ)
fn report_failed_files(failed: &[(String, String)]) {
    if failed.is_empty() {
        return;
    }
    let (name, reason) = &failed[0];
    let extra = failed.len().saturating_sub(1);
    let more = if extra > 0 {
        format!(" ほか{extra}件")
    } else {
        String::new()
    };
    tray::notify(
        "Knit",
        &format!(
            "{}件を保存できませんでした({reason}: {name}{more})。保存先(Downloads\\Knit)の空き容量を確認してください",
            failed.len()
        ),
    );
    println!("[file] 保存失敗 {failed:?}");
}

/// 同名衝突で「名前 (n)」として保存した件数の通知用追記。無ければ空文字列
fn rename_note(renamed: usize) -> String {
    if renamed == 0 {
        return String::new();
    }
    format!("。同名の {renamed} 件は「名前 (n)」として保存しました")
}

/// 大容量経路(ファイル・画像)。本線とは別の TCP 接続
pub(crate) static BULK_LINK: bulk::Link = bulk::Link::new();
pub(crate) static BULK: std::sync::OnceLock<bulk::Endpoint> = std::sync::OnceLock::new();

/// 大容量経路の受信完了(Mac からのファイル・画像)。
/// 受け取った結果は XferAck で相手へも知らせる(版 15 以降の相手は
/// 「送ったのに届いていない」ことに気づける)
pub(crate) fn win_on_bulk(e: bulk::Event) {
    match e {
        bulk::Event::Denied { files } => {
            // この端末の共有設定で受け取りを拒否した。相手が誤って
            // 「送信しました」と扱わないよう、結果を知らせる
            println!("[file] この端末の設定で {files} 件の受信を拒否しました");
            // 拒否したバッチで受信中表示(RX_DRAG)を掴んだままにしない。
            // 残ると表示が恒久的に「受信中」のままになり、Esc が存在しない
            // 転送への DragCancel を送ってしまう
            *RX_DRAG.lock().unwrap_or_else(|e| e.into_inner()) = None;
            RX_BYTES.store(0, Ordering::Relaxed);
            let _ = send_main_msg(&Msg::XferAck {
                accepted: 0,
                rejected: files,
                scope: true,
            });
        }
        bulk::Event::Interrupted { saved } => {
            // バッチの途中で切断・中止(Esc 中止・切断の伝播)。保存済みの分が
            // 通知なしで Downloads\Knit に残るのを黙らせない
            println!("[file] 転送が中断されました(保存済み {saved} 件)");
            tray::notify(
                "Knit",
                &format!("転送の途中で中断されました(受信済みの {saved} 件は保存されています。残りは接続が戻ってから再送してください)"),
            );
        }
        bulk::Event::Files {
            paths,
            drop,
            drag_id,
            failed,
            renamed,
            denied,
        } => {
            let _ = send_main_msg(&Msg::XferAck {
                accepted: paths.len(),
                rejected: failed.len(),
                scope: denied > 0,
            });
            // 受信中表示を解く(Esc での中止対象も無くなる)
            *RX_DRAG.lock().unwrap_or_else(|e| e.into_inner()) = None;
            RX_BYTES.store(0, Ordering::Relaxed);
            report_failed_files(&failed);
            if paths.is_empty() {
                return;
            }
            let files: Vec<String> = paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let n = files.len();
            FILES_RX.fetch_add(n as u64, Ordering::Relaxed);
            // 自分が渡す CF_HDROP を Mac へ送り返さない
            *LAST_RECV_FILES.lock().unwrap_or_else(|e| e.into_inner()) = Some(files_key(&files));
            // 受け渡しIDがあれば、越えてきた押下が続いている間だけ開始する。
            // IDのない旧版Macからは、完了時点の押下状態で判断する。
            // 予告(DragOffer)の件数と一致しない転送はドラッグにしない
            use knit_common::drag::{await_claim, Claim};
            let outcome = match drag_id {
                Some(id) => {
                    let claimed = dragdrop::claim(id, n);
                    if matches!(claimed, Claim::Unknown) {
                        // 本線と大容量経路は別 TCP のため、小さい転送では完了が
                        // 予告(DragOffer)より先に届くことがある。押下の受けを待つ窓
                        // (dragdrop::start)と同じ時間だけ予告の遅着を待つ
                        await_claim(dragdrop::CLAIM_WAIT_MS, || dragdrop::claim(id, n))
                    } else {
                        claimed
                    }
                }
                None if drop && BTN_W[0].load(Ordering::Relaxed) => Claim::Carried,
                None => Claim::Released,
            };
            // 予告より受け取れた件数が少ない場合は、その分を通知に含める
            let mut mismatch: Option<(usize, usize)> = None;
            let saved_dir = || {
                std::env::var_os("USERPROFILE")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default()
                    .join("Downloads")
                    .join("Knit")
            };
            match outcome {
                Claim::Carried => {
                    // まだ押している=掴んだまま → 本物の OLE ドラッグを開始
                    dragdrop::start(files);
                    return;
                }
                Claim::Mismatch {
                    expected,
                    received: got,
                } => {
                    println!("[drag] 予告 {expected} 件のうち {got} 件のみ受信のためドラッグにしません");
                    mismatch = Some((expected, got));
                    // 受け取れた分は無駄にしない: このままクリップボードへ載せる
                }
                Claim::Released | Claim::Unknown if drag_id.is_some() => {
                    // 予告のあった転送だが、ドラッグとして扱えない(押下がもう
                    // 続いていない=離した後・Mac へ戻った後・取消後、または予告が
                    // 時間内に届かなかった=別経路での順序入れ替わり)。
                    // 勝手にクリップボードを変えると利用者が直前にコピーした
                    // 内容を消すため、保存先だけ知らせる
                    println!(
                        "[drag] {}ため受信のみ扱い(クリップボードは変えません)",
                        if matches!(outcome, Claim::Unknown) {
                            "予告が届かなかった"
                        } else {
                            "押下が終わっている"
                        }
                    );
                    // 履歴には載せる(メニューから選び直せるように。
                    // Mac の受信も履歴に載るため挙動を揃える)
                    history_push_files(&paths, "Mac");
                    let dir = saved_dir();
                    tray::notify(
                        "Knit",
                        &format!(
                            "ファイルを受信: {n} 件({} へ保存しました){}",
                            dir.display(),
                            rename_note(renamed)
                        ),
                    );
                    return;
                }
                _ => {
                    if drop {
                        println!("[drag] 転送完了時点でボタン非押下のため Ctrl+V 形式へフォールバック");
                    }
                }
            }
            if clipboard_write_files(&files) {
                LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
                history_push_files(&paths, "Mac");
                println!("[file] 受信完了: {n} 件(クリップボードに載せました)");
                let dir = saved_dir();
                let message = match mismatch {
                    Some((expected, got)) => format!(
                        "ファイルは {got}/{expected} 件のみの受信です{}(Ctrl+V で貼り付け可)。実体は {}",
                        rename_note(renamed),
                        dir.display()
                    ),
                    None => format!(
                        "ファイルを受信: {n} 件{}(Ctrl+V で貼り付け可)。実体は {}",
                        rename_note(renamed),
                        dir.display()
                    ),
                };
                tray::notify("Knit", &message);
            } else {
                // クリップボードに載せられなくても沈黙しない。dragdrop::fallback
                // の失敗側と同じ文言で扱いを揃える(実体は Downloads\Knit に残る)
                println!("[drag] 受信ファイルは Downloads\\Knit に保持しています");
                tray::notify(
                    "Knit",
                    "ドロップを開始・完了できませんでした。受信ファイルはDownloads\\Knitに保存されています。掴んだまま境界を越えて、相手の画面上で離すとその場に置けます",
                );
            }
        }
        bulk::Event::Image(dib) => {
            if !CLIP_SHARE_W.load(Ordering::Relaxed) {
                // 黙って捨てない: この端末の設定で受け取らない状態を可視化する
                // (60 秒に 1 回だけ。Mac がクリップボード共有を切っている場合もここに来る)
                static IMG_DENY_MS: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                let now = crate::state::now_ms();
                if now.saturating_sub(IMG_DENY_MS.swap(now, Ordering::Relaxed)) >= 60_000 {
                    tray::notify(
                        "Knit",
                        "このPCの設定でクリップボード共有がオフのため、届いた画像を破棄しました",
                    );
                }
                println!("[clip] このPCの設定で画像の受け取りを拒否しました");
                return;
            }
            let ok = clipboard_write_dib(&dib);
            if ok {
                LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
                history_push_image(&dib, "Mac");
            }
            println!(
                "[clip] mac->win image {}KB {}",
                dib.len() / 1024,
                if ok { "ok" } else { "FAILED" }
            );
        }
    }
}
