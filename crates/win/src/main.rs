// sd-win: Windows 側サーバ。TCP で受けた入力イベントを SendInput で注入する。
// 必須: 対話セッション起動 + OpenInputDesktop(フル権限) + SetThreadDesktop
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use sd_common::keymap::mac_kc_to_win_vk;

static DEBUG_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
use sd_common::proto::{decode, encode, Msg, PORT, VERSION};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VK_CONTROL, VK_MENU,
    VK_SHIFT, VK_LWIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowTextW,
    IsWindowVisible, SetCursorPos, SetForegroundWindow, ShowWindow, SM_CXSCREEN, SM_CYSCREEN,
    SW_RESTORE,
};

const INPUT_MOUSE: u32 = 0;
const MOUSEEVENTF_MOVE: u32 = 0x0001;
const VK_LBUTTON_SENTINEL: u16 = 0xFF; // 使用しない(ボタンは専用関数で)
const VK_XBUTTON1: u16 = 0x05;
const VK_XBUTTON2: u16 = 0x06;

// ---------- Win32 直宣言(desktop 接続) ----------
#[link(name = "user32")]
unsafe extern "system" {
    fn OpenInputDesktop(dwFlags: u32, fInherit: bool, dwDesiredAccess: u32) -> *mut core::ffi::c_void;
    fn SetThreadDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
    fn CloseDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
    // クリップボード
    fn OpenClipboard(hWndNewOwner: *mut core::ffi::c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(uFormat: u32) -> *mut core::ffi::c_void;
    fn SetClipboardData(uFormat: u32, hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

// ---------- Win32 直宣言(グローバルメモリ) ----------
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> *mut core::ffi::c_void;
    fn GlobalLock(hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalUnlock(hMem: *mut core::ffi::c_void) -> i32;
    fn GlobalFree(hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

// ---------- Win32 直宣言(IME 制御) ----------
#[link(name = "imm32")]
unsafe extern "system" {
    fn ImmGetContext(hwnd: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn ImmSetOpenStatus(himc: *mut core::ffi::c_void, fOpen: i32) -> i32;
    fn ImmReleaseContext(hwnd: *mut core::ffi::c_void, himc: *mut core::ffi::c_void) -> i32;
}

/// フォアグラウンドウィンドウの IME を開(かな)/閉じ(英数)する。
/// キーエミュレート(VK_KANJI 等)と違い方向指定が確実。
/// IME コンテキストが取れないウィンドウでは半角/全角キー相当の
/// VK_KANJI 注入へフォールバックする(トグル動作)。
fn ime_set_open(open: bool) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_null() {
            let himc = ImmGetContext(hwnd);
            if !himc.is_null() {
                let ok = ImmSetOpenStatus(himc, open as i32);
                ImmReleaseContext(hwnd, himc);
                if ok != 0 {
                    println!("[ime] ImmSetOpenStatus({open}) ok");
                    return;
                }
                println!("[ime] ImmSetOpenStatus({open}) failed -> fallback");
            } else {
                println!("[ime] ImmGetContext=null -> fallback");
            }
        } else {
            println!("[ime] no foreground window -> fallback");
        }
        // フォールバック: 半角/全角キー(VK_KANJI)の押し離し
        inject_key(0xF4, false);
        inject_key(0xF4, true);
    }
}

const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;
/// 最後に Mac から受信して書き込んだテキスト(エコーバック送信防止)
static LAST_RECV_CLIP: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
const CLIP_MAX_CHARS: usize = 512 * 1024;

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
                while *p.add(len) != 0 && len <= CLIP_MAX_CHARS {
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

fn clipboard_write_text(s: &str) -> bool {
    let mut utf16: Vec<u16> = s.encode_utf16().collect();
    utf16.push(0);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
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

// ---------- INPUT 手動パック(type+pad+32byte共用体=40byte) ----------
#[repr(C)]
#[derive(Clone, Copy)]
struct InputBuf {
    itype: u32,
    _pad: u32,
    body: [u32; 6],
    extra: usize,
}

fn send_input_buf(buf: InputBuf) -> bool {
    assert_eq!(std::mem::size_of::<InputBuf>(), std::mem::size_of::<INPUT>());
    unsafe {
        SendInput(
            1,
            &buf as *const _ as *const INPUT,
            std::mem::size_of::<InputBuf>() as i32,
        ) == 1
    }
}

fn inject_key(vk: u16, up: bool) -> bool {
    send_input_buf(InputBuf {
        itype: INPUT_KEYBOARD,
        _pad: 0,
        body: [vk as u32, if up { KEYEVENTF_KEYUP } else { 0 }, 0, 0, 0, 0],
        extra: 0,
    })
}

fn inject_mouse_move_rel(dx: i32, dy: i32) -> bool {
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [dx as u32, dy as u32, 0, MOUSEEVENTF_MOVE, 0, 0],
        extra: 0,
    })
}

fn inject_mouse_btn(btn: u8, down: bool) -> bool {
    const LEFTDOWN: u32 = 0x0002;
    const LEFTUP: u32 = 0x0004;
    const RIGHTDOWN: u32 = 0x0008;
    const RIGHTUP: u32 = 0x0010;
    const MIDDLEDOWN: u32 = 0x0020;
    const MIDDLEUP: u32 = 0x0040;
    let flags = match (btn, down) {
        (0, true) => LEFTDOWN,
        (0, false) => LEFTUP,
        (1, true) => RIGHTDOWN,
        (1, false) => RIGHTUP,
        (2, true) => MIDDLEDOWN,
        (2, false) => MIDDLEUP,
        _ => return true,
    };
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [0, 0, 0, flags, 0, 0],
        extra: 0,
    })
}

fn inject_scroll(dx: f64, dy: f64) -> bool {
    // WHEEL_DELTA=120。縦優先で1イベントにまとめる
    const WHEEL: u32 = 0x0800;
    const HWHEEL: u32 = 0x1000;
    let mut ok = true;
    if dy.abs() >= 1.0 {
        let amount = (-dy * 120.0).round() as i32; // 下スクロール(Mac dy負)→Winは正
        ok &= send_input_buf(InputBuf {
            itype: INPUT_MOUSE,
            _pad: 0,
            body: [0, 0, amount as u32, WHEEL, 0, 0],
            extra: 0,
        });
    }
    if dx.abs() >= 1.0 {
        let amount = (dx * 120.0).round() as i32;
        ok &= send_input_buf(InputBuf {
            itype: INPUT_MOUSE,
            _pad: 0,
            body: [0, 0, amount as u32, HWHEEL, 0, 0],
            extra: 0,
        });
    }
    ok
}

// ---------- 修飾キー状態管理(Mac mods → Win VK) ----------
struct ModState {
    ctrl: bool,
    alt: bool,
    win: bool,
    shift: bool,
}
impl ModState {
    fn new() -> Self {
        Self { ctrl: false, alt: false, win: false, shift: false }
    }
    fn apply(&mut self, ctrl: bool, opt: bool, cmd: bool, shift: bool) {
        // Mac: cmd→Win Ctrl, option→Alt, ctrl→Winキー, shift→Shift
        let want = [(VK_CONTROL, cmd), (VK_MENU, opt), (VK_LWIN, ctrl), (VK_SHIFT, shift)];
        let have = [
            (VK_CONTROL, self.ctrl),
            (VK_MENU, self.alt),
            (VK_LWIN, self.win),
            (VK_SHIFT, self.shift),
        ];
        for (vk, pressed_now) in have {
            let target = want.iter().find(|(v, _)| *v == vk).map(|(_, t)| *t).unwrap_or(false);
            if pressed_now != target {
                inject_key(vk, !target);
            }
        }
        self.ctrl = cmd;
        self.alt = opt;
        self.win = ctrl;
        self.shift = shift;
    }
    fn release_all(&mut self) {
        self.apply(false, false, false, false);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let token = std::env::var("SEAMLESS_DESK_TOKEN")
        .unwrap_or_else(|_| "seamless-desk-dev".to_string());
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);

    // 対話デスクトップへ接続(SSH 起動では失敗する。schtasks/スタートアップ起動を使う)
    unsafe {
        let desk = OpenInputDesktop(0, false, 0x01FF);
        if desk.is_null() {
            eprintln!("[fatal] OpenInputDesktop failed. 対話セッションで起動してください");
            exit(1);
        }
        if SetThreadDesktop(desk) == 0 {
            eprintln!("[fatal] SetThreadDesktop failed");
            CloseDesktop(desk);
            exit(1);
        }
    }

    let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    println!("[info] desktop attached. screen {w}x{h}. listening on :{port}");

    if args.iter().any(|a| a == "--debug-keys") {
        DEBUG_KEYS.store(true, Ordering::Relaxed);
    }
    let host = args
        .iter()
        .position(|a| a == "--host")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "100.100.10.9".to_string());
    println!("[info] desktop attached. screen {w}x{h}. connecting to {host}:{port}");

    use std::net::ToSocketAddrs;
    let addr = (host.as_str(), port).to_socket_addrs().ok().and_then(|mut it| it.next());
    let addr = match addr {
        Some(a) => a,
        None => {
            eprintln!("[fatal] invalid host");
            exit(1);
        }
    };

    // クライアントとして Mac へ接続し続ける(Mac 発コネクションは本環境で不通のため)
    let mut backoff = 500u64;
    loop {
        match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            Ok(s) => {
                println!("[conn] connected");
                if let Err(e) = serve(s, &token, w, h) {
                    println!("[disc] {e}");
                }
                backoff = 500;
            }
            Err(e) => println!("[conn] failed: {e}"),
        }
        std::thread::sleep(Duration::from_millis(backoff));
        backoff = (backoff * 2).min(5000);
    }
}

