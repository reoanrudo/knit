// tsunagu-mac: Mac 側クライアント。CGEventTap で入力を横流しし、Windows へ送信する。
// 画面右端でカーソルが Mac→Windows 切替、Windows カーソル左端(または F13)で復帰。
#![allow(non_camel_case_types)]

mod audio;
mod file_drag;
mod gui;
mod incoming_drag;

use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tsunagu_common::proto::{compatible, decode, encode, safe_peer_name, Msg, PORT, VERSION};
use tsunagu_common::{bulk, envutil, secure};

// ---------- CoreGraphics C API 直宣言 ----------
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGSize {
    pub w: f64,
    pub h: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

type CGEventRef = *mut core::ffi::c_void;
type CFMachPortRef = *mut core::ffi::c_void;
type CFRunLoopRef = *mut core::ffi::c_void;
type CFRunLoopSourceRef = *mut core::ffi::c_void;
type CFAllocatorRef = *mut core::ffi::c_void;
type CFStringRef = *mut core::ffi::c_void;
type CGEventMask = u64;
type CGEventFlags = u64;

// EventType 定数(SDK ヘッダ実測値)
const EVT_LEFT_DOWN: u32 = 1;
const EVT_LEFT_UP: u32 = 2;
const EVT_RIGHT_DOWN: u32 = 3;
const EVT_RIGHT_UP: u32 = 4;
const EVT_MOUSE_MOVED: u32 = 5;
const EVT_LEFT_DRAGGED: u32 = 6;
const EVT_RIGHT_DRAGGED: u32 = 7;
const EVT_KEY_DOWN: u32 = 10;
const EVT_KEY_UP: u32 = 11;
const EVT_FLAGS_CHANGED: u32 = 12;
/// NSSystemDefined(F 行のメディアキー・輝度等)。key イベントとして届かない
/// ため、Windows モードではここから翻訳する(実績: F5 が届かなかった)
const EVT_SYSTEM_DEFINED: u32 = 14;
const EVT_SCROLL_WHEEL: u32 = 22;
const EVT_OTHER_DOWN: u32 = 25;
const EVT_OTHER_UP: u32 = 26;
const EVT_OTHER_DRAGGED: u32 = 27;

// field 定数(SDK ヘッダ実測値)
const FIELD_DELTA_X: i32 = 4;
const FIELD_DELTA_Y: i32 = 5;
const FIELD_KEYCODE: i32 = 9;
const FIELD_SCROLL_A1: i32 = 96; // PointDeltaAxis1(縦, ピクセル)
const FIELD_SCROLL_A2: i32 = 97; // PointDeltaAxis2(横, ピクセル)

// flags(IOLLEvent.h 実測値)
const FLAG_SHIFT: CGEventFlags = 0x0002_0000;
const FLAG_CTRL: CGEventFlags = 0x0004_0000;
const FLAG_OPT: CGEventFlags = 0x0008_0000;
const FLAG_CMD: CGEventFlags = 0x0010_0000;
const FLAG_FN: CGEventFlags = 0x8000_0000; // kCGEventFlagMaskSecondaryFn

const KC_F13: i64 = 105;
/// 切替ホットキー(Mac keycode)。TSUNAGU_HOTKEY_KC で変更可。
/// MacBook 内蔵キーボードには F13 が無いため、例えば右Cmd(54)等に変えられる
static HOTKEY_KC: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(KC_F13);

fn hotkey_kc() -> i64 {
    HOTKEY_KC.load(Ordering::Relaxed)
}

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: i32,
        place: i32,
        options: u32,
        events_of_interest: CGEventMask,
        callback: unsafe extern "C" fn(
            proxy: *mut core::ffi::c_void,
            event_type: u32,
            event: CGEventRef,
            user_info: *mut core::ffi::c_void,
        ) -> CGEventRef,
        user_info: *mut core::ffi::c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: i32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayRegisterReconfigurationCallback(
        callback: unsafe extern "C" fn(u32, u32, *mut core::ffi::c_void),
        user_info: *mut core::ffi::c_void,
    ) -> i32;
    fn CGWarpMouseCursorPosition(new: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connect: bool) -> i32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGSetLocalEventsSuppressionInterval(seconds: f64) -> i32;
    fn CGEventCreate(allocator: CFAllocatorRef) -> CGEventRef;
    fn CGEventCreateMouseEvent(
        source: CFAllocatorRef,
        mouse_type: u32,
        mouse_position: CGPoint,
        button: u64,
    ) -> CGEventRef;
    fn CGEventSetIntegerValueField(event: CGEventRef, field: i32, value: i64);
    fn CGEventPost(tap: i32, event: CGEventRef);
    fn CFRelease(cf: *mut core::ffi::c_void);
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const core::ffi::c_char,
        encoding: u32,
    ) -> CFStringRef;
    static kCFBooleanTrue: *const core::ffi::c_void;
    // Deskflow hideCursor/showCursor が使う非公開 CGS API(カーソル非表示の安定化)
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(
        cid: i32,
        target_cid: i32,
        key: CFStringRef,
        value: *const core::ffi::c_void,
    ) -> i32;
    fn CFMachPortCreateRunLoopSource(
        alloc: CFAllocatorRef,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopGetMain() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: CFStringRef;
}

// ---------- ObjC ランタイム直宣言(NSPasteboard 操作) ----------
// NSPasteboard は AppKit のクラスのため、リンクしてクラス登録を発生させる必要がある
#[link(name = "AppKit", kind = "framework")]
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    fn objc_getClass(name: *const core::ffi::c_char) -> *mut core::ffi::c_void;
    fn sel_registerName(name: *const core::ffi::c_char) -> *mut core::ffi::c_void;
    fn objc_msgSend(
        receiver: *mut core::ffi::c_void,
        sel: *mut core::ffi::c_void,
        ...
    ) -> *mut core::ffi::c_void;
    /// ファイル URL のみを読む指定キー(readObjectsForClasses:options: 用)
    static NSPasteboardURLReadingFileURLsOnlyKey: *mut core::ffi::c_void;
    fn objc_autoreleasePoolPush() -> *mut core::ffi::c_void;
    fn objc_autoreleasePoolPop(pool: *mut core::ffi::c_void);
}

/// バックグラウンドスレッドで ObjC の一時オブジェクトを扱う区間を囲む。
/// メインスレッド以外には autorelease pool が無く、nsstring 等が解放されずに
/// 溜まり続ける(120ms 周期の監視で確定的にリークしていた。レビュー B-P1-1)
fn with_pool<T>(f: impl FnOnce() -> T) -> T {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let r = f();
        objc_autoreleasePoolPop(pool);
        r
    }
}

/// パスワードマネージャ等が「記録・共有しないで」と印を付けたコピーか
/// (nspasteboard.org の慣行。1Password/Bitwarden/キーチェーン等が付ける)
unsafe fn pb_is_concealed(pb: ID) -> bool {
    let types = msg0(pb, sel_registerName(c"types".as_ptr()));
    if types.is_null() {
        return false;
    }
    [
        "org.nspasteboard.ConcealedType",
        "org.nspasteboard.TransientType",
        "com.agilebits.onepassword",
    ]
    .iter()
    .any(|t| {
        let f: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            types,
            sel_registerName(c"containsObject:".as_ptr()),
            nsstring(t),
        ) != 0
    })
}

/// 最後に Windows と同期したクリップボードの changeCount
static LAST_SYNC_COUNT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);

// ---------- クリップボード履歴(Universal Clipboard History) ----------
/// 送信・受信したテキストの履歴。メニューバーから選んで Mac のクリップボードへ
/// 復元できる。機密判定(smartguard)と秘匿指定は送信側で弾くため、履歴へは届かない
pub static HISTORY: Mutex<tsunagu_common::history::History> =
    Mutex::new(tsunagu_common::history::History::new(50));
/// GUI の履歴メニュー再構築用の世代(push で更新、gui.rs が変化検知に使う)
pub static HISTORY_LAST_ID: AtomicU64 = AtomicU64::new(0);
/// 接続相手の名前(hello で受け取る)。通知へ出す
pub static PEER_NAME: Mutex<String> = Mutex::new(String::new());

/// 履歴の保存先(env と同じ ~/.config/tsunagu/)。アプリバンドルには書かない
pub fn history_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(|home| std::path::Path::new(&home).join(".config/tsunagu/history.json"))
}

pub fn history_save() {
    let Some(path) = history_path() else { return };
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

fn history_push(text: &str, device: &str) {
    let ts = tsunagu_common::history::now_epoch_ms();
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
fn history_push_files(paths: &[std::path::PathBuf], device: &str) {
    let ts = tsunagu_common::history::now_epoch_ms();
    if let Ok(mut h) = HISTORY.lock() {
        if h.push_files(paths, device, ts).is_some() {
            HISTORY_LAST_ID.store(h.last_id(), Ordering::Relaxed);
            drop(h);
            history_save();
        }
    }
}

/// 画像履歴の本体を置くディレクトリ(env と同じ ~/.config/tsunagu/images/)
fn image_store_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config/tsunagu/images"))
}

/// images/ に残す実体の上限。履歴 cap(50)より少し余裕を持たせた件数
const IMAGE_KEEP: usize = 60;

/// 画像を履歴へ載せる(ビジョン§10 の Image 分類)。本体は BMP バイトを
/// 内容ハッシュ名で images/ へ保存し、履歴には「ファイル名\tバイト数」だけ
/// 残す。同名=同一内容のため二重保存は起きない
fn history_push_image(bmp: &[u8], device: &str) {
    let Some(dir) = image_store_dir() else { return };
    let name =
        tsunagu_common::history::image_file_name(tsunagu_common::history::fnv1a64(bmp), "bmp");
    let _ = std::fs::create_dir_all(&dir);
    tsunagu_common::history::restrict_dir(&dir);
    let path = dir.join(&name);
    if !path.exists() {
        if tsunagu_common::history::write_private(&path, bmp).is_err() {
            eprintln!("[clip] 画像の履歴保存に失敗しました(ディスク容量等)");
            return;
        }
        tsunagu_common::history::prune_image_store(&dir, IMAGE_KEEP);
    }
    let ts = tsunagu_common::history::now_epoch_ms();
    if let Ok(mut h) = HISTORY.lock() {
        if h.push_image(&name, bmp.len(), device, ts).is_some() {
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
    if let Some((name, _size)) = tsunagu_common::history::parse_image_entry(&text) {
        let path = image_store_dir().map(|d| d.join(&name));
        let bmp = path.and_then(|p| std::fs::read(p).ok());
        let Some(bmp) = bmp else {
            eprintln!("[clip] 履歴の画像本体が見つからないため復元しません");
            notify(
                "tsunagu",
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
        }
        return;
    }
    if tsunagu_common::history::looks_like_file_paths(&text) {
        let paths: Vec<std::path::PathBuf> = text
            .lines()
            .map(|l| std::path::PathBuf::from(l.trim()))
            .collect();
        if !paths.iter().all(|p| p.exists()) {
            eprintln!("[clip] 履歴のファイル参照の一部が見つからないため復元しません");
            notify(
                "tsunagu",
                "履歴のファイルが見つからないため復元できませんでした(移動・削除済み)",
            );
            return;
        }
        let ok = with_pool(|| unsafe { mac_clipboard_write_files(&paths) });
        if ok {
            *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = None;
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            history_push_files(&paths, "Mac");
            eprintln!(
                "[clip] 履歴から Mac のクリップボードへファイル {} 件を復元",
                paths.len()
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
    }
}

/// Mac の最前面アプリ名(NSWorkspace。権限不要)
unsafe fn mac_frontmost_app_name() -> Option<String> {
    let ws = msg0(
        objc_getClass(c"NSWorkspace".as_ptr()),
        sel_registerName(c"sharedWorkspace".as_ptr()),
    );
    if ws.is_null() {
        return None;
    }
    let app = msg0(ws, sel_registerName(c"frontmostApplication".as_ptr()));
    if app.is_null() {
        return None;
    }
    let name = msg0(app, sel_registerName(c"localizedName".as_ptr()));
    if name.is_null() {
        return None;
    }
    let utf8 = msg0_cstr(name, sel_registerName(c"UTF8String".as_ptr()));
    if utf8.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned(),
    )
}

/// 最前面アプリと Windows のアプリ列挙を名前で照合する(越境 App Handoff 用)。
/// 完全一致(大小無視)を優先し、次に Windows 側名前の部分一致(Mac 名が
/// 4 文字以上のときだけ。短い名前の誤爆を防ぐ下限)
pub(crate) fn app_handoff_match<'a>(
    mac_name: &str,
    win_apps: &'a [(String, String)],
) -> Option<&'a (String, String)> {
    let mac = mac_name.trim().to_lowercase();
    if mac.is_empty() {
        return None;
    }
    if let Some(hit) = win_apps
        .iter()
        .find(|(n, _)| n.trim().to_lowercase() == mac)
    {
        return Some(hit);
    }
    if mac.chars().count() >= 4 {
        if let Some(hit) = win_apps
            .iter()
            .find(|(n, _)| n.trim().to_lowercase().contains(&mac))
        {
            return Some(hit);
        }
    }
    None
}

/// Windows へ入る時の App Handoff: 最前面アプリを相手でも開く。
/// クリップボード同期とは独立した実験的機能
fn try_app_handoff() {
    if !APP_HANDOFF.load(Ordering::Relaxed) || !CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    let name = with_pool(|| unsafe { mac_frontmost_app_name() }).unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let win_apps = WIN_APPS.lock().map(|a| a.clone()).unwrap_or_default();
    if win_apps.is_empty() {
        // 検索窓を一度も開いていなければ列挙が無い。要求だけ投げておく
        eprintln!("[handoff] Windows のアプリ一覧が未取得のため照合を飛ばします");
        let _ = send_msg(&Msg::AppsQuery);
        return;
    }
    match app_handoff_match(&name, &win_apps) {
        Some((win_name, path)) => {
            eprintln!("[handoff] Mac 最前面「{name}」→ Windows「{win_name}」を起動します");
            let _ = send_msg_reported(&Msg::RunApp { path: path.clone() });
        }
        None => eprintln!("[handoff] Mac 最前面「{name}」に対応する Windows アプリはありません"),
    }
}

/// Windows へ入る時に Mac のクリップボードを渡す(Deskflow と同じ「画面を離れる時に
/// 同期」方式)。コピーのたびに送る旧方式は、Mac 内だけのコピペでも最大 200MB の
/// ファイルを流し、パスワード等も即座に相手へ渡っていた
fn sync_clipboard_to_win() {
    try_app_handoff();
    if !CLIP_SHARE.load(Ordering::Relaxed) || !CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    // 貼り付け元アプリの遅延提供データ読み出しでタップを止めないよう別スレッドで行う
    std::thread::spawn(move || {
        with_pool(|| unsafe {
            let cnt = clipboard_change_count();
            if LAST_SYNC_COUNT.swap(cnt, Ordering::Relaxed) == cnt {
                return;
            }
            if pb_is_concealed(general_pasteboard()) {
                eprintln!("[clip] 秘匿指定のコピー(パスワード等)のため送りません");
                return;
            }
            let text = mac_get_clipboard().filter(|t| !t.is_empty() && t.len() <= CLIP_MAX_BYTES);
            let Some(text) = text else {
                if let Some(files) = mac_clipboard_files() {
                    let key = mac_files_key(&files);
                    let dup = *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) == key;
                    if !dup {
                        *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = key;
                        eprintln!(
                            "[file] クリップボードのファイル {} 件を渡します",
                            files.len()
                        );
                        history_push_files(&files, "Mac");
                        send_files_to_win(files, false);
                    }
                } else if now_ms().saturating_sub(LAST_IMG_RX_MS.load(Ordering::Relaxed)) < 1_000 {
                    // 受信画像の載せ直後に来た同期: 送り返しの恐れがあるため見送る
                    eprintln!("[clip] 画像受信直後のため同期を控えます");
                    return;
                } else if let Some(dib) = mac_clipboard_image_dib() {
                    match BULK_LINK.send(|w| bulk::send_image(w, &dib)) {
                        Ok(()) => {
                            eprintln!("[clip] mac->win image {}KB", dib.len() / 1024);
                            history_push_image(&dib_to_bmp(&dib), "Mac");
                        }
                        Err(e) => eprintln!("[clip] mac->win image 送信失敗: {e}"),
                    }
                }
                return;
            };
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
            if tsunagu_common::smartguard::looks_secret(&text) == Some(true) {
                eprintln!("[clip] smartguard: 機密の可能性が高いため Windows へ送りません");
                smart_secret_notify("Windows へは送りませんでした");
                return;
            }
            eprintln!("[clip] mac->win {} bytes", text.len());
            history_push(&text, "Mac");
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
        "tsunagu",
        &format!("クリップボードに機密の可能性があるため {detail}、履歴にも載せません(TSUNAGU_SMART_SECRET=0 で無効化)"),
    );
}

/// 最後に Windows から受信して書き込んだテキスト(エコーバック送信防止)
static LAST_RECV_CLIP: Mutex<Option<String>> = Mutex::new(None);
/// 最後の画像受信時刻(ms)。受信画像のクリップボード載せ→changeCount 更新の
/// 間に切替同期が走ると画像を送り返してしまう(最大 64MB の無駄転送)ため、
/// 直近の受信では画像送信を 1 回控える(changeCount 保護の二重ガード)
static LAST_IMG_RX_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const CLIP_MAX_BYTES: usize = 1024 * 1024; // 1MB(Win側と同じ上限)

type ID = *mut core::ffi::c_void;
type SEL = *mut core::ffi::c_void;

// objc_msgSend は可変引数宣言のまま呼ぶと引数の渡りが壊れる(SIGSEGV実績あり)ため、
// 呼び出しシグネチャごとに transmute した固定シグネチャで呼ぶ(rust-objc 界の定番方式)
unsafe fn msg0(target: ID, sel: SEL) -> ID {
    let f: unsafe extern "C" fn(ID, SEL) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}
unsafe fn msg1_id(target: ID, sel: SEL, a: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, a)
}
unsafe fn msg1_cstr(target: ID, sel: SEL, p: *const core::ffi::c_char) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, *const core::ffi::c_char) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, p)
}
unsafe fn msg2_bool(target: ID, sel: SEL, a: ID, b: ID) -> u8 {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, a, b)
}
unsafe fn msg0_isize(target: ID, sel: SEL) -> isize {
    let f: unsafe extern "C" fn(ID, SEL) -> isize =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}
unsafe fn msg0_cstr(target: ID, sel: SEL) -> *const core::ffi::c_char {
    let f: unsafe extern "C" fn(ID, SEL) -> *const core::ffi::c_char =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}

unsafe fn nsstring(s: &str) -> ID {
    let mut buf = s.as_bytes().to_vec();
    buf.push(0);
    msg1_cstr(
        objc_getClass(c"NSString".as_ptr()),
        sel_registerName(c"stringWithUTF8String:".as_ptr()),
        buf.as_ptr() as *const core::ffi::c_char,
    )
}

unsafe fn general_pasteboard() -> ID {
    msg0(
        objc_getClass(c"NSPasteboard".as_ptr()),
        sel_registerName(c"generalPasteboard".as_ptr()),
    )
}

/// ドラッグ用ペーストボード(NSPasteboardNameDrag = "Apple CFPasteboard drag")。
/// Finder 等のファイルドラッグ中のみファイル URL が載る。セレクタは
/// pasteboardWithName:(クラスメソッド)。NSPasteboardNameDrag 定数の実体文字列を直接渡す
unsafe fn drag_pasteboard() -> ID {
    let name = nsstring("Apple CFPasteboard drag");
    msg1_id(
        objc_getClass(c"NSPasteboard".as_ptr()),
        sel_registerName(c"pasteboardWithName:".as_ptr()),
        name,
    )
}

/// NSPasteboard へテキストを書き込む(Windows→Mac 受信時)
unsafe fn mac_set_clipboard(text: &str) -> bool {
    let pb = general_pasteboard();
    if pb.is_null() {
        eprintln!("[clip] set: pasteboard=null");
        return false;
    }
    msg0(pb, sel_registerName(c"clearContents".as_ptr()));
    let s = nsstring(text);
    let uti = nsstring("public.utf8-plain-text");
    let ok = msg2_bool(pb, sel_registerName(c"setString:forType:".as_ptr()), s, uti);
    if ok == 0 {
        eprintln!(
            "[clip] set failed: str={} uti={}",
            !s.is_null(),
            !uti.is_null()
        );
    }
    ok != 0
}

/// DIB(Windows 画像)に BMP ファイルヘッダを付与して BMP データへ変換する。
/// NSBitmapImageRep は BMP ファイル形式を受け付けるため
fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    if dib.len() < 40 {
        return Vec::new();
    }
    // BITMAPINFOHEADER: biSize は先頭4バイト(旧実装は誤って biWidth を読んでいた)
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let bpp = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let comp = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
    let clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize;
    let palette = if clr_used > 0 {
        clr_used * 4
    } else if bpp == 8 {
        1024
    } else {
        0
    };
    // BI_BITFIELDS(biCompression=3) が biSize=40 で来た場合だけ、ヘッダ直後に
    // 12 バイトのカラーマスクが付く(Windows のクリップボードが実際に出す形。
    // これを飛ばさないと画像全体が 3px ずれる。biSize>=52 はマスク込みのサイズ)
    let masks = if comp == 3 && header_size == 40 {
        12
    } else {
        0
    };
    let off = 14 + header_size + palette + masks;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(off as u32).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// BMP 画像を Mac のクリップボードへ TIFF として書き込む(Windows→Mac 画像同期)
unsafe fn mac_set_clipboard_image_bmp(bmp: &[u8]) -> bool {
    let pb = general_pasteboard();
    if pb.is_null() || bmp.is_empty() {
        return false;
    }
    // NSData dataWithBytes:length:
    let data = {
        let f: unsafe extern "C" fn(ID, SEL, *const u8, usize) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            objc_getClass(c"NSData".as_ptr()),
            sel_registerName(c"dataWithBytes:length:".as_ptr()),
            bmp.as_ptr(),
            bmp.len(),
        )
    };
    if data.is_null() {
        eprintln!("[clip] image FAILED at NSData");
        return false;
    }
    // NSBitmapImageRep imageRepWithData:
    let rep = {
        let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            objc_getClass(c"NSBitmapImageRep".as_ptr()),
            sel_registerName(c"imageRepWithData:".as_ptr()),
            data,
        )
    };
    if rep.is_null() {
        eprintln!("[clip] image FAILED at imageRepWithData (BMP 不整合の可能性)");
        return false;
    }
    // [rep TIFFRepresentation]
    let tiff = msg0(rep, sel_registerName(c"TIFFRepresentation".as_ptr()));
    if tiff.is_null() {
        eprintln!("[clip] image FAILED at TIFFRepresentation");
        return false;
    }
    msg0(pb, sel_registerName(c"clearContents".as_ptr()));
    let uti = nsstring("public.tiff");
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let ok = f(
        pb,
        sel_registerName(c"setData:forType:".as_ptr()),
        tiff,
        uti,
    );
    if ok == 0 {
        eprintln!(
            "[clip] image FAILED at setData (data={} rep={} tiff={} bmp={}B)",
            !data.is_null(),
            !rep.is_null(),
            !tiff.is_null(),
            bmp.len()
        );
    }
    ok != 0
}

/// NSPasteboard からテキストを読む(Mac→Windows 送信時)
unsafe fn mac_get_clipboard() -> Option<String> {
    let pb = general_pasteboard();
    if pb.is_null() {
        return None;
    }
    let uti = nsstring("public.utf8-plain-text");
    let s = msg1_id(pb, sel_registerName(c"stringForType:".as_ptr()), uti);
    if s.is_null() {
        return None;
    }
    let utf8 = msg0_cstr(s, sel_registerName(c"UTF8String".as_ptr()));
    if utf8.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned(),
    )
}

fn clipboard_change_count() -> isize {
    unsafe {
        msg0_isize(
            general_pasteboard(),
            sel_registerName(c"changeCount".as_ptr()),
        )
    }
}

