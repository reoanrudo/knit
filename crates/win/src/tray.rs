// タスクトレイ常駐(Shell_NotifyIcon)。コンソールなし運用の状態可視化と終了操作。
// 「Windows 側のターミナルを消したら繋がらない」問題の恒久对策:
// このプロセス自体が GUI サブシステム+トレイ常駐で動き、ターミナル前提を消す。
// NOTIFYICONDATAW は ABI が安定しているため自前定義(Shell feature への依存を避ける)
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};

use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, LoadImageW, PostMessageW,
    RegisterClassW, SetForegroundWindow, SetTimer, SetWindowTextW, ShowWindow, TrackPopupMenu,
    TranslateMessage, HMENU, MSG, WNDCLASSW, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, SW_HIDE, SW_SHOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_SETFONT, WM_TIMER,
};

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: HWND, verb: *const u16, file: *const u16, params: *const u16,
        dir: *const u16, show: i32,
    ) -> isize;
}

const WM_TRAY: u32 = WM_APP + 1;
const MENU_QUIT: u32 = 1001;
const MENU_STATUS: u32 = 1002;
const MENU_AUDIO: u32 = 1003;
const MENU_OPENLOG: u32 = 1004;
const MENU_RESTART: u32 = 1005;

#[link(name = "shell32")]
#[link(name = "user32")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn Shell_NotifyIconW(dw_message: u32, lp_data: *mut NotifyIconData) -> i32;
    fn GetModuleHandleW(lp_file_name: *const u16) -> *mut core::ffi::c_void;
}

// ---------- 自前 NOTIFYICONDATAW(x64 ABI: サイズ976) ----------
#[repr(C)]
struct NotifyIconData {
    cb_size: u32,
    hwnd: HWND,
    u_id: u32,
    u_flags: u32,
    u_callback_message: u32,
    h_icon: *mut core::ffi::c_void,
    sz_tip: [u16; 128],
    dw_state: u32,
    dw_state_mask: u32,
    sz_info: [u16; 256],
    u_timeout: u32,
    sz_info_title: [u16; 64],
    dw_info_flags: u32,
    guid_item: [u8; 16],
    h_balloon_icon: *mut core::ffi::c_void,
}
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_INFO: u32 = 0x10;
const NIIF_INFO: u32 = 0x01;

static TRAY_HWND: AtomicUsize = AtomicUsize::new(0);
static TRAY_HICON: AtomicUsize = AtomicUsize::new(0);
static TRAY_HINST: AtomicUsize = AtomicUsize::new(0);
static STATUS_HWND: AtomicUsize = AtomicUsize::new(0);
static LABEL_STATE: AtomicUsize = AtomicUsize::new(0);
static LABEL_BUILD: AtomicUsize = AtomicUsize::new(0);
static LABEL_AUDIO: AtomicUsize = AtomicUsize::new(0);
static LABEL_RTT: AtomicUsize = AtomicUsize::new(0);
static LABEL_SPK: AtomicUsize = AtomicUsize::new(0);
static LABEL_FILES: AtomicUsize = AtomicUsize::new(0);

fn wide_into(buf: &mut [u16], s: &str) {
    for (dst, src) in buf.iter_mut().zip(s.encode_utf16()) {
        *dst = src;
    }
}

fn build_line() -> String {
    format!("バージョン: {} ({})", crate::VERSION_STR, crate::BUILD_ID)
}
fn audio_line() -> String {
    if crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed) {
        "音声転送: ON(Windows の音を Mac で再生)".to_string()
    } else {
        "音声転送: OFF".to_string()
    }
}
/// Mac が測定した RTT(接続品質)。未測定/切断時は --
fn rtt_line() -> String {
    if !crate::CONNECTED.load(Ordering::Relaxed) {
        return "遅延: --".to_string();
    }
    let ms = crate::RTT_MS.load(Ordering::Relaxed);
    if ms == 0 {
        "遅延: 計測中…".to_string()
    } else {
        format!("遅延: {ms}ms")
    }
}
/// ファイル受信の累計(ステータス窓の表示)
fn files_line() -> String {
    let n = crate::FILES_RX.load(Ordering::Relaxed);
    if n == 0 {
        "ファイル受信: なし".to_string()
    } else {
        format!("ファイル受信: 累計 {n} 件")
    }
}

