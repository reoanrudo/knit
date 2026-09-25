// sd-mac: Mac 側クライアント。CGEventTap で入力を横流しし、Windows へ送信する。
// 画面右端でカーソルが Mac→Windows 切替、Windows カーソル左端(または F13)で復帰。
#![allow(non_camel_case_types)]

use sd_common::proto::{decode, encode, Msg, PORT, VERSION};
use std::sync::atomic::{AtomicBool, Ordering};
use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

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

const KC_F13: i64 = 105;

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: i32, place: i32, options: u32, events_of_interest: CGEventMask,
        callback: unsafe extern "C" fn(
            proxy: *mut core::ffi::c_void, event_type: u32, event: CGEventRef, user_info: *mut core::ffi::c_void,
        ) -> CGEventRef,
        user_info: *mut core::ffi::c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetType(event: CGEventRef) -> u32;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: i32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGWarpMouseCursorPosition(new: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connect: bool) -> i32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGSetLocalEventsSuppressionInterval(seconds: f64) -> i32;
    fn CGEventCreate(allocator: CFAllocatorRef) -> CGEventRef;
    fn CFRelease(cf: *mut core::ffi::c_void);
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef, c_str: *const core::ffi::c_char, encoding: u32,
    ) -> CFStringRef;
    static kCFBooleanTrue: *const core::ffi::c_void;
    // Deskflow hideCursor/showCursor が使う非公開 CGS API(カーソル非表示の安定化)
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(
        cid: i32, target_cid: i32, key: CFStringRef, value: *const core::ffi::c_void,
    ) -> i32;
    fn CFMachPortCreateRunLoopSource(alloc: CFAllocatorRef, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
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
}

/// 最後に Windows から受信して書き込んだテキスト(エコーバック送信防止)
static LAST_RECV_CLIP: Mutex<Option<String>> = Mutex::new(None);
const CLIP_MAX_BYTES: usize = 512 * 1024;

type ID = *mut core::ffi::c_void;
type SEL = *mut core::ffi::c_void;

// objc_msgSend は可変引数宣言のまま呼ぶと引数の渡りが壊れる(SIGSEGV実績あり)ため、
// 呼び出しシグネチャごとに transmute した固定シグネチャで呼ぶ(rust-objc 界の定番方式)
unsafe fn msg0(target: ID, sel: SEL) -> ID {
    let f: unsafe extern "C" fn(ID, SEL) -> ID = std::mem::transmute(objc_msgSend as usize);
    f(target, sel)
}
unsafe fn msg1_id(target: ID, sel: SEL, a: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID = std::mem::transmute(objc_msgSend as usize);
    f(target, sel, a)
}
unsafe fn msg1_cstr(target: ID, sel: SEL, p: *const core::ffi::c_char) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, *const core::ffi::c_char) -> ID =
        std::mem::transmute(objc_msgSend as usize);
    f(target, sel, p)
}
unsafe fn msg2_bool(target: ID, sel: SEL, a: ID, b: ID) -> u8 {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 =
        std::mem::transmute(objc_msgSend as usize);
    f(target, sel, a, b)
}
unsafe fn msg0_isize(target: ID, sel: SEL) -> isize {
    let f: unsafe extern "C" fn(ID, SEL) -> isize = std::mem::transmute(objc_msgSend as usize);
    f(target, sel)
}
unsafe fn msg0_cstr(target: ID, sel: SEL) -> *const core::ffi::c_char {
    let f: unsafe extern "C" fn(ID, SEL) -> *const core::ffi::c_char =
        std::mem::transmute(objc_msgSend as usize);
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
        eprintln!("[clip] set failed: str={} uti={}", !s.is_null(), !uti.is_null());
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
    Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
}

fn clipboard_change_count() -> isize {
    unsafe { msg0_isize(general_pasteboard(), sel_registerName(c"changeCount".as_ptr())) }
}