/// NSPasteboard にファイル参照(Finder の ⌘C 等)があるか調べ、パス群を返す。
/// readObjectsForClasses:options:(NSURL + FileURLsOnly=YES)で読むことで
/// NSFilenamesPboardType / public.file-url / alias のどの載せ方でも拾う
unsafe fn pb_files(pb: ID) -> Option<Vec<std::path::PathBuf>> {
    if pb.is_null() {
        return None;
    }
    let url_cls = objc_getClass(c"NSURL".as_ptr());
    if url_cls.is_null() {
        return None;
    }
    let classes = {
        let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            objc_getClass(c"NSArray".as_ptr()),
            sel_registerName(c"arrayWithObject:".as_ptr()),
            url_cls,
        )
    };
    let options = {
        let yes = {
            let f: unsafe extern "C" fn(ID, SEL, u8) -> ID =
                std::mem::transmute(objc_msgSend as *const () as usize);
            f(
                objc_getClass(c"NSNumber".as_ptr()),
                sel_registerName(c"numberWithBool:".as_ptr()),
                1,
            )
        };
        let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            objc_getClass(c"NSDictionary".as_ptr()),
            sel_registerName(c"dictionaryWithObject:forKey:".as_ptr()),
            yes,
            NSPasteboardURLReadingFileURLsOnlyKey,
        )
    };
    let urls = {
        let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            pb,
            sel_registerName(c"readObjectsForClasses:options:".as_ptr()),
            classes,
            options,
        )
    };
    if urls.is_null() {
        return None;
    }
    let n = msg0_isize(urls, sel_registerName(c"count".as_ptr()));
    if n <= 0 {
        return None;
    }
    // 65 件目以降は無通知で欠けるため打ち切りをログへ残す
    if n > 64 {
        eprintln!("[clip] ファイル参照 {n} 件のうち先頭 64 件のみ扱います");
    }
    let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let mut out = Vec::new();
    for i in 0..n.min(64) {
        let url = at(
            urls,
            sel_registerName(c"objectAtIndex:".as_ptr()),
            i as usize,
        );
        if url.is_null() {
            continue;
        }
        let path = msg0(url, sel_registerName(c"path".as_ptr()));
        if path.is_null() {
            continue;
        }
        let utf8 = msg0_cstr(path, sel_registerName(c"UTF8String".as_ptr()));
        if utf8.is_null() {
            continue;
        }
        let s = std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned();
        if !s.is_empty() {
            out.push(std::path::PathBuf::from(s));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// general クリップボードのファイル参照(⌘C 検出用の既存経路)
unsafe fn mac_clipboard_files() -> Option<Vec<std::path::PathBuf>> {
    pb_files(general_pasteboard())
}

/// NSPasteboard へファイル参照を書き込む(Finder の ⌘C 相当)。
/// Windows からのファイル受信完了時に呼ぶ。writeObjects: が file URL を
/// 載せるため、Finder への ⌘V がそのまま動く
unsafe fn mac_clipboard_write_files(paths: &[std::path::PathBuf]) -> bool {
    let pb = general_pasteboard();
    if pb.is_null() || paths.is_empty() {
        return false;
    }
    let url_cls = objc_getClass(c"NSURL".as_ptr());
    if url_cls.is_null() {
        return false;
    }
    let make_url: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let mut urls = Vec::new();
    for p in paths {
        let s = nsstring(&p.to_string_lossy());
        if s.is_null() {
            continue;
        }
        let url = make_url(url_cls, sel_registerName(c"fileURLWithPath:".as_ptr()), s);
        if !url.is_null() {
            urls.push(url);
        }
    }
    if urls.is_empty() {
        return false;
    }
    msg0(pb, sel_registerName(c"clearContents".as_ptr()));
    let make_arr: unsafe extern "C" fn(ID, SEL, *const ID, usize) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let arr = make_arr(
        objc_getClass(c"NSArray".as_ptr()),
        sel_registerName(c"arrayWithObjects:count:".as_ptr()),
        urls.as_ptr(),
        urls.len(),
    );
    let write: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    write(pb, sel_registerName(c"writeObjects:".as_ptr()), arr) != 0
}

/// ファイル群の指紋(パス+合計サイズ)。形式は common::files::key に統一
fn mac_files_key(paths: &[std::path::PathBuf]) -> String {
    let v: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    bulk::files_key(&v)
}

/// ファイル群を Windows へ送る(FileBegin → FileChunk… → FileEnd)。
/// GUI メニュー(NSOpenPanel)と Finder の ⌘C 検出の両方から呼ぶ。別スレッド実行。
/// drop=true は「掴んだまま境界越え」: FileDropBegin で始め FileDropEnd で終わり、
/// Windows 側はクリップボードではなく OLE ドラッグとして扱う
pub fn send_files_to_win(paths: Vec<std::path::PathBuf>, drop: bool) {
    if FILE_TX_BUSY.swap(true, Ordering::Relaxed) {
        eprintln!("[file] 送信中のため要求を無視しました");
        return;
    }
    std::thread::spawn(move || {
        let total = bulk::total_size(&paths);
        // 空ファイルのみの選択(total=0)は正当な送信のため、件数 0 だけを拒否する
        if paths.is_empty() || total > bulk::MAX_TOTAL {
            eprintln!("[file] 送信拒否: {} 件 / 合計 {total} bytes", paths.len());
            notify(
                "tsunagu",
                &format!(
                    "ファイルを送信できません(合計 {}MiB。1回の上限 {})",
                    total / 1024 / 1024,
                    bulk::file_limit_label()
                ),
            );
            FILE_TX_BUSY.store(false, Ordering::Relaxed);
            return;
        }
        eprintln!(
            "[file] 送信開始: {} 件 / 合計 {}KB{}",
            paths.len(),
            total / 1024,
            if drop { "(掴みドラッグ)" } else { "" }
        );
        let t0 = std::time::Instant::now();
        // 進捗は 10% 刻みでログへ(巨大転送中に固まって見えるのを防ぐ)。
        // クロージャは send で消費されるため、再試行側にも同じ形を書く
        macro_rules! send_with_progress {
            () => {
                BULK_LINK.send(|w| {
                    let mut last_step = 0u64;
                    bulk::send_files_with_progress(w, &paths, drop, |sent, total| {
                        if total > 0 {
                            let step = sent * 10 / total.max(1);
                            if step > last_step {
                                last_step = step;
                                eprintln!("[file] 転送 {}%({}/{})", step * 10, sent, total);
                            }
                        }
                    })
                })
            };
        }
        // 未接続(NotConnected)は本線再接続直後の bulk 張り直しの窓(最大約 5 秒)で起きる。
        // ユーザー操作がログ 1 行で失われるのを防ぐため、少し待って 1 回だけやり直す
        let mut r = send_with_progress!();
        if r.as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotConnected)
        {
            eprintln!("[file] bulk 経路の再接続を待って再試行します");
            std::thread::sleep(Duration::from_millis(2500));
            r = send_with_progress!();
        }
        match r {
            Ok(n) => {
                let secs = t0.elapsed().as_secs_f64().max(0.001);
                eprintln!(
                    "[file] 送信完了({n} 件, {:.1}MB/s)",
                    total as f64 / 1024.0 / 1024.0 / secs
                );
                if drop {
                    notify(
                        "tsunagu",
                        &format!(
                            "{n} 件({})を Windows へ掴んで渡しました",
                            human_bytes(total)
                        ),
                    );
                } else {
                    notify(
                        "tsunagu",
                        &format!(
                            "{n} 件({})を Windows へ送信しました(Ctrl+V で貼り付け)",
                            human_bytes(total)
                        ),
                    );
                }
            }
            Err(e) => {
                eprintln!("[file] 送信失敗: {e}");
                notify(
                    "tsunagu",
                    "Windows へファイルを送れませんでした(ファイル転送経路が未接続)",
                );
            }
        }
        FILE_TX_BUSY.store(false, Ordering::Relaxed);
    });
}

/// Mac のクリップボード画像(PNG/TIFF/JPEG)を Windows の CF_DIB 形式にする。
/// NSBitmapImageRep で BMP に書き出し、先頭 14 バイトのファイルヘッダを外すと DIB になる
/// (Windows 側に画像デコーダを持たずに済む)
unsafe fn mac_clipboard_image_dib() -> Option<Vec<u8>> {
    let pb = general_pasteboard();
    let data = ["public.png", "public.tiff", "public.jpeg"]
        .iter()
        .map(|t| msg1_id(pb, sel_registerName(c"dataForType:".as_ptr()), nsstring(t)))
        .find(|d| !d.is_null())?;
    let rep = msg1_id(
        objc_getClass(c"NSBitmapImageRep".as_ptr()),
        sel_registerName(c"imageRepWithData:".as_ptr()),
        data,
    );
    if rep.is_null() {
        return None;
    }
    let props = msg0(
        objc_getClass(c"NSDictionary".as_ptr()),
        sel_registerName(c"dictionary".as_ptr()),
    );
    let repr: unsafe extern "C" fn(ID, SEL, usize, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    // NSBitmapImageFileTypeBMP = 1
    let bmp = repr(
        rep,
        sel_registerName(c"representationUsingType:properties:".as_ptr()),
        1,
        props,
    );
    if bmp.is_null() {
        return None;
    }
    let len = msg0_isize(bmp, sel_registerName(c"length".as_ptr())) as usize;
    let ptr = msg0(bmp, sel_registerName(c"bytes".as_ptr())) as *const u8;
    if ptr.is_null() || len <= 14 || len > bulk::MAX_IMAGE {
        return None;
    }
    Some(std::slice::from_raw_parts(ptr, len)[14..].to_vec())
}

/// 大容量経路の受信完了(Windows からのファイル・画像)
fn mac_on_bulk(e: bulk::Event) {
    with_pool(|| match e {
        bulk::Event::Files {
            paths,
            drag_id: Some(id),
            ..
        } => incoming_drag::receive(id, paths),
        bulk::Event::Files { paths, .. } => {
            let n = paths.len();
            let ok = unsafe { mac_clipboard_write_files(&paths) };
            // 自分が載せたファイルを Windows へ送り返さない
            *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = mac_files_key(&paths);
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            if ok {
                eprintln!("[file] win->mac 受信: {n} 件(⌘V で貼り付け可)");
                history_push_files(&paths, "Windows");
                let dir = std::env::var_os("HOME")
                    .map(|h| std::path::Path::new(&h).join("Downloads/Tsunagu"))
                    .unwrap_or_default();
                let total = tsunagu_common::bulk::total_size(&paths);
                notify(
                    "tsunagu",
                    &format!(
                        "ファイルを受信: {n} 件({})(⌘V で貼り付け可)。実体は {}",
                        human_bytes(total),
                        dir.display()
                    ),
                );
            } else {
                eprintln!("[file] win->mac 受信: {n} 件(クリップボード載せ失敗)");
            }
        }
        bulk::Event::Image(dib) => {
            if !CLIP_SHARE.load(Ordering::Relaxed) {
                return;
            }
            LAST_IMG_RX_MS.store(now_ms(), Ordering::Relaxed);
            let bmp = dib_to_bmp(&dib);
            let ok = unsafe { mac_set_clipboard_image_bmp(&bmp) };
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            if ok {
                history_push_image(&bmp, "Windows");
            }
            eprintln!(
                "[clip] win->mac image {}KB {}",
                dib.len() / 1024,
                if ok { "ok" } else { "FAILED" }
            );
        }
    })
}

/// Mac 流ショートカットの翻訳対応表。(kc, 修飾) → 翻訳先 (kc, ctrl, opt, cmd, shift)。
/// 翻訳先の修飾は「既定マップ(cmd→Ctrl / opt→Alt / ctrl→Win)」の意味で並べる
/// (CMD_ALT 交換は tap 側の送信時に行う)。対応をテストで固定するため表へ切り出した。
/// fn+F11 は FN フラグを見るためここに含めない
pub(crate) fn mac_shortcut_translation(
    kc: u16,
    ctrl: bool,
    opt: bool,
    cmd: bool,
    shift: bool,
) -> Option<(u16, bool, bool, bool, bool)> {
    if cmd && ctrl && !opt {
        // ⌘Ctrl+Q = 画面ロック(Win+L)
        return (kc == 12).then_some((37, true, false, false, false));
    }
    if cmd && opt && !ctrl {
        // ⌘⌥Esc = タスクマネージャ(Ctrl+Shift+Esc)
        return (kc == 53).then_some((53, false, false, true, true));
    }
    if cmd && !ctrl && !opt {
        // 戻り値: (翻訳先 kc, ctrl, opt, cmd, shift)
        let t = match kc {
            // ⌘←→ = 行頭/行末(Home/End)。Shift は透過(行選択)
            123 => Some((115, false, false, false, shift)),
            124 => Some((119, false, false, false, shift)),
            // ⌘↑↓ = 文書先頭/末尾(Ctrl+Home/End)。Shift 透過
            126 => Some((115, false, false, true, shift)),
            125 => Some((119, false, false, true, shift)),
            // ⌘M/⌘H = 最小化(Win+Down)
            43 | 4 => Some((125, true, false, false, false)),
            // ⌘] / ⌘[ = 次タブ / 前タブ(Ctrl(+Shift)+Tab)。⌘⇧[ も「前タブ」
            30 => Some((48, false, false, true, false)),
            33 => Some((48, false, false, true, true)),
            // ⌘⇧4 / ⌘⇧3 = スクリーンショット(Win+Shift+S)
            21 | 18 if shift => Some((1, true, false, false, true)),
            // ⌘⇧5 = 画面録画(Win+Alt+R)
            23 if shift => Some((15, false, true, false, false)),
            // ⌘Q = ウィンドウを閉じる(Alt+F4)
            12 => Some((118, false, true, false, false)),
            // ⌘G / ⌘⇧G = 次を検索 / 前を検索(F3 / Shift+F3)
            32 => Some((99, false, false, false, shift)),
            // ⌘. = キャンセル → Escape
            47 => Some((53, false, false, false, false)),
            // ⌘Space = IME/言語切替(Win+Space)
            49 => Some((49, true, false, false, false)),
            _ => None,
        };
        return t;
    }
    if opt && !cmd && !ctrl {
        // ⌥←→ = 単語移動(Ctrl+←→)。Shift は透過(単語選択)
        let t = match kc {
            123 => Some((123, false, false, true, shift)),
            124 => Some((124, false, false, true, shift)),
            _ => None,
        };
        return t;
    }
    None
}

/// ゲームモード(Windows 側がカーソルの閉じ込め等を検知して要求)。true の間は
/// 絶対位置ではなく相対移動で送る(FPS・3D ソフトの視点回転のため)
static GAME_REL: AtomicBool = AtomicBool::new(false);
/// 画面ロックの連動(TSUNAGU_LOCK_SYNC=0 で無効)
static LOCK_SYNC: AtomicBool = AtomicBool::new(true);
/// 右⌘ → Windows の右 Ctrl(TSUNAGU_RCMD_CTRL=0 で無効)。
/// 右⌘をホットキー(TSUNAGU_HOTKEY_KC=54 等)に設定している場合は
/// 先の分岐で握られるため適用されない(競合しない)
static RCMD_CTRL: AtomicBool = AtomicBool::new(true);
/// 右⌘(kc 54)の押下状態。押下中は cmd フラグを rcmd へ置き換えて送る
///(フラグだけだと左右を区別できないため、kc 単位でここで分離する)
static R_RIGHT_CMD: AtomicBool = AtomicBool::new(false);
/// IME 状態同期: Windows へ入る時に Mac のかな/英数を相手の IME 開閉へ反映
///(TSUNAGU_IME_SYNC=0 で無効)
static IME_SYNC: AtomicBool = AtomicBool::new(true);
/// Continue Here: Windows 画面操作中の ⌥⌘T で Mac の前面ブラウザの URL を
/// Windows の既定ブラウザで開く(TSUNAGU_CONTINUE_HERE=0 で無効)
static CONTINUE_HERE: AtomicBool = AtomicBool::new(true);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
    static kTISPropertyInputModeID: CFStringRef;
    fn TISCopyCurrentKeyboardInputSource() -> *mut core::ffi::c_void;
    fn TISGetInputSourceProperty(
        source: *mut core::ffi::c_void,
        key: CFStringRef,
    ) -> *mut core::ffi::c_void;
    fn CFStringGetCString(
        s: *const core::ffi::c_void,
        buf: *mut core::ffi::c_char,
        size: usize,
        encoding: u32,
    ) -> u8;
}

/// 入力モード ID(InputModeID)から Windows の IME 開閉に対応する状態を引く。
/// macOS 標準の日本語入力のみ対象(サードパーティ IME の ID は入力モードを
/// 反映しないことがあるため、誤って ON を送らないよう対象外=同期しない)。
/// Roman(英数)のみ OFF、他の日本語系(ひらがな/カタカナ/半角カナ/全角英数)は ON
fn ime_mode_state(mode: &str) -> Option<bool> {
    if !mode.starts_with("com.apple.inputmethod.Japanese") {
        return None;
    }
    Some(!mode.ends_with(".Roman"))
}

/// 現在の入力ソースの IME 状態。取得失敟(nil・変換失敗)は None=送らない
fn current_ime_state() -> Option<bool> {
    unsafe {
        let src = TISCopyCurrentKeyboardInputSource();
        if src.is_null() {
            return None;
        }
        let v = TISGetInputSourceProperty(src, kTISPropertyInputModeID);
        let mut buf = [0u8; 128];
        let ok = !v.is_null()
            && CFStringGetCString(
                v,
                buf.as_mut_ptr() as *mut core::ffi::c_char,
                buf.len(),
                0x0800_0100, // kCFStringEncodingUTF8
            ) != 0;
        CFRelease(src);
        if !ok {
            return None;
        }
        let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        ime_mode_state(&String::from_utf8_lossy(&buf[..n]))
    }
}

/// osascript を 1 本実行し、成功時は stdout を返す。失敗は stderr を
/// ログへ出して(制御文字置換済み)None
fn osascript_output(script: &str) -> Option<String> {
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if out.status.success() {
        return Some(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err: String = String::from_utf8_lossy(&out.stderr)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    eprintln!("[url] osascript: {}", err.trim());
    None
}

/// 前面ブラウザの前面タブの URL(Continue Here 用)。
/// 2 段階で実行する: ①System Events で前面アプリ名を取得、②そのアプリ専用の
/// 取得スクリプトだけ実行。1 スクリプトに全ブラウザの tell を並べると、
/// インストールされていないアプリ(Edge 等)の用語解決で構文エラー(-2741)に
/// なる実績があるため、静的な複合は組まない
/// Firefox は URL の AppleScript 対応が無いため対象外
fn frontmost_browser_url() -> Option<String> {
    let front = osascript_output(
        r#"tell application "System Events" to get name of first application process whose frontmost is true"#,
    )?;
    let get_url = match front.trim() {
        "Safari" => r#"tell application "Safari" to get URL of front document"#,
        "Google Chrome" => {
            r#"tell application "Google Chrome" to get URL of active tab of front window"#
        }
        "Microsoft Edge" => {
            r#"tell application "Microsoft Edge" to get URL of active tab of front window"#
        }
        "Brave Browser" => {
            r#"tell application "Brave Browser" to get URL of active tab of front window"#
        }
        _ => return None,
    };
    let url = osascript_output(get_url)?;
    let url = url.trim();
    (!url.is_empty()).then(|| url.to_string())
}

/// Continue Here の本体(⌥⌘T の押下エッジで別スレッドから呼ぶ)。
/// 失敗は TCC の自動化 未承認(初回に Mac 側で許可が必要)でも起きるため、
/// 通知は 60 秒に 1 回に間引いて出す
fn continue_here() {
    static LAST_ERR_MS: AtomicU64 = AtomicU64::new(0);
    match frontmost_browser_url() {
        Some(url) if tsunagu_common::urlx::transferable(&url) => {
            send_msg(&Msg::OpenUrl { url });
            eprintln!("[url] Continue Here: 送信しました");
        }
        Some(_) => eprintln!("[url] Continue Here: 転送できない形式の URL"),
        None => {
            eprintln!("[url] Continue Here: 前面ブラウザの URL を取得できません");
            let now = now_ms();
            if now.saturating_sub(LAST_ERR_MS.swap(now, Ordering::Relaxed)) > 60_000 {
                notify(
                    "tsunagu",
                    "ブラウザの URL を取得できませんでした(初回は Mac 側で自動化の許可が必要です)",
                );
            }
        }
    }
}

/// Secure Input(パスワード欄等でキー入力の横取りを OS が止める状態)の原因アプリ名。
/// この間はキーボードを Windows へ送れないため、切替時に知らせる(Deskflow と同じ配慮)
fn secure_input_app() -> Option<String> {
    if unsafe { IsSecureEventInputEnabled() } == 0 {
        return None;
    }
    let out = std::process::Command::new("ioreg")
        .args(["-l", "-w", "0", "-d", "1"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let pid = text
        .split("kCGSSessionSecureInputPID\"=")
        .nth(1)
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .filter(|s| !s.is_empty());
    let name = pid.and_then(|pid| {
        let o = std::process::Command::new("ps")
            .args(["-p", pid, "-o", "comm="])
            .output()
            .ok()?;
        let n = String::from_utf8_lossy(&o.stdout)
            .trim()
            .rsplit('/')
            .next()?
            .to_string();
        (!n.is_empty()).then_some(n)
    });
    Some(name.unwrap_or_else(|| "不明なアプリ".into()))
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGSessionCopyCurrentDictionary() -> *const core::ffi::c_void;
    fn CFDictionaryGetValue(
        d: *const core::ffi::c_void,
        key: *const core::ffi::c_void,
    ) -> *const core::ffi::c_void;
    fn CFBooleanGetValue(b: *const core::ffi::c_void) -> u8;
}

/// Mac の画面がロックされているか
fn screen_locked() -> bool {
    unsafe {
        let d = CGSessionCopyCurrentDictionary();
        if d.is_null() {
            return false;
        }
        let key = CFStringCreateWithCString(
            std::ptr::null_mut(),
            c"CGSSessionScreenIsLocked".as_ptr(),
            0x0800_0100,
        );
        let v = if key.is_null() {
            std::ptr::null()
        } else {
            CFDictionaryGetValue(d, key as *const _)
        };
        let locked = !v.is_null() && CFBooleanGetValue(v) != 0;
        if !key.is_null() {
            CFRelease(key as *mut _);
        }
        CFRelease(d as *mut _);
        locked
    }
}

/// 本線がいま繋がっている Windows のアドレス(大容量経路の接続先・経路診断に使う)
static PEER_IP: Mutex<Option<std::net::IpAddr>> = Mutex::new(None);

// ---- 複数台接続(ビジョン: 台数制限の撤回)。Mac=サーバは複数の Windows を
// 同時に保持し、アクティブな 1 台へ入力を送る。切替はメニューバーの「接続先」から ----

/// 接続中の相手 1 台分。writer はアクティブ時のみ STREAM_SLOT へ貸し出し、
/// 非アクティブ時はここで待機する
pub(crate) struct PeerEntry {
    /// 端末識別子(hello の id。旧版相手は IP 由来の代替値)
    pub id: String,
    pub name: String,
    pub ip: std::net::IpAddr,
    /// 相手の代表画面サイズ(スケール算出用)
    pub screen: (f64, f64),
    /// 相手の全モニター構成(版 13 以降で自動交換。旧版相手は空)
    pub monitors: Vec<tsunagu_common::proto::Monitor>,
    /// 非アクティブ時の送信口(アクティブ時は None: STREAM_SLOT が保持する)
    pub writer: Option<secure::Writer>,
    /// セッションの世代(同一端末の再接続で置き換えを判別する。大きいほど新しい)
    pub gen: u64,
}
pub(crate) static PEERS: Mutex<Vec<PeerEntry>> = Mutex::new(Vec::new());
/// アクティブなピアの添字(未接続は usize::MAX)
pub(crate) static ACTIVE_PEER: Mutex<usize> = Mutex::new(usize::MAX);
/// セッション世代の採番(同一端末への重複接続で、古い方を正しく破棄するため)
static PEER_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 自分(Mac)の全モニターを列挙する(自動認知。CG 座標系のまま相手へ渡す)
fn mac_monitors() -> Vec<tsunagu_common::proto::Monitor> {
    let mut out = Vec::new();
    unsafe {
        let mut ids = [0u32; 16];
        let mut n = 0;
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..(n as usize).min(16)] {
                let b = CGDisplayBounds(*id);
                out.push(tsunagu_common::proto::Monitor {
                    x: b.origin.x as i32,
                    y: b.origin.y as i32,
                    w: b.size.w as i32,
                    h: b.size.h as i32,
                });
            }
        }
    }
    out
}

/// my_id がいまアクティブなピアか(pong の反映先判定などに使う)。
/// 空 id はクライアントモード(単一接続)を表し、常にアクティブ扱い
fn is_active_peer(my_id: &str) -> bool {
    if my_id.is_empty() {
        return true;
    }
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    peers.get(act).is_some_and(|p| p.id == my_id)
}

/// ピアをアクティブへ切り替える(送信口・画面・設定同期・通知)。
/// ロック順は STREAM_SLOT → PEERS → ACTIVE_PEER で統一し、逆順で取得しないこと
pub(crate) fn activate_peer(new: usize, reason: &str) {
    let slot_arc = STREAM_SLOT.get().unwrap();
    let mut slot = slot_arc.lock().unwrap_or_else(|e| e.into_inner());
    let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let mut act = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    if new >= peers.len() {
        return;
    }
    if *act < peers.len() {
        if let Some(w) = slot.take() {
            peers[*act].writer = Some(w);
        }
    }
    let Some(w) = peers[new].writer.take() else {
        return;
    };
    *slot = Some(w);
    let (name, ip, screen) = (peers[new].name.clone(), peers[new].ip, peers[new].screen);
    *act = new;
    drop(act);
    drop(peers);
    drop(slot);
    *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = screen;
    *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = name.clone();
    *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) = Some(ip);
    CONNECTED.store(true, Ordering::Relaxed);
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    RTT_MS.store(0, Ordering::Relaxed);
    // 旧相手のファイル転送経路を切る(接続側は本線の接続先へ追従して張り直す)
    BULK_LINK.clear();
    send_cfg();
    eprintln!("[conn] 接続先を {name} へ切り替え({reason})");
    // 再接続・経路昇格の置き換えは頻発するため通知は出さない(明示的な切替だけ知らせる)
    if !reason.contains("再接続") {
        notify("tsunagu", &format!("{name} へ切り替えました({reason})"));
    }
}

/// 非アクティブピアへの間欠 ping(生存確認)。書けなくなった相手は一覧から外す。
/// アクティブ 1 台の旧構成では「繋いでいない相手が黙って消える」検知が無かった
fn keepalive_inactive_peers() {
    std::thread::spawn(|| {
        use std::io::Write as _;
        loop {
            std::thread::sleep(Duration::from_secs(10));
            let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
            let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
            let mut dead: Vec<String> = Vec::new();
            for (i, p) in peers.iter_mut().enumerate() {
                if i == act {
                    continue;
                }
                let Some(w) = p.writer.as_mut() else { continue };
                let wire = encode(&Msg::Ping { ts: now_ms() });
                if w.write_all(wire.as_bytes())
                    .and_then(|_| w.flush())
                    .is_err()
                {
                    dead.push(p.id.clone());
                }
            }
            let mut removed_before_active = 0usize;
            for id in &dead {
                if let Some(i) = peers.iter().position(|p| &p.id == id) {
                    if act != usize::MAX && i < act {
                        removed_before_active += 1;
                    }
                    peers.remove(i);
                    eprintln!("[conn] 相手(id={id})が応答しなくなったため一覧から外しました");
                }
            }
            if removed_before_active > 0 {
                *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner()) =
                    act - removed_before_active;
            }
        }
    });
}

/// 接続経路の短い表示(メニューバー用)。LAN 内なら "LAN 直"、100.x なら "Tailscale"
pub fn route_label() -> &'static str {
    match *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(ip) if tsunagu_common::net::is_tailscale(ip) => "Tailscale",
        Some(_) => "LAN 直",
        None => "",
    }
}

