use crate::DragQueryFileW;
use crate::state::WTX;
use crate::tray;
use crate::xfer::{BULK_LINK, files_key, send_files_to_mac};
use knit_common::bulk;
use knit_common::proto::{encode, Msg};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// クリップボード
#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(hWndNewOwner: *mut core::ffi::c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(uFormat: u32) -> *mut core::ffi::c_void;
    fn SetClipboardData(uFormat: u32, hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GetClipboardSequenceNumber() -> u32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
}

// ---------- Win32 直宣言(グローバルメモリ) ----------
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> *mut core::ffi::c_void;
    fn GlobalLock(hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalUnlock(hMem: *mut core::ffi::c_void) -> i32;
    fn GlobalFree(hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalSize(hMem: *mut core::ffi::c_void) -> usize;
}

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const GMEM_MOVEABLE: u32 = 0x0002;
/// 最後に Mac から受信して書き込んだテキスト(エコーバック送信防止)
pub(crate) static LAST_RECV_CLIP: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 最後に Mac から受信してクリップボードへ載せたファイル群の指紋(エコーバック防止)
pub(crate) static LAST_RECV_FILES: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub(crate) const CLIP_MAX_CHARS: usize = 1024 * 1024; // 1MB
/// Mac のクリップボード共有設定(Cfg で同期)。OFF の間は Windows からも送らない
pub(crate) static CLIP_SHARE_W: AtomicBool = AtomicBool::new(true);
/// Mac 側のファイル共有設定(Cfg.files の鏡像。版 15 以降)。false の間は
/// Mac へファイルを送らない(届けても捨てられるだけのため)
pub(crate) static FILES_SHARE_W: AtomicBool = AtomicBool::new(true);
/// 最後に Mac と同期したクリップボードのシーケンス番号
pub(crate) static LAST_SYNC_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// ---------- クリップボード履歴(Universal Clipboard History) ----------
/// 送信・受信したテキストの履歴。トレイメニューから選んで復元できる。
/// 機密判定(smartguard)と秘匿指定は送信側で弾くため、履歴へは届かない
pub static HISTORY: std::sync::Mutex<knit_common::history::History> =
    std::sync::Mutex::new(knit_common::history::History::new(50));

/// 履歴の保存先(音声設定と同じユーザープロファイル。配布先の書込み権限に依存しない)
pub fn history_path() -> Option<std::path::PathBuf> {
    knit_common::envutil::data_dir().map(|dir| dir.join("history.json"))
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
    }
}

pub(crate) fn history_push(text: &str, device: &str) {
    let ts = knit_common::history::now_epoch_ms();
    if let Ok(mut h) = HISTORY.lock() {
        if h.push_text(text, device, ts).is_some() {
            drop(h);
            history_save();
        }
    }
}

/// ファイル群を履歴へ載せる(ビジョン§10 の File 分類)。送信・受信の
/// すべてのファイル移動で呼ぶ
pub(crate) fn history_push_files(paths: &[std::path::PathBuf], device: &str) {
    let ts = knit_common::history::now_epoch_ms();
    if let Ok(mut h) = HISTORY.lock() {
        if h.push_files(paths, device, ts).is_some() {
            drop(h);
            history_save();
        }
    }
}

/// 画像履歴の本体を置くディレクトリ(履歴 JSON と同じ %LOCALAPPDATA%\Knit\images)
fn image_store_dir() -> Option<std::path::PathBuf> {
    knit_common::envutil::data_dir().map(|dir| dir.join("images"))
}

/// images/ に残す実体の上限。履歴 cap(50)より少し余裕を持たせた件数
const IMAGE_KEEP: usize = 60;

/// 画像を履歴へ載せる(ビジョン§10 の Image 分類)。本体は CF_DIB の生バイトを
/// 内容ハッシュ名で images/ へ保存し、履歴には「ファイル名\tバイト数」だけ残す
pub(crate) fn history_push_image(dib: &[u8], device: &str) {
    let Some(dir) = image_store_dir() else { return };
    let name = knit_common::history::image_file_name(knit_common::history::fnv1a64(dib), "dib");
    let _ = std::fs::create_dir_all(&dir);
    knit_common::history::restrict_dir(&dir);
    let path = dir.join(&name);
    if !path.exists() {
        // 失敗時は部分書き込みの実体を消す(exists チェックで壊れた実体が
        // 固定化され、復元だけが恒久失敗するのを防ぐ)
        if knit_common::history::write_private_or_remove(&path, dib).is_err() {
            println!("[clip] 画像の履歴保存に失敗しました(ディスク容量等)");
            return;
        }
    } else {
        // 同じ画像の再コピー: 実体は既に有るため書き直さず mtime だけ現在へ
        // 更新する(刈り込みが mtime 順に残すため。Mac 側と対称の修正)
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
        if h.push_image(&name, dib.len(), device, ts).is_some() {
            drop(h);
            history_save();
        }
    }
}