// ---------- 共有状態 ----------
static WIN_MODE: AtomicBool = AtomicBool::new(false);
static CONNECTED: AtomicBool = AtomicBool::new(false);
static TX: OnceLock<Sender<String>> = OnceLock::new();
static STREAM_SLOT: OnceLock<Arc<Mutex<Option<TcpStream>>>> = OnceLock::new();
static SCREEN_W: OnceLock<f64> = OnceLock::new();
static SCREEN_H: OnceLock<f64> = OnceLock::new();
static TAP_PORT: OnceLock<usize> = OnceLock::new();
static DIAG_MOVE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_KEY_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_SEND_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DIAG_WARP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_PONG_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 復帰直後は右端判定を一定時間無効化する(再突入チャタリング防止)
static EDGE_GUARD_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 自前管理のカーソル位置(delta 積算)。タップ内での毎イベント CGEventCreate は
/// 負荷としてカクつきに効くため、積算+間欠同期(Deskflow の m_xCursor 方式)にする。
static CUR_POS: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
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
/// スクロール速度除数(ピクセル→ノッチ変換。大きいほど遅い)。SEAMLESS_SCROLL_DIV で調整可。
static SCROLL_DIV: OnceLock<f64> = OnceLock::new();
/// マウス移動の倍率(Mac の加速済み delta に Windows の加速が重なる調整用)。
/// SEAMLESS_MOUSE_SCALE で指定(例: 0.7 で遅く、1.5 で速く)。
static MOUSE_SCALE: OnceLock<f64> = OnceLock::new();

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
    // Deskflow leave() 相当: hideCursor(プロパティ付き) → suppression間隔最小化 → 関連切断 → warp固定
    unsafe {
        set_cursor_in_background();
        let d = CGMainDisplayID();
        CGDisplayHideCursor(d);
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
        let lock_x = SCREEN_W.get().copied().unwrap_or(2056.0) - 2.0;
        CGWarpMouseCursorPosition(CGPoint { x: lock_x, y: lock_y });
        *LOCK_POS.lock().unwrap() = Some((lock_x, lock_y));
    }
}

/// Windows モード終了: 関連を復元し、右端の内側へカーソルを戻す。
/// ny は Windows 側カーソルの高さ(0..1)。与えられた場合は同じ高さへ戻す(境界連続性)。
fn leave_win_mode_cursor_unlock(ny: Option<f64>) {
    // Deskflow enter() 相当: 関連復元 → showCursor(プロパティ付き) → suppression解除 → 位置復帰
    unsafe {
        EDGE_GUARD_UNTIL_MS.store(now_ms() + 300, Ordering::Relaxed);
        if let Some(loc) = live_cursor() {
            *CUR_POS.lock().unwrap() = (loc.x, loc.y);
        }
        *LOCK_POS.lock().unwrap() = None;
        CGAssociateMouseAndMouseCursorPosition(true);
        set_cursor_in_background();
        let d = CGMainDisplayID();
        CGDisplayShowCursor(d);
        CGSetLocalEventsSuppressionInterval(0.0); // Deskflow setZeroSuppressionInterval
        if let Some(w) = SCREEN_W.get() {
            let y = match ny {
                Some(n) => {
                    let h = SCREEN_H.get().copied().unwrap_or(1000.0);
                    (n.clamp(0.0, 1.0) * h).clamp(20.0, (h - 20.0).max(20.0))
                }
                None => 400.0,
            };
            CGWarpMouseCursorPosition(CGPoint { x: *w - 60.0, y });
        }
    }
}

fn send_msg(msg: &Msg) {
    DIAG_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
    if let Some(tx) = TX.get() {
        let line = encode(msg);
        let _ = tx.send(line);
    }
}