/// Tailscale の経路状態(0=不明/Tailscale 外, 1=直結, 2=中継(DERP))。
/// 中継は遅延が数倍になるため、切り替わった時に知らせる
pub static TS_PATH: AtomicU8 = AtomicU8::new(0);

/// 本線を確実に切る(スロットから外すだけでは受信スレッドが読み出しを待ち続ける)
fn drop_stream(reason: &str) {
    let taken = STREAM_SLOT
        .get()
        .and_then(|s| s.lock().unwrap_or_else(|e| e.into_inner()).take());
    if let Some(s) = taken {
        eprintln!("[conn] {reason}。接続を張り直します");
        s.shutdown();
    }
}

/// `tailscale status --json` から相手への経路が直結か中継かを調べる
fn tailscale_path(peer: std::net::IpAddr) -> Option<u8> {
    let out = [
        "tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    ]
    .iter()
    .find_map(|bin| {
        std::process::Command::new(bin)
            .args(["status", "--json"])
            .output()
            .ok()
    })
    .filter(|o| o.status.success())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let ip = peer.to_string();
    v["Peer"].as_object()?.values().find_map(|p| {
        let has = p["TailscaleIPs"]
            .as_array()?
            .iter()
            .any(|x| x.as_str() == Some(ip.as_str()));
        has.then(|| {
            if p["CurAddr"].as_str().unwrap_or("").is_empty() {
                2
            } else {
                1
            }
        })
    })
}

/// 大容量経路(ファイル・画像)。本線とは別の TCP 接続
static BULK_LINK: bulk::Link = bulk::Link::new();
static BULK: OnceLock<bulk::Endpoint> = OnceLock::new();

/// macOS の通知センターへ表示(接続/切断のユーザー可視化)。
/// osascript 経由で追加権限なしで出せる。失敗しても本体には影響しない。
/// osascript 起動に数百msかかるため別スレッドで発火し、accept/受信スレッドを
/// ブロックしない(レビュー Wave1 A-M9/D-F3)
fn notify(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    // osascript の文字列リテラルで特別な意味を持つ文字を先に無効化する(流用時に
    // ファイル名等が入っても構文エラーで通知だけ落ちる、という事故を防ぐ)
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('"', "'")
            .replace(['\n', '\r', '\t'], " ")
    };
    std::thread::spawn(move || {
        let out = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"{}\"",
                    esc(&body),
                    esc(&title)
                ),
            ])
            .output();
        let _ = out;
    });
}

// ---------- Search My Desk(ビジョン§14: デスク横断検索) ----------
/// Mac 操作中の ⌥⌘S で開く検索窓。アプリ・デスクのファイル/フォルダ・コマンド・
/// 履歴・URL を 1 つの窓から扱う。`TSUNAGU_DESK_SEARCH=0` で無効化
pub static DESK_SEARCH: AtomicBool = AtomicBool::new(true);
/// Windows 側のアプリ候補(AppsReply で受け取る。検索窓が開いている間に届く)
pub static WIN_APPS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
/// 越境 App Handoff(ビジョン§12 の第一歩・実験的): Windows へ切り替えたとき
/// Mac の最前面アプリと同じアプリを Windows で起動する。勝手にアプリが
/// 開く驚きを避けるため既定は無効(`TSUNAGU_APP_HANDOFF=1` で有効)
static APP_HANDOFF: AtomicBool = AtomicBool::new(false);
/// デスクのファイル/フォルダ候補(検索窓の Files/Folders 対象)。
/// 走査が重いため 10 分キャッシュし、検索窓を開くたびに裏で更新する
pub static DESK_FILES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static DESK_FILES_TS: AtomicU64 = AtomicU64::new(0);
/// 検索窓の Enter 押下時に ⌥(option)が押されていたか(=Windows へ投げる)。
/// tap スレッドが立てて gui スレッドが読む
pub(crate) static SEARCH_ENTER_OPT: AtomicBool = AtomicBool::new(false);

/// /Applications・/System/Applications・~/Applications の .app を
/// (表示名, パス)で列挙する。名前順でソートする(検索の順位安定のため)。
/// 標準アプリ(Terminal 等)は /System/Applications 側にある
pub fn list_apps() -> Vec<(String, String)> {
    // Utilities 等、1 階層深い場所にある .app も含める(Terminal は
    // /System/Applications/Utilities/ にある)
    fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("app") {
                out.push(p);
            } else if p.is_dir() {
                if let Ok(sub) = std::fs::read_dir(&p) {
                    for e2 in sub.flatten() {
                        let p2 = e2.path();
                        if p2.extension().and_then(|x| x.to_str()) == Some("app") {
                            out.push(p2);
                        }
                    }
                }
            }
        }
    }
    let mut paths = Vec::new();
    for root in ["/Applications", "/System/Applications"] {
        collect(std::path::Path::new(root), &mut paths);
    }
    if let Some(home) = std::env::var_os("HOME") {
        collect(
            &std::path::Path::new(&home).join("Applications"),
            &mut paths,
        );
    }
    let mut out = Vec::new();
    for p in paths {
        if p.extension().and_then(|x| x.to_str()) == Some("app") {
            let name = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !name.is_empty() {
                out.push((name, p.to_string_lossy().into_owned()));
            }
        }
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

/// 検索窓の Files/Folders 候補(ビジョン§14)。Desktop・Downloads・Documents の
/// 直下と 1 階層下のサブフォルダを走査する。ホーム全体を走ると dotfiles や
/// 巨大フォルダ(Library 等)に時間がかかるため、日常的に探す場所に絞る。
/// 呼び出し側は検索窓を開くタイミングで別スレッドから呼ぶ(走査中も入力は止めない)
pub fn refresh_desk_files() {
    let now = now_ms();
    // 0=未走査(now_ms は起動からの経過時間のため、初回を 10 分待たせない)。
    // 二重起動の抑止は store で間に合う(競合で走査が重なっても結果は同じ)
    let last = DESK_FILES_TS.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < 600_000 {
        return;
    }
    DESK_FILES_TS.store(now, Ordering::Relaxed);
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let home = std::path::PathBuf::from(home);
    // (表示名, パス, 変更時刻)。新しいもの順に並べてから名前・パスへ落とす
    let mut items: Vec<(String, String, u64)> = Vec::new();
    for root in ["Desktop", "Downloads", "Documents"] {
        collect_desk_files(&home.join(root), 0, &mut items);
    }
    items.sort_by(|a, b| b.2.cmp(&a.2));
    let n = items.len();
    let mapped: Vec<(String, String)> = items
        .into_iter()
        .map(|(name, path, _)| (name, path))
        .collect();
    let changed = DESK_FILES
        .lock()
        .map(|mut f| std::mem::replace(&mut *f, mapped).len() != n)
        .unwrap_or(false);
    eprintln!("[search] デスクのファイル/フォルダ {} 件を索引しました", n);
    // 検索窓が開いている間に索引が揃ったら候補へ反映する(閉じていれば何もしない)
    if changed {
        dispatch_search_refresh();
    }
}

/// collect の上限。巨大フォルダで検索窓の起動が重くならないための保険
const DESK_FILES_MAX: usize = 4000;
/// 走査しない(候補にも出さない)ディレクトリ名
const DESK_FILES_SKIP: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    "venv",
    ".venv",
    "Library",
    "dist",
    "build",
];

fn collect_desk_files(dir: &std::path::Path, depth: u8, out: &mut Vec<(String, String, u64)>) {
    if out.len() >= DESK_FILES_MAX {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if out.len() >= DESK_FILES_MAX {
            return;
        }
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || DESK_FILES_SKIP.contains(&name) {
            continue;
        }
        let meta = match e.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.push((name.to_string(), p.to_string_lossy().into_owned(), mtime));
        if meta.is_dir() && depth == 0 {
            collect_desk_files(&p, 1, out);
        }
    }
}

/// 検索窓の Commands 候補(ビジョン§14)。(表示名, コマンド ID)
pub fn desk_commands() -> Vec<(String, String)> {
    vec![
        ("Windows をロック".to_string(), "win_lock".to_string()),
        ("画面を暗くする".to_string(), "display_sleep".to_string()),
        (
            "クリップボード共有を切替".to_string(),
            "clip_share_toggle".to_string(),
        ),
        (
            "クリップボード履歴を消去".to_string(),
            "history_clear".to_string(),
        ),
        (
            "Windows のアプリ一覧を更新".to_string(),
            "win_apps_refresh".to_string(),
        ),
    ]
}

/// コマンド候補の実行。ID は desk_commands が列挙したものだけ
pub fn run_desk_command(id: &str) {
    match id {
        "win_lock" => {
            // 画面ロックの連動(既存の Msg::Lock と同じ経路)
            if send_msg_reported(&Msg::Lock) {
                eprintln!("[cmd] Windows をロックしました");
            } else {
                notify("Tsunagu", "未接続のため Windows をロックできませんでした");
            }
        }
        "display_sleep" => {
            // ディスプレイだけスリープ(ロックはしない。任意のキーで復帰)
            let _ = std::process::Command::new("/usr/bin/pmset")
                .arg("displaysleepnow")
                .spawn();
            eprintln!("[cmd] ディスプレイをスリープさせました");
        }
        "clip_share_toggle" => {
            let next = !CLIP_SHARE.load(Ordering::Relaxed);
            CLIP_SHARE.store(next, Ordering::Relaxed);
            notify(
                "tsunagu",
                if next {
                    "クリップボード共有をオンにしました"
                } else {
                    "クリップボード共有をオフにしました"
                },
            );
            eprintln!(
                "[cmd] クリップボード共有: {}",
                if next { "オン" } else { "オフ" }
            );
        }
        "history_clear" => history_clear(),
        "win_apps_refresh" => {
            send_msg(&Msg::AppsQuery);
            eprintln!("[cmd] Windows のアプリ一覧を問い合わせました");
        }
        other => eprintln!("[cmd] 未知のコマンド: {other}"),
    }
}

/// 検索窓の Enter/Esc 押下状態。窓を閉じると tap の握り範囲外になるため
/// up を受け取れないことがあり、開き直し時に残った true が初回 Enter を
/// 「2 回目の押下」として無視する(実測)。開くたび gui 側からリセットする
pub(crate) static SEARCH_ENTER_DOWN: AtomicBool = AtomicBool::new(false);
pub(crate) static SEARCH_ESC_DOWN: AtomicBool = AtomicBool::new(false);

/// 検索窓の表示をメインスレッドへ依頼する(tap はバックグラウンドスレッド。
/// AppKit の窓操作は必ずメインスレッドで行う)
fn dispatch_show_search() {
    unsafe {
        let target = gui::target_id();
        if target.is_null() {
            eprintln!("[search] GUI target 未初期化のため検索窓を開けません");
            return;
        }
        let f: unsafe extern "C" fn(ID, SEL, SEL, ID, u8) =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            target,
            sel_registerName(c"performSelectorOnMainThread:withObject:waitUntilDone:".as_ptr()),
            sel_registerName(c"sdShowSearch:".as_ptr()),
            std::ptr::null_mut(),
            0,
        );
    }
}

/// 検索窓用: キーコードを文字へ変換する(US/JIS 共通の QWERTY 位置)。
/// IME を bypass して検索窓に直接積むための最小変換(shift は US 記号)
fn keychar(kc: u16, shift: bool) -> Option<&'static str> {
    Some(match kc {
        0 => {
            if shift {
                "A"
            } else {
                "a"
            }
        }
        1 => {
            if shift {
                "S"
            } else {
                "s"
            }
        }
        2 => {
            if shift {
                "D"
            } else {
                "d"
            }
        }
        3 => {
            if shift {
                "F"
            } else {
                "f"
            }
        }
        4 => {
            if shift {
                "H"
            } else {
                "h"
            }
        }
        5 => {
            if shift {
                "G"
            } else {
                "g"
            }
        }
        6 => {
            if shift {
                "Z"
            } else {
                "z"
            }
        }
        7 => {
            if shift {
                "X"
            } else {
                "x"
            }
        }
        8 => {
            if shift {
                "C"
            } else {
                "c"
            }
        }
        9 => {
            if shift {
                "V"
            } else {
                "v"
            }
        }
        11 => {
            if shift {
                "B"
            } else {
                "b"
            }
        }
        12 => {
            if shift {
                "Q"
            } else {
                "q"
            }
        }
        13 => {
            if shift {
                "W"
            } else {
                "w"
            }
        }
        14 => {
            if shift {
                "E"
            } else {
                "e"
            }
        }
        15 => {
            if shift {
                "R"
            } else {
                "r"
            }
        }
        16 => {
            if shift {
                "Y"
            } else {
                "y"
            }
        }
        17 => {
            if shift {
                "T"
            } else {
                "t"
            }
        }
        18 => {
            if shift {
                "!"
            } else {
                "1"
            }
        }
        19 => {
            if shift {
                "@"
            } else {
                "2"
            }
        }
        20 => {
            if shift {
                "#"
            } else {
                "3"
            }
        }
        21 => {
            if shift {
                "$"
            } else {
                "4"
            }
        }
        22 => {
            if shift {
                "^"
            } else {
                "6"
            }
        }
        23 => {
            if shift {
                "%"
            } else {
                "5"
            }
        }
        24 => {
            if shift {
                "+"
            } else {
                "="
            }
        }
        25 => {
            if shift {
                "("
            } else {
                "9"
            }
        }
        26 => {
            if shift {
                "&"
            } else {
                "7"
            }
        }
        27 => {
            if shift {
                "_"
            } else {
                "-"
            }
        }
        28 => {
            if shift {
                "*"
            } else {
                "8"
            }
        }
        29 => {
            if shift {
                ")"
            } else {
                "0"
            }
        }
        30 => {
            if shift {
                "}"
            } else {
                "]"
            }
        }
        31 => {
            if shift {
                "O"
            } else {
                "o"
            }
        }
        32 => {
            if shift {
                "U"
            } else {
                "u"
            }
        }
        33 => {
            if shift {
                "{"
            } else {
                "["
            }
        }
        34 => {
            if shift {
                "I"
            } else {
                "i"
            }
        }
        35 => {
            if shift {
                "P"
            } else {
                "p"
            }
        }
        37 => {
            if shift {
                "L"
            } else {
                "l"
            }
        }
        38 => {
            if shift {
                "J"
            } else {
                "j"
            }
        }
        39 => {
            if shift {
                "\""
            } else {
                "'"
            }
        }
        40 => {
            if shift {
                "K"
            } else {
                "k"
            }
        }
        41 => {
            if shift {
                ":"
            } else {
                ";"
            }
        }
        42 => {
            if shift {
                "|"
            } else {
                "\\"
            }
        }
        43 => {
            if shift {
                "<"
            } else {
                ","
            }
        }
        44 => {
            if shift {
                "?"
            } else {
                "/"
            }
        }
        45 => {
            if shift {
                "N"
            } else {
                "n"
            }
        }
        46 => {
            if shift {
                "M"
            } else {
                "m"
            }
        }
        47 => {
            if shift {
                ">"
            } else {
                "."
            }
        }
        49 => " ",
        50 => {
            if shift {
                "~"
            } else {
                "`"
            }
        }
        _ => return None,
    })
}

/// Windows 側アプリ候補の到着を検索窓へ反映する(メインスレッドで再絞り込み)。
/// 窓が閉じていれば何もしない
fn dispatch_search_refresh() {
    unsafe {
        let target = gui::target_id();
        if target.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(ID, SEL, SEL, ID, u8) =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            target,
            sel_registerName(c"performSelectorOnMainThread:withObject:waitUntilDone:".as_ptr()),
            sel_registerName(c"sdSearchRefresh:".as_ptr()),
            std::ptr::null_mut(),
            0,
        );
    }
}

/// バイト数を通知・表示用に整形する(2048→"2 KB"、3_500_000→"3.3 MB")
pub fn human_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", n as f64 / (1024 * 1024 * 1024) as f64)
    } else if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024 * 1024) as f64)
    } else if n >= 1024 {
        format!("{} KB", (n + 1023) / 1024)
    } else {
        format!("{n} B")
    }
}

/// この Mac のホスト名(kern.hostname、ドメイン部は除く)。hello で相手へ出す
pub fn hostname_label() -> String {
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const std::ffi::c_char,
            oldp: *mut core::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut core::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    unsafe {
        let name = c"kern.hostname";
        let mut size = 0usize;
        if sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return "Mac".into();
        }
        let mut buf = vec![0u8; size];
        if sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return "Mac".into();
        }
        let s = std::ffi::CStr::from_bytes_until_nul(&buf)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_default();
        let short = s.split('.').next().unwrap_or("").trim().to_string();
        if short.is_empty() {
            "Mac".into()
        } else {
            short.chars().take(40).collect()
        }
    }
}

// ---------- 共有状態 ----------
static WIN_MODE: AtomicBool = AtomicBool::new(false);
static CONNECTED: AtomicBool = AtomicBool::new(false);
/// ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)。トグル時に Windows へ Cfg で同期
static CMD_ALT: AtomicBool = AtomicBool::new(false);
/// 接続中の Windows スピーカーミュート(true=Mac のみ発音。既定 ON)。
/// トグル時に Windows へ Cfg で同期(TSUNAGU_MUTE_SPK=0 で初期無効化)
static SPK_MUTE: AtomicBool = AtomicBool::new(true);
/// スクロール方向の反転(既定 false=Windows 標準の指の動きに合わせてある)
/// スクロール方向の手動上書き(true=Windows 標準。false 既定=Mac の設定に合わせる)
static SCROLL_FLIP: AtomicBool = AtomicBool::new(false);
/// macOS の「自然スクロール」設定(起動時に取得。true=トラックパッドのコンテンツ追従)
static NATURAL_SCROLL: AtomicBool = AtomicBool::new(true);

/// macOS のスクロール方向設定を読む(失敗時は出荷既定の自然スクロール扱い)。
/// ユーザーが Mac で使っている向きへ Windows 側も自動で合わせるために使う
fn detect_natural_scroll() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "com.apple.swipescrolledirection"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() != "0")
        .unwrap_or(true)
}
/// Windows との RTT(ms)。ping/pong 往復で測定(メニュー状態行の表示用)
static RTT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// ファイル送信中(多重送信の抑制)
static FILE_TX_BUSY: AtomicBool = AtomicBool::new(false);
/// 直近に送ったクリップボードファイルの指紋(同じ ⌘C の再送防止)
static LAST_SENT_FILES: Mutex<String> = Mutex::new(String::new());
static TX: OnceLock<Sender<String>> = OnceLock::new();
static STREAM_SLOT: OnceLock<Arc<Mutex<Option<secure::Writer>>>> = OnceLock::new();
static TAP_PORT: OnceLock<usize> = OnceLock::new();
static DIAG_MOVE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_KEY_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_SEND_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DIAG_WARP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_MODE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_SCROLL_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_ABS_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_SELF_HEAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TAP_REARM_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// ウォッチドッグ用: 最終タップ受信時刻と最終 abs 送信時刻(ms)。
/// 「ユーザーが操作中なのに Windows へ届いていない」状態を検知する
static LAST_EVENT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_ABS_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 境界ダブルタップ切替(Deskflow switchDoubleTap 相当)。
/// TSUNAGU_EDGE_TAPS(既定2)= 境界に連続で2回当てた時だけ切替。1回の到達では
/// 切替しないため、境界付近での日常作業と Windows への移動が分離される。
/// GUI から実行中に切り替え可能なため AtomicU32(初期値は起動時に store)
static EDGE_TAPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(2);
static EDGE_AT_EDGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static EDGE_LAST_HIT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
// ---- Deskflow 標準オプション(画面位置 links / switchDelay / switchDoubleTap /
// ---- switchCorners / clipboardSharing)----
/// Windows 画面の位置(0=右/1=左/2=上/3=下)。Deskflow links 相当
pub static SIDE: AtomicU8 = AtomicU8::new(0);
/// 端に N ms 滞ってから切替(switchDelay。0=無効でダブルタップ/即時判定)
pub static SWITCH_DELAY_MS: AtomicU64 = AtomicU64::new(0);
/// ダブルタップの判定窓 ms(switchDoubleTap)
pub static DOUBLE_TAP_MS: AtomicU64 = AtomicU64::new(700);
/// 四隅の切替禁止サイズ px(switchCornerSize。0=無効)
pub static CORNER_PX: AtomicU64 = AtomicU64::new(0);
/// クリップボード共有(clipboardSharing)
pub static CLIP_SHARE: AtomicBool = AtomicBool::new(true);
/// スクロール互換モード(TSUNAGU_SCROLL_COMPAT=1 / 設定窓): 120 未満の
/// ホイール量を無視する古い設計のアプリ向けに 1 ノッチ(120)単位で送る。
/// 既定 OFF=高解像度(0.05 ノッチ刻み)で滑らかに
pub static SCROLL_COMPAT: AtomicBool = AtomicBool::new(false);
/// 端到達の開始時刻(switchDelay の滞在計測用)
static EDGE_STAY_SINCE_MS: AtomicU64 = AtomicU64::new(0);
/// 横スワイプ(戻る/進む)の状態: (累積 dx, 最終イベント時刻, 最終発火時刻)
static SWIPE_ACC: std::sync::Mutex<(f64, u64, u64)> = std::sync::Mutex::new((0.0, 0, 0));
/// ドラッグ中切替(TSUNAGU_DRAG_SWITCH=1): 押したまま境界を越えられる
pub static DRAG_SWITCH: AtomicBool = AtomicBool::new(false);
/// 速度越境(Crossing Intelligence・ビジョン§24): 境界への速度が十分大きい越えは
/// 滞在待ち/ダブルタップをスキップする。`TSUNAGU_FAST_EDGE=0` で無効化
pub static FAST_EDGE: AtomicBool = AtomicBool::new(true);
/// Mac 流ショートカット翻訳(TSUNAGU_MAC_KEYS=0 で無効)。タップ内で毎イベント
/// 設定を引かないよう起動時にキャッシュする
static MAC_KEYS: AtomicBool = AtomicBool::new(true);
/// 2本指横スワイプ→戻る/進む(TSUNAGU_SWIPE_NAV=0 で横ホイールのまま)
static SWIPE_NAV: AtomicBool = AtomicBool::new(true);
/// 現在押下中のマウスボタン(0=左,1=右,2=中)。切替時の持ち込み再送に使う
static BTN_DOWN: [AtomicBool; 3] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