/// この PC のスピーカー状態(接続中ミュート=Mac のみ発音 の表示)
fn spk_line() -> String {
    if !crate::audio::AUDIO_ACTIVE.load(Ordering::Relaxed) {
        return "スピーカー: --(音声転送なし)".to_string();
    }
    let mode = crate::SPK_MUTE_MODE.load(Ordering::Relaxed);
    let conn = crate::CONNECTED.load(Ordering::Relaxed);
    match (mode, conn) {
        (true, true) => "スピーカー: ミュート中(Mac のみ発音)".to_string(),
        (true, false) => "スピーカー: 接続時にミュート".to_string(),
        _ => "スピーカー: 常時鳴らす".to_string(),
    }
}
fn set_text(h: usize, s: &str) {
    if h == 0 {
        return;
    }
    let mut w: Vec<u16> = s.encode_utf16().collect();
    w.push(0);
    unsafe { SetWindowTextW(h as _, w.as_ptr()) };
}

fn tray_status_text() -> String {
    let conn = if crate::CONNECTED.load(Ordering::Relaxed) { "接続済" } else { "切断(再接続中)" };
    format!("tsunagu: {conn} / {}", crate::BUILD_ID)
}

/// バルーン通知(接続/切断の可視化)。どのスレッドからでも呼べる
pub fn notify(title: &str, text: &str) {
    unsafe {
        let hwnd = TRAY_HWND.load(Ordering::Relaxed) as HWND;
        if hwnd.is_null() {
            return;
        }
        let mut nid = std::mem::zeroed::<NotifyIconData>();
        nid.cb_size = std::mem::size_of::<NotifyIconData>() as u32;
        nid.hwnd = hwnd;
        nid.u_id = 1;
        nid.u_flags = NIF_INFO;
        nid.dw_info_flags = NIIF_INFO;
        wide_into(&mut nid.sz_info_title, title);
        wide_into(&mut nid.sz_info, text);
        Shell_NotifyIconW(NIM_MODIFY, &mut nid);
    }
}

unsafe fn update_tip() {
    let hwnd = TRAY_HWND.load(Ordering::Relaxed) as HWND;
    if hwnd.is_null() {
        return;
    }
    let mut nid = std::mem::zeroed::<NotifyIconData>();
    nid.cb_size = std::mem::size_of::<NotifyIconData>() as u32;
    nid.hwnd = hwnd;
    nid.u_id = 1;
    nid.u_flags = NIF_TIP;
    wide_into(&mut nid.sz_tip, &tray_status_text());
    Shell_NotifyIconW(NIM_MODIFY, &mut nid);
}

unsafe extern "system" fn tray_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let mouse = (lparam & 0xFFFF) as u32;
            if mouse == WM_LBUTTONUP {
                open_status_window(); // 左クリック=アプリ画面(Windows 標準操作)
            } else if mouse == WM_RBUTTONUP {
                open_menu(hwnd);
            }
            0
        }
        WM_TIMER => {
            update_tip();
            update_labels();
            0
        }
        WM_COMMAND => {
            handle_command((wparam & 0xFFFF) as u32);
            0
        }
        WM_DESTROY => {
            let mut nid = std::mem::zeroed::<NotifyIconData>();
            nid.cb_size = std::mem::size_of::<NotifyIconData>() as u32;
            nid.hwnd = hwnd;
            nid.u_id = 1;
            Shell_NotifyIconW(NIM_DELETE, &mut nid);
            PostMessageW(hwnd, WM_NULL, 0, 0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// メニュー/ボタン共通のコマンド処理
unsafe fn handle_command(id: u32) {
    match id {
        MENU_STATUS => open_status_window(),
        MENU_AUDIO => {
            let next = !crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed);
            crate::audio::AUDIO_ENABLED.store(next, Ordering::Relaxed);
            println!("[tray] 音声転送 -> {next}");
            update_labels();
            update_tip();
        }
        MENU_OPENLOG => {
            let mut log: Vec<u16> = r"C:\Users\<user>\tsunagu\tsunagu-win.log".encode_utf16().collect();
            log.push(0);
            let mut verb = wide("open");
            let mut np = wide("notepad.exe");
            ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), np.as_ptr(), log.as_ptr(), std::ptr::null(), 5 /*SW_SHOW*/);
        }
        MENU_RESTART => {
            // exe を止めると毎分の自動復帰タスクが起こす=確実な再起動
            eprintln!("[tray] 再起動します(自動復帰タスクが起こします)");
            std::process::exit(0);
        }
        MENU_QUIT => {
            eprintln!("[tray] メニューから終了しました");
            std::process::exit(0);
        }
        _ => {}
    }
}