// ---------- イベントタップコールバック ----------
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

    // F13 = 手動トグル(常に有効、握る)
    if event_type == EVT_KEY_DOWN || event_type == EVT_FLAGS_CHANGED {
        let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
        if kc == KC_F13 && event_type == EVT_KEY_DOWN {
            if connected {
                let next = !win_mode;
                WIN_MODE.store(next, Ordering::Relaxed);
                eprintln!("[mode] {} (F13)", if next { "WINDOWS" } else { "MAC" });
                if next {
                    enter_win_mode_cursor_lock();
                } else {
                    leave_win_mode_cursor_unlock(None);
                }
            }
            return std::ptr::null_mut();
        }
    }

    if matches!(event_type, EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED) {
        DIAG_MOVE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    if matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED) {
        DIAG_KEY_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    if !win_mode {
        // Mac モード: 右端到達で Windows モードへ。
        // Deskflow onMouseMove 準拠: イベント位置はキュー滞留で数フレーム遅れるため、
        // CGEventCreate(NULL) のライブカーソル位置で判定する(境界の応答性の鍵)
        if matches!(event_type, EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED)
            && connected
            && now_ms() >= EDGE_GUARD_UNTIL_MS.load(Ordering::Relaxed)
        {
            if let Some(w) = SCREEN_W.get() {
                // delta 積算でカーソル位置を追跡(Deskflow の m_xCursor 方式)。
                // 32イベントに1回ライブ位置へ同期しドリフトを補正する
                let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
                let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
                let n = CUR_SYNC_N.fetch_add(1, Ordering::Relaxed);
                let mut pos = CUR_POS.lock().unwrap();
                pos.0 += dx;
                pos.1 += dy;
                if n % 32 == 0 {
                    if let Some(loc) = live_cursor() {
                        *pos = (loc.x, loc.y);
                    }
                }
                let (px, _py) = *pos;
                drop(pos);
                if px >= *w - 2.0 {
                    // 切替の瞬間はライブ位置で正確な高さを取る
                    let loc = live_cursor().unwrap_or(CGPoint { x: *w, y: 400.0 });
                    WIN_MODE.store(true, Ordering::Relaxed);
                    eprintln!("[mode] WINDOWS (edge) at ({:.0},{:.0})", loc.x, loc.y);
                    let mut ny = 0.5;
                    if let Some(sh) = SCREEN_H.get() {
                        ny = (1.0 - (loc.y / *sh)).clamp(0.0, 1.0);
                    }
                    send_msg(&Msg::Warp { nx: 0.03, ny });
                    enter_win_mode_cursor_lock();
                    return std::ptr::null_mut();
                }
            }
        }
        return event; // 素通し
    }

    // Windows モード: 全イベントを握って転送
    let flags = CGEventGetFlags(event);
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
                    55 => cmd,
                    56 | 60 => shift,
                    58 | 61 => opt,
                    59 | 62 => ctrl,
                    _ => false,
                }
            } else {
                event_type == EVT_KEY_DOWN
            };
            send_msg(&Msg::Key { kc, down, ctrl, opt, cmd, shift });
        }
        EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED => {
            let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
            let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
            if dx != 0.0 || dy != 0.0 {
                let sc = MOUSE_SCALE.get().copied().unwrap_or(1.0);
                send_msg(&Msg::MouseMove { dx: dx * sc, dy: dy * sc });
            }
            // カーソル固定の巻き戻しは 200ms 監視スレッドに集約した
            // (タップ内で毎イベント CGEventCreate すると負荷でカクつくため)
        }
        EVT_LEFT_DOWN | EVT_LEFT_UP => send_msg(&Msg::MouseButton { btn: 0, down: event_type == EVT_LEFT_DOWN }),
        EVT_RIGHT_DOWN | EVT_RIGHT_UP => send_msg(&Msg::MouseButton { btn: 1, down: event_type == EVT_RIGHT_DOWN }),
        EVT_OTHER_DOWN | EVT_OTHER_UP => send_msg(&Msg::MouseButton { btn: 2, down: event_type == EVT_OTHER_DOWN }),
        EVT_SCROLL_WHEEL => {
            let dy = CGEventGetIntegerValueField(event, FIELD_SCROLL_A1) as f64;
            let dx = CGEventGetIntegerValueField(event, FIELD_SCROLL_A2) as f64;
            if dx != 0.0 || dy != 0.0 {
                // ピクセル delta → ノッチ単位へ累積変換。0.25ノッチ刻みで送る。
                // 整数ノッチ単位だと遅いスクロールがカクつくため、細かい量子化で滑らかに。
                // 除数を大きくすると遅くなる(従来40は速すぎたので既定120)。端数は持ち越し。
                const Q: f64 = 0.25; // 量子化幅(ノッチ)= Windows 側は 30 wheel units 刻み
                let div = SCROLL_DIV.get().copied().unwrap_or(120.0);
                let mut acc = SCROLL_ACC.lock().unwrap();
                acc.0 += -dx / div;
                acc.1 += -dy / div;
                let (ix, iy) = ((acc.0 / Q).trunc() * Q, (acc.1 / Q).trunc() * Q);
                if ix != 0.0 || iy != 0.0 {
                    acc.0 -= ix;
                    acc.1 -= iy;
                    send_msg(&Msg::Scroll { dx: ix, dy: iy });
                }
            }
        }
        _ => {}
    }
    std::ptr::null_mut() // 握りつぶす
}

const BUILD_ID: &str = "build-20260925-163544-4d0b81b";