/// 押下時の基準値・取得中の世代・ファイル群を同じ状態で管理し、遅い読み出しが
/// 次のクリックに混ざらないようにする。境界切替の許可と切替時の送信に使う
static FILE_DRAG: Mutex<file_drag::FileDrag> = Mutex::new(file_drag::FileDrag::new());
/// 合成 LeftMouseUp(kCGEventSourceUserData=42)に刻む識別マジック。
/// 掴み切替直後の Mac 側ドラッグ完結用投稿であり、Win へ転送してはならない
const SYNTH_UP_MAGIC: i64 = 0x54554e41475550; // "TSUNAGUP" 的な一意値
const FIELD_EVENT_SOURCE_USER_DATA: i32 = 42;

unsafe fn make_drag_end_event(pos: CGPoint) -> CGEventRef {
    let event = CGEventCreateMouseEvent(std::ptr::null_mut(), EVT_LEFT_UP, pos, 0);
    if !event.is_null() {
        CGEventSetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA, SYNTH_UP_MAGIC);
    }
    event
}

/// SIDE(0=右/1=左/2=上/3=下/4=右上/5=右下/6=左上/7=左下)を文字列表現で
pub fn side_name() -> &'static str {
    match SIDE.load(Ordering::Relaxed) {
        1 => "左",
        2 => "上",
        3 => "下",
        4 => "右上",
        5 => "右下",
        6 => "左上",
        7 => "左下",
        _ => "右",
    }
}

/// SIDE を「境界方向(0=右/1=左/2=上/3=下)」へ正規化(斜めは水平境界に寄せる)
fn side_dir() -> u8 {
    match SIDE.load(Ordering::Relaxed) {
        1 | 6 | 7 => 1,
        2 => 2,
        3 => 3,
        _ => 0,
    }
}

/// 接続する境界の範囲(境界に沿った位置の割合 0..1)。Mac の「ディスプレイ配置」
/// と同じ発想: Windows 画面が Mac の端のどの範囲に接しているか。
/// 斜め(4-7)は半分、それ以外は全域。配置エディタのドラッグで更に細かく決まる
pub static LAY_RANGE: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 1.0));

/// SIDE を設定し、Windows へ Cfg で同期する
pub fn set_side(v: u8) {
    let v = v.min(7);
    SIDE.store(v, Ordering::Relaxed);
    *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = match v {
        4 | 6 => (0.0, 0.5),
        5 | 7 => (0.5, 1.0),
        _ => (0.0, 1.0),
    };
    send_cfg();
    eprintln!("[cfg] Windows の位置 -> {}", side_name());
}

/// Windows と共有する設定を一括送信する(接続確立時と各設定の変更時)
pub fn send_cfg() {
    send_msg(&Msg::Cfg {
        cmd_alt: CMD_ALT.load(Ordering::Relaxed),
        spk_mute: SPK_MUTE.load(Ordering::Relaxed),
        side: SIDE.load(Ordering::Relaxed),
        clip: CLIP_SHARE.load(Ordering::Relaxed),
    });
}
static LAST_PONG_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 単調時計の ms。壁時計(SystemTime)は NTP 補正やスリープ復帰で飛び、
/// ダブルタップ判定・復帰ガード・pong 監視を誤動作させるため使わない。
/// 0 を「未設定」の意味で使う箇所があるため 1 秒のオフセットを足す
fn now_ms() -> u64 {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
        + 1_000
}

/// 復帰直後は右端判定を一定時間無効化する(再突入チャタリング防止)
static EDGE_GUARD_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 自前管理のカーソル位置(delta 積算)。タップ内での毎イベント CGEventCreate は
/// 負荷としてカクつきに効くため、積算+間欠同期(Deskflow の m_xCursor 方式)にする。
static CUR_POS: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// 直近 100ms の移動量(px)と窓の開始時刻(ms)。切替時の速度計装用
///(§24 Crossing Intelligence: 実機の速い/遅い到達の分布を見てから誤 Cross
/// 判定のしきい値を設計する。現状は判定には使わない)
static RECENT_PX: Mutex<(f64, u64)> = Mutex::new((0.0, 0));

/// 100ms 窓の移動量から速度(px/秒)を引く(計装ログと単体テストで使う)
fn px_per_sec(px: f64, window_ms: u64) -> f64 {
    if window_ms == 0 {
        return 0.0;
    }
    px * 1000.0 / window_ms as f64
}
/// 接続相手(Windows)の画面サイズ(px)。hello で受信しスケール自動算出に使う
pub(crate) static WIN_SCREEN: Mutex<(f64, f64)> = Mutex::new((1920.0, 1080.0));
/// WIN モード中の Windows 仮想カーソル位置(px)。絶対位置送信モードで使う
static WIN_CUR: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));

/// Windows の解像度が変わった(Screen)とき、仮想カーソルを旧画面内の比率の
/// 位置へ写し直す。補正しないと旧サイズの絶対 px のまま残り、縮小時は境界へ
/// 張り付いて side=1(右配置)の誤帰還、拡大時は位置が飛ぶ
fn rescale_win_cur(wc: (f64, f64), old: (f64, f64), new: (f64, f64)) -> (f64, f64) {
    if old.0 <= 0.0 || old.1 <= 0.0 || new.0 <= 0.0 || new.1 <= 0.0 {
        return wc;
    }
    (wc.0 * new.0 / old.0, wc.1 * new.1 / old.1)
}
/// 前回 Windows モードを出た位置(0..1)。次回の切替はそこへ戻る(Deskflow 標準の体験)
static LAST_WIN_POS: Mutex<(f64, f64)> = Mutex::new((-1.0, -1.0));
/// 絶対位置送信モード(既定ON。TSUNAGU_MOUSE_MODE=rel で旧・相対移動に戻す)
static MOUSE_ABS_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// 切替方式: false=境界+ホットキー(既定)/ true=ホットキー(F13)のみで切替、
/// 切替後は境界を超えても戻らないロック状態になる(TSUNAGU_SWITCH_MODE=hotkey)
static HOTKEY_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CUR_SYNC_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// ライブカーソル位置を取得(CFRelease まで面倒を見る)
unsafe fn live_cursor() -> Option<CGPoint> {
    let probe = CGEventCreate(std::ptr::null_mut());
    if probe.is_null() {
        return None;
    }
    let loc = CGEventGetLocation(probe);
    CFRelease(probe);
    Some(loc)
}
/// WIN モード中のカーソル固定位置(右端内側, y)。漏れ移動を warp で巻き戻す基準。
static LOCK_POS: Mutex<Option<(f64, f64)>> = Mutex::new(None);
/// スクロール変換の累積残高(dx, dy)[ノッチ]。除数を大きくしても細かい動きを失わないための仕組み。
static SCROLL_ACC: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// スクロール速度除数(ピクセル→ノッチ変換。大きいほど遅い)。設定ウィンドウの
/// スライダーからも可変(TSUNAGU_SCROLL_DIV は初期値)。f64 を AtomicU64 ビットで保持
static SCROLL_DIV: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(60.0f64.to_bits());

/// 現在のスクロール除数を f64 で読む
pub fn scroll_div() -> f64 {
    f64::from_bits(SCROLL_DIV.load(Ordering::Relaxed))
}

/// スクロール除数を設定(20..240 にクランプ)。設定ウィンドウから呼ばれる
pub fn set_scroll_div(v: f64) {
    let clamped = v.clamp(20.0, 240.0);
    SCROLL_DIV.store(clamped.to_bits(), Ordering::Relaxed);
}
/// マウス移動の倍率(Mac の加速済み delta に Windows の加速が重なる調整用)。
/// カーソル速度倍率(abs 座標系)。設定ウィンドウのスライダーから可変。
/// f64 を AtomicU64 ビットで保持(スクロール除数と同じ方式)
static MOUSE_SCALE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1.0f64.to_bits());

/// 現在のカーソル速度倍率
pub fn mouse_scale() -> f64 {
    f64::from_bits(MOUSE_SCALE.load(Ordering::Relaxed))
}

/// カーソル速度倍率を設定(0.2..3.0 にクランプ)
pub fn set_mouse_scale(v: f64) {
    MOUSE_SCALE.store(v.clamp(0.2, 3.0).to_bits(), Ordering::Relaxed);
}
/// 境界切替の判定閾値(px、境界からの距離)。設定窓スライダーで可変。
/// f64 を AtomicU64 ビットで保持
static EDGE_PX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(2.0f64.to_bits());

/// 現在の境界判定閾値(px)
pub fn edge_px() -> f64 {
    f64::from_bits(EDGE_PX.load(Ordering::Relaxed))
}

/// 境界判定閾値を設定(0..50px にクランプ)。大きいほど切替が敏感になる
pub fn set_edge_px(v: f64) {
    EDGE_PX.store(v.clamp(0.0, 50.0).to_bits(), Ordering::Relaxed);
}
/// 画面構成。ディスプレイの抜き差し・配置変更の通知で作り直す(Deskflow と同じく
/// CGDisplayRegisterReconfigurationCallback を使う。旧実装は起動時に一度だけ確定させ、
/// モニターを抜き差しすると境界がずれたままだった)
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geo {
    /// メイン画面の大きさ(絶対位置モードの速度換算・配置エディタ用)
    pub main_w: f64,
    pub main_h: f64,
    /// 全ディスプレイを合わせた領域の端(グローバル座標)
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    /// 各辺で Windows に接する出口ディスプレイの、辺に沿った範囲
    /// (左右の辺は y の範囲、上下の辺は x の範囲)
    exit: [(f64, f64); 4],
}

const GEO_DEFAULT: Geo = Geo {
    main_w: 2056.0,
    main_h: 1329.0,
    min_x: 0.0,
    max_x: 2056.0,
    min_y: 0.0,
    max_y: 1329.0,
    exit: [(0.0, 1329.0), (0.0, 1329.0), (0.0, 2056.0), (0.0, 2056.0)],
};

static GEO: Mutex<Option<Geo>> = Mutex::new(None);

pub(crate) fn geo() -> Geo {
    GEO.lock()
        .unwrap_or_else(|e| e.into_inner())
        .unwrap_or(GEO_DEFAULT)
}

/// 検索窓を置くべき位置(AppKit 座標): カーソルがある画面の中央。
/// CG 座標系(top-left origin)と AppKit(bottom-left origin)は y 軸が逆向き
/// ため appkit_y = geo().max_y - cg_y で変換する。
/// 多画面環境で main 以外を見ているときに窓が見えない場所へ出るのを防ぐ(実測)
pub(crate) fn cursor_screen_center_appkit(win_w: f64, win_h: f64) -> (f64, f64) {
    unsafe {
        let cur = live_cursor().unwrap_or(CGPoint { x: 0.0, y: 0.0 });
        let mut ids = [0u32; 16];
        let mut n = 0u32;
        let b = if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            ids[..(n as usize).min(16)]
                .iter()
                .map(|id| CGDisplayBounds(*id))
                .find(|b| {
                    cur.x >= b.origin.x
                        && cur.x < b.origin.x + b.size.w
                        && cur.y >= b.origin.y
                        && cur.y < b.origin.y + b.size.h
                })
                .unwrap_or_else(|| CGDisplayBounds(CGMainDisplayID()))
        } else {
            CGDisplayBounds(CGMainDisplayID())
        };
        let g = geo();
        let cx = b.origin.x + (b.size.w - win_w) / 2.0;
        let cy = g.max_y - (b.origin.y + b.size.h / 2.0) - win_h / 2.0;
        (cx, cy)
    }
}

impl Geo {
    /// dir(0=右/1=左/2=上/3=下)の境界までの距離。左右に別モニターがある環境でも
    /// 「全体の端」で測るため、Mac 内のモニター間移動では切り替わらない
    fn gap(&self, dir: u8, x: f64, y: f64) -> f64 {
        match dir {
            1 => x - self.min_x,
            2 => y - self.min_y,
            3 => self.max_y - y,
            _ => self.max_x - x,
        }
    }
    fn exit_span(&self, dir: u8) -> (f64, f64) {
        self.exit[dir.min(3) as usize]
    }
    /// 境界に沿った位置の比率(0..1)。左右の辺は y、上下の辺は x で測る
    fn along_ratio(&self, dir: u8, x: f64, y: f64) -> f64 {
        let (lo, hi) = self.exit_span(dir);
        let v = if dir >= 2 { x } else { y };
        if hi > lo {
            ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
        } else {
            0.5
        }
    }
    /// 比率から境界に沿った座標へ(端から 20px は避ける)
    fn along_pos(&self, dir: u8, r: Option<f64>) -> f64 {
        let (lo, hi) = self.exit_span(dir);
        match r {
            Some(n) => {
                (lo + n.clamp(0.0, 1.0) * (hi - lo)).clamp(lo + 20.0, (hi - 20.0).max(lo + 20.0))
            }
            None => (lo + hi) / 2.0,
        }
    }
    /// 境界から inset だけ内側の、境界に沿った比率 r の点
    fn inside_point(&self, dir: u8, inset: f64, r: Option<f64>) -> (f64, f64) {
        let a = self.along_pos(dir, r);
        match dir {
            1 => (self.min_x + inset, a),
            2 => (a, self.min_y + inset),
            3 => (a, self.max_y - inset),
            _ => (self.max_x - inset, a),
        }
    }
}

fn compute_geo() -> Geo {
    unsafe {
        let mb = CGDisplayBounds(CGMainDisplayID());
        let mut g = Geo {
            main_w: mb.size.w,
            main_h: mb.size.h,
            min_x: mb.origin.x,
            max_x: mb.origin.x + mb.size.w,
            min_y: mb.origin.y,
            max_y: mb.origin.y + mb.size.h,
            exit: [
                (mb.origin.y, mb.origin.y + mb.size.h),
                (mb.origin.y, mb.origin.y + mb.size.h),
                (mb.origin.x, mb.origin.x + mb.size.w),
                (mb.origin.x, mb.origin.x + mb.size.w),
            ],
        };
        let mut ids = [0u32; 16];
        let mut n = 0u32;
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..(n as usize).min(16)] {
                let b = CGDisplayBounds(*id);
                let (l, r, t, btm) = (
                    b.origin.x,
                    b.origin.x + b.size.w,
                    b.origin.y,
                    b.origin.y + b.size.h,
                );
                // 同じ辺を複数のディスプレイが共有する(縦に並べた外部モニター等)場合は
                // 出口の範囲を合算する
                let widen = |e: &mut (f64, f64), lo: f64, hi: f64| *e = (e.0.min(lo), e.1.max(hi));
                if r > g.max_x {
                    g.max_x = r;
                    g.exit[0] = (t, btm);
                } else if r == g.max_x {
                    widen(&mut g.exit[0], t, btm);
                }
                if l < g.min_x {
                    g.min_x = l;
                    g.exit[1] = (t, btm);
                } else if l == g.min_x {
                    widen(&mut g.exit[1], t, btm);
                }
                if t < g.min_y {
                    g.min_y = t;
                    g.exit[2] = (l, r);
                } else if t == g.min_y {
                    widen(&mut g.exit[2], l, r);
                }
                if btm > g.max_y {
                    g.max_y = btm;
                    g.exit[3] = (l, r);
                } else if btm == g.max_y {
                    widen(&mut g.exit[3], l, r);
                }
            }
        }
        g
    }
}

fn refresh_geo() {
    let g = compute_geo();
    *GEO.lock().unwrap_or_else(|e| e.into_inner()) = Some(g);
    eprintln!(
        "[screen] main {}x{} / 全体 x={:.0}..{:.0} y={:.0}..{:.0}",
        g.main_w, g.main_h, g.min_x, g.max_x, g.min_y, g.max_y
    );
}

/// ディスプレイ構成の変更通知(メイン RunLoop 上で呼ばれる)。変更完了時だけ作り直す
unsafe extern "C" fn display_reconfigured(
    _display: u32,
    flags: u32,
    _user: *mut core::ffi::c_void,
) {
    const BEGIN: u32 = 1; // kCGDisplayBeginConfigurationFlag
    if flags & BEGIN == 0 {
        refresh_geo();
    }
}

/// カーソル非表示状態の管理(hide/show の対称性を保証し、復帰時に必ず表示する)
static CURSOR_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Windows モード開始: カーソル移動とマウス入力の関連を切断し、
/// Mac カーソルを画面右端の固定位置へ置く(Synergy/Deskflow 方式)
/// Deskflow hideCursor/showCursor 内の「SetsCursorInBackground」プロパティ設定。
/// バックグラウンド接続でもカーソル表示状態を維持し、非表示がランダムに解除されるのを防ぐ
unsafe fn set_cursor_in_background() {
    let key = CFStringCreateWithCString(
        std::ptr::null_mut(),
        c"SetsCursorInBackground".as_ptr(),
        0, // kCFStringEncodingMacRoman
    );
    if !key.is_null() {
        let cid = _CGSDefaultConnection();
        CGSSetConnectionProperty(cid, cid, key, kCFBooleanTrue);
        CFRelease(key);
    }
}

fn enter_win_mode_cursor_lock() {
    sync_clipboard_to_win();
    // IME Follow Cursor(ビジョン§7): Mac のかな/英数の状態を Windows 側の
    // IME 開閉へ乗せていく。「画面を移る時だけ同期」の原則どおりここでだけ送る。
    // 日本語入力以外(英字レイアウト)は送らない=Windows 側は現状維持
    if IME_SYNC.load(Ordering::Relaxed) {
        if let Some(on) = current_ime_state() {
            send_msg(&Msg::Ime { kana: on });
            eprintln!(
                "[ime] Mac の状態を Windows へ同期: {}",
                if on { "かな(ON)" } else { "英数(OFF)" }
            );
        }
    }
    std::thread::spawn(|| {
        static LAST: AtomicU64 = AtomicU64::new(0);
        if let Some(app) = secure_input_app() {
            eprintln!("[secure] Secure Input 有効(原因: {app})。キーボードは Windows へ届きません");
            let now = now_ms();
            if now.saturating_sub(LAST.swap(now, Ordering::Relaxed)) > 60_000 {
                notify(
                    "tsunagu",
                    &format!("「{app}」がパスワード入力等の保護を有効にしているため、キーボードを Windows へ送れません。そのアプリの入力欄から離れてください"),
                );
            }
        }
    });
    // 持ち越していたスクロール残量を切替時に捨てる(切替直後の意図しないスクロール防止)
    *SCROLL_ACC.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
    // Deskflow leave() 相当: hideCursor(プロパティ付き) → suppression間隔最小化 → 関連切断 → warp固定
    unsafe {
        set_cursor_in_background();
        let d = CGMainDisplayID();
        // hide が多重に積もると show が追いつかずカーソルが消えたままになるため
        // フラグで 1 回だけ隠す
        if !CURSOR_HIDDEN.swap(true, Ordering::Relaxed) {
            CGDisplayHideCursor(d);
        }
        CGSetLocalEventsSuppressionInterval(0.0001);
        CGAssociateMouseAndMouseCursorPosition(false);
        // 関連切断は非同期で効き始めるため、切替直後の漏れ移動が数ピクセル出る。
        // 固定位置を右端内側に warp しておき、以降の漏れは都度巻き戻す(境界の同時移動対策)
        let mut lock_y = 400.0;
        let ev = CGEventCreate(std::ptr::null_mut());
        if !ev.is_null() {
            let loc = CGEventGetLocation(ev);
            lock_y = loc.y;
            CFRelease(ev);
        }
        // 全ディスプレイを合わせた領域の、Windows 側の辺に固定する(メイン画面の端に
        // 固定すると、その先にサブモニターがある環境で隠れカーソルが別画面へ飛ぶ)
        let dir = side_dir();
        let g = geo();
        let lock_x2 = live_cursor()
            .map(|p| p.x)
            .unwrap_or((g.min_x + g.max_x) / 2.0);
        let (lock_x, lock_y) = match dir {
            1 => (g.min_x + 2.0, lock_y),
            2 => (lock_x2, g.min_y + 2.0),
            3 => (lock_x2, g.max_y - 2.0),
            _ => (g.max_x - 2.0, lock_y),
        };
        CGWarpMouseCursorPosition(CGPoint {
            x: lock_x,
            y: lock_y,
        });
        // タップが握った位置を CUR_POS にも反映(積算の起点を正しくする)
        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (lock_x, lock_y);
        *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) = Some((lock_x, lock_y));
    }
}

/// Windows モード終了: 関連を復元し、右端の内側へカーソルを戻す。
/// ny は Windows 側カーソルの高さ(0..1)。与えられた場合は同じ高さへ戻す(境界連続性)。
fn leave_win_mode_cursor_unlock(ny: Option<f64>) {
    // Windows が自力で検知できない離脱(ホットキー/Mac 内完結の左端/ウォッチドッグ)でも
    // 押下中のキー・ボタンが Windows に残らないよう、必ず後片付けを依頼する
    send_msg(&Msg::Leave);
    // Deskflow enter() 相当: 関連復元 → showCursor(プロパティ付き) → suppression解除 → 位置復帰
    unsafe {
        // 次回の切替で同じ場所へ戻れるよう、Windows 画面内の現在地を記憶する
        {
            let wc = *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
            let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
            if ww > 0.0 && wh > 0.0 && wc.0 >= 0.0 {
                *LAST_WIN_POS.lock().unwrap_or_else(|e| e.into_inner()) =
                    ((wc.0 / ww).clamp(0.05, 0.95), (wc.1 / wh).clamp(0.05, 0.95));
            }
        }
        // 時間ガードを 400ms に増強し、復帰直後の再突入を防ぐ(距離ガードの代替)
        EDGE_GUARD_UNTIL_MS.store(now_ms() + 400, Ordering::Relaxed);
        // 注意: ここでライブ位置を同期すると「復帰ワープ前」の境界位置
        // (2309)を掴んでしまい、ガード明けに即再突入する原因になる。
        // CUR_POS はこの後のワープ先で上書きするため、ここでは同期しない

        *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        CGAssociateMouseAndMouseCursorPosition(true);
        set_cursor_in_background();
        let d = CGMainDisplayID();
        // カーソルの再表示漏れ(透明のまま戻るバグ)を防ぐため、フラグが立って
        // いるときは show を複数回呼んで確実に表示する(呼び過ぎても無害)
        if CURSOR_HIDDEN.swap(false, Ordering::Relaxed) {
            for _ in 0..3 {
                CGDisplayShowCursor(d);
            }
        }
        CGSetLocalEventsSuppressionInterval(0.0); // Deskflow setZeroSuppressionInterval
                                                  // 復帰位置: ダブルタップ切替が有効な間は出た境界のすぐ内側(60px)へ戻す。
                                                  // 1回の到達では切替しなくなったため境界近くでも再突入せず、境界を
                                                  // 跨いで戻ってくる連続的な体験になる。
                                                  // 1回切替(TSUNAGU_EDGE_TAPS=1)では従来どおり MacBook 側へ退けて
                                                  // 誤再突入を防ぐ
                                                  // SIDE(Windows の位置)に応じた復帰座標: 出てきた境界のすぐ内側へ。
                                                  // ny は Windows 側カーソルの「境界に沿った比率」(side 0/1=縦、2/3=横)
        let g = geo();
        let dir = side_dir();
        let taps = EDGE_TAPS.load(Ordering::Relaxed);
        let inset: f64 = if taps >= 2 { 60.0 } else { 150.0 };
        let (mut x, y) = g.inside_point(dir, inset, ny);
        if dir == 0 && taps < 2 {
            // 1回切替では MacBook 側へ十分退けて誤再突入を防ぐ
            x = x.min(g.main_w - 100.0);
        }
        CGWarpMouseCursorPosition(CGPoint { x, y });
        // ワープ先を CUR_POS へ反映(同期をワープ前に取ると境界値が残り
        // 復帰直後に必ず再突入して戻れなくなる)
        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (x, y);
        eprintln!("[return] -> mac ({x:.0},{y:.0})");
    }
}

fn send_msg(msg: &Msg) {
    DIAG_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
    if let Some(tx) = TX.get() {
        let line = encode(msg);
        let _ = tx.send(line);
    }
}

/// 送信の成否を返す版(未接続時に操作を通知したい UI から使う)
pub fn send_msg_reported(msg: &Msg) -> bool {
    DIAG_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
    match TX.get() {
        Some(tx) => tx.send(encode(msg)).is_ok(),
        None => false,
    }
}