/// 起動時に画像履歴の実体を刈り込む(履歴 load の後に 1 回)。保存時の刈り込みは
/// 画像が届いた時しか走らないため、起動をまたぐと総量超過が残り続くのを防ぐ
pub fn prune_image_store_now() {
    let Some(dir) = image_store_dir() else { return };
    knit_common::history::prune_image_store(
        &dir,
        IMAGE_KEEP,
        knit_common::history::MAX_IMAGE_STORE_BYTES,
    );
}

/// 履歴の全消去(トレイメニュー)。保存ファイルも消して次回起動に残さない
pub fn history_clear() {
    if let Ok(mut h) = HISTORY.lock() {
        h.clear();
    }
    if let Some(path) = history_path() {
        let _ = std::fs::remove_file(path);
    }
    println!("[clip] 履歴を消しました");
}

/// トレイメニューからの履歴復元。クリップボードへ書き戻し、再送の
/// エコー防止のため同期基準を進める。画像の履歴は images/ から CF_DIB の
/// 本体を、ファイル参照の履歴(絶対パスの改行区切り)は CF_HDROP として
/// 載せ直す(Explorer の Ctrl+C 相当)
pub fn history_restore_by_id(id: u64) {
    let Some(entry) = HISTORY.lock().ok().and_then(|h| h.get(id).cloned()) else {
        return;
    };
    // 画像の履歴(ビジョン§10): images/ から CF_DIB の本体を読み戻す
    if entry.kind == knit_common::history::Kind::Image {
        let Some((name, _)) = knit_common::history::parse_image_entry(&entry.text) else {
            return;
        };
        let dib = image_store_dir().and_then(|d| std::fs::read(d.join(&name)).ok());
        let Some(dib) = dib else {
            tray::notify(
                "Knit",
                "履歴の画像が見つからないため復元できませんでした(削除済み)",
            );
            return;
        };
        if clipboard_write_dib(&dib) {
            LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
            history_push_image(&dib, "Windows");
            println!(
                "[clip] 履歴({})から画像を Windows のクリップボードへ復元({}KB)",
                entry.device,
                dib.len() / 1024
            );
        } else {
            tray::notify(
                "Knit",
                "クリップボードを書き込めませんでした(他アプリが使用中。少し待って再試行)",
            );
        }
        return;
    }
    if entry.kind == knit_common::history::Kind::File
        || knit_common::history::looks_like_file_paths(&entry.text)
    {
        let total = entry.text.lines().filter(|l| !l.trim().is_empty()).count();
        let paths: Vec<String> = entry
            .text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && std::path::Path::new(l).exists())
            .collect();
        if paths.is_empty() {
            tray::notify(
                "Knit",
                "履歴のファイルが見つからないため復元できませんでした(移動・削除済み)",
            );
            return;
        }
        let missing = total - paths.len();
        if missing > 0 {
            tray::notify(
                "Knit",
                &format!("履歴のファイル {}/{} 件を復元しました(残り {missing} 件は移動・削除済み)", paths.len(), total),
            );
        }
        if clipboard_write_files(&paths) {
            LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
            *LAST_RECV_FILES.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(crate::xfer::files_key(&paths));
            let pb: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
            history_push_files(&pb, "Windows");
            println!(
                "[clip] 履歴({})からファイル {} 件を復元",
                entry.device,
                paths.len()
            );
        } else {
            tray::notify(
                "Knit",
                "クリップボードを書き込めませんでした(他アプリが使用中。少し待って再試行)",
            );
        }
        return;
    }
    if clipboard_write_text(&entry.text) {
        LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
        // 復元内容を「受信済み」として印を置く(mac の history_restore と同じ扱い)。
        // 印が無いと復元した古いテキストが次の切替で相手へ再送される
        *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = Some(entry.text.clone());
        history_push(&entry.text, "Windows");
        println!(
            "[clip] 履歴({})を Windows のクリップボードへ復元({} bytes)",
            entry.device,
            entry.text.len()
        );
    } else {
        tray::notify(
            "Knit",
            "クリップボードを書き込めませんでした(他アプリが使用中。少し待って再試行)",
        );
    }
}