fn main() {
    eprintln!("[info] sd-mac {BUILD_ID}");
    let args: Vec<String> = std::env::args().collect();
    let _host = args
        .iter()
        .position(|a| a == "--host")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "100.84.0.2".to_string());
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);
    let token = std::env::var("SEAMLESS_DESK_TOKEN").unwrap_or_else(|_| "seamless-desk-dev".to_string());

    let test_mode = args.iter().any(|a| a == "--test");

    let (screen_w, screen_h) = unsafe {
        let d = CGMainDisplayID();
        let b = CGDisplayBounds(d);
        (b.size.w, b.size.h)
    };
    let _ = SCREEN_W.set(screen_w);
    let _ = SCREEN_H.set(screen_h);
    unsafe {
        if let Some(loc) = live_cursor() {
            *CUR_POS.lock().unwrap() = (loc.x, loc.y);
        }
    }
    if let Some(d) = std::env::var("SEAMLESS_SCROLL_DIV").ok().and_then(|v| v.parse::<f64>().ok()) {
        if d > 0.0 {
            let _ = SCROLL_DIV.set(d);
        }
    }
    if let Some(m) = std::env::var("SEAMLESS_MOUSE_SCALE").ok().and_then(|v| v.parse::<f64>().ok()) {
        if m > 0.0 {
            let _ = MOUSE_SCALE.set(m);
        }
    }
    eprintln!(
        "[info] screen {screen_w}x{screen_h}. listening on :{port} (server mode). scroll_div={}",
        SCROLL_DIV.get().copied().unwrap_or(120.0)
    );

    // 送信チャネル + 書き込みストリームスロット(接続が変わるたび差し替え)
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let _ = TX.set(tx);
    let slot: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
    let _ = STREAM_SLOT.set(slot.clone());

    // 単一の送信スレッド(チャネル→ストリーム差し替え方式)
    std::thread::spawn(move || {
        use std::io::Write;
        let mut ping_at = std::time::Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap();
                    if let Some(s) = guard.as_mut() {
                        if writeln!(s, "{line}").and_then(|_| s.flush()).is_err() {
                            *guard = None; // 書けなくなったら外す(接続ループが検知)
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
            if ping_at.elapsed() >= Duration::from_secs(5) {
                ping_at = std::time::Instant::now();
                // 15 秒 pong が無ければ実質切断扱いでストリームを外す
                // (TCP が生きていても相手プロセスが固まった場合を拾う)
                if now_ms().saturating_sub(LAST_PONG_MS.load(Ordering::Relaxed)) > 15_000 {
                    eprintln!("[conn] pong timeout. dropping stream");
                    let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap();
                    *guard = None;
                    continue;
                }
                let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap();
                if let Some(s) = guard.as_mut() {
                    if writeln!(s, "{}", encode(&Msg::Ping)).and_then(|_| s.flush()).is_err() {
                        *guard = None;
                    }
                }
            }
        }
    });

    // サーバ(受信待ち)スレッド: Windows からの接続を受け入れる
    // (本環境では Mac 発コネクションが不通なため、Windows 発に限定した設計)
    std::thread::spawn(move || {
        use std::io::{BufRead, Write};
        let listener = match std::net::TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[fatal] listen :{port} failed: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("[info] listening on :{port}");
        loop {
            let (stream, peer) = match listener.accept() {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("[conn] accept error: {e}");
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
            eprintln!("[conn] accepted from {peer}");
            stream.set_nodelay(true).ok();
            stream.set_read_timeout(Some(Duration::from_secs(20))).ok();
            // hello を待つ(検証して hello_ok を返す)
            let mut reader = std::io::BufReader::new(match stream.try_clone() {
                Ok(s) => s,
                Err(_) => continue,
            });
            let mut line = String::new();
            // 最初の行=hello
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    eprintln!("[conn] closed before hello");
                    continue;
                }
                Ok(_) => {}
            }
            let ok = match decode(&line) {
                Some(Msg::Hello { ver, name, token: t }) if ver == VERSION && t == token => {
                    let _ = name;
                    true
                }
                _ => false,
            };
            if !ok {
                eprintln!("[conn] invalid hello");
                continue;
            }
            {
                let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap();
                *guard = Some(stream);
            }
            // hello_ok 送信は送信スレッド経由で確実に
            send_msg(&Msg::HelloOk { name: "macbook".into(), w: screen_w as i32, h: screen_h as i32 });
            CONNECTED.store(true, Ordering::Relaxed);
            LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
            eprintln!("[conn] established");
            // 以降の受信ループ(Return / Pong / Bye)
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Some(msg) = decode(&line) {
                            match msg {
                                Msg::Return { ny } => {
                                    WIN_MODE.store(false, Ordering::Relaxed);
                                    eprintln!("[mode] MAC (return)");
                                    leave_win_mode_cursor_unlock(Some(ny));
                                }
                                Msg::Clip { text } => {
                                    if text.len() <= CLIP_MAX_BYTES {
                                        *LAST_RECV_CLIP.lock().unwrap() = Some(text.clone());
                                        unsafe { mac_set_clipboard(&text) };
                                        eprintln!("[clip] win->mac {} bytes", text.len());
                                    }
                                }
                                Msg::Pong => {
                                    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
                                }
                                Msg::Bye => break,
                                _ => {}
                            }
                        }
                    }
                }
            }
            {
                let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap();
                *guard = None;
            }
            CONNECTED.store(false, Ordering::Relaxed);
            WIN_MODE.store(false, Ordering::Relaxed);
            eprintln!("[conn] lost. waiting for reconnect...");
        }
    });

    // 実機E2E: notepadへ入力して Ctrl+S → ファイル名 → Enter で保存
    if args.iter().any(|a| a == "--test2") {
        std::thread::spawn(|| {
            for _ in 0..100 {
                if CONNECTED.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !CONNECTED.load(Ordering::Relaxed) {
                eprintln!("[test2] NOT CONNECTED");
                return;
            }
            std::thread::sleep(Duration::from_millis(800));
            WIN_MODE.store(true, Ordering::Relaxed);
            send_msg(&Msg::Minimize { title: "Windows Terminal".into() });
            send_msg(&Msg::Minimize { title: "terminal".into() });
            std::thread::sleep(Duration::from_millis(500));
            send_msg(&Msg::Focus { title: "メモ帳".into() });
            std::thread::sleep(Duration::from_millis(800));
            eprintln!("[test2] typing into notepad...");
            let type_str = |pairs: &[(u16, bool)]| {
                for &(kc, shift) in pairs {
                    send_msg(&Msg::Key { kc, down: true, ctrl: false, opt: false, cmd: false, shift });
                    send_msg(&Msg::Key { kc, down: false, ctrl: false, opt: false, cmd: false, shift });
                    std::thread::sleep(Duration::from_millis(25));
                }
            };
            // "seamless e2e ok" (Mac keycode)
            let body: Vec<(u16, bool)> = "seamless e2e ok".chars().filter_map(|c| {
                let kc = match c {
                    'a' => 0, 'b' => 11, 'c' => 8, 'd' => 2, 'e' => 14, 'f' => 3, 'g' => 5,
                    'h' => 4, 'i' => 34, 'j' => 38, 'k' => 40, 'l' => 37, 'm' => 46, 'n' => 45,
                    'o' => 31, 'p' => 35, 'q' => 12, 'r' => 15, 's' => 1, 't' => 17, 'u' => 32,
                    'v' => 9, 'w' => 13, 'x' => 7, 'y' => 16, 'z' => 6, ' ' => 49,
                    _ => return None,
                };
                Some((kc, false))
            }).collect();
            type_str(&body);
            std::thread::sleep(Duration::from_millis(300));
            // Cmd+S -> Win Ctrl+S(保存ダイアログ)
            send_msg(&Msg::Key { kc: 1, down: true, ctrl: false, opt: false, cmd: true, shift: false });
            send_msg(&Msg::Key { kc: 1, down: false, ctrl: false, opt: false, cmd: true, shift: false });
            std::thread::sleep(Duration::from_millis(800));
            // ファイル名欄: Cmd+A(全選択)して上書き
            send_msg(&Msg::Key { kc: 0, down: true, ctrl: false, opt: false, cmd: true, shift: false });
            send_msg(&Msg::Key { kc: 0, down: false, ctrl: false, opt: false, cmd: true, shift: false });
            std::thread::sleep(Duration::from_millis(200));
            // "e2eok.txt"
            let name: Vec<(u16, bool)> = "e2eok.txt".chars().filter_map(|c| {
                let kc = match c {
                    'a' => 0, 'e' => 14, 'k' => 40, 'o' => 31, 't' => 17, 'x' => 7,
                    '.' => 47, '2' => 19,
                    _ => return None,
                };
                Some((kc, false))
            }).collect();
            type_str(&name);
            std::thread::sleep(Duration::from_millis(200));
            // Enter(36)
            send_msg(&Msg::Key { kc: 36, down: true, ctrl: false, opt: false, cmd: false, shift: false });
            send_msg(&Msg::Key { kc: 36, down: false, ctrl: false, opt: false, cmd: false, shift: false });
            std::thread::sleep(Duration::from_millis(500));
            eprintln!("[test2] done (typed + saved)");
            WIN_MODE.store(false, Ordering::Relaxed);
        });
    }

    // テストモード: 接続確立後にキー列を自動送信(E2E検証用)
    if test_mode {
        std::thread::spawn(|| {
            for _ in 0..100 {
                if CONNECTED.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !CONNECTED.load(Ordering::Relaxed) {
                eprintln!("[test] NOT CONNECTED");
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
            WIN_MODE.store(true, Ordering::Relaxed);
            eprintln!("[test] sending key sequence...");
            // "SDEOK" の Mac keycode 列
            for kc in [1u16, 2, 14, 31, 40] {
                send_msg(&Msg::Key { kc, down: true, ctrl: false, opt: false, cmd: false, shift: false });
                send_msg(&Msg::Key { kc, down: false, ctrl: false, opt: false, cmd: false, shift: false });
                std::thread::sleep(Duration::from_millis(50));
            }
            send_msg(&Msg::MouseMove { dx: 120.0, dy: 60.0 });
            send_msg(&Msg::Scroll { dx: 0.0, dy: 1.0 });
            std::thread::sleep(Duration::from_millis(300));
            eprintln!("[test] sent all");
            WIN_MODE.store(false, Ordering::Relaxed);
        });
    }

    // 診断モード: 1秒ごとにモード/受信・送信カウント/実カーソル位置を記録
    if args.iter().any(|a| a == "--diag") {
        DIAG_ENABLED.store(true, Ordering::Relaxed);
        std::thread::spawn(|| {
            let mut last_cursor = (0.0f64, 0.0f64);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let (mode, mv, kd, sd, wp) = (
                    WIN_MODE.load(Ordering::Relaxed),
                    DIAG_MOVE_COUNT.load(Ordering::Relaxed),
                    DIAG_KEY_COUNT.load(Ordering::Relaxed),
                    DIAG_SEND_COUNT.load(Ordering::Relaxed),
                    DIAG_WARP_COUNT.load(Ordering::Relaxed),
                );
                unsafe {
                    let ev = CGEventCreate(std::ptr::null_mut());
                    let p = if ev.is_null() { CGPoint { x: 0.0, y: 0.0 } } else { CGEventGetLocation(ev) };
                    let moved = (p.x - last_cursor.0).abs() + (p.y - last_cursor.1).abs() > 1.0;
                    eprintln!(
                        "[diag] mode={} move_recv={mv} key_recv={kd} sent={sd} warp_fixed={wp} cursor=({:.0},{:.0}) cursor_moving={}",
                        if mode { "WIN" } else { "MAC" }, p.x, p.y, moved
                    );
                    last_cursor = (p.x, p.y);
                }
            }
        });
    }

    // クリップボード監視(Mac→Windows 方向): changeCount の変化でテキストを送る
    std::thread::spawn(|| {
        let mut last_count = clipboard_change_count();
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let cnt = clipboard_change_count();
            if cnt == last_count {
                continue;
            }
            last_count = cnt;
            if !CONNECTED.load(Ordering::Relaxed) {
                continue;
            }
            let Some(text) = (unsafe { mac_get_clipboard() }) else { continue };
            if text.is_empty() || text.len() > CLIP_MAX_BYTES {
                continue;
            }
            // 自分が Windows から受信して書き込んだ内容は送り返さない(ループ防止)
            if LAST_RECV_CLIP.lock().unwrap().as_deref() == Some(text.as_str()) {
                continue;
            }
            eprintln!("[clip] mac->win {} bytes", text.len());
            send_msg(&Msg::Clip { text });
        }
    });

    // WIN モード中のカーソル固定監視(改善ループ4):
    // イベントタップ経由の巻き戻しは移動イベントが来た時しか働かない。
    // 慣性や関連切断の効き遅れでカーソルが動いたままになる場合に備え、
    // 常時 200ms ごとに固定位置へ巻き戻す(境界の同時移動抑止の最終防衛)
    std::thread::spawn(|| {
        let mut fixes: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(200));
            if !WIN_MODE.load(Ordering::Relaxed) {
                continue;
            }
            let Some((lx, ly)) = *LOCK_POS.lock().unwrap() else { continue };
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
    eprintln!("[info] tap active. カーソルを画面右端へ動かすと Windows モード / F13 でトグル");
    unsafe { CFRunLoopRun() };
}