/// ホットキー/メニューバーGUI からの手動トグル(F13 とメニューの共通経路)。
/// 切替状態遷移はここを含む既存6経路のまま(一元化はモジュール分割 Phase5 で実施)
fn do_toggle(reason: &str) {
    if !CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    let next = !WIN_MODE.load(Ordering::Relaxed);
    WIN_MODE.store(next, Ordering::Relaxed);
    if next {
        DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    eprintln!("[mode] {} ({reason})", if next { "WINDOWS" } else { "MAC" });
    if next {
        enter_win_mode_cursor_lock();
    } else {
        // 戻り先の高さは Windows 側カーソルの現在高さに合わせる
        let ny = {
            let wc = *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
            let (_ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
            if wh > 0.0 {
                (wc.1 / wh).clamp(0.0, 1.0)
            } else {
                0.5
            }
        };
        leave_win_mode_cursor_unlock(Some(ny));
    }
}

// ---------- イベントタップコールバック ----------
/// NSSystemDefined(Type 14)の内容を読む。CGEvent API に data1/subtype の
/// 取得フィールドが無いため、NSEvent(eventWithCGEvent:) 経由で読む。
/// F 行のメディアキーのときだけ呼ばれるため頻度は低い。subtype 8 以外は None
unsafe fn ns_media_event(event: CGEventRef) -> Option<(i64, i64)> {
    unsafe extern "C" {
        fn objc_retain(id: ID) -> ID;
        fn objc_release(id: ID);
    }
    let cls = objc_getClass(c"NSEvent".as_ptr());
    if cls.is_null() {
        return None;
    }
    let mk: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let get: unsafe extern "C" fn(ID, SEL) -> i64 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let e = mk(cls, sel_registerName(c"eventWithCGEvent:".as_ptr()), event);
    if e.is_null() {
        return None;
    }
    objc_retain(e);
    let sub = get(e, sel_registerName(c"subtype".as_ptr()));
    let data1 = get(e, sel_registerName(c"data1".as_ptr()));
    objc_release(e);
    if sub != 8 {
        return None; // 8 = NX_SUBTYPE_AUX_CONTROL_BUTTONS(メディアキー)
    }
    Some((sub, data1))
}

unsafe extern "C" fn tap_callback(
    _proxy: *mut core::ffi::c_void,
    event_type: u32,
    event: CGEventRef,
    _user_info: *mut core::ffi::c_void,
) -> CGEventRef {
    // Deskflow 同様、タイムアウトで無効化されたら再び有効化する(でないと抑制が静かに止まる)
    if event_type == 0xFFFFFFFE || event_type == 0xFFFFFFFD {
        if let Some(&tap) = TAP_PORT.get() {
            CGEventTapEnable(tap as CFMachPortRef, true);
        }
        return std::ptr::null_mut();
    }
    let win_mode = WIN_MODE.load(Ordering::Relaxed);
    let connected = CONNECTED.load(Ordering::Relaxed);

    // ホットキー(F13 既定 / TSUNAGU_HOTKEY_KC)= 手動トグル(常に有効、握る)。
    // 修飾キー(右Cmd=54 等)は flagsChanged として届くため、flags の該当ビットで
    // 押下/解放を判別し、押下側でのみトグルする(up・解放側は握るだけ)。
    // 旧実装は KEY_DOWN しかトグルせず(=修飾キー指定が機能しない)、かつ up 把握の
    // 分岐が外側条件により到達不能なデッドコードだった(レビュー Wave1-X5)
    if matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED) {
        let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
        // 本体キーボードの音量(F10-12)とメディア(F7-F9)キーは fn フラグ付きで届く。
        // Windows 側の音量・メディア操作として転送し、Mac 側の操作は握る。
        // fn 無しは F キーとしての使用のため素通り(誤転送防止)
        if event_type == EVT_KEY_DOWN && win_mode && CGEventGetFlags(event) & FLAG_FN != 0 {
            let op = match kc {
                72 => Some(0u8), // F12 音量 up
                73 => Some(1),   // F11 音量 down
                74 => Some(2),   // F10 ミュート
                100 => Some(3),  // F7 前の曲へ
                101 => Some(4),  // F8 再生・一時停止
                103 => Some(5),  // F9 次の曲へ
                _ => None,
            };
            if let Some(op) = op {
                send_msg(&Msg::Vol { op });
                return std::ptr::null_mut(); // Mac 側の音量・再生変更を抑制
            }
        }
        if kc == hotkey_kc() {
            let pressed = match event_type {
                EVT_KEY_DOWN => true,
                EVT_KEY_UP => false,
                _ => {
                    let flags = CGEventGetFlags(event);
                    match kc {
                        54 | 55 => flags & FLAG_CMD != 0,
                        58 | 61 => flags & FLAG_OPT != 0,
                        59 | 62 => flags & FLAG_CTRL != 0,
                        56 | 60 => flags & FLAG_SHIFT != 0,
                        _ => true,
                    }
                }
            };
            // キーリピートは down が連続で届くため、押下エッジ(未押下→押下)だけで
            // トグルする。押しっぱなしでの高速トグル暴発を防ぐ
            static HOTKEY_DOWN: AtomicBool = AtomicBool::new(false);
            if pressed && !HOTKEY_DOWN.swap(true, Ordering::Relaxed) && connected {
                do_toggle("hotkey");
            }
            if !pressed {
                HOTKEY_DOWN.store(false, Ordering::Relaxed);
            }
            return std::ptr::null_mut(); // トグル専用キーのため down/up 両方握る
        }
    }

    if matches!(
        event_type,
        EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED
    ) {
        DIAG_MOVE_COUNT.fetch_add(1, Ordering::Relaxed);
        LAST_EVENT_MS.store(now_ms(), Ordering::Relaxed);
    }
    if matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED) {
        DIAG_KEY_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    // 自己投稿のUPは元アプリだけに届け、物理的な押下や転送先の状態を変えない。
    if event_type == EVT_LEFT_UP
        && CGEventGetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA) == SYNTH_UP_MAGIC
    {
        return event;
    }
    // マウスボタンの押下状態はモードに関係なく追跡する(ドラッグ中切替の
    // 持ち込み判定に使う。Mac モードの素通し経路でも更新が必要)
    match event_type {
        EVT_LEFT_DOWN => {
            // FinderへDownを渡す前に基準値だけ取得する。URLの読み出しは別スレッド。
            // 押下後のポーリングで基準を作ると、既に始まったドラッグを見逃す。
            let baseline = if !win_mode && connected {
                with_pool(|| {
                    let pb = drag_pasteboard();
                    (!pb.is_null())
                        .then(|| msg0_isize(pb, sel_registerName(c"changeCount".as_ptr())))
                })
            } else {
                None
            };
            FILE_DRAG
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .begin(baseline);
            BTN_DOWN[0].store(true, Ordering::Relaxed);
        }
        EVT_LEFT_DRAGGED => FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).moved(),
        EVT_LEFT_UP => {
            FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).end();
            BTN_DOWN[0].store(false, Ordering::Relaxed);
        }
        EVT_RIGHT_DOWN => BTN_DOWN[1].store(true, Ordering::Relaxed),
        EVT_RIGHT_UP => BTN_DOWN[1].store(false, Ordering::Relaxed),
        EVT_OTHER_DOWN => BTN_DOWN[2].store(true, Ordering::Relaxed),
        EVT_OTHER_UP => BTN_DOWN[2].store(false, Ordering::Relaxed),
        _ => {}
    }

    if !win_mode {
        // Mac モード: 右端到達で Windows モードへ。
        // Deskflow onMouseMove 準拠: イベント位置はキュー滞留で数フレーム遅れるため、
        // CGEventCreate(NULL) のライブカーソル位置で判定する(境界の応答性の鍵)
        // ファイル掴み中(ドラッグペーストボードにファイル+左ボタン押下)は設定に
        // 依らず常に「掴んだまま境界越え」を許可する(掴んでいる意図が明確なため)
        let file_drag_ready = FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).ready();
        // 受信ドラッグ進行中は掴み検出を無効化する: 受信ドラッグ自身が
        // ドラッグ用ペーストボードを変えるため、押下と無関係に偽の掴みになる
        let file_drag_ready = file_drag_ready && !incoming_drag::blocking();
        let drag_ok = DRAG_SWITCH.load(Ordering::Relaxed) || file_drag_ready;
        if (matches!(event_type, EVT_MOUSE_MOVED)
            || (drag_ok
                && matches!(event_type, EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED)))
            && connected
            && !HOTKEY_ONLY.load(Ordering::Relaxed) // hotkey モードでは境界切替しない(ロック)
            && now_ms() >= EDGE_GUARD_UNTIL_MS.load(Ordering::Relaxed)
        {
            {
                // delta 積算でカーソル位置を追跡(Deskflow の m_xCursor 方式)。
                // 32イベントに1回ライブ位置へ同期しドリフトを補正する
                let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
                let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
                // 速度計装: 100ms を超えたら窓を作り直す(§24 のデータ取り)
                {
                    let now = now_ms();
                    let mut r = RECENT_PX.lock().unwrap_or_else(|e| e.into_inner());
                    if now.saturating_sub(r.1) > 100 {
                        *r = (0.0, now);
                    }
                    r.0 += dx.abs() + dy.abs();
                }
                let n = CUR_SYNC_N.fetch_add(1, Ordering::Relaxed);
                let mut pos = CUR_POS.lock().unwrap_or_else(|e| e.into_inner());
                pos.0 += dx;
                pos.1 += dy;
                if n % 16 == 0 {
                    if let Some(loc) = live_cursor() {
                        *pos = (loc.x, loc.y);
                    }
                }
                let (px, py) = *pos;
                drop(pos);
                let edge = edge_px();
                let g = geo();
                let dir = side_dir();
                let (lay_lo, lay_hi) = *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner());
                // SIDE に応じた「切替境界までの距離」(小さいほど端に近い)。
                // Deskflow の links 相当+斜め(4-7)は境界の半分(上/下)でのみ接続
                let gap = |x: f64, y: f64| -> f64 {
                    let base = g.gap(dir, x, y);
                    if base > edge + 40.0 {
                        return base; // 境界から遠い=範囲判定不要
                    }
                    // 境界付近でのみ接続範囲(配置エディタ/斜め配置)の制限を適用する。
                    // 範囲は出口ディスプレイの辺に沿った比率で測る
                    let f = g.along_ratio(dir, x, y);
                    if f < lay_lo || f > lay_hi {
                        return f64::MAX; // Windows 画面が接している範囲外
                    }
                    base
                };
                // 境界から十分内側へ戻ったらヒット状態をリセット(次の到達を1回目として数える)
                if gap(px, py) > edge + 8.0 {
                    EDGE_AT_EDGE.store(false, Ordering::Relaxed);
                    EDGE_STAY_SINCE_MS.store(0, Ordering::Relaxed);
                }
                if gap(px, py) <= edge {
                    // 受信ドラッグ(NSDraggingSession)進行中: 物理ボタンは押されたまま
                    // Mac 側のドロップを続けるため、ここでは Windows へ自動切替しない。
                    // 切替すると入力転送がセッションからマウス入力を奪い、ended も
                    // ドロップも来なくなる(実測)。掴んだまま境界に触れた場合は
                    // ボタンを離して終わらせるのが意図された復帰操作
                    if incoming_drag::blocking() {
                        // 操作を宙吊りにした理由が利用者に分かるよう、
                        // 初回だけ案内を出す(連投しない)
                        static DRAG_GUIDE_MS: AtomicU64 = AtomicU64::new(0);
                        let now = now_ms();
                        if now.saturating_sub(DRAG_GUIDE_MS.swap(now, Ordering::Relaxed)) >= 120_000
                        {
                            notify("tsunagu", "ファイルを掴んだままです。Mac のドロップ先でボタンを離すとそこへ置けます(境界では切り替わりません)");
                        }
                        return event;
                    }
                    // switchCorners(+cornerSize): 四隅 N px 内では切替しない(誤爆防止)
                    let corner = CORNER_PX.load(Ordering::Relaxed) as f64;
                    if corner > 0.0
                        && (px < g.min_x + corner || px > g.max_x - corner)
                        && (py < g.min_y + corner || py > g.max_y - corner)
                    {
                        return event;
                    }
                    // 二段階判定: 積算値が閾値を超えても、実カーソル(ライブ位置)が
                    // 境界付近でなければ発火しない。ドリフトが残っていても
                    // MacBook 中央などでの誤発火を構造的に防ぐ
                    let Some(loc) = live_cursor() else {
                        return event;
                    };
                    if gap(loc.x, loc.y) > edge + 40.0 {
                        // 積算ドリフト検出: 実位置で CUR_POS を補正して通過
                        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (loc.x, loc.y);
                        return event;
                    }
                    // Crossing Intelligence(ビジョン§24): 境界への速度が十分大きければ
                    // 「意図的な越え」とみなして滞在待ち(switchDelay)とダブルタップを
                    // スキップする。ゆっくり端に触れた場合だけ従来どおりの誤爆防止が働く。
                    // 計装(下のログ)で集めた分布をもとに閾値は 1200px/s とする。
                    // `TSUNAGU_FAST_EDGE=0` で無効化
                    let speed = {
                        let r = *RECENT_PX.lock().unwrap_or_else(|e| e.into_inner());
                        px_per_sec(r.0, now_ms().saturating_sub(r.1))
                    };
                    let fast = FAST_EDGE.load(Ordering::Relaxed) && speed >= 1200.0;
                    // switchDelay: 端に N ms 滞ってから切替(0=無効)。
                    // 滞在判定は「端に到達し続けている」間のみ継続する
                    let delay = SWITCH_DELAY_MS.load(Ordering::Relaxed);
                    if delay > 0 && !fast {
                        let now = now_ms();
                        let since = EDGE_STAY_SINCE_MS.load(Ordering::Relaxed);
                        if since == 0 {
                            EDGE_STAY_SINCE_MS.store(now, Ordering::Relaxed);
                            return event; // 滞在計測を開始(まだ切替ない)
                        }
                        if now.saturating_sub(since) < delay {
                            return event; // まだ規定時間に達していない
                        }
                        EDGE_STAY_SINCE_MS.store(0, Ordering::Relaxed);
                    } else if delay == 0 && !fast && !EDGE_AT_EDGE.swap(true, Ordering::Relaxed) {
                        // switchDoubleTap: 閾値を「下から跨いだ瞬間」だけをヒットと数え、
                        // 判定窓(DOUBLE_TAP_MS)以内の 2回目のヒットでのみ切替する。
                        // カーソルが境界に張り付いたまま出す delta は継続扱いで数えない
                        let taps = EDGE_TAPS.load(Ordering::Relaxed);
                        let now = now_ms();
                        let win_ms = DOUBLE_TAP_MS.load(Ordering::Relaxed).max(100);
                        let prev = EDGE_LAST_HIT_MS.swap(now, Ordering::Relaxed);
                        let fire = taps <= 1 || (prev > 0 && now.saturating_sub(prev) <= win_ms);
                        if !fire {
                            // 1回目: 境界から少し内側へ弾き返す。壁に当たった感触で
                            // 「もう一度押すと通る」ことを体感させる(本質の可視化)
                            eprintln!("[edge] 1回目の到達(跳ね返し)");
                            // 配置番号(0-7)ではなく方向で判定する(旧実装は左上/左下配置で
                            // 右端へ弾き返していた)
                            let (bx, by) = match dir {
                                1 => (g.min_x + 15.0, loc.y),
                                2 => (loc.x, g.min_y + 15.0),
                                3 => (loc.x, g.max_y - 15.0),
                                _ => (g.max_x - 15.0, loc.y),
                            };
                            CGWarpMouseCursorPosition(CGPoint { x: bx, y: by });
                            *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (bx, by);
                            EDGE_AT_EDGE.store(false, Ordering::Relaxed);
                            return event;
                        }
                        EDGE_LAST_HIT_MS.store(0, Ordering::Relaxed);
                    }
                    WIN_MODE.store(true, Ordering::Relaxed);
                    DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
                    // 到達時の速度を添える(§24 用の計装。速い=意図的な越え、
                    // 遅い=停止しようとして端に触れた、の分布を実機で見る)
                    // fast 経路は滞在/ダブルタップをスキップしたことが分かるよう明記する
                    eprintln!(
                        "[mode] WINDOWS (edge{}) at ({:.0},{:.0}) v={speed:.0}px/s",
                        if fast { ", fast" } else { "" },
                        loc.x,
                        loc.y
                    );

                    // ドラッグ中の切替: 既定は全ボタンを離して持ち込まない(誤ドラッグ防止。
                    // レビュー Wave1 C-S13)。TSUNAGU_DRAG_SWITCH=1 では逆に押下中の
                    // ボタンを Windows 側で押し直す=「掴んだまま境界を越える」体験。
                    // 掴みドラッグ中は EVT_MOUSE_MOVED 由来の切替でも持ち込む
                    // (押下直後の軽い移動は MOVED として届くことがある=実測。
                    // 持ち込み漏れは Win 側のフォールバックを誘発する)
                    if event_type != EVT_MOUSE_MOVED || file_drag_ready {
                        if drag_ok {
                            for b in 0u8..=2 {
                                if BTN_DOWN[b as usize].load(Ordering::Relaxed) {
                                    send_msg(&Msg::MouseButton { btn: b, down: true });
                                }
                            }
                        } else {
                            for b in 0u8..=2 {
                                send_msg(&Msg::MouseButton {
                                    btn: b,
                                    down: false,
                                });
                            }
                        }
                    }
                    // ファイル掴み切替: 掴んだファイルを Windows へ流し、Mac 側の
                    // ドラッグは合成 LeftMouseUp で完結させる(Finder の宙吊り防止)。
                    // UP は tap コールバック内で post できないため別スレッド投稿。
                    // 投稿イベントは自分の HID タップを再通過する(定番の再帰問題)ため
                    // kCGEventSourceUserData にマジックを刻み、tap 側で識別して
                    // 「Mac へ素通し・Windows へは転送しない」処理をする(転送すると
                    // 押したままのユーザー意図に反して Win 側が離した扱いになる)
                    let dragged_files = FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Some(files) = dragged_files {
                        if !files.is_empty() {
                            // ⌘C ポーリング経由の再送を指紋で抜く(通常は載らないが保険)
                            let key = mac_files_key(&files);
                            *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = key;
                            eprintln!("[file] 掴みドラッグ切替: {} 件を転送します", files.len());
                            history_push_files(&files, "Mac");
                            send_files_to_win(files, true);
                        }
                        std::thread::spawn(|| {
                            std::thread::sleep(Duration::from_millis(60));
                            unsafe {
                                let pos = live_cursor().unwrap_or(CGPoint { x: 0.0, y: 0.0 });
                                let e = make_drag_end_event(pos);
                                if !e.is_null() {
                                    CGEventPost(0 /* kCGHIDEventTap */, e);
                                    CFRelease(e as *mut core::ffi::c_void);
                                }
                            }
                        });
                    }
                    // 前回 Windows に出た位置があればそこへ戻し、なければ境界の対応高さへ。
                    // 高さは出口ディスプレイの y 範囲で正規化(反転なし: 画面上端同士が対応)
                    let (mut nx, mut ny) = *LAST_WIN_POS.lock().unwrap_or_else(|e| e.into_inner());
                    if nx < 0.0 {
                        // 初回: 越えた境界の対応位置から入る(Windows 側の反対の辺)
                        let r = g.along_ratio(dir, loc.x, loc.y);
                        (nx, ny) = match dir {
                            1 => (0.95, r),
                            2 => (r, 0.95),
                            3 => (r, 0.05),
                            _ => (0.05, r),
                        };
                    }
                    send_msg(&Msg::Warp { nx, ny });
                    eprintln!("[warp] -> win ({:.2},{:.2})", nx, ny);
                    // 絶対位置モードの仮想カーソルを Warp 先で初期化
                    {
                        let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                        *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner()) = (nx * ww, ny * wh);
                    }
                    enter_win_mode_cursor_lock();
                    return std::ptr::null_mut();
                }
            }
        }
        // Search My Desk(ビジョン§14): Mac 操作中の ⌥⌘S でデスク横断検索を
        // 開く(アプリ・履歴・URL)。押下エッジで発火し down/up を握る
        //(Continue Here と同じ。up 単体が前面アプリへ漏れないようにする)。
        // ※境界判定の if はマウス系イベントしか通さないため、その内側に置くと
        // KEY_DOWN が絶対に届かず発火しない(実測: 検索窓が開かなかった)
        if DESK_SEARCH.load(Ordering::Relaxed) && matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP) {
            let fl = CGEventGetFlags(event);
            if fl & FLAG_OPT != 0
                && fl & FLAG_CMD != 0
                && fl & FLAG_CTRL == 0
                && fl & FLAG_SHIFT == 0
            {
                let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
                if kc == 1
                /* S */
                {
                    static SEARCH_S_DOWN: AtomicBool = AtomicBool::new(false);
                    let down = event_type == EVT_KEY_DOWN;
                    if down && !SEARCH_S_DOWN.swap(true, Ordering::Relaxed) {
                        eprintln!("[search] ⌥⌘S 検出: 検索窓の表示を依頼します");
                        dispatch_show_search();
                    }
                    if !down {
                        SEARCH_S_DOWN.store(false, Ordering::Relaxed);
                    }
                    return std::ptr::null_mut();
                }
            }
        }
        // 検索窓が開いている間は文字キーを握って field へ直接積む(IME 経由だと
        // 日本語モードでは確定・変換に消費されて検索できない=実測)。
        // ⌘/⌥/⌃/Fn 付きはシステムショートカットのため素通しする
        if DESK_SEARCH.load(Ordering::Relaxed)
            && matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP)
            && gui::search_open()
        {
            let fl = CGEventGetFlags(event);
            let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
            let down = event_type == EVT_KEY_DOWN;
            // ⌥Enter: 先頭候補を Windows へ投げる(ビジョン§13 Throw)。
            // 装飾キーなし判定の外で先に処理する(でないると ⌥Enter が
            // システムショートカットとして素通ししてしまう)
            if kc == 36 && fl & FLAG_OPT != 0 && fl & (FLAG_CMD | FLAG_CTRL | FLAG_FN) == 0 {
                if down && !SEARCH_ENTER_DOWN.swap(true, Ordering::Relaxed) {
                    SEARCH_ENTER_OPT.store(true, Ordering::Relaxed);
                    eprintln!("[search] ⌥Enter: 先頭候補を Windows へ投げます");
                    gui::dispatch_search_enter();
                }
                if !down {
                    SEARCH_ENTER_DOWN.store(false, Ordering::Relaxed);
                }
                return std::ptr::null_mut();
            }
            if fl & (FLAG_CMD | FLAG_OPT | FLAG_CTRL | FLAG_FN) == 0 {
                match kc {
                    // Enter: 先頭候補を実行(押下エッジ。up は握る)
                    36 => {
                        if down && !SEARCH_ENTER_DOWN.swap(true, Ordering::Relaxed) {
                            gui::dispatch_search_enter();
                        }
                        if !down {
                            SEARCH_ENTER_DOWN.store(false, Ordering::Relaxed);
                        }
                        return std::ptr::null_mut();
                    }
                    // Backspace: 末尾 1 文字削除
                    51 => {
                        if down {
                            gui::dispatch_search_backspace();
                        }
                        return std::ptr::null_mut();
                    }
                    // ↑↓: 候補の選択を動かす(Enter は選択位置を実行)。
                    // 以前は矢印を握らず Enter が常に先頭候補だったため、
                    // 先頭のアプリを誤起動する事故が起きた(実績)
                    125 | 126 => {
                        if down {
                            gui::dispatch_search_arrow(kc == 125);
                        }
                        return std::ptr::null_mut();
                    }
                    // Esc: 検索窓を閉じる(トグル表示で閉じる)
                    53 => {
                        if down && !SEARCH_ESC_DOWN.swap(true, Ordering::Relaxed) {
                            dispatch_show_search();
                        }
                        if !down {
                            SEARCH_ESC_DOWN.store(false, Ordering::Relaxed);
                        }
                        return std::ptr::null_mut();
                    }
                    _ => {
                        if let Some(ch) = keychar(kc, fl & FLAG_SHIFT != 0) {
                            if down {
                                gui::dispatch_search_text(ch);
                            }
                            // keyUp も握る(IME・前面アプリへ漏れないように)
                            return std::ptr::null_mut();
                        }
                    }
                }
            }
        }
        return event; // 素通し
    }

    // Windows モード: 全イベントを握って転送
    let flags = CGEventGetFlags(event);
    // F 行のメディア(NSSystemDefined, subtype 8)の翻訳: 輝度(F1/F2)とキーボード
    // 照明(F5/F6)は macOS が key イベントではなく system-defined で配るため、
    // key 経路だけだと Windows で反応しない(実績: F5 等が効かなかった)。
    // 対応する F キーとして届け直す。音量・再生(F7〜F12)は key 経路
    //(kc 72-74/100/101/103)で処理済みのためここでは転送しない(二重送信の防止)
    if event_type == EVT_SYSTEM_DEFINED {
        if let Some((_, data1)) = ns_media_event(event) {
            let nx = (data1 >> 16) & 0xFFFF;
            let down = ((data1 >> 8) & 0xFF) == 0x0A; // 0x0A=押下 / 0x0B=解放
            let kc: u16 = match nx {
                3 => 122, // 輝度を下げる → F1
                2 => 120, // 輝度を上げる → F2
                22 => 96, // キーボード照明を下げる → F5
                21 => 97, // キーボード照明を上げる → F6
                // 未対応タイプ(聴写キー等がここに来る機種あり)は記録して次の
                // 対応表追加に備える。毎回 1 行だけで洪水にはならない
                _ => {
                    eprintln!(
                        "[media] 未対応 nx={nx}({})",
                        if down { "down" } else { "up" }
                    );
                    0
                }
            };
            if kc != 0 {
                send_msg(&Msg::Key {
                    kc,
                    down,
                    ctrl: false,
                    opt: false,
                    cmd: false,
                    shift: false,
                    tr: false,
                    rcmd: false,
                });
                eprintln!(
                    "[media] nx={nx} -> kc={kc}({})",
                    if down { "down" } else { "up" }
                );
            }
        }
        return std::ptr::null_mut();
    }
    let (ctrl, opt, cmd, shift) = (
        flags & FLAG_CTRL != 0,
        flags & FLAG_OPT != 0,
        flags & FLAG_CMD != 0,
        flags & FLAG_SHIFT != 0,
    );
    match event_type {
        EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED => {
            let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
            let down = if event_type == EVT_FLAGS_CHANGED {
                // flagsChanged は「その修飾が押された」イベントのみ来る(離す時は flags から消える)
                // 押下状態は flags から判定
                match kc {
                    54 | 55 => cmd,
                    56 | 60 => shift,
                    58 | 61 => opt,
                    59 | 62 => ctrl,
                    _ => false,
                }
            } else {
                event_type == EVT_KEY_DOWN
            };
            // 右⌘(kc 54)の押下状態を追跡し、押下中は cmd を rcmd(右 Ctrl)へ
            // 置き換えて送る。左⌘(55)は従来どおり cmd のまま。combos でも
            // 「右⌘+C = 右 Ctrl+C」になる(rcmd 押下中は cmd を落とす)
            if event_type == EVT_FLAGS_CHANGED && kc == 54 {
                R_RIGHT_CMD.store(cmd, Ordering::Relaxed);
            }
            let rcmd = RCMD_CTRL.load(Ordering::Relaxed) && R_RIGHT_CMD.load(Ordering::Relaxed);
            let cmd = if rcmd { false } else { cmd };
            // 実機のキーコード特定用: 「右⌘が効かない」報告の切り分け。
            // ここに出ない=そのキーは 54 ではない(外付けの配列差・キーリマップ等)
            if event_type == EVT_FLAGS_CHANGED && (kc == 54 || kc == 55) {
                eprintln!(
                    "[rcmd] kc={} -> {}(rcmd={})",
                    kc,
                    if cmd || rcmd { "押下" } else { "解放" },
                    rcmd
                );
            }
            if down && (kc == 104 || kc == 102) {
                eprintln!(
                    "[ime] kc={kc} ({}) 転送",
                    if kc == 104 { "かな" } else { "英数" }
                );
            }
            // Caps Lock は Mac では押すたびに flagsChanged が 1 回だけ来る(押下/解放の
            // 区別がない)。Windows はキーの押し離しでトグルするため 1 回を down+up に展開する
            if event_type == EVT_FLAGS_CHANGED && kc == 57 {
                for d in [true, false] {
                    send_msg(&Msg::Key {
                        kc,
                        down: d,
                        ctrl,
                        opt,
                        cmd,
                        shift,
                        tr: false,
                        rcmd,
                    });
                }
                return std::ptr::null_mut();
            }
            // Continue Here(ビジョン§11): ⌥⌘T で Mac の前面ブラウザの URL を
            // Windows の既定ブラウザで開く。「Mac で見ていたページを Windows でもう
            // 一度探す」摩擦を 1 回で消す。osascript が 100-300ms かかるため
            // タップを塞がないよう別スレッドで取得する。
            // 押下エッジだけで発火する(キーリピートで osascript とタブが
            // 連発するのを防ぐ)。up も握る(down だけ握ると up 単体が
            // Windows へ転送され、修飾の押し替えが前面アプリへ漏れる)
            if kc == 17 && opt && cmd && !ctrl && !shift && CONTINUE_HERE.load(Ordering::Relaxed) {
                static CONT_T_DOWN: AtomicBool = AtomicBool::new(false);
                let down = event_type == EVT_KEY_DOWN;
                if down && !CONT_T_DOWN.swap(true, Ordering::Relaxed) {
                    // 直近の発火から 1.5 秒は再送しない(押し直しの連打対策)
                    static LAST_FIRE_MS: AtomicU64 = AtomicU64::new(0);
                    let now = now_ms();
                    if now.saturating_sub(LAST_FIRE_MS.load(Ordering::Relaxed)) >= 1_500 {
                        LAST_FIRE_MS.store(now, Ordering::Relaxed);
                        std::thread::spawn(continue_here);
                    }
                }
                if !down {
                    CONT_T_DOWN.store(false, Ordering::Relaxed);
                }
                return std::ptr::null_mut();
            }
            // ---- Mac 流ショートカットの Windows 翻訳(指癖をそのまま通す) ----
            // 元キーは握りつぶし、翻訳先の Key を送る。修飾の対応:
            //   cmd→Win Ctrl / opt→Win Alt / ctrl→Win キー(既定マップ)
            // 注意: flagsChanged(mod キー単体)は翻訳しない。
            // 常時有効(マスト機能)。TSUNAGU_MAC_KEYS=0 でのみオフ
            if event_type != EVT_FLAGS_CHANGED && MAC_KEYS.load(Ordering::Relaxed) {
                // 翻訳先の修飾は「既定マップ(cmd→Ctrl / opt→Alt)」で解釈させる。
                // CMD_ALT=true でも翻訳の意味が変わらないよう、cmd/opt を差し替える
                let swap = crate::CMD_ALT.load(Ordering::Relaxed);
                let send = |kc2: u16, d: bool, c: bool, o: bool, m: bool, sh: bool, r: bool| {
                    let (c2, o2, m2) = if swap { (c, m, o) } else { (c, o, m) };
                    send_msg(&Msg::Key {
                        kc: kc2,
                        down: d,
                        ctrl: c2,
                        opt: o2,
                        cmd: m2,
                        shift: sh,
                        tr: true,
                        rcmd: r,
                    });
                };
                // fn+F11(Mac のデスクトップ表示)= Win+D(FN フラグは表の外)
                if kc == 103 && flags & FLAG_FN != 0 {
                    send(2, down, true, false, false, false, rcmd); // D + ctrl フラグ(Win キー)
                    return std::ptr::null_mut();
                }
                if let Some((kc2, c, o, m, s)) = mac_shortcut_translation(kc, ctrl, opt, cmd, shift)
                {
                    send(kc2, down, c, o, m, s, rcmd);
                    return std::ptr::null_mut(); // 元キーは送らない
                }
            }
            send_msg(&Msg::Key {
                kc,
                down,
                ctrl,
                opt,
                cmd,
                shift,
                tr: false,
                rcmd,
            });
        }
        EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED => {
            let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
            let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
            if dx != 0.0 || dy != 0.0 {
                let sc = mouse_scale();
                if MOUSE_ABS_MODE.load(Ordering::Relaxed) && !GAME_REL.load(Ordering::Relaxed) {
                    // 絶対位置モード: Mac の加速済み delta に Windows 側の加速が
                    // 二重に乗るのを防ぎつつ、画面比率で見た目の移動距離を揃える
                    let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                    let (mw, mh) = {
                        let g = geo();
                        (g.main_w, g.main_h)
                    };
                    let (sx, sy) = (ww / mw, wh / mh); // 方向別スケール(改善B)
                                                       // 重要: WIN_CUR のガードをこのブロック内で必ず解放してから
                                                       // leave_win_mode_cursor_unlock を呼ぶ(内部で WIN_CUR を再ロック
                                                       // するため、保持したまま呼ぶと自己デッドロックでタップが固まる)
                    let (nx, ny, at_left) = {
                        let mut wc = WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
                        wc.0 = (wc.0 + dx * sc * sx).clamp(0.0, ww - 2.0);
                        wc.1 = (wc.1 + dy * sc * sy).clamp(0.0, wh - 2.0);
                        // Mac 側へ戻る辺は Windows の位置で決まる(右配置なら Windows の左端、
                        // 左配置なら右端、上配置なら下端、下配置なら上端)。旧実装は常に
                        // 左端で判定し、上下配置では Windows の左端に触れるだけで戻っていた
                        let at_edge = match side_dir() {
                            1 => wc.0 >= ww - 3.0,
                            2 => wc.1 >= wh - 3.0,
                            3 => wc.1 <= 2.0,
                            _ => wc.0 <= 2.0,
                        };
                        (
                            wc.0 / ww,
                            wc.1 / wh,
                            !HOTKEY_ONLY.load(Ordering::Relaxed)
                                && event_type == EVT_MOUSE_MOVED
                                && at_edge,
                        )
                    };
                    // 毎イベント送信(量子化スキップは低速時にステップ感が出るため廃止)
                    LAST_ABS_MS.store(now_ms(), Ordering::Relaxed);
                    DIAG_ABS_COUNT.fetch_add(1, Ordering::Relaxed);
                    send_msg(&Msg::MouseAbs { nx, ny });
                    // 左端到達はMac内完結で即復帰(Win往復のRTT分を削減)
                    // ドラッグ中は意図しない復帰をしない(ボタン操作中の境界越えのため)
                    if at_left {
                        WIN_MODE.store(false, Ordering::Relaxed);
                        DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
                        eprintln!("[mode] MAC (abs-edge)");
                        // 境界に沿った比率: 左右の辺は縦位置、上下の辺は横位置
                        leave_win_mode_cursor_unlock(Some(if side_dir() >= 2 { nx } else { ny }));
                    }
                } else {
                    // 相対移動モード(従来互換)
                    send_msg(&Msg::MouseMove {
                        dx: dx * sc,
                        dy: dy * sc,
                    });
                }
            }
            // カーソル固定の巻き戻しは 200ms 監視スレッドに集約した
            // (タップ内で毎イベント CGEventCreate すると負荷でカクつくため)
        }
        EVT_LEFT_DOWN | EVT_LEFT_UP => {
            let d = event_type == EVT_LEFT_DOWN;
            BTN_DOWN[0].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 0, down: d });
        }
        EVT_RIGHT_DOWN | EVT_RIGHT_UP => {
            let d = event_type == EVT_RIGHT_DOWN;
            BTN_DOWN[1].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 1, down: d });
        }
        EVT_OTHER_DOWN | EVT_OTHER_UP => {
            let d = event_type == EVT_OTHER_DOWN;
            BTN_DOWN[2].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 2, down: d });
        }
        EVT_SCROLL_WHEEL => {
            let dy = CGEventGetIntegerValueField(event, FIELD_SCROLL_A1) as f64;
            let dx = CGEventGetIntegerValueField(event, FIELD_SCROLL_A2) as f64;
            // 2本指の横スワイプ →「戻る/進む」。Mac の体感(1スワイプ=1ページ)を
            // 忠実に再現する: ジェスチャは「300ms イベントが途切れるまで」を一続きと
            // みなし、その間の発火は 1 回だけ(指を離した後の慣性 delta が届いても
            // 再発火しない=2段階戻りの防止)。閾値 60px・横優勢(|dx|*2>|dy|)のみ。
            // TSUNAGU_SWIPE_NAV=0 で従来の横ホイールへ戻せる
            let swipe_nav = SWIPE_NAV.load(Ordering::Relaxed);
            if swipe_nav && win_mode && dx != 0.0 && dx.abs() * 2.0 > dy.abs() {
                let now = now_ms();
                let mut acc = SWIPE_ACC.lock().unwrap_or_else(|e| e.into_inner());
                // 前回のイベントから 300ms 以上空いていたら新しいジェスチャ
                // (=累積と発火済みフラグの両方を引き直す)
                if now.saturating_sub(acc.1) > 300 {
                    if acc.2 != 0 {
                        eprintln!("[swipe] gesture 追加分={:.0}(発火済みのため不採用)", acc.0);
                    } else if acc.0.abs() > 8.0 {
                        eprintln!("[swipe] gesture total={:.0}(未達)", acc.0);
                    }
                    *acc = (0.0, now, 0);
                }
                acc.0 += dx;
                acc.1 = now;
                // acc.2 != 0 = このジェスチャで発火済み。以後の累積は破棄扱い
                if acc.2 == 0 && acc.0.abs() >= 60.0 {
                    // Mac の操作感: 指を右へスワイプ(ページを左へめくる)=戻る。
                    // dx>0=指右 → XButton1(戻る)、dx<0=指左 → XButton2(進む)
                    let btn = if acc.0 > 0.0 { 3u8 } else { 4 }; // 3=戻る, 4=進む
                    send_msg(&Msg::MouseButton { btn, down: true });
                    send_msg(&Msg::MouseButton { btn, down: false });
                    eprintln!(
                        "[swipe] {} 送信(total={:.0})",
                        if btn == 3 { "戻る" } else { "進む" },
                        acc.0
                    );
                    *acc = (0.0, now, now); // 発火済みマーク(ジェスチャ完結まで保持)
                }
                // 横優勢ジェスチャはここで完結(縦の揺れも無視し二重発火を防ぐ)
                return std::ptr::null_mut();
            }
            if dx != 0.0 || dy != 0.0 {
                // ピクセル delta → ノッチ単位へ累積変換。0.05ノッチ(=6 wheel units)刻みで
                // 送る=Windows のプレシジョンタッチパッドと同じ高解像度スクロール。
                // 0.25刻み(30 units)は低速スクロールがカクつくため細かくした。
                // 除数を大きくすると遅くなる(設定ウィンドウのスライダーで可変)。端数は持ち越し
                // 互換モードは 1 ノッチ(120)単位に量子化(旧来のホイール相当)。
                // 既定は 0.05 ノッチ(=6 wheel units)の高解像度
                let q: f64 = if SCROLL_COMPAT.load(Ordering::Relaxed) {
                    1.0
                } else {
                    0.05
                };
                let div = scroll_div();
                // 方向: 既定は Mac の操作感に合わせる(自然スクロール設定を起動時に
                // 取得)。SCROLL_FLIP=true は「Windows 標準」への手動上書き。
                // 実測: 自然スクロール環境で Mac と同じ向きになるのは -1 側
                let aligned = if NATURAL_SCROLL.load(Ordering::Relaxed) {
                    -1.0
                } else {
                    1.0
                };
                let sgn = if SCROLL_FLIP.load(Ordering::Relaxed) {
                    -aligned
                } else {
                    aligned
                };
                let mut acc = SCROLL_ACC.lock().unwrap_or_else(|e| e.into_inner());
                acc.0 += sgn * dx / div;
                acc.1 += sgn * dy / div;
                // 異常な残高(1e6超)は何かの暴発なので捨てる
                if acc.0.abs() > 1.0e6 || acc.1.abs() > 1.0e6 {
                    *acc = (0.0, 0.0);
                }
                let (ix, iy) = ((acc.0 / q).trunc() * q, (acc.1 / q).trunc() * q);
                if ix != 0.0 || iy != 0.0 {
                    acc.0 -= ix;
                    acc.1 -= iy;
                    DIAG_SCROLL_COUNT.fetch_add(1, Ordering::Relaxed);
                    send_msg(&Msg::Scroll { dx: ix, dy: iy });
                }
            }
        }
        _ => {}
    }
    std::ptr::null_mut() // 握りつぶす
}

