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
    TranslateMessage, HMENU, WNDCLASSW, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE,
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

// ---------- ダークテーマ(モダンUI)の色定義(0xRRGGBB) ----------
const CLR_BG: u32 = 0xF0F0F0; // 窓背景(Windows 標準ライト)
const CLR_CARD: u32 = 0xFFFFFF; // カード面(白)
const CLR_HEAD: u32 = 0x1A1A1A; // 見出し・状態行(黒)
const CLR_TEXT: u32 = 0x444444; // 本文
const CLR_SUB: u32 = 0x888888; // 補足
const CLR_ACCENT: u32 = 0x0067C0; // 標準アクセント(Windows 11 青)
const CLR_DANGER: u32 = 0xC2504B; // 終了ボタン(赤系)
/// 0xRRGGBB → COLORREF(0x00BBGGRR)
fn rgb(c: u32) -> u32 {
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}
const TRANSPARENT_BK: i32 = 1;

// ---------- 描画に必要な Gdi32/user32(自前 extern) ----------
#[link(name = "gdi32")]
#[link(name = "user32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> *mut core::ffi::c_void;
    fn CreatePen(style: i32, width: i32, color: u32) -> *mut core::ffi::c_void;
    fn SelectObject(hdc: *mut core::ffi::c_void, obj: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn DeleteObject(obj: *mut core::ffi::c_void) -> i32;
    fn SetTextColor(hdc: *mut core::ffi::c_void, color: u32) -> u32;
    fn SetBkColor(hdc: *mut core::ffi::c_void, color: u32) -> u32;
    fn SetBkMode(hdc: *mut core::ffi::c_void, mode: i32) -> i32;
    fn FillRect(hdc: *mut core::ffi::c_void, rect: *const Rect, brush: *mut core::ffi::c_void) -> i32;
    fn RoundRect(
        hdc: *mut core::ffi::c_void, l: i32, t: i32, r: i32, b: i32, ew: i32, eh: i32,
    ) -> i32;
    fn DrawTextW(
        hdc: *mut core::ffi::c_void, text: *mut u16, count: i32, rect: *mut Rect, flags: u32,
    ) -> i32;
    fn SetWindowPos(
        hwnd: HWND, after: HWND, x: i32, y: i32, w: i32, h: i32, flags: u32,
    ) -> i32;
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
const DT_CENTER: u32 = 0x1;
const DT_VCENTER: u32 = 0x4;
const DT_SINGLELINE: u32 = 0x20;

/// WM_DRAWITEM の lparam(Win32 ABI)
#[repr(C)]
struct DrawItemStruct {
    ctl_type: u32,
    ctl_id: u32,
    item_id: u32,
    item_action: u32,
    item_state: u32,
    hwnd_item: *mut core::ffi::c_void,
    hdc: *mut core::ffi::c_void,
    rc_item: Rect,
    item_data: usize,
}

/// DWM: タイトルバーをダークへ(Windows 10 1809+/11 対応。失敗時は無地のまま)
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: HWND, attr: u32, val: *const core::ffi::c_void, size: u32,
    ) -> i32;
}
unsafe fn apply_dark_titlebar(hwnd: HWND) {
    unsafe {
        let dark: i32 = 1;
        const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const core::ffi::c_void,
            4,
        );
    }
}
const MENU_QUIT: u32 = 1001;
const MENU_STATUS: u32 = 1002;
const MENU_AUDIO: u32 = 1003;
const MENU_OPENLOG: u32 = 1004;
const MENU_RESTART: u32 = 1005;
const MENU_SAVEHOST: u32 = 1006;
const MENU_BACKMAC: u32 = 1007;
const MENU_OPENFOLDER: u32 = 1008;
// ラベルのコントロール ID(WM_CTLCOLORSTATIC での色分けに使う)
const ID_LBL_STATE: u32 = 210;
const ID_HEAD_CONN: u32 = 211;
const ID_HEAD_ACT: u32 = 212;
const ID_LBL_BUILD: u32 = 213;
const ID_LBL_RTT: u32 = 214;
const ID_LBL_AUDIO: u32 = 215;
const ID_LBL_SPK: u32 = 216;
const ID_LBL_FILES: u32 = 217;

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
static LABEL_MACCFG: AtomicUsize = AtomicUsize::new(0);
static LABEL_FOOTER: AtomicUsize = AtomicUsize::new(0);
static LABEL_LOGO: AtomicUsize = AtomicUsize::new(0);
/// プロセス起動時刻(稼働時間表示用)
static START_AT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static EDIT_HOST: AtomicUsize = AtomicUsize::new(0);
/// 現在接続先としているホスト(サーバー編集欄の初期値)
pub static HOST_NOW: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

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
/// exe と同じフォルダの .env の TSUNAGU_HOST 行を書き換える(無ければ追記)
fn save_host_to_env(host: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent() else { return };
    let path = dir.join(".env");
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with("TSUNAGU_HOST"))
        .map(|l| l.to_string())
        .collect();
    lines.push(format!("TSUNAGU_HOST={host}"));
    let _ = std::fs::write(&path, lines.join("\r\n") + "\r\n");
}