/// smartguard の通知間引き(誤検知の連打防止。60 秒に 1 回)
static SMART_SECRET_NOTIFY: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
fn smart_secret_notify() {
    let due = {
        let mut g = SMART_SECRET_NOTIFY
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let ok = g
            .map(|t| t.elapsed() >= Duration::from_secs(60))
            .unwrap_or(true);
        if ok {
            *g = Some(Instant::now());
        }
        ok
    };
    if due {
        tray::notify(
            "Knit",
            "クリップボードに機密の可能性があるため Mac へは送りませんでした(履歴にも載りません。KNIT_SMART_SECRET=0 で無効化)",
        );
    }
}

/// 1MB 上限超過テキストの通知間引き(同じ巨大コピーが切替のたびに通知するのを
/// 防ぐ。60 秒に 1 回)。文言は実際の文字数・サイズを含む共通の生成器を使う
static BIG_TEXT_NOTIFY: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
fn big_text_notify(text: &str) {
    let due = {
        let mut g = BIG_TEXT_NOTIFY
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let ok = g
            .map(|t| t.elapsed() >= Duration::from_secs(60))
            .unwrap_or(true);
        if ok {
            *g = Some(Instant::now());
        }
        ok
    };
    if due {
        println!(
            "[clip] win->mac テキストが大きすぎます({} bytes > 上限 1MB)",
            text.len()
        );
        tray::notify(
            "Knit",
            &knit_common::history::too_large_clip_message(text),
        );
    }
}

pub(crate) fn clipboard_seq() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// パスワードマネージャ等が「監視・共有しないで」と付ける登録形式があるか
/// (KeePass/1Password/Bitwarden 等が付ける Windows の慣行)。
/// 形式 ID の登録は不変のため 1 回だけ行いキャッシュする
fn clipboard_is_excluded() -> bool {
    static FMTS: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    let fmts = FMTS.get_or_init(|| {
        [
            "ExcludeClipboardContentFromMonitorProcessing",
            "Clipboard Viewer Ignore",
        ]
        .iter()
        .filter_map(|n| {
            let w: Vec<u16> = n.encode_utf16().chain(std::iter::once(0)).collect();
            let fmt = unsafe { RegisterClipboardFormatW(w.as_ptr()) };
            (fmt != 0).then_some(fmt)
        })
        .collect()
    });
    fmts.iter()
        .any(|&f| unsafe { IsClipboardFormatAvailable(f) } != 0)
}