/// 表示用のリリースバージョン(設定ウィンドウ等)
pub const VERSION_STR: &str = env!("CARGO_PKG_VERSION");
const BUILD_ID: &str = "build-20260927-201932-68d4857";

fn main() {
    eprintln!("[info] tsunagu-mac {BUILD_ID}");
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--probe-search") {
        // Search My Desk の実機検証。AppKit を使うため対話セッション(GUI)で実行する
        let ok = if gui::start() {
            gui::probe_search()
        } else {
            eprintln!("[probe-search] AppKit を初期化できません(対話セッションで実行してください)");
            false
        };
        eprintln!("[probe-search] {}", if ok { "OK" } else { "FAILED" });
        std::process::exit(if ok { 0 } else { 1 });
    }
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-incoming-drag") {
        incoming_drag::probe();
        return;
    }
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-setup") {
        gui::setup::probe_invitation();
        return;
    }
    if args.iter().any(|a| a == "--preview-setup") {
        let _ = gui::setup::first_run(true);
        return;
    }
    if args.iter().any(|a| a == "--preview-ui") {
        gui::UI_PREVIEW.store(true, Ordering::Relaxed);
        if gui::start() {
            gui::SHOW_AT_START.store(true, Ordering::Relaxed);
            unsafe { gui::run_app() };
        }
        return;
    }
    gui::restore_preferences();

    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);
    // 既存のenvを優先。新規利用者だけOS保護の接続キーと初回導入を使用する。
    let no_gui = args.iter().any(|a| a == "--no-gui")
        || envutil::get("TSUNAGU_NO_GUI").is_some_and(|v| v == "1");
    let mut registered_now = false;
    // トークンは双方向の共有鍵。ハンドシェイクの成否が oracle になるため短い
    // トークンは LAN 内の総当たりで破られる。128bit 相当(32 文字)を下限に
    let token = if let Some(t) = envutil::get("TSUNAGU_TOKEN").filter(|t| t.len() >= 32) {
        t
    } else if envutil::get("TSUNAGU_TOKEN").is_some_and(|t| !t.is_empty()) {
        eprintln!("[fatal] TSUNAGU_TOKEN が短すぎます(32 文字未満)。scripts/gen-token.sh で生成してください");
        std::process::exit(1);
    } else {
        match tsunagu_common::credentials::load() {
            Ok(Some(t)) => t,
            Ok(None) if !no_gui => match gui::setup::first_run(false) {
                Some(t) => {
                    registered_now = true;
                    t
                }
                None => return,
            },
            Ok(None) => {
                eprintln!("[setup] 接続キー未設定。GUIで初回登録を完了してください。");
                return;
            }
            Err(_) => {
                if !no_gui {
                    gui::setup::error("保存した接続キーを読み取れません。キーチェーンのアクセス許可を確認してください。");
                }
                eprintln!("[setup] credential store unavailable");
                return;
            }
        }
    };

    if !no_gui && !gui::setup::ensure_permission() {
        return;
    }
    refresh_geo();
    let (screen_w, screen_h) = {
        let g = geo();
        (g.main_w, g.main_h)
    };
    unsafe { CGDisplayRegisterReconfigurationCallback(display_reconfigured, std::ptr::null_mut()) };
    unsafe {
        if let Some(loc) = live_cursor() {
            *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (loc.x, loc.y);
        }
    }
    if let Some(d) = envutil::get("TSUNAGU_SCROLL_DIV").and_then(|v| v.parse::<f64>().ok()) {
        if d > 0.0 {
            set_scroll_div(d);
        }
    }
    if let Some(m) = envutil::get("TSUNAGU_MOUSE_SCALE").and_then(|v| v.parse::<f64>().ok()) {
        if m > 0.0 {
            set_mouse_scale(m);
        }
    }
    if let Some(e) = envutil::get("TSUNAGU_EDGE_PX").and_then(|v| v.parse::<f64>().ok()) {
        if e >= 0.0 && e < 100.0 {
            set_edge_px(e);
        }
    }
    if let Some(m) = envutil::get("TSUNAGU_MOUSE_MODE") {
        if m.eq_ignore_ascii_case("rel") {
            MOUSE_ABS_MODE.store(false, Ordering::Relaxed);
        }
    }
    if let Some(m) = envutil::get("TSUNAGU_SWITCH_MODE") {
        if m.eq_ignore_ascii_case("hotkey") {
            HOTKEY_ONLY.store(true, Ordering::Relaxed);
        }
    }
    if let Some(t) = envutil::get("TSUNAGU_EDGE_TAPS").and_then(|v| v.parse::<u32>().ok()) {
        if t >= 1 && t <= 3 {
            EDGE_TAPS.store(t, Ordering::Relaxed);
        }
    }
    if let Some(k) = envutil::get("TSUNAGU_HOTKEY_KC").and_then(|v| v.parse::<i64>().ok()) {
        if (1..=127).contains(&k) {
            HOTKEY_KC.store(k, Ordering::Relaxed);
        }
    }
    // メニューで切替可能な設定の初期値(.env 経由でも指定できる)
    NATURAL_SCROLL.store(detect_natural_scroll(), Ordering::Relaxed);
    eprintln!(
        "[info] macOS scroll: {} / tsunagu 方向: {}",
        if NATURAL_SCROLL.load(Ordering::Relaxed) {
            "自然スクロール"
        } else {
            "標準(非自然)"
        },
        if envutil::get("TSUNAGU_SCROLL_FLIP").as_deref() == Some("1") {
            "Windows 標準(手動上書き)"
        } else {
            "Mac に合わせる"
        },
    );
    if envutil::get("TSUNAGU_SCROLL_FLIP").as_deref() == Some("1") {
        SCROLL_FLIP.store(true, Ordering::Relaxed);
    }
    // Deskflow 標準オプション(画面位置/切替/隅/クリップボード)
    match envutil::get("TSUNAGU_SIDE").as_deref() {
        Some("left") => SIDE.store(1, Ordering::Relaxed),
        Some("up") => SIDE.store(2, Ordering::Relaxed),
        Some("down") => SIDE.store(3, Ordering::Relaxed),
        Some("upright") => SIDE.store(4, Ordering::Relaxed),
        Some("lowright") | Some("downright") => SIDE.store(5, Ordering::Relaxed),
        Some("upleft") => SIDE.store(6, Ordering::Relaxed),
        Some("lowleft") | Some("downleft") => SIDE.store(7, Ordering::Relaxed),
        _ => {}
    }
    if let Some(v) = envutil::get("TSUNAGU_SWITCH_DELAY").and_then(|v| v.parse::<u64>().ok()) {
        SWITCH_DELAY_MS.store(v.min(5000), Ordering::Relaxed);
    }
    if let Some(v) = envutil::get("TSUNAGU_DOUBLE_TAP_MS").and_then(|v| v.parse::<u64>().ok()) {
        DOUBLE_TAP_MS.store(v.clamp(100, 3000), Ordering::Relaxed);
    }
    if let Some(v) = envutil::get("TSUNAGU_CORNER_PX").and_then(|v| v.parse::<u64>().ok()) {
        CORNER_PX.store(v.min(500), Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_CLIP").as_deref() == Some("0") {
        CLIP_SHARE.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_DRAG_SWITCH").as_deref() == Some("1") {
        DRAG_SWITCH.store(true, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_FAST_EDGE").as_deref() == Some("0") {
        FAST_EDGE.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_DESK_SEARCH").as_deref() == Some("0") {
        DESK_SEARCH.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_APP_HANDOFF").as_deref() == Some("1") {
        APP_HANDOFF.store(true, Ordering::Relaxed);
        eprintln!("[handoff] 越境 App Handoff を有効化しました(TSUNAGU_APP_HANDOFF=1)");
    }
    if envutil::get("TSUNAGU_MAC_KEYS").as_deref() == Some("0") {
        MAC_KEYS.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_SWIPE_NAV").as_deref() == Some("0") {
        SWIPE_NAV.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_LOCK_SYNC").as_deref() == Some("0") {
        LOCK_SYNC.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_IME_SYNC").as_deref() == Some("0") {
        IME_SYNC.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_CONTINUE_HERE").as_deref() == Some("0") {
        CONTINUE_HERE.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_RCMD_CTRL").as_deref() == Some("0") {
        RCMD_CTRL.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_SCROLL_COMPAT").as_deref() == Some("1") {
        SCROLL_COMPAT.store(true, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_CMD_ALT").as_deref() == Some("1") {
        CMD_ALT.store(true, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_MUTE_SPK").as_deref() == Some("0") {
        SPK_MUTE.store(false, Ordering::Relaxed);
    }
    eprintln!(
        "[info] screen {screen_w}x{screen_h}. listening on :{port} (server mode). scroll_div={} mouse_scale={} edge_px={} clip_max={}KB mouse_mode={} switch_mode={} hotkey_kc={} edge_taps={}",
        scroll_div(),
        mouse_scale(),
        edge_px(),
        CLIP_MAX_BYTES / 1024,
        if MOUSE_ABS_MODE.load(Ordering::Relaxed) { "abs" } else { "rel" },
        if HOTKEY_ONLY.load(Ordering::Relaxed) { "hotkey(ロック)" } else { "edge" },
        hotkey_kc(),
        EDGE_TAPS.load(Ordering::Relaxed)
    );
    // 起動時に受信フォルダと履歴を用意する(通知のパスが必ず有効になる)
    let _ = std::fs::create_dir_all(
        std::env::var_os("HOME")
            .map(|h| std::path::Path::new(&h).join("Downloads/Tsunagu"))
            .unwrap_or_default(),
    );
    history_load();
    eprintln!("[info] 操作ガイド: カーソルを画面端へ動かすと Windows へ移ります。メニューバーにクリップボード履歴があります");

    // 送信チャネル + 書き込みストリームスロット(接続が変わるたび差し替え)
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let _ = TX.set(tx);
    let slot: Arc<Mutex<Option<secure::Writer>>> = Arc::new(Mutex::new(None));
    let _ = STREAM_SLOT.set(slot.clone());

    // 単一の送信スレッド(チャネル→ストリーム差し替え方式)
    std::thread::spawn(move || {
        use std::io::Write;
        let mut ping_at = std::time::Instant::now();
        // スリープ復帰の検知: 単調時計はスリープ中に進まないため、壁時計との差が
        // 開いたら眠っていたと分かる。眠っている間に相手側の接続は切れているのが
        // 普通で、単調時計基準の pong 監視では気づけないため、即座に張り直す
        let mut wall = std::time::SystemTime::now();
        let mut mono = std::time::Instant::now();
        loop {
            let (wall_now, mono_now) = (std::time::SystemTime::now(), std::time::Instant::now());
            let slept = wall_now
                .duration_since(wall)
                .unwrap_or_default()
                .saturating_sub(mono_now.duration_since(mono));
            (wall, mono) = (wall_now, mono_now);
            if slept > Duration::from_secs(3) {
                drop_stream(&format!("スリープ復帰を検知({}秒)", slept.as_secs()));
                BULK_LINK.clear();
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    // 送信の束ね(coalescing): 高頻度のマウス移動は 1 行 1 write+flush だと
                    // 小パケット連打になり、WiFi の揺らぎで束になって到着=カクつきの原因。
                    // キューに滞留中の行をまとめて 1 回の write にする(順序は保存され、
                    // Windows 側は行ごとに注入するため見た目の滑らかさが向上する)
                    // マウス移動(mouse_abs)は束ねない: 束ねると複数の目標位置が
                    // 同一フレームに到達して中間が描画されず、カクつきの原因になる。
                    // 移動は「最新位置の即時配送」が滑らかさの本体
                    let is_move = line.starts_with("{\"t\":\"mouse_abs\"");
                    let mut buf = line;
                    let mut total = buf.len();
                    if !is_move {
                        for _ in 0..32 {
                            if total > 256 * 1024 {
                                break; // 巨大行(ファイル chunk 等)の連結は程々に
                            }
                            match rx.try_recv() {
                                Ok(next) => {
                                    total += next.len();
                                    buf.push_str(&next);
                                }
                                Err(_) => break,
                            }
                        }
                    } else {
                        // mouse_abs は束ねない代わりに、キューに連なる古い位置を
                        // 最新 1 件へ間引く(遅延バースト後の古い位置の逐次再生=
                        // カクつきの防止)。move 以外の行は順序保存のため追記して終える
                        while total <= 256 * 1024 {
                            match rx.try_recv() {
                                Ok(next) if next.starts_with("{\"t\":\"mouse_abs\"") => {
                                    buf = next; // 古い位置は捨てて最新だけ送る
                                    total = buf.len();
                                }
                                Ok(next) => {
                                    total += next.len();
                                    buf.push_str(&next);
                                    break;
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    let mut guard = STREAM_SLOT
                        .get()
                        .unwrap()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if let Some(s) = guard.as_mut() {
                        // encode() が行末 \n を持つため writeln! だと二重改行で
                        // ワイヤが \n\n になる(受信側の空行パースが倍増する)。write_all で送る
                        if s.write_all(buf.as_bytes()).and_then(|_| s.flush()).is_err() {
                            if let Some(s) = guard.take() {
                                s.shutdown();
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
            if ping_at.elapsed() >= Duration::from_secs(3) {
                ping_at = std::time::Instant::now();
                // 10 秒 pong が無ければ実質切断扱いでストリームを外す
                // (TCP が生きていても相手プロセスが固まった場合を拾う)
                if now_ms().saturating_sub(LAST_PONG_MS.load(Ordering::Relaxed)) > 10_000 {
                    drop_stream("pong が 10 秒途絶");
                    continue;
                }
                let mut guard = STREAM_SLOT
                    .get()
                    .unwrap()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(s) = guard.as_mut() {
                    if s.write_all(encode(&Msg::Ping { ts: now_ms() }).as_bytes())
                        .and_then(|_| s.flush())
                        .is_err()
                    {
                        if let Some(s) = guard.take() {
                            s.shutdown();
                        }
                    }
                }
            }
        }
    });

    // 画面ロックの連動(1 秒毎)。Mac がロックされたら Windows 操作中でも制御を
    // Mac へ戻し(ロック中の入力を Windows へ流さない)、Windows もロックする
    std::thread::spawn(|| {
        let mut was = false;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = screen_locked();
            if now && !was {
                eprintln!("[lock] Mac がロックされました");
                if WIN_MODE.swap(false, Ordering::Relaxed) {
                    leave_win_mode_cursor_unlock(None);
                }
                if LOCK_SYNC.load(Ordering::Relaxed) && CONNECTED.load(Ordering::Relaxed) {
                    send_msg(&Msg::Lock);
                }
            }
            was = now;
        }
    });

    // Tailscale 経路の診断(30 秒毎)。直結から中継(DERP)へ落ちると遅延が数倍になり
    // 「カクつき」の原因になるため、切り替わった時だけ通知する
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(30));
        let peer = *PEER_IP.lock().unwrap_or_else(|e| e.into_inner());
        let Some(peer) = peer
            .filter(|p| CONNECTED.load(Ordering::Relaxed) && tsunagu_common::net::is_tailscale(*p))
        else {
            continue;
        };
        let Some(now) = tailscale_path(peer) else {
            continue;
        };
        let before = TS_PATH.swap(now, Ordering::Relaxed);
        if before != now {
            eprintln!(
                "[net] Tailscale 経路: {}",
                if now == 1 { "直結" } else { "中継(DERP)" }
            );
            if now == 2 {
                notify("tsunagu", "Windows との通信が中継経由になりました(遅延が増えます)。同じネットワークか有線直結を推奨します");
            }
        }
    });

    // 音声受信・再生(Windows→Mac。独立ポート 24901。TSUNAGU_AUDIO=0 で無効)
    if envutil::get("TSUNAGU_AUDIO").as_deref() != Some("0") {
        // 音声は本線ポートからの差分 +1(24900→24901)
        audio::start(token.clone(), port + 1);
    }

    // 接続方向: 既定は Mac=サーバ(本環境のAP隔離対策)。TSUNAGU_ROLE=client +
    // TSUNAGU_HOST(または --host)で Mac=クライアント(通常ネットワークの配布先向け。
    // その場合は Windows 側を TSUNAGU_ROLE=server で待ち受ける)
    let client_role = envutil::get("TSUNAGU_ROLE").as_deref() == Some("client");
    let bulk_ep: &'static bulk::Endpoint = BULK.get_or_init(|| bulk::Endpoint {
        link: &BULK_LINK,
        token: token.clone(),
        dir: std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join("Downloads/Tsunagu"),
        on_event: mac_on_bulk,
        log: |s| eprintln!("{s}"),
    });
    if client_role {
        let host = args
            .iter()
            .position(|a| a == "--host")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .or_else(|| envutil::get("TSUNAGU_HOST"));
        eprintln!(
            "[info] client mode: connecting to {}",
            host.as_deref().unwrap_or("LAN から自動検出")
        );
        std::thread::spawn(move || {
            bulk::connect_loop(
                bulk_ep,
                || {
                    let ip = *PEER_IP.lock().unwrap_or_else(|e| e.into_inner());
                    ip.map(|ip| std::net::SocketAddr::new(ip, port + bulk::PORT_OFFSET))
                },
                || CONNECTED.load(Ordering::Relaxed),
            )
        });
        std::thread::spawn(move || client_thread(host, port, token, screen_w, screen_h));
    } else {
        let bind = envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        // LAN 自動発見への応答(ブロードキャストを受けるため常に 0.0.0.0 で待つ)
        let tk = token.clone();
        std::thread::spawn(move || {
            if let Err(e) = tsunagu_common::discover::respond(
                "0.0.0.0",
                port + tsunagu_common::discover::PORT_OFFSET,
                &tk,
                tsunagu_common::net::is_allowed,
            ) {
                eprintln!("[disc] 発見応答の待受に失敗: {e}(自動発見が使えません)");
            }
        });
        std::thread::spawn(move || {
            bulk::serve(
                bulk_ep,
                &bind,
                port + bulk::PORT_OFFSET,
                tsunagu_common::net::is_allowed,
            )
        });
        std::thread::spawn(move || server_thread(port, token, screen_w, screen_h));
    }

    // 診断モード: 1秒ごとにモード/受信・送信カウント/実カーソル位置を記録
    if args.iter().any(|a| a == "--diag") {
        DIAG_ENABLED.store(true, Ordering::Relaxed);
        // 検証用の切替指示(--diag 起動時のみ): 一時ディレクトリへ "toggle" と書くと
        // 画面を切り替える。クリップボードは画面を移る時にだけ同期するため、自動検証で
        // 「移る」操作を起こす手段が要る(verify.sh が使う)。/tmp 共有領域だとローカルの
        // 他ユーザーから切替できるため temp_dir()=$TMPDIR(ユーザー固有)へ置く
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(200));
            let p = std::env::temp_dir().join("tsunagu-cmd");
            if let Ok(cmd) = std::fs::read_to_string(&p) {
                let _ = std::fs::remove_file(&p);
                if cmd.trim() == "toggle" {
                    do_toggle("verify");
                }
            }
        });
        std::thread::spawn(|| {
            let mut last_cursor = (0.0f64, 0.0f64);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let (mode, mv, kd, sd, wp, mc, sc, ab, heal) = (
                    WIN_MODE.load(Ordering::Relaxed),
                    DIAG_MOVE_COUNT.load(Ordering::Relaxed),
                    DIAG_KEY_COUNT.load(Ordering::Relaxed),
                    DIAG_SEND_COUNT.load(Ordering::Relaxed),
                    DIAG_WARP_COUNT.load(Ordering::Relaxed),
                    DIAG_MODE_COUNT.load(Ordering::Relaxed),
                    DIAG_SCROLL_COUNT.load(Ordering::Relaxed),
                    DIAG_ABS_COUNT.load(Ordering::Relaxed),
                    DIAG_SELF_HEAL.load(Ordering::Relaxed),
                );
                unsafe {
                    let ev = CGEventCreate(std::ptr::null_mut());
                    let p = if ev.is_null() {
                        CGPoint { x: 0.0, y: 0.0 }
                    } else {
                        CGEventGetLocation(ev)
                    };
                    let moved = (p.x - last_cursor.0).abs() + (p.y - last_cursor.1).abs() > 1.0;
                    eprintln!(
                        "[diag] mode={} moves={mv} keys={kd} sent={sd} scrolls={sc} abs={ab} warp_fixed={wp} switches={mc} self_heal={heal} cursor=({:.0},{:.0}) moving={}",
                        if mode { "WIN" } else { "MAC" }, p.x, p.y, moved
                    );
                    last_cursor = (p.x, p.y);
                }
            }
        });
    }

    // 起動時点のクリップボードは送らない(以後の変化だけを切替時に同期する)
    LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);

    // 基準値はMouseDownで取得済み。重いURL読み出しはタップの外で行う。
    // 取得中に離す・押し直す・越境する場合は、世代の異なる結果を捨てる。
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(16));
        let probe = FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).probe();
        let Some(probe) = probe else { continue };
        with_pool(|| unsafe {
            let pb = drag_pasteboard();
            if pb.is_null() {
                return;
            }
            let cnt = msg0_isize(pb, sel_registerName(c"changeCount".as_ptr()));
            if cnt == probe.baseline {
                return;
            }
            if let Some(files) = pb_files(pb) {
                if msg0_isize(pb, sel_registerName(c"changeCount".as_ptr())) != cnt {
                    return;
                }
                let n = files.len();
                if FILE_DRAG
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .complete(probe, cnt, files)
                {
                    eprintln!("[file] ファイル掴み検出: {n} 件");
                }
            }
        });
    });

    // WIN モード中のカーソル固定監視(改善ループ4):
    // イベントタップ経由の巻き戻しは移動イベントが来た時しか働かない。
    // 慣性や関連切断の効き遅れでカーソルが動いたままになる場合に備え、
    // 常時 200ms ごとに固定位置へ巻き戻す(境界の同時移動抑止の最終防衛)
    std::thread::spawn(|| {
        let mut fixes: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(150));
            // 自己修復: WIN モードでないのにカーソルが隠れたままの異常状態
            // (将来の同種バグや予期しない経路)を検知し、表示を復元する
            if !WIN_MODE.load(Ordering::Relaxed) && CURSOR_HIDDEN.load(Ordering::Relaxed) {
                unsafe {
                    let d = CGMainDisplayID();
                    for _ in 0..3 {
                        CGDisplayShowCursor(d);
                    }
                    CGAssociateMouseAndMouseCursorPosition(true);
                }
                CURSOR_HIDDEN.store(false, Ordering::Relaxed);
                DIAG_SELF_HEAL.fetch_add(1, Ordering::Relaxed);
                eprintln!("[cursor] self-heal: 復帰漏れを修復しました");
            }
            // タップ健全性: システムがタイムアウトでタップを無効化した際、
            // 無効化通知を取り逃しても 1 秒毎の冪等な再 enable で必ず復帰させる
            if let Some(&tap) = TAP_PORT.get() {
                if TAP_REARM_N.fetch_add(1, Ordering::Relaxed) % 7 == 0 {
                    // 150ms×7 ≒ 1秒毎
                    unsafe { CGEventTapEnable(tap as CFMachPortRef, true) };
                }
            }
            // ウォッチドッグ: WIN モード中にユーザーがマウスを動かしているのに
            // (直近2秒以内にタップ受信) Windows への転送が5秒止まっている状態は
            // 異常。強制的に Mac へ復帰させ、操作不能な状態に陥らないようにする。
            // LAST_ABS_MS は絶対位置モードしか更新しないため、条件にモードを含めないと
            // 相対モード(rel)で必ず誤発火する(rel が5秒で強制復帰されていた実績バグ)
            if WIN_MODE.load(Ordering::Relaxed)
                && MOUSE_ABS_MODE.load(Ordering::Relaxed)
                && !GAME_REL.load(Ordering::Relaxed)
            {
                let now = now_ms();
                let last_ev = LAST_EVENT_MS.load(Ordering::Relaxed);
                let last_abs = LAST_ABS_MS.load(Ordering::Relaxed);
                if last_ev > 0
                    && now.saturating_sub(last_ev) < 2_000
                    && now.saturating_sub(last_abs) > 5_000
                {
                    WIN_MODE.store(false, Ordering::Relaxed);
                    eprintln!("[watchdog] WIN中に転送停止を検知。強制復帰します");
                    leave_win_mode_cursor_unlock(None);
                    continue;
                }
            }
            if !WIN_MODE.load(Ordering::Relaxed) {
                continue;
            }
            let Some((lx, ly)) = *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) else {
                continue;
            };
            unsafe {
                let Some(loc) = live_cursor() else { continue };
                if (loc.x - lx).abs() > 1.0 || (loc.y - ly).abs() > 1.0 {
                    CGWarpMouseCursorPosition(CGPoint { x: lx, y: ly });
                    fixes += 1;
                    DIAG_WARP_COUNT.store(fixes, Ordering::Relaxed);
                }
            }
        }
    });

    // イベントタップ(メインスレッドで RunLoop)
    let mask: CGEventMask = (1 << EVT_LEFT_DOWN)
        | (1 << EVT_LEFT_UP)
        | (1 << EVT_RIGHT_DOWN)
        | (1 << EVT_RIGHT_UP)
        | (1 << EVT_MOUSE_MOVED)
        | (1 << EVT_LEFT_DRAGGED)
        | (1 << EVT_RIGHT_DRAGGED)
        | (1 << EVT_OTHER_DRAGGED)
        | (1 << EVT_KEY_DOWN)
        | (1 << EVT_KEY_UP)
        | (1 << EVT_FLAGS_CHANGED)
        // F 行のメディア(輝度・照明)は NSSystemDefined で届くため、
        // マスクに入れないとタップ自体が受け取らない(F5 不反応の実績)
        | (1 << EVT_SYSTEM_DEFINED)
        | (1 << EVT_SCROLL_WHEEL)
        | (1 << EVT_OTHER_DOWN)
        | (1 << EVT_OTHER_UP);

    let tap = unsafe {
        CGEventTapCreate(
            0, // kCGHIDEventTap(ヘッダ実測: 0=HID, 1=Session, 2=Annotated。Deskflow は HID)
            0, // kCGHeadInsertEventTap
            0, // kCGEventTapOptionDefault = 0(抑制可/フィルタ)
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    };
    if tap.is_null() {
        eprintln!("[fatal] CGEventTapCreate failed(アクセシビリティ権限を確認)");
        std::process::exit(1);
    }
    let _ = TAP_PORT.set(tap as usize);
    unsafe {
        let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
        let rl = CFRunLoopGetMain();
        CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    eprintln!(
        "[info] tap active. カーソルを画面右端へ動かすと Windows モード / F13・メニューでトグル"
    );
    // メニューバー GUI(既定ON。--no-gui / TSUNAGU_NO_GUI=1 で CUI のみ)。
    // AppKit が使えない環境(ssh 由来のセッション等)では start() が失敗し、
    // 従来どおり CFRunLoop で継続する(タップはメインRunLoop共通モードのため共存可)
    let no_gui = args.iter().any(|a| a == "--no-gui")
        || envutil::get("TSUNAGU_NO_GUI").is_some_and(|v| v == "1");
    if !no_gui && gui::start() {
        eprintln!("[gui] メニューバー常駐を開始しました");
        // --show-prefs: 起動直後に設定ウィンドウを開く(スクリーンショット検証用)。
        // 実際の生成は NSApp.run 後のタイマー初回で行う
        if registered_now || args.iter().any(|a| a == "--show-prefs") {
            gui::SHOW_AT_START.store(true, Ordering::Relaxed);
        }
        unsafe { gui::run_app() }; // NSApp.run(戻らない。終了はメニューから)
    } else {
        unsafe { CFRunLoopRun() };
    }
}

// ---------- 接続セッション(サーバ/クライアント両モード共通) ----------

/// 1行の長さ上限(画像base64 5MB 上限に対し余裕を持たせる。
/// 未認証helloを含む巨大行によるメモリ消費(DoS)対策)
const MAX_LINE: u64 = 8 * 1024 * 1024;

/// 認証済み接続の受信ループ(Return / Pong / Clip / Bye)
fn session_receive_loop(reader: &mut std::io::BufReader<secure::Reader>, my_id: &str) {
    use std::io::BufRead;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            // 切断理由(EOF 以外)を残す: pong 途絶の張替えか read エラーかの区別が
            // 「なぜ切れたか」の追跡に必要。不正行(decode 失敗)は既存どおり無視
            Err(e) => {
                eprintln!("[conn] read error: {e}(kind={:?})", e.kind());
                break;
            }
            Ok(_) if line.len() as u64 > MAX_LINE => {
                eprintln!("[conn] line too large. dropping connection");
                break;
            }
            Ok(_) => {
                if let Some(msg) = decode(&line) {
                    match msg {
                        Msg::DragOffer {
                            id,
                            count,
                            total,
                            position,
                        } => incoming_drag::offer(id, count, total, position),
                        Msg::DragCommit { id } => incoming_drag::commit(id),
                        Msg::DragCancel { id } => incoming_drag::cancel(id),
                        Msg::AppsReply { apps } => {
                            // Search My Desk の横断化: Windows 側のアプリ候補を受け取り、
                            // 検索窓が開いていれば絞り込みをやり直す(AppKit はメインスレッド)
                            *WIN_APPS.lock().unwrap_or_else(|e| e.into_inner()) = apps.clone();
                            eprintln!("[search] Windows アプリ {} 件を受信", apps.len());
                            dispatch_search_refresh();
                        }
                        Msg::Return { ny } => {
                            if HOTKEY_ONLY.load(Ordering::Relaxed) {
                                // hotkey モードでは Windows 側の左端到達を無視し、
                                // F13 で戻すまで Windows のまま(ロック状態)
                                continue;
                            }
                            // abs-left 復帰後に遅延到着した Return で二重に leave
                            // され再ワープされるのを防ぐ(既に Mac の場合は無視)
                            if !WIN_MODE.load(Ordering::Relaxed) {
                                continue;
                            }
                            WIN_MODE.store(false, Ordering::Relaxed);
                            eprintln!("[mode] MAC (return)");
                            leave_win_mode_cursor_unlock(Some(ny));
                        }
                        Msg::Clip { text } => {
                            if !CLIP_SHARE.load(Ordering::Relaxed) {
                                continue;
                            }
                            if text.len() > CLIP_MAX_BYTES {
                                // 巨大コピーの連投で通知が洪水にならないよう 60 秒に間引く
                                static BIG_CLIP_NOTIFY_MS: AtomicU64 = AtomicU64::new(0);
                                let now = now_ms();
                                if now
                                    .saturating_sub(BIG_CLIP_NOTIFY_MS.swap(now, Ordering::Relaxed))
                                    >= 60_000
                                {
                                    notify("tsunagu", &format!("クリップボードが大きすぎるため同期しません(上限 {}MB。履歴にも載りません)", CLIP_MAX_BYTES / (1024 * 1024)));
                                }
                                continue;
                            }
                            {
                                // Windows の CRLF は Mac 向けに LF へ正規化
                                let text = if text.contains("\r\n") {
                                    text.replace("\r\n", "\n")
                                } else {
                                    text
                                };
                                *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(text.clone());
                                with_pool(|| unsafe { mac_set_clipboard(&text) });
                                LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
                                history_push(&text, "Windows");
                                eprintln!("[clip] win->mac {} bytes", text.len());
                            }
                        }
                        Msg::Pong { ts } => {
                            // アクティブな相手の pong だけ生存時刻・RTT に反映する
                            // (非アクティブ peers への keepalive 応答で上書めないように)
                            if is_active_peer(my_id) {
                                let now = now_ms();
                                LAST_PONG_MS.store(now, Ordering::Relaxed);
                                // ping/pong の往復時間を RTT として保持し、Windows 側の
                                // ステータス窓表示にも回す(接続品質の見える化)
                                let rtt = now.saturating_sub(ts).min(60_000);
                                RTT_MS.store(rtt, Ordering::Relaxed);
                                send_msg(&Msg::Stat { rtt });
                            }
                        }
                        Msg::Ping { ts } => {
                            // アクティブなら既定の送信経路。非アクティブな相手からの
                            // keepalive には、そのセッション自身の writer へ直接返す
                            if is_active_peer(my_id) {
                                send_msg(&Msg::Pong { ts });
                            } else {
                                use std::io::Write as _;
                                let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(w) = peers
                                    .iter_mut()
                                    .find(|p| p.id == my_id)
                                    .and_then(|p| p.writer.as_mut())
                                {
                                    let wire = encode(&Msg::Pong { ts });
                                    let _ = w.write_all(wire.as_bytes()).and_then(|_| w.flush());
                                }
                            }
                        }
                        Msg::Rel { on } => {
                            if GAME_REL.swap(on, Ordering::Relaxed) != on {
                                eprintln!(
                                    "[game] ゲームモード -> {}",
                                    if on {
                                        "ON(相対移動)"
                                    } else {
                                        "OFF(絶対位置)"
                                    }
                                );
                            }
                        }
                        Msg::Screen { w, h } if w > 0 && h > 0 => {
                            // WIN_SCREEN と WIN_CUR はこの順で保持する(他箇所は
                            // 同時保持しないため順序固定でデッドロックなし)
                            let mut wc = WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
                            let mut ws = WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                            let old = *ws;
                            *ws = (w as f64, h as f64);
                            *wc = rescale_win_cur(*wc, old, *ws);
                            eprintln!("[info] win screen changed {w}x{h}");
                        }
                        Msg::Bye => break,
                        _ => {}
                    }
                }
            }
        }
    }
}