/// Mac 側の設定(画面位置・⌘キー割当)の表示。Mac から Cfg で同期された値
fn maccfg_line() -> String {
    let side = match crate::SIDE_W.load(Ordering::Relaxed) {
        1 => "左",
        2 => "上",
        3 => "下",
        _ => "右",
    };
    let cmd = if crate::CMD_ALT.load(Ordering::Relaxed) { "Alt" } else { "Ctrl" };
    format!("Mac の設定: Windows は{side}・⌘キーは {cmd}")
}

/// フッター: 接続先サーバーと稼働時間(1 秒タイマーで更新)
fn footer_line() -> String {
    let host = HOST_NOW.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let up = START_AT
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs();
    let (h, m) = (up / 3600, (up % 3600) / 60);
    format!("接続先: {host} ・ 稼働 {h}時間{m:02}分")
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
            let verb = wide("open");
            let np = wide("notepad.exe");
            ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), np.as_ptr(), log.as_ptr(), std::ptr::null(), 5 /*SW_SHOW*/);
        }
        MENU_BACKMAC => {
            // Mac へ制御を返す(Return を送る=左端到達と同じ経路)
            let guard = crate::WTX.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(crate::proto_return());
                println!("[tray] Mac へ戻る");
            }
        }
        MENU_OPENFOLDER => {
            // 受信フォルダ(DOWNLOADS\\Tsunagu)をエクスプローラーで開く
            let dir = std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
                .join("Downloads")
                .join("Tsunagu");
            let _ = std::fs::create_dir_all(&dir);
            let mut path: Vec<u16> = dir.to_string_lossy().encode_utf16().collect();
            path.push(0);
            let verb = wide("open");
            let target = wide("explorer.exe");
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                5,
            );
        }
        MENU_SAVEHOST => {
            // サーバー(Mac)アドレスを .env へ保存して再起動(自動復帰が起こす)
            let hwnd = EDIT_HOST.load(Ordering::Relaxed) as HWND;
            if !hwnd.is_null() {
                extern "system" {
                    fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
                    fn GetWindowTextLengthW(hwnd: HWND) -> i32;
                }
                let len = GetWindowTextLengthW(hwnd);
                let mut buf = vec![0u16; len as usize + 1];
                GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
                let text = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
                let host = text.trim().to_string();
                if !host.is_empty() {
                    save_host_to_env(&host);
                    eprintln!("[tray] サーバーを {host} へ変更し再起動します");
                    std::process::exit(0);
                }
            }
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
    const WM_PAINT2: u32 = 0x000F;
    const WM_ERASEBKGND2: u32 = 0x0014;
    const WM_CTLCOLORSTATIC2: u32 = 0x0138;
    const WM_CTLCOLOREDIT2: u32 = 0x0133;
    const WM_DRAWITEM2: u32 = 0x002B;
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
        WM_ERASEBKGND2 => 1, // 背景は WM_PAINT で全描き(ちらつき防止)
        WM_PAINT2 => {
            unsafe { paint_status(hwnd) };
            0
        }
        WM_CTLCOLOREDIT2 => {
            // サーバー編集欄: 白背景+黒文字(標準ライト)
            unsafe {
                let hdc = wparam as *mut core::ffi::c_void;
                SetTextColor(hdc, 0x1A1A1A);
                SetBkColor(hdc, 0xFFFFFF);
                static EDIT_BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
                let b = *EDIT_BRUSH.get_or_init(|| CreateSolidBrush(0xFFFFFF) as usize);
                b as LRESULT
            }
        }
        WM_CTLCOLORSTATIC2 => {
            // ラベルの文字色をテーマへ(見出し/状態=白、本文=グレー、補助=暗グレー)。
            // 背景は透過(WM_PAINT のカード面がそのまま見える)
            unsafe {
                let hdc = wparam as *mut core::ffi::c_void;
                let child = lparam as HWND;
                extern "system" {
                    fn GetDlgCtrlID(hwnd: HWND) -> i32;
                }
                let id = GetDlgCtrlID(child);
                // RTT は値で色分け(緑=快適/黄=やや遅延/赤=遅延)
                let color = match id as u32 {
                    ID_LBL_STATE | ID_HEAD_CONN | ID_HEAD_ACT => rgb(CLR_HEAD),
                    ID_LBL_BUILD | 1002 => rgb(CLR_SUB),
                    ID_LBL_RTT => {
                        let ms = crate::RTT_MS.load(Ordering::Relaxed);
                        if !crate::CONNECTED.load(Ordering::Relaxed) || ms == 0 {
                            rgb(CLR_TEXT)
                        } else if ms <= 10 {
                            0x53D769 // 緑
                        } else if ms <= 40 {
                            0xF5D547 // 黄
                        } else {
                            0xF26B5B // 赤
                        }
                    }
                    _ => rgb(CLR_TEXT),
                };
                SetTextColor(hdc, color);
                SetBkMode(hdc, TRANSPARENT_BK);
                // 背景ブラシを窓背景色で返す: 透過(NULL_BRUSH)だと文字更新時に
                // 古い文字が残って重なって見える(ゴースト)ため不透明で塗る
                static BG_BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
                let b = *BG_BRUSH.get_or_init(|| CreateSolidBrush(rgb(CLR_BG)) as usize);
                b as LRESULT
            }
        }
        WM_DRAWITEM2 => {
            // オーナードローボタン: 角丸フラット+中央白文字(終了のみ赤系)
            unsafe {
                let dis = lparam as *const DrawItemStruct;
                if dis.is_null() {
                    return 0;
                }
                let d = &*dis;
                let brush_color = if d.ctl_id == MENU_QUIT { CLR_DANGER } else { CLR_ACCENT };
                let brush = CreateSolidBrush(rgb(brush_color));
                let pen = CreatePen(0 /*PS_SOLID*/, 1, rgb(brush_color));
                let old_b = SelectObject(d.hdc, brush);
                let old_p = SelectObject(d.hdc, pen);
                RoundRect(
                    d.hdc,
                    d.rc_item.left,
                    d.rc_item.top,
                    d.rc_item.right,
                    d.rc_item.bottom,
                    8,
                    8,
                );
                SelectObject(d.hdc, old_b);
                SelectObject(d.hdc, old_p);
                DeleteObject(brush);
                DeleteObject(pen);
                SetTextColor(d.hdc, 0xFFFFFF);
                SetBkMode(d.hdc, TRANSPARENT_BK);
                // ボタン文字はウィンドウテキストから取る
                let mut buf = [0u16; 64];
                extern "system" {
                    fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
                }
                let len = GetWindowTextW(d.hwnd_item as HWND, buf.as_mut_ptr(), 64);
                if len > 0 {
                    let mut r = d.rc_item;
                    r.left += 4;
                    r.right -= 4;
                    DrawTextW(d.hdc, buf.as_mut_ptr(), len, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                }
            }
            1 // 描画済み
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 窓の全面描画: ダーク背景 + 接続情報カード(角丸)
unsafe fn paint_status(hwnd: HWND) {
    unsafe {
        #[repr(C)]
        struct PaintStruct {
            hdc: *mut core::ffi::c_void,
            erase: i32,
            rc_paint: Rect,
            restore: i32,
            inc_update: i32,
            reserved: [u8; 32],
        }
        extern "system" {
            fn BeginPaint(hwnd: HWND, ps: *mut PaintStruct) -> *mut core::ffi::c_void;
            fn EndPaint(hwnd: HWND, ps: *const PaintStruct) -> i32;
            fn GetClientRect(hwnd: HWND, rect: *mut Rect) -> i32;
        }
        let mut ps = std::mem::zeroed::<PaintStruct>();
        let hdc = BeginPaint(hwnd, &mut ps);
        if hdc.is_null() {
            return;
        }
        let mut rc = std::mem::zeroed::<Rect>();
        GetClientRect(hwnd, &mut rc);
        // 背景
        let bg = CreateSolidBrush(rgb(CLR_BG));
        FillRect(hdc, &rc, bg);
        DeleteObject(bg);
        // 接続カード(RTT/スピーカー/ファイル を囲む角丸面)
        let card = Rect { left: 12, top: 72, right: rc.right - 12, bottom: 216 };
        let card_brush = CreateSolidBrush(rgb(CLR_CARD));
        let card_pen = CreatePen(0, 1, rgb(0xE0E0E0));
        let ob = SelectObject(hdc, card_brush);
        let op = SelectObject(hdc, card_pen);
        RoundRect(hdc, card.left, card.top, card.right, card.bottom, 10, 10);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(card_brush);
        DeleteObject(card_pen);
        EndPaint(hwnd, &ps);
    }
}

/// モダンな見た目のための Segoe UI フォント生成(通常/太字)。
/// 既定の DEFAULT_GUI_FONT は古いシステムフォントになるため使わない
/// 近未来ロゴ用の等幅フォント(Consolas)
unsafe fn segoe_mono(height: i32) -> *mut core::ffi::c_void {
    unsafe {
        let mut name: Vec<u16> = "Consolas".encode_utf16().collect();
        name.push(0);
        extern "system" {
            fn CreateFontW(
                height: i32, width: i32, escapement: i32, orientation: i32, weight: i32,
                italic: u32, underline: u32, strikeout: u32, charset: u32, outprecision: u32,
                clipprecision: u32, quality: u32, pitchandfamily: u32, face: *const u16,
            ) -> *mut core::ffi::c_void;
        }
        CreateFontW(height, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, name.as_ptr())
    }
}

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
        let class = wide("SDWinStatusWnd");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(status_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: TRAY_HICON.load(Ordering::Relaxed) as *mut core::ffi::c_void,
            hCursor: std::ptr::null_mut(),
            // 背景は WM_PAINT で自前描画するため未指定(WM_ERASEBKGND も抑制)
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        // 表示位置はカーソル(トレイ)の近く=画面内に収まる左上側(Windows 標準の
        // トレイウィンドウ挙動)。座標系はそのままで十分実用
        let mut wx: i32 = 60;
        let mut wy: i32 = 60;
        {
            let mut pt = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut pt) != 0 {
                wx = (pt.x - 480).max(0);
                wy = (pt.y - 470).max(0);
            }
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("Tsunagu ステータス").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            wx, wy, 460, 446,
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
        // ヘッダー左にアプリアイコン(SS_ICON スタティック)
        let icon32 = load_tray_icon_size(32);
        if !icon32.is_null() {
            let ic = make_child("STATIC", "", 0x3 /*SS_ICON*/, 18, 14, 34, 34, 0);
            PostMessageW(ic as _, 0x0172 /*STM_SETICON*/, icon32 as usize, 0);
        }
        // 状態行は太字・大きめで最初に目に入るように。以降は通常行
        let state_h = make_child("STATIC", "状態: …", 0, 60, 16, 378, 26, ID_LBL_STATE as usize);
        PostMessageW(state_h as _, WM_SETFONT, font_bold as usize, 1);
        let _ = LABEL_STATE.store(state_h, Ordering::Relaxed);
        let _ = LABEL_BUILD.store(make_child("STATIC", &build_line(), 0, 18, 46, 420, 20, ID_LBL_BUILD as usize), Ordering::Relaxed);
        // 見出し「接続」(太字)とカード内の項目(paint_status のカード矩形に合わせる)
        let head_conn = make_child("STATIC", "接続", 0, 26, 80, 400, 18, ID_HEAD_CONN as usize);
        PostMessageW(head_conn as _, WM_SETFONT, font_bold as usize, 1);
        let _ = LABEL_RTT.store(make_child("STATIC", &rtt_line(), 0, 38, 102, 390, 20, ID_LBL_RTT as usize), Ordering::Relaxed);
        let _ = LABEL_AUDIO.store(make_child("STATIC", &audio_line(), 0, 38, 124, 390, 20, ID_LBL_AUDIO as usize), Ordering::Relaxed);
        let _ = LABEL_SPK.store(make_child("STATIC", &spk_line(), 0, 38, 146, 390, 20, ID_LBL_SPK as usize), Ordering::Relaxed);
        let _ = LABEL_FILES.store(make_child("STATIC", &files_line(), 0, 38, 168, 390, 20, ID_LBL_FILES as usize), Ordering::Relaxed);
        let _ = LABEL_MACCFG.store(make_child("STATIC", &maccfg_line(), 0, 38, 190, 390, 20, 1001 as usize), Ordering::Relaxed);
        // 見出し「サーバー」(太字)+ 接続先(Mac)編集 + 保存して再接続
        let head_srv = make_child("STATIC", "サーバー(Mac)", 0, 18, 228, 420, 18, 1000 as usize);
        PostMessageW(head_srv as _, WM_SETFONT, font_bold as usize, 1);
        let host_init = HOST_NOW.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let edit = make_child(
            "EDIT",
            &host_init,
            0x0080 /*ES_AUTOHSCROLL*/ | 1 /*WS_BORDER*/,
            18,
            252,
            250,
            26,
            0,
        );
        PostMessageW(edit as _, WM_SETFONT, font as usize, 1);
        let _ = EDIT_HOST.store(edit, Ordering::Relaxed);
        const BS_OWNERDRAW2: u32 = 0x000B;
        make_child("BUTTON", "保存して再接続", 0, 276, 250, 144, 30, MENU_SAVEHOST as usize);
        // 見出し「操作」(太字)
        let head_act = make_child("STATIC", "操作", 0, 18, 294, 420, 18, ID_HEAD_ACT as usize);
        PostMessageW(head_act as _, WM_SETFONT, font_bold as usize, 1);
        // ボタンはオーナードロー(角丸フラット・WM_DRAWITEM で描画)
        const BS_OWNERDRAW: u32 = 0x000B;
        make_child("BUTTON", "Mac へ戻る", 0, 18, 316, 128, 36, MENU_BACKMAC as usize);
        make_child("BUTTON", "受信フォルダ", 0, 152, 316, 124, 36, MENU_OPENFOLDER as usize);
        make_child("BUTTON", "音声 ON/OFF", 0, 284, 316, 80, 36, MENU_AUDIO as usize);
        make_child("BUTTON", "終了", 0, 370, 316, 50, 36, MENU_QUIT as usize);
        make_child("BUTTON", "ログを開く", 0, 18, 358, 128, 36, MENU_OPENLOG as usize);
        make_child("BUTTON", "再起動", 0, 152, 358, 124, 36, MENU_RESTART as usize);
        // フッター(接続先と稼働時間)
        let _ = LABEL_FOOTER.store(make_child("STATIC", &footer_line(), 0, 18, 406, 420, 20, 1002 as usize), Ordering::Relaxed);
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
    set_text(LABEL_MACCFG.load(Ordering::Relaxed), &maccfg_line());
    set_text(LABEL_FOOTER.load(Ordering::Relaxed), &footer_line());
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
    let mut bm = wide("Mac へ戻る");
    AppendMenuW(menu, MF_STRING, MENU_BACKMAC as usize, bm.as_ptr());
    let mut fo = wide("受信フォルダを開く");
    AppendMenuW(menu, MF_STRING, MENU_OPENFOLDER as usize, fo.as_ptr());
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
/// 指定サイズのアプリアイコン(exe 横の app.ico。無ければ既定)
unsafe fn load_tray_icon_size(size: i32) -> *mut core::ffi::c_void {
    unsafe {
        if let Ok(exe) = std::env::current_exe() {
            let ico = exe.parent().map(|d| d.join("app.ico"));
            if let Some(path) = ico.filter(|p| p.exists()) {
                let mut w: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
                w.push(0);
                let h = LoadImageW(
                    std::ptr::null_mut(),
                    w.as_ptr(),
                    IMAGE_ICON,
                    size,
                    size,
                    LR_LOADFROMFILE,
                );
                if !h.is_null() {
                    return h;
                }
            }
        }
        LoadIconW(std::ptr::null_mut(), windows_sys::Win32::UI::WindowsAndMessaging::IDI_APPLICATION)
    }
}

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
    // デバッグ/スクリーンショット検証用: TSUNAGU_STATUS_SHOW=1 で起動時に窓を開く
    if tsunagu_common::envutil::get("TSUNAGU_STATUS_SHOW").as_deref() == Some("1") {
        open_status_window();
    }

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