/// Mac へ制御が戻る時に Windows のクリップボードを渡す(Deskflow と同じ「画面を
/// 離れる時に同期」方式)。旧方式は 200ms ごとに本文と画像全体を読み、画像は毎回
/// base64 化して比較していた(常時の CPU・メモリ負荷。レビュー D-F1)。
/// シーケンス番号が変わっていない限りクリップボードを開きもしない
pub(crate) fn sync_clipboard_to_mac() {
    if !CLIP_SHARE_W.load(Ordering::Relaxed) || !knit_common::share::allow_clip() {
        return;
    }
    let seq = clipboard_seq();
    let prev_seq = LAST_SYNC_SEQ.swap(seq, Ordering::Relaxed);
    if prev_seq == seq {
        return;
    }
    let Some(tx) = WTX.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return;
    };
    std::thread::spawn(move || {
        if clipboard_is_excluded() {
            println!("[clip] 秘匿指定のコピー(パスワード等)のため送りません");
            return;
        }
        if let Some(text) = clipboard_read_text() {
            let echo = LAST_RECV_CLIP
                .lock()
                .map(|g| g.as_deref() == Some(text.as_str()))
                .unwrap_or(false);
            if !text.is_empty() && text.len() <= CLIP_MAX_CHARS && !echo {
                // 実験的ガード: ローカル ollaya が動いていれば機密テキストを検査する。
                // 無し・失敗は None=現行どおり送る。判定はこの同期スレッド内で完結
                if knit_common::smartguard::looks_secret(&text) == Some(true) {
                    println!("[clip] smartguard: 機密の可能性が高いため Mac へ送りません");
                    smart_secret_notify();
                    return;
                }
                println!("[clip] win->mac {} bytes", text.len());
                history_push(&text, "Windows");
                let _ = tx.send(encode(&Msg::Clip { text }));
            } else if !text.is_empty() && text.len() > CLIP_MAX_CHARS {
                // 1MB 超のテキストは Mac へ送れない。黙って落とすと「コピーしたのに
                // 貼り付けられない」状態になるため、サイズ・文字数を伝える
                big_text_notify(&text);
            }
            return;
        }
        if let Some(dib) = clipboard_read_dib() {
            // 画像の前にファイル参照(CF_HDROP)を確認する: 画像とファイル参照の
            // 両形式を載せるコピー(Office 系等)で、Mac 側(text → files → image)
            // と同じものが同期されるように優先度を揃える
            if let Some(files) = clipboard_read_files() {
                send_files_or_note(&files);
                return;
            }
            if dib.len() <= bulk::MAX_IMAGE {
                if crate::xfer::tx_busy() {
                    // ファイル転送中に画像を送ると、bulk の送信口(転送が終わるまで
                    // ロックを保持)を待ってこのスレッドが留まり、転送完了後に古い
                    // 画像が上書いてしまう。Mac 側と同じ Busy 扱いで今回はスキップし、
                    // 同期済みの印を戻して次の同期(シーケンス変化)で再送する
                    LAST_SYNC_SEQ
                        .compare_exchange(seq, prev_seq, Ordering::Relaxed, Ordering::Relaxed)
                        .ok();
                    println!("[clip] 他の転送中のため画像同期を保留します(次の同期で再試行します)");
                    return;
                }
                match BULK_LINK.send(|w| bulk::send_image(w, &dib)) {
                    Ok(()) => {
                        println!("[clip] win->mac image {}KB", dib.len() / 1024);
                        history_push_image(&dib, "Windows");
                    }
                    Err(e) => println!("[clip] win->mac image 送信失敗: {e}"),
                }
            }
            return;
        }
        if let Some(files) = clipboard_read_files() {
            send_files_or_note(&files);
        }
    });
}

/// CF_HDROP の内容を Mac へ送る(Mac 側がファイル共有を切っているときは
/// 空振りさせず、この Windows の通知で伝える)
fn send_files_or_note(files: &[String]) {
    let key = files_key(files);
    let echo = LAST_RECV_FILES
        .lock()
        .map(|g| g.as_deref() == Some(key.as_str()))
        .unwrap_or(false);
    if echo {
        return;
    }
    if !FILES_SHARE_W.load(Ordering::Relaxed) {
        println!("[file] Mac 側でファイル共有がオフのため送りません");
        tray::notify("Knit", "Mac 側の設定でファイルの受け渡しがオフのため、コピーしたファイルは送りませんでした");
        return;
    }
    println!("[file] CF_HDROP: {} files", files.len());
    send_files_to_mac(files);
}

fn clipboard_read_text() -> Option<String> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None; // 他プロセス占有中(次回ポーリングで再試行)
        }
        let h = GetClipboardData(CF_UNICODETEXT);
        let mut locked = false;
        let out = if h.is_null() {
            None // テキスト形式ではない(画像等)
        } else {
            let p = GlobalLock(h) as *const u16;
            if p.is_null() {
                None
            } else {
                locked = true;
                let mut len = 0usize;
                while *p.add(len) != 0 && len < CLIP_MAX_CHARS {
                    len += 1;
                }
                Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
            }
        };
        if locked {
            GlobalUnlock(h);
        }
        CloseClipboard();
        out
    }
}

/// クリップボードから画像(CF_DIB)の生バイトを読む(Windows→Mac 画像同期用)
fn clipboard_read_dib() -> Option<Vec<u8>> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_DIB);
        let out = if h.is_null() {
            None
        } else {
            let size = GlobalSize(h);
            let p = GlobalLock(h) as *const u8;
            if p.is_null() || size == 0 {
                None
            } else {
                Some(std::slice::from_raw_parts(p, size).to_vec())
            }
        };
        if !h.is_null() {
            GlobalUnlock(h);
        }
        CloseClipboard();
        out
    }
}