unsafe extern "system" fn status_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            handle_command((wparam & 0xFFFF) as u32);
            0
        }
        WM_CLOSE => {
            // 閉じても破棄せず隠すだけ(常駐アプリの標準動作)
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// モダンな見た目のための Segoe UI フォント生成(通常/太字)。
/// 既定の DEFAULT_GUI_FONT は古いシステムフォントになるため使わない
unsafe fn segoe_font(bold: bool, height: i32) -> *mut core::ffi::c_void {
    unsafe {
        let mut name: Vec<u16> = "Segoe UI".encode_utf16().collect();
        name.push(0);
        extern "system" {
            fn CreateFontW(
                height: i32, width: i32, escapement: i32, orientation: i32, weight: i32,
                italic: u32, underline: u32, strikeout: u32, charset: u32, outprecision: u32,
                clipprecision: u32, quality: u32, pitchandfamily: u32, face: *const u16,
            ) -> *mut core::ffi::c_void;
        }
        CreateFontW(
            height, 0, 0, 0, if bold { 700 } else { 400 }, 0, 0, 0, 1 /*DEFAULT_CHARSET*/,
            0, 0, 5 /*CLEARTYPE_QUALITY*/, 0, name.as_ptr(),
        )
    }
}

fn wide(s: &str) -> Vec<u16> {
    let mut w: Vec<u16> = s.encode_utf16().collect();
    w.push(0);
    w
}

/// ステータスウィンドウ(アプリ本体の画面)を開く。トレイ左クリック/メニューから
unsafe fn open_status_window() {
    unsafe {
        let existing = STATUS_HWND.load(Ordering::Relaxed) as HWND;
        if !existing.is_null() {
            ShowWindow(existing, SW_SHOW);
            windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(existing);
            return;
        }
        let hinst = TRAY_HINST.load(Ordering::Relaxed) as *mut core::ffi::c_void;
        let mut class = wide("SDWinStatusWnd");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(status_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: TRAY_HICON.load(Ordering::Relaxed) as *mut core::ffi::c_void,
            hCursor: std::ptr::null_mut(),
            // 標準のシステム色(白系)で塗る=未指定だと背景が残って見苦しい
            hbrBackground: 6 /*COLOR_WINDOW + 1*/ as *mut core::ffi::c_void,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        // タイトルバー+枠で十分なクライアント領域になるよう補正は省略(十分実用)
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("tsunagu").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            60, 60, 460, 380,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            eprintln!("[tray] ステータスウィンドウ生成失敗");
            return;
        }
        let _ = STATUS_HWND.store(hwnd as usize, Ordering::Relaxed);
        let font = segoe_font(false, 17);
        let font_bold = segoe_font(true, 19);
        let make_child = |class_name: &str, text: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: usize| -> usize {
            let child = CreateWindowExW(
                0,
                wide(class_name).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                x, y, w, h,
                hwnd,
                id as *mut core::ffi::c_void,
                hinst,
                std::ptr::null(),
            );
            if !child.is_null() {
                PostMessageW(child, WM_SETFONT, font as usize, 1);
            }
            child as usize
        };
        // 状態行は太字・大きめで最初に目に入るように。以降は通常行
        let state_h = make_child("STATIC", "状態: …", 0, 18, 20, 420, 26, 0);
        PostMessageW(state_h as _, WM_SETFONT, font_bold as usize, 1);
        let _ = LABEL_STATE.store(state_h, Ordering::Relaxed);
        let _ = LABEL_BUILD.store(make_child("STATIC", &build_line(), 0, 18, 50, 420, 20, 0), Ordering::Relaxed);
        // 見出し「接続」(太字)。項目は 1 段字下げして階層を見せる
        let head_conn = make_child("STATIC", "接続", 0, 18, 78, 420, 18, 0);
        PostMessageW(head_conn as _, WM_SETFONT, font_bold as usize, 1);
        let _ = LABEL_RTT.store(make_child("STATIC", &rtt_line(), 0, 30, 100, 410, 20, 0), Ordering::Relaxed);
        let _ = LABEL_AUDIO.store(make_child("STATIC", &audio_line(), 0, 30, 124, 410, 20, 0), Ordering::Relaxed);
        let _ = LABEL_SPK.store(make_child("STATIC", &spk_line(), 0, 30, 148, 410, 20, 0), Ordering::Relaxed);
        let _ = LABEL_FILES.store(make_child("STATIC", &files_line(), 0, 30, 172, 410, 20, 0), Ordering::Relaxed);
        // 見出し「操作」(太字)
        let head_act = make_child("STATIC", "操作", 0, 18, 202, 420, 18, 0);
        PostMessageW(head_act as _, WM_SETFONT, font_bold as usize, 1);
        make_child("BUTTON", "ログを開く", 0, 18, 226, 100, 34, MENU_OPENLOG as usize);
        make_child("BUTTON", "音声 ON/OFF", 0, 124, 226, 108, 34, MENU_AUDIO as usize);
        make_child("BUTTON", "再起動", 0, 238, 226, 88, 34, MENU_RESTART as usize);
        make_child("BUTTON", "終了", 0, 332, 226, 88, 34, MENU_QUIT as usize);
        ShowWindow(hwnd, SW_SHOW);
        windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
        update_labels();
    }
}

/// ラベル類の定期更新(WM_TIMER から)
unsafe fn update_labels() {
    set_text(LABEL_STATE.load(Ordering::Relaxed), &tray_status_text());
    set_text(LABEL_BUILD.load(Ordering::Relaxed), &build_line());
    set_text(LABEL_AUDIO.load(Ordering::Relaxed), &audio_line());
    set_text(LABEL_RTT.load(Ordering::Relaxed), &rtt_line());
    set_text(LABEL_SPK.load(Ordering::Relaxed), &spk_line());
    set_text(LABEL_FILES.load(Ordering::Relaxed), &files_line());
}

unsafe fn open_menu(hwnd: HWND) {
    let menu: HMENU = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let status = tray_status_text();
    let mut w = Vec::new();
    w.extend(status.encode_utf16());
    w.push(0);
    AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, w.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    let mut open_w = wide("ステータスを開く");
    AppendMenuW(menu, MF_STRING, MENU_STATUS as usize, open_w.as_ptr());
    let mut audio_w = wide(&audio_line());
    AppendMenuW(menu, MF_STRING, MENU_AUDIO as usize, audio_w.as_ptr());
    let mut log_w = wide("ログを開く");
    AppendMenuW(menu, MF_STRING, MENU_OPENLOG as usize, log_w.as_ptr());
    let mut rs = wide("再起動");
    AppendMenuW(menu, MF_STRING, MENU_RESTART as usize, rs.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    let mut quit = wide("終了");
    AppendMenuW(menu, MF_STRING, MENU_QUIT as usize, quit.as_ptr());
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, std::ptr::null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);
}

/// exe と同じフォルダの app.ico を読む(無ければ既定アイコン)
unsafe fn load_tray_icon() -> *mut core::ffi::c_void {
    if let Ok(exe) = std::env::current_exe() {
        let ico = exe.parent().map(|d| d.join("app.ico"));
        if let Some(path) = ico.filter(|p| p.exists()) {
            let mut w: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().collect();
            w.push(0);
            let h = LoadImageW(
                std::ptr::null_mut(),
                w.as_ptr(),
                IMAGE_ICON,
                0,
                0,
                LR_LOADFROMFILE | LR_DEFAULTSIZE,
            );
            if !h.is_null() {
                return h;
            }
        }
    }
    LoadIconW(std::ptr::null_mut(), windows_sys::Win32::UI::WindowsAndMessaging::IDI_APPLICATION)
}

unsafe fn tray_loop() {
    let mut class: Vec<u16> = "SDWinTray".encode_utf16().collect();
    class.push(0);
    let hinst = GetModuleHandleW(std::ptr::null());
    let _ = TRAY_HINST.store(hinst as usize, Ordering::Relaxed);
    let icon = load_tray_icon();
    let _ = TRAY_HICON.store(icon as usize, Ordering::Relaxed);
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(tray_wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: icon,
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
    };
    if RegisterClassW(&wc) == 0 {
        eprintln!("[tray] RegisterClassW 失敗(トレイなしで継続)");
        return;
    }
    let mut title: Vec<u16> = "tsunagu".encode_utf16().collect();
    title.push(0);
    // 可視化しないメッセージウィンドウ(トレイのコールバック受け)
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        title.as_ptr(),
        0, // WS_OVERLAPPED(非表示のまま ShowWindow しない)
        0, 0, 0, 0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinst,
        std::ptr::null(),
    );
    if hwnd.is_null() {
        eprintln!("[tray] CreateWindowExW 失敗(トレイなしで継続)");
        return;
    }
    let _ = TRAY_HWND.store(hwnd as usize, Ordering::Relaxed);

    let mut nid = std::mem::zeroed::<NotifyIconData>();
    nid.cb_size = std::mem::size_of::<NotifyIconData>() as u32;
    nid.hwnd = hwnd;
    nid.u_id = 1;
    nid.u_flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.u_callback_message = WM_TRAY;
    nid.h_icon = icon;
    wide_into(&mut nid.sz_tip, &tray_status_text());
    if Shell_NotifyIconW(NIM_ADD, &mut nid) == 0 {
        eprintln!("[tray] Shell_NotifyIconW 失敗(トレイなしで継続)");
        return;
    }
    SetTimer(hwnd, 1, 1000, None);
    eprintln!("[tray] タスクトレイに常駐しました");

    let mut msg: windows_sys::Win32::UI::WindowsAndMessaging::MSG =
        std::mem::zeroed();
    loop {
        let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
        if r <= 0 {
            break;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

/// トレイを開始(別スレッドでメッセージループ)。失敗しても本体は継続する
pub fn start() {
    std::thread::spawn(|| unsafe { tray_loop() });
}