/// セッション終了の共通後処理(スロット解除・切断通知・WIN 中なら正規 leave)
fn on_disconnect() {
    incoming_drag::reset();
    {
        let mut guard = STREAM_SLOT
            .get()
            .unwrap()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
    CONNECTED.store(false, Ordering::Relaxed);
    // 旧セッションの RTT が再接続直後に「前の接続の値」として表示されるのを防ぐ
    RTT_MS.store(0, Ordering::Relaxed);
    BULK_LINK.clear();
    // WIN モード中の切断は正規の leave 経由で復帰させる(カーソル表示・
    // EDGE_GUARD・CUR_POS 整合を自己修復スレッドの「たまたま」に任せない)
    if WIN_MODE.swap(false, Ordering::Relaxed) {
        eprintln!("[return] -> mac (disconnect)");
        leave_win_mode_cursor_unlock(None);
    }
    eprintln!("[conn] lost. waiting for reconnect...");
    notify("tsunagu", "切断しました(自動で再接続します)");
}

/// 待受モード(既定): Windows からの接続を受け入れる
/// (本環境では Mac 発コネクションが不通なため、Windows 発に限定した設計)
fn server_thread(port: u16, token: String, screen_w: f64, screen_h: f64) {
    use std::io::{BufRead, Read};
    // 待受アドレス: 既定は全インターフェース(LAN 直を受け入れる)。
    // 防御は is_allowed(接続元絞り)+ Noise ハンドシェイクが担う
    let bind_ip = envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[fatal] listen {bind_ip}:{port} failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("[info] server mode. listening on {bind_ip}:{port}");
    let mut accept_errs: u32 = 0;
    // 認証に失敗し続ける接続の連打を鈍らせる(正規の接続が成功すれば即回復)
    let mut throttle = secure::FailThrottle::new();
    // 非アクティブピアへの生存確認(複数台保持のために一度だけ起こす)
    keepalive_inactive_peers();
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(x) => {
                accept_errs = 0;
                x
            }
            Err(e) => {
                // fd 枯渇等で失敗が続くと 500ms 毎の洪水になるため、最初と
                // その後 20 回毎(≒10 秒)だけ出す
                accept_errs += 1;
                if accept_errs == 1 || accept_errs % 20 == 0 {
                    eprintln!("[conn] accept error: {e}(連続 {accept_errs} 回目)");
                }
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        eprintln!("[conn] accepted from {peer}");
        // 接続元の制限(LAN・有線直結・Tailscale のみ)。認証は暗号化ハンドシェイクで行う
        if !tsunagu_common::net::is_allowed(peer.ip()) {
            eprintln!("[conn] rejected: {peer} は許可範囲外です(TSUNAGU_ALLOW_ANY=1 で許可)");
            continue;
        }
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(Duration::from_secs(12))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
        let (r, mut w) = match secure::accept(stream, &token, b"tsunagu-main") {
            Ok(x) => x,
            Err(e) => {
                eprintln!("[conn] 暗号化ハンドシェイク失敗 ({peer}): {e}(トークン不一致の可能性)");
                std::thread::sleep(throttle.fail());
                continue;
            }
        };
        // hello を待つ(検証して hello_ok を返す)
        let mut reader = std::io::BufReader::new(r);
        let mut line = String::new();
        // 最初の行=hello(この時点では未認証のため、take で読み込み段階から制限する)
        match (&mut reader).take(MAX_LINE + 1).read_line(&mut line) {
            Ok(0) | Err(_) => {
                eprintln!("[conn] closed before hello");
                std::thread::sleep(throttle.fail());
                continue;
            }
            Ok(_) if line.len() as u64 > MAX_LINE => {
                eprintln!("[conn] hello too large. dropped");
                std::thread::sleep(throttle.fail());
                continue;
            }
            Ok(_) => {}
        }
        let parsed = match decode(&line) {
            Some(Msg::Hello {
                ver,
                w,
                h,
                name,
                id,
                monitors,
                ..
            }) if compatible(ver) => {
                // 表示名は制御文字・Bidi オーバーライドを除去してから載せる
                let disp = {
                    let n = safe_peer_name(name.trim());
                    if n.is_empty() {
                        "Windows".to_string()
                    } else {
                        n
                    }
                };
                // 端末 id は再接続の紐付けに使う。旧版(版 12 以前)は空=IP 由来の代替
                let dev = if id.is_empty() {
                    format!("legacy-{}", peer.ip())
                } else {
                    id
                };
                Some((disp, dev, monitors, w.max(1) as f64, h.max(1) as f64))
            }
            _ => None,
        };
        let Some((disp, dev, mons, win_w, win_h)) = parsed else {
            eprintln!("[conn] invalid hello");
            std::thread::sleep(throttle.fail());
            continue;
        };
        throttle.success();
        eprintln!(
            "[conn] established: {disp} (id={dev}) 画面 {}x{} モニター: {}",
            win_w as i32,
            win_h as i32,
            tsunagu_common::proto::Monitor::summary(&mons)
        );
        // セッションはスレッドへ分離し、accept 側は次の接続を待つ(複数台の同時保持)
        std::thread::spawn(move || {
            use std::io::Write as _;
            // hello_ok はこのセッションの writer へ直接返す(アクティブ化前でも届くように)
            let ok = encode(&Msg::HelloOk {
                name: hostname_label(),
                w: screen_w as i32,
                h: screen_h as i32,
                ver: VERSION,
                id: tsunagu_common::proto::device_id(),
                monitors: mac_monitors(),
            });
            if w.write_all(ok.as_bytes()).and_then(|_| w.flush()).is_err() {
                eprintln!("[conn] hello_ok 送信に失敗しました");
                return;
            }
            // 同一端末の重複接続(経路昇格で Tailscale と LAN 直の 2 本が同時に
            // 張られる等)は「新しい世代で置き換える」。古いセッションは終了時に
            // 世代が一致しないため一覧を触らず静かに終わる
            let my_gen = PEER_GEN.fetch_add(1, Ordering::Relaxed) + 1;
            let (idx, replace_active, make_first);
            {
                let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                match peers.iter().position(|p| p.id == dev) {
                    // 再接続: 名前・画面構成は前回と変わっている可能性があるため更新する
                    Some(i) => {
                        let was_active = act == i;
                        if !was_active {
                            // 待機中の旧 writer があれば破棄する
                            if let Some(old) = peers[i].writer.take() {
                                old.shutdown();
                            }
                        }
                        peers[i].name = disp.clone();
                        peers[i].ip = peer.ip();
                        peers[i].screen = (win_w, win_h);
                        peers[i].monitors = mons.clone();
                        peers[i].gen = my_gen;
                        peers[i].writer = None;
                        idx = i;
                        replace_active = was_active;
                    }
                    None => {
                        peers.push(PeerEntry {
                            id: dev.clone(),
                            name: disp.clone(),
                            ip: peer.ip(),
                            screen: (win_w, win_h),
                            monitors: mons.clone(),
                            writer: None,
                            gen: my_gen,
                        });
                        idx = peers.len() - 1;
                        replace_active = false;
                    }
                }
                make_first = act >= peers.len();
            }
            if replace_active {
                // アクティブだった旧セッションの TCP を先に切る(旧セッションは
                // まもなく終了し、世代不一致のため一覧を触らない)
                drop_stream("同一端末の新しい接続に置き換え");
            }
            // 新しいセッションの writer を預けてからアクティブへ切り替える。
            // 既に他の端末がアクティブでも、後から確立した接続を正とする
            // (経路昇格の張替えで古い経路が残らないようにする)
            {
                let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                peers[idx].writer = Some(w);
            }
            activate_peer(
                idx,
                if make_first {
                    "初回接続"
                } else {
                    "再接続"
                },
            );
            session_receive_loop(&mut reader, &dev);
            // ---- 切断: 世代が一致する場合だけこのピアを一覧から外し、アクティブ
            // だった場合は残りへ自動で切り替える(残りが無ければ従来どおりの切断扱い) ----
            let mut next = None;
            {
                let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                let Some(i) = peers.iter().position(|p| p.id == dev && p.gen == my_gen) else {
                    // 新しい接続へ置き換え済み。一覧は触らない
                    eprintln!("[conn] セッション終了(新しい接続へ置き換え済み)");
                    return;
                };
                let was_active = act == i;
                peers.remove(i);
                if was_active {
                    if peers.is_empty() {
                        *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner()) = usize::MAX;
                    } else {
                        next = Some(i.min(peers.len() - 1));
                    }
                } else if act > i {
                    // 前詰めで添字がずれる分を補正する
                    *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner()) = act - 1;
                }
            }
            match next {
                Some(i) => activate_peer(i, "自動切替"),
                None => on_disconnect(),
            }
        });
    }
}