/// 直近の OpenClipboard 失敗の GetLastError(診断用。5=ACCESS_DENIED は
/// 他プロセスが開いている=「busy clipboard」の正体)
pub(crate) static CLIP_OPEN_ERR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn clipboard_write_text(s: &str) -> bool {
    let mut utf16: Vec<u16> = s.encode_utf16().collect();
    utf16.push(0);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            CLIP_OPEN_ERR.store(
                windows_sys::Win32::Foundation::GetLastError(),
                Ordering::Relaxed,
            );
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, utf16.len() * 2);
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        let p = GlobalLock(h) as *mut u16;
        if p.is_null() {
            GlobalFree(h);
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), p, utf16.len());
        GlobalUnlock(h);
        if SetClipboardData(CF_UNICODETEXT, h).is_null() {
            GlobalFree(h); // 設定失敗時は呼び出し側の解放責任
            CloseClipboard();
            return false;
        }
        CloseClipboard();
        true
    }
}

/// ファイルパス群を CF_HDROP の HGLOBAL へパックする(クリップボード操作を
/// 含まない純粋な生成。ドラッグ越境の IDataObject::GetData でも使う)。
/// DROPFILES ヘッダ(20byte): pFiles=20, pt=(0,0), fNC=0, fWide=1 の後ろに
/// UTF16 パス群(\0 区切り、リスト終端に追加の \0)が続く
pub(crate) fn make_hdrop_global(paths: &[String]) -> *mut core::ffi::c_void {
    const HEAD: usize = 20;
    let mut w: Vec<u16> = Vec::new();
    for p in paths {
        w.extend(p.encode_utf16());
        w.push(0);
    }
    w.push(0); // リスト終端
    let total = HEAD + w.len() * 2;
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, total);
        if h.is_null() {
            return std::ptr::null_mut();
        }
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            GlobalFree(h);
            return std::ptr::null_mut();
        }
        std::ptr::write_bytes(p, 0, total);
        let buf = std::slice::from_raw_parts_mut(p, total);
        buf[0..4].copy_from_slice(&(HEAD as u32).to_le_bytes()); // pFiles
        buf[16..20].copy_from_slice(&1i32.to_le_bytes()); // fWide = UTF16
        std::ptr::copy_nonoverlapping(w.as_ptr(), p.add(HEAD) as *mut u16, w.len());
        GlobalUnlock(h);
        h
    }
}

/// ファイルパス群をクリップボードへ(CF_HDROP)。Mac からのファイル受信完了時に
/// 呼ぶ。受け取ったファイルは Windows 側でそのまま Ctrl+V で貼り付けられる
pub(crate) fn clipboard_write_files(paths: &[String]) -> bool {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = make_hdrop_global(paths);
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        if SetClipboardData(CF_HDROP, h).is_null() {
            GlobalFree(h); // 設定失敗時は呼び出し側の解放責任
            CloseClipboard();
            return false;
        }
        CloseClipboard();
        true
    }
}

/// クリップボードからファイル参照(CF_HDROP)のパス群を読む。
/// エクスプローラーでファイルをコピー(Ctrl+C)した際に載る形式
fn clipboard_read_files() -> Option<Vec<String>> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let mut out = None;
        let h = GetClipboardData(CF_HDROP);
        if !h.is_null() {
            const COUNT: u32 = 0xFFFF_FFFF; // iFile=-1 で個数問い合わせ
            let n = DragQueryFileW(h, COUNT, std::ptr::null_mut(), 0);
            if n > 64 {
                println!("[clip] CF_HDROP {n} 件のうち先頭 64 件のみ扱います");
                tray::notify(
                    "Knit",
                    &format!("一度に同期できるのは 64 件までです({n} 件のうち先頭 64 件のみ送ります)"),
                );
            }
            let mut v = Vec::new();
            for i in 0..n.min(64) {
                let len = DragQueryFileW(h, i, std::ptr::null_mut(), 0);
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u16; len as usize + 1];
                DragQueryFileW(h, i, buf.as_mut_ptr(), buf.len() as u32);
                v.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
            if !v.is_empty() {
                out = Some(v);
            }
        }
        CloseClipboard();
        out
    }
}

/// 画像を CF_DIB としてクリップボードへ(Mac からの画像受信)
pub(crate) fn clipboard_write_dib(dib: &[u8]) -> bool {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, dib.len());
        let p = if h.is_null() {
            std::ptr::null_mut()
        } else {
            GlobalLock(h) as *mut u8
        };
        if p.is_null() {
            if !h.is_null() {
                GlobalFree(h);
            }
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(dib.as_ptr(), p, dib.len());
        GlobalUnlock(h);
        if SetClipboardData(CF_DIB, h).is_null() {
            GlobalFree(h);
            CloseClipboard();
            return false;
        }
        CloseClipboard();
        true
    }
}