fn serve(stream: TcpStream, token: &str, w: i32, h: i32) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    let mut writer = stream.try_clone()?;
    // クライアントとして hello を送る
    let mut hello_sent = false;
    for _ in 0..3 {
        let hello = encode(&Msg::Hello { ver: VERSION, name: "desktop".into(), token: token.to_string() });
        if writeln!(writer, "{hello}").and_then(|_| writer.flush()).is_ok() {
            hello_sent = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if !hello_sent {
        return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "hello send failed"));
    }
    let reader = BufReader::new(stream);
    let mut mods = ModState::new();
    // マウス移動のサブピクセル残高。Mac のトラックパッドは 1px 未満の delta が
    // 連続するため、毎回 round すると遅い移動が消えてカクカクする。整数部のみ注入し
    // 端数は次イベントへ持ち越す。
    let mut accum = (0.0f64, 0.0f64);
    let mut hello_done = false;
    let mut last_return_notify = Instant::now() - Duration::from_secs(10);
    let running = Arc::new(AtomicBool::new(true));
    let running_w = running.clone();

    // heartbeat 監視スレッド(15秒 pong なしで切断)
    let hb_writer = writer.try_clone()?;
    let hb_running = running.clone();
    std::thread::spawn(move || {
        let mut last_recv = Instant::now();
        let _ = hb_writer; // ping送信は省略(クライアント主導)
        while hb_running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            // 読み取り側が last_recv を共有しない簡易版: ソケット生死は read ブロックで判定
            // (TCP keepalive 相当。切断了ら read が Err を返し running=false になる)
        }
    });
    let _ = hb_writer;

    // クリップボード監視スレッド(Windows→Mac 方向)。起動時点の内容は送らない。
    let mut cb_writer = writer.try_clone()?;
    let cb_running = running.clone();
    std::thread::spawn(move || {
        let mut last_sent = clipboard_read_text();
        while cb_running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
            let Some(text) = clipboard_read_text() else { continue };
            if text.len() > CLIP_MAX_CHARS || last_sent.as_deref() == Some(text.as_str()) {
                continue;
            }
            // Mac から受信して書き込んだ内容は送り返さない(ループ防止)
            let echo = LAST_RECV_CLIP
                .lock()
                .map(|g| g.as_deref() == Some(text.as_str()))
                .unwrap_or(false);
            if echo {
                continue;
            }
            if writeln!(cb_writer, "{}", encode(&Msg::Clip { text: text.clone() }))
                .and_then(|_| cb_writer.flush())
                .is_ok()
            {
                println!("[clip] win->mac {} bytes", text.len());
                last_sent = Some(text);
            }
        }
    });

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                running_w.store(false, Ordering::Relaxed);
                return Err(e);
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg = match decode(&line) {
            Some(m) => m,
            None => continue,
        };
        match msg {
            Msg::HelloOk { name, w: mw, h: mh } => {
                println!("[hello] ok from {name} (mac screen {mw}x{mh})");
                hello_done = true;
            }
            Msg::Ping => {
                let _ = writeln!(writer, "{}", encode(&Msg::Pong));
                let _ = writer.flush();
            }
            Msg::Key { kc, down, ctrl, opt, cmd, shift } => {
                if !hello_done {
                    continue;
                }
                if DEBUG_KEYS.load(Ordering::Relaxed) && down {
                    let ch = sd_common::charmap::mac_kc_to_char(kc);
                    println!("[key] kc={kc} ch={ch:?} mods c={ctrl} o={opt} m={cmd} s={shift}");
                }
                // Mac JIS の 英数(102)/かな(104)キーは Windows 側 IME の開閉に変換する
                // (HIToolbox 実測: kVK_JIS_Eisu=102, kVK_JIS_Kana=104)
                if down {
                    match kc {
                        104 => {
                            ime_set_open(true);
                            if DEBUG_KEYS.load(Ordering::Relaxed) {
                                println!("[ime] kana(kc=104) -> IME on");
                            }
                            continue;
                        }
                        102 => {
                            ime_set_open(false);
                            if DEBUG_KEYS.load(Ordering::Relaxed) {
                                println!("[ime] eisu(kc=102) -> IME off");
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
                mods.apply(ctrl, opt, cmd, shift);
                if let Some(vk) = mac_kc_to_win_vk(kc) {
                    let ok = inject_key(vk, !down);
                    if DEBUG_KEYS.load(Ordering::Relaxed) && !ok {
                        println!("[key] INJECT FAILED kc={kc}");
                    }
                }
            }
            Msg::MouseMove { dx, dy } => {
                if !hello_done {
                    continue;
                }
                accum.0 += dx;
                accum.1 += dy;
                let (ix, iy) = (accum.0.trunc(), accum.1.trunc());
                if ix != 0.0 || iy != 0.0 {
                    accum.0 -= ix;
                    accum.1 -= iy;
                    inject_mouse_move_rel(ix as i32, iy as i32);
                }
                maybe_notify_return(&writer, &mut last_return_notify, h, &mut mods);
            }
            Msg::MouseButton { btn, down } => {
                if !hello_done {
                    continue;
                }
                inject_mouse_btn(btn, down);
            }
            Msg::Scroll { dx, dy } => {
                if !hello_done {
                    continue;
                }
                inject_scroll(dx, dy);
            }
            Msg::Focus { title } => {
                if !hello_done {
                    continue;
                }
                let hwnd = find_window_by_title(&title);
                if !hwnd.is_null() {
                    unsafe {
                        ShowWindow(hwnd, SW_RESTORE);
                        SetForegroundWindow(hwnd);
                    }
                    println!("[focus] {title} -> focused");
                } else {
                    println!("[focus] window not found: {title}");
                }
            }
            Msg::Warp { nx, ny } => {
                if !hello_done {
                    continue;
                }
                let x = (nx.clamp(0.0, 1.0) * w as f64) as i32;
                let y = (ny.clamp(0.0, 1.0) * h as f64) as i32;
                unsafe { SetCursorPos(x, y) };
                last_return_notify = Instant::now();
            }
            Msg::Minimize { title } => {
                if !hello_done {
                    continue;
                }
                let hwnd = find_window_by_title(&title);
                if !hwnd.is_null() {
                    unsafe { ShowWindow(hwnd, 6); } // SW_MINIMIZE
                    println!("[minimize] {title}");
                } else {
                    println!("[minimize] window not found: {title}");
                }
            }
            Msg::Clip { text } => {
                if text.len() > CLIP_MAX_CHARS {
                    continue;
                }
                *LAST_RECV_CLIP.lock().unwrap() = Some(text.clone());
                if clipboard_write_text(&text) {
                    println!("[clip] mac->win {} bytes", text.len());
                } else {
                    println!("[clip] mac->win write failed");
                }
            }
            Msg::Bye => {
                running_w.store(false, Ordering::Relaxed);
                break;
            }
            _ => {}
        }
    }
    mods.release_all();
    running_w.store(false, Ordering::Relaxed);
    Ok(())
}

/// カーソルが画面左端に達したら Mac へ復帰通知(連打防止 1 秒クールダウン)。
/// カーソル高さも正規化して送り、Mac 側の復帰位置に反映させる(境界の連続性)。
fn maybe_notify_return(
    mut writer: &TcpStream,
    last: &mut Instant,
    h: i32,
    mods: &mut ModState,
) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    if p.x <= 0 && last.elapsed() >= Duration::from_secs(1) {
        let ny = if h > 0 { (p.y as f64 / h as f64).clamp(0.0, 1.0) } else { 0.5 };
        let _ = writeln!(writer, "{}", encode(&Msg::Return { ny }));
        *last = Instant::now();
        // Mac へ制御を返すため、押しっぱなしの修飾キーを離して後片付けする
        mods.release_all();
    }
}


// ---------- タイトル部分一致でメインウィンドウを検索 ----------
struct EnumCtx {
    needle: String,
    hwnd: *mut core::ffi::c_void,
}

unsafe extern "system" fn enum_cb(hwnd: *mut core::ffi::c_void, lparam: isize) -> i32 {
    let ctx = &mut *(lparam as *mut EnumCtx);
    let mut buf = [0u16; 256];
    let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
    let title = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
    if IsWindowVisible(hwnd) != 0 && title.to_lowercase().contains(&ctx.needle.to_lowercase()) {
        ctx.hwnd = hwnd;
        return 0; // 列挙停止
    }
    1
}

fn find_window_by_title(needle: &str) -> *mut core::ffi::c_void {
    let mut ctx = EnumCtx {
        needle: needle.to_string(),
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(enum_cb), &mut ctx as *mut _ as isize);
    }
    ctx.hwnd
}