/// 接続モードの 1 セッション分(ハンドシェイク+本体)。Result はリトライ理由
fn client_attempt(s: TcpStream, token: &str, screen_w: f64, screen_h: f64) -> Result<(), String> {
    use std::io::{BufRead, Read, Write};
    s.set_read_timeout(Some(Duration::from_secs(12))).ok();
    eprintln!("[conn] connected");
    s.set_nodelay(true).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let (r, mut hw) = secure::connect(s, token, b"tsunagu-main")
        .map_err(|e| format!("暗号化ハンドシェイク失敗: {e}(トークン不一致の可能性)"))?;
    // hello(自画面サイズを相手へ伝える。相手は hello_ok で自画面を返す)
    let hello = encode(&Msg::Hello {
        ver: VERSION,
        name: hostname_label(),
        token: String::new(),
        w: screen_w as i32,
        h: screen_h as i32,
        id: tsunagu_common::proto::device_id(),
        monitors: mac_monitors(),
    });
    hw.write_all(hello.as_bytes())
        .and_then(|_| hw.flush())
        .map_err(|_| "hello send failed".to_string())?;
    // hello_ok を待つ(行長制限付き)
    let mut reader = std::io::BufReader::new(r);
    let mut line = String::new();
    (&mut reader)
        .take(MAX_LINE + 1)
        .read_line(&mut line)
        .map_err(|_| "hello_ok read failed".to_string())?;
    if line.len() as u64 > MAX_LINE {
        return Err("hello_ok too large".into());
    }
    match decode(&line) {
        Some(Msg::HelloOk {
            w: mw,
            h: mh,
            name,
            monitors,
            ..
        }) if mw > 0 && mh > 0 => {
            *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = (mw as f64, mh as f64);
            *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = {
                let n = safe_peer_name(name.trim());
                if n.is_empty() {
                    "Windows".into()
                } else {
                    n
                }
            };
            eprintln!(
                "[info] win screen {mw}x{mh} モニター: {}",
                tsunagu_common::proto::Monitor::summary(&monitors)
            );
        }
        _ => return Err("invalid hello_ok".into()),
    }
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    {
        let mut guard = STREAM_SLOT
            .get()
            .unwrap()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = Some(hw);
    }
    CONNECTED.store(true, Ordering::Relaxed);
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    // 現在の ⌘キー設定を同期(クライアントモードの確立時)
    send_cfg();
    eprintln!("[conn] established");
    let peer = PEER_NAME.lock().map(|n| n.clone()).unwrap_or_default();
    notify(
        "tsunagu",
        &format!(
            "{} と接続しました",
            if peer.is_empty() {
                "Windows".into()
            } else {
                peer
            }
        ),
    );
    // クライアントモードは単一接続のため my_id 空(=常にアクティブ扱い)
    session_receive_loop(&mut reader, "");
    on_disconnect();
    Ok(())
}

/// 接続モード(TSUNAGU_ROLE=client): Windows(サーバ)へ接続し続ける
fn client_thread(host: Option<String>, port: u16, token: String, screen_w: f64, screen_h: f64) {
    let mut backoff = 500u64;
    loop {
        let addrs = tsunagu_common::connect::resolve(host.as_deref(), port, &token);
        if addrs.is_empty() {
            // 候補が空なら first_reachable を呼ばない(Windows 側と同じ: 3.5 秒の空待ち防止)
            eprintln!("[conn] 接続先が見つかりません");
            std::thread::sleep(Duration::from_millis(backoff));
            backoff = (backoff * 2).min(3000);
            continue;
        }
        let t0 = std::time::Instant::now();
        match tsunagu_common::connect::first_reachable(&addrs, Duration::from_secs(3)) {
            Some((s, a)) => {
                eprintln!("[conn] connected ({a}) in {}ms", t0.elapsed().as_millis());
                *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) = Some(a.ip());
                match client_attempt(s, &token, screen_w, screen_h) {
                    Ok(()) => backoff = 500,
                    Err(e) => eprintln!("[conn] {e}"),
                }
            }
            None => eprintln!("[conn] どの接続先にも繋がりません: {:?}", host),
        }
        std::thread::sleep(Duration::from_millis(backoff));
        backoff = (backoff * 2).min(3000);
    }
}

#[cfg(test)]
mod drag_end_tests {
    use super::*;

    unsafe extern "C" {
        fn CGEventGetType(event: CGEventRef) -> u32;
    }

    #[test]
    fn handoff_creates_a_tagged_left_release() {
        unsafe {
            // 作成だけを検証する。実際のポインタやボタン状態は操作しない。
            let event = make_drag_end_event(CGPoint { x: 100.0, y: 100.0 });
            assert!(!event.is_null());
            let event_type = CGEventGetType(event);
            let tag = CGEventGetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA);
            CFRelease(event);
            assert_eq!(
                event_type, EVT_LEFT_UP,
                "元の左ドラッグを終えるイベントであること"
            );
            assert_eq!(tag, SYNTH_UP_MAGIC, "物理ボタンの解放と区別できること");
        }
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::mac_shortcut_translation as tr;

    /// 翻訳対応の固定(タップ実装と表の乖離を防ぐ)。kc は Mac keycode
    #[test]
    fn shortcut_table_matches_spec() {
        // ⌘← = Home(Shift 透過: ⌘⇧← は Shift+Home)
        assert_eq!(
            tr(123, false, false, true, false),
            Some((115, false, false, false, false))
        );
        assert_eq!(
            tr(123, false, false, true, true),
            Some((115, false, false, false, true))
        );
        // ⌘→ = End、⌘↑ = Ctrl+Home、⌘↓ = Ctrl+End
        assert_eq!(
            tr(124, false, false, true, false),
            Some((119, false, false, false, false))
        );
        assert_eq!(
            tr(126, false, false, true, false),
            Some((115, false, false, true, false))
        );
        assert_eq!(
            tr(125, false, false, true, false),
            Some((119, false, false, true, false))
        );
        // ⌘M / ⌘H = Win+Down(最小化)
        assert_eq!(
            tr(43, false, false, true, false),
            Some((125, true, false, false, false))
        );
        assert_eq!(
            tr(4, false, false, true, false),
            Some((125, true, false, false, false))
        );
        // ⌘] = Ctrl+Tab / ⌘[ = Ctrl+Shift+Tab(⌘⇧[ も前タブ)
        assert_eq!(
            tr(30, false, false, true, false),
            Some((48, false, false, true, false))
        );
        assert_eq!(
            tr(33, false, false, true, false),
            Some((48, false, false, true, true))
        );
        assert_eq!(
            tr(33, false, false, true, true),
            Some((48, false, false, true, true))
        );
        // ⌘⇧4 / ⌘⇧3 = Win+Shift+S。⇧無しの ⌘4 は素の F4 相当へ翻訳しない
        assert_eq!(
            tr(21, false, false, true, true),
            Some((1, true, false, false, true))
        );
        assert_eq!(
            tr(18, false, false, true, true),
            Some((1, true, false, false, true))
        );
        assert_eq!(tr(21, false, false, true, false), None);
        // ⌘⇧5 = Win+Alt+R
        assert_eq!(
            tr(23, false, false, true, true),
            Some((15, false, true, false, false))
        );
        // ⌘Q = Alt+F4
        assert_eq!(
            tr(12, false, false, true, false),
            Some((118, false, true, false, false))
        );
        // ⌘G = F3 / ⌘⇧G = Shift+F3
        assert_eq!(
            tr(32, false, false, true, false),
            Some((99, false, false, false, false))
        );
        assert_eq!(
            tr(32, false, false, true, true),
            Some((99, false, false, false, true))
        );
        // ⌘. = Esc
        assert_eq!(
            tr(47, false, false, true, false),
            Some((53, false, false, false, false))
        );
        // ⌘Space = Win+Space
        assert_eq!(
            tr(49, false, false, true, false),
            Some((49, true, false, false, false))
        );
        // ⌘⌥Esc = Ctrl+Shift+Esc
        assert_eq!(
            tr(53, false, true, true, false),
            Some((53, false, false, true, true))
        );
        // ⌘Ctrl+Q = Win+L(⌘Q より優先)
        assert_eq!(
            tr(12, true, false, true, false),
            Some((37, true, false, false, false))
        );
        // ⌥← = Ctrl+←(単語移動)
        assert_eq!(
            tr(123, false, true, false, false),
            Some((123, false, false, true, false))
        );
        // 翻訳対象外: 素の A、⌘A(そのまま渡る)、⌥A
        assert_eq!(tr(0, false, false, false, false), None);
        assert_eq!(tr(0, false, false, true, false), None);
        assert_eq!(tr(0, false, true, false, false), None);
    }
}

#[cfg(test)]
mod geo_tests {
    use super::Geo;

    /// MacBook(0..2056) の左に 1920 幅、右に 2560 幅のモニターがある構成
    fn three_screens() -> Geo {
        Geo {
            main_w: 2056.0,
            main_h: 1329.0,
            min_x: -1920.0,
            max_x: 4616.0,
            min_y: -200.0,
            max_y: 1329.0,
            exit: [
                (-200.0, 1240.0),
                (0.0, 1080.0),
                (-1920.0, 0.0),
                (0.0, 2056.0),
            ],
        }
    }

    #[test]
    fn edges_are_measured_on_the_whole_desktop() {
        let g = three_screens();
        // MacBook の左端(x=0)は左モニターへの通り道なので、左配置でも切替境界ではない
        assert!(g.gap(1, 0.0, 500.0) > 1000.0);
        assert_eq!(g.gap(1, -1920.0, 500.0), 0.0);
        // MacBook の右端も右モニターへの通り道
        assert!(g.gap(0, 2056.0, 500.0) > 1000.0);
        assert_eq!(g.gap(0, 4616.0, 500.0), 0.0);
    }

    #[test]
    fn return_point_is_inside_the_exit_display() {
        let g = three_screens();
        let (x, y) = g.inside_point(1, 60.0, Some(0.5));
        assert_eq!(x, -1860.0);
        assert_eq!(y, 540.0);
        let (x, y) = g.inside_point(0, 60.0, Some(0.0));
        assert_eq!(x, 4556.0);
        assert_eq!(y, -180.0); // 端から 20px は避ける
        assert_eq!(g.along_ratio(0, 4616.0, 520.0), 0.5);
        // 上下の辺は横位置で測る
        assert_eq!(g.along_ratio(3, 1028.0, 1329.0), 0.5);
    }
}

#[cfg(test)]
mod win_cur_tests {
    use super::rescale_win_cur as rs;

    #[test]
    fn rescale_keeps_ratio_on_shrink_and_grow() {
        // 2560x1440 → 1920x1080: 画面内の同じ比率位置へ写す(張り付かせない)
        assert_eq!(
            rs((2000.0, 1000.0), (2560.0, 1440.0), (1920.0, 1080.0)),
            (1500.0, 750.0)
        );
        // 拡大時も比率維持(位置が飛ばない)
        assert_eq!(
            rs((960.0, 540.0), (1920.0, 1080.0), (2560.0, 1440.0)),
            (1280.0, 720.0)
        );
        // 同一サイズなら不変
        assert_eq!(
            rs((123.0, 456.0), (1920.0, 1080.0), (1920.0, 1080.0)),
            (123.0, 456.0)
        );
    }

    #[test]
    fn rescale_ignores_invalid_sizes() {
        // 初期値(0x0)や不正値はそのまま(0 除算・暴発写像の防止)
        assert_eq!(rs((10.0, 20.0), (0.0, 0.0), (1920.0, 1080.0)), (10.0, 20.0));
        assert_eq!(
            rs((10.0, 20.0), (1920.0, 1080.0), (0.0, 1080.0)),
            (10.0, 20.0)
        );
    }

    #[test]
    fn px_per_sec_converts_window_to_seconds() {
        use super::px_per_sec as v;
        assert_eq!(v(120.0, 100), 1200.0);
        assert_eq!(v(3.0, 30), 100.0);
        // 窓が 0ms(初回イベント等)は速度不定ではなく 0 扱い(0 除算回避)
        assert_eq!(v(50.0, 0), 0.0);
    }
}

#[cfg(test)]
mod ime_tests {
    use super::ime_mode_state as st;

    #[test]
    fn japanese_modes_map_to_ime_open_state() {
        // ひらがな/カタカナ/半角カナ/全角英数は ON
        assert_eq!(st("com.apple.inputmethod.Japanese.Hiragana"), Some(true));
        assert_eq!(st("com.apple.inputmethod.Japanese.Katakana"), Some(true));
        assert_eq!(
            st("com.apple.inputmethod.Japanese.HalfWidthKana"),
            Some(true)
        );
        assert_eq!(
            st("com.apple.inputmethod.Japanese.FullWidthRoman"),
            Some(true)
        );
        // 日本語入力の英数モードは OFF
        assert_eq!(st("com.apple.inputmethod.Japanese.Roman"), Some(false));
        // サードパーティ IME は入力モードを反映しない ID を返すことがあるため
        // 対象外(誤 ON を送らない。Apple 純正のみ同期)
        assert_eq!(st("com.google.inputmethod.Japanese.base"), None);
        assert_eq!(st("com.google.inputmethod.Japanese.base.Roman"), None);
        // 日本語入力以外・レイアウト指定も None=同期しない(勝手に閉じない)
        assert_eq!(st("com.apple.keylayout.ABC"), None);
        assert_eq!(st("com.apple.keylayout.US"), None);
    }
}

#[cfg(test)]
mod dib_tests {
    use super::dib_to_bmp;

    /// biSize=40 の BITMAPINFOHEADER を組み立てる。comp は biCompression
    fn info_header(w: i32, h: i32, bpp: u16, comp: u32) -> Vec<u8> {
        let mut b = vec![0u8; 40];
        b[0..4].copy_from_slice(&40u32.to_le_bytes());
        b[4..8].copy_from_slice(&w.to_le_bytes());
        b[8..12].copy_from_slice(&h.to_le_bytes());
        b[12..14].copy_from_slice(&1u16.to_le_bytes());
        b[14..16].copy_from_slice(&bpp.to_le_bytes());
        b[16..20].copy_from_slice(&comp.to_le_bytes());
        b
    }

    #[test]
    fn bitfields_masks_after_info_header_are_skipped() {
        // Windows のクリップボードは biSize=40 + BI_BITFIELDS で、ヘッダ直後に
        // 12 バイトのカラーマスクを付ける。offbits はマスクの後でなければ
        // 画像全体が 3px ずれる(実機で緑が赤に化けた実績)
        let mut dib = info_header(8, 8, 32, 3);
        dib.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0]); // RGB マスク
        dib.extend_from_slice(&[0xAA; 8 * 8 * 4]);
        let bmp = dib_to_bmp(&dib);
        let off = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize;
        assert_eq!(off, 14 + 40 + 12, "ピクセル開始はマスクの直後");
        assert_eq!(bmp[off], 0xAA, "マスク列をピクセルとして読まない");
    }

    #[test]
    fn plain_header_and_v5_header_offsets_are_unchanged() {
        let dib = [info_header(4, 4, 32, 0), vec![0x11; 4 * 4 * 4]].concat();
        let bmp = dib_to_bmp(&dib);
        let off = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize;
        assert_eq!(off, 14 + 40, "BI_RGB はマスクなし");

        // biSize>=52(V4/V5)はマスクがヘッダサイズに含まれるため加算しない
        let mut v5 = info_header(4, 4, 32, 3);
        v5[0..4].copy_from_slice(&124u32.to_le_bytes());
        v5.resize(124, 0);
        v5.extend_from_slice(&[0x22; 4 * 4 * 4]);
        let bmp = dib_to_bmp(&v5);
        let off = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize;
        assert_eq!(off, 14 + 124, "V5 ヘッダは二重に足さない");
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::app_handoff_match;

    fn apps() -> Vec<(String, String)> {
        vec![
            ("Visual Studio Code".to_string(), "C:/code.exe".to_string()),
            ("Windows Terminal".to_string(), "C:/wt.exe".to_string()),
            ("Blender".to_string(), "C:/blender.exe".to_string()),
        ]
    }

    #[test]
    fn matches_exact_name_case_insensitively() {
        let a = apps();
        let hit = app_handoff_match("Visual Studio CODE", &a).unwrap();
        assert_eq!(hit.1, "C:/code.exe", "大小違いの完全一致にヒット");
        assert!(
            app_handoff_match("  Blender  ", &a).is_some(),
            "前後空白は無視"
        );
    }

    #[test]
    fn matches_by_containment_with_a_long_enough_name() {
        let a = apps();
        // Mac の「Terminal」は Windows の「Windows Terminal」の部分文字列
        let hit = app_handoff_match("Terminal", &a).unwrap();
        assert_eq!(hit.1, "C:/wt.exe");
        // 4 文字未満の部分一致は誤爆のもとなので拾わない
        assert!(
            app_handoff_match("Ble", &a).is_none(),
            "4 文字未満の contains は不可"
        );
        assert!(
            app_handoff_match("Safari", &a).is_none(),
            "存在しないアプリは不一致"
        );
        assert!(app_handoff_match("", &a).is_none(), "空の名前は不一致");
    }
}
