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
    fn CFMachPortCreateRunLoopSource(alloc: CFAllocatorRef, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
    fn CFRunLoopGetMain() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: CFStringRef;
}

// ---------- 共有状態 ----------
static WIN_MODE: AtomicBool = AtomicBool::new(false);
static CONNECTED: AtomicBool = AtomicBool::new(false);
static TX: OnceLock<Sender<String>> = OnceLock::new();
static STREAM_SLOT: OnceLock<Arc<Mutex<Option<TcpStream>>>> = OnceLock::new();
static SCREEN_W: OnceLock<f64> = OnceLock::new();

fn send_msg(msg: &Msg) {
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
            }
            return std::ptr::null_mut();
        }
    }

    if !win_mode {
        // Mac モード: 右端到達で Windows モードへ
        if matches!(event_type, EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED)
            && connected
        {
            if let Some(w) = SCREEN_W.get() {
                let loc = CGEventGetLocation(event);
                if loc.x >= *w - 2.0 {
                    WIN_MODE.store(true, Ordering::Relaxed);
                    eprintln!("[mode] WINDOWS (edge)");
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
                send_msg(&Msg::MouseMove { dx, dy });
            }
        }
        EVT_LEFT_DOWN | EVT_LEFT_UP => send_msg(&Msg::MouseButton { btn: 0, down: event_type == EVT_LEFT_DOWN }),
        EVT_RIGHT_DOWN | EVT_RIGHT_UP => send_msg(&Msg::MouseButton { btn: 1, down: event_type == EVT_RIGHT_DOWN }),
        EVT_OTHER_DOWN | EVT_OTHER_UP => send_msg(&Msg::MouseButton { btn: 2, down: event_type == EVT_OTHER_DOWN }),
        EVT_SCROLL_WHEEL => {
            let dy = CGEventGetIntegerValueField(event, FIELD_SCROLL_A1) as f64;
            let dx = CGEventGetIntegerValueField(event, FIELD_SCROLL_A2) as f64;
            if dx != 0.0 || dy != 0.0 {
                // Mac のピクセル delta → Windows detent(120 単位)への概算変換
                send_msg(&Msg::Scroll { dx: -dx / 40.0, dy: -dy / 40.0 });
            }
        }
        _ => {}
    }
    std::ptr::null_mut() // 握りつぶす
}

fn main() {
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
    eprintln!("[info] screen {screen_w}x{screen_h}. listening on :{port} (server mode)");

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
            eprintln!("[conn] established");
            // 以降の受信ループ(Return / Pong / Bye)
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Some(msg) = decode(&line) {
                            match msg {
                                Msg::Return => {
                                    WIN_MODE.store(false, Ordering::Relaxed);
                                    eprintln!("[mode] MAC (return)");
                                    if let Some(w) = SCREEN_W.get() {
                                        unsafe {
                                            CGWarpMouseCursorPosition(CGPoint { x: *w - 60.0, y: 400.0 });
                                        }
                                    }
                                }
                                Msg::Pong => {}
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
            0, // kCGSessionEventTap
            0, // kCGHeadInsertEventTap
            1, // kCGEventTapOptionDefault(抑制可)
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    };
    if tap.is_null() {
        eprintln!("[fatal] CGEventTapCreate failed(アクセシビリティ権限を確認)");
        std::process::exit(1);
    }
    unsafe {
        let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
        let rl = CFRunLoopGetMain();
        CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    eprintln!("[info] tap active. カーソルを画面右端へ動かすと Windows モード / F13 でトグル");
    unsafe { CFRunLoopRun() };
}
