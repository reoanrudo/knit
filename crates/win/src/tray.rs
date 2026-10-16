// タスクトレイ常駐(Shell_NotifyIcon)。コンソールなし運用の状態可視化と終了操作。
// 「Windows 側のターミナルを消したら繋がらない」問題の恒久对策:
// このプロセス自体が GUI サブシステム+トレイ常駐で動き、ターミナル前提を消す。
// NOTIFYICONDATAW は ABI が安定しているため自前定義(Shell feature への依存を避ける)
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
mod preferences;
mod settings_ui;
pub(crate) use preferences::{
    host_mode_pref, unlock_enabled, unlock_secret_plain, unlock_secret_stored, HOST_MODE,
    UNLOCK_SYNC,
};
pub mod setup;
static UI_PREVIEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn preview() {
    UI_PREVIEW.store(true, Ordering::Relaxed);
    unsafe {
        tray_loop();
    }
}

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};

use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, LoadIconW, LoadImageW, PostMessageW, RegisterClassW,
    SetForegroundWindow, SetTimer, SetWindowTextW, ShowWindow, TrackPopupMenu, TranslateMessage,
    HMENU, IMAGE_ICON, LR_LOADFROMFILE, MF_GRAYED, MF_SEPARATOR, MF_STRING, SW_HIDE, SW_SHOW,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CHILD, WS_VISIBLE,
};

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, utype: u32) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: HWND,
        verb: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> isize;
}

const WM_TRAY: u32 = WM_APP + 1;

// ---------- 設定画面の配色。Windows の「アプリをダークにする」個人設定に追従する ----------
pub(super) struct Theme {
    bg: u32,        // 窓背景
    card: u32,      // カード面
    head: u32,      // 見出し・状態行
    text: u32,      // 本文
    sub: u32,       // 補足・無効
    accent: u32,    // 標準アクセント(Windows 11 青)
    edge: u32,      // カードの縁
    nav_sel: u32,   // 選択中ナビ項目の面
    btn_idle: u32,  // 通常ボタンの面
    btn_pressed: u32, // 押下中ボタンの面
    btn_primary_pressed: u32, // 押下中の主要ボタン
    danger: u32,    // 破壊的操作(初期化)のボタン面
    danger_pressed: u32, // 押下中の破壊的操作
    edit_text: u32, // 入力欄の文字
    edit_bg: u32,   // 入力欄の面
    diagram_fill: u32, // 配置図の四角
    diagram_line: u32, // 配置図の線
    diagram_mac: u32,  // 配置図の Mac 側
}
pub(super) fn theme() -> &'static Theme {
    if DARK_MODE.load(Ordering::Relaxed) {
        &DARK
    } else {
        &LIGHT
    }
}
static DARK_MODE: AtomicBool = AtomicBool::new(false);
static LIGHT: Theme = Theme {
    bg: 0xF3F4F8, card: 0xFFFFFF, head: 0x222638, text: 0x424A5E, sub: 0x626B7D,
    accent: 0x515FD1, edge: 0xE3E6ED, nav_sel: 0xE2E6FA,
    btn_idle: 0xEEF0F7, btn_pressed: 0xDFE3EF, btn_primary_pressed: 0x3C49AE,
    danger: 0xB3261E, danger_pressed: 0x8C1D18,
    edit_text: 0x222638, edit_bg: 0xFFFFFF, diagram_fill: 0xF2F4FB,
    diagram_line: 0xCDD2E4, diagram_mac: 0x7C869C,
};
static DARK: Theme = Theme {
    bg: 0x202124, card: 0x2B2C30, head: 0xE8EAED, text: 0xC7CBD4, sub: 0x9AA0A6,
    accent: 0x8F9BF5, edge: 0x3A3C42, nav_sel: 0x343A5E,
    btn_idle: 0x3A3D45, btn_pressed: 0x4A4E59, btn_primary_pressed: 0x6C77D8,
    danger: 0xC4534C, danger_pressed: 0x9E403A,
    edit_text: 0xE8EAED, edit_bg: 0x303236, diagram_fill: 0x2B2C30,
    diagram_line: 0x4A4E59, diagram_mac: 0xA8AEBE,
};
/// 画面ロック連動(KNIT_LOCK_SYNC)。既定で有効(mac 側と同じ既定値)
fn lock_sync_enabled() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        knit_common::envutil::get("KNIT_LOCK_SYNC")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}

/// このセッションがいまロック中か(WTS_SESSION_LOCK/UNLOCK で管理。初期値 false)。
/// Mac からの Msg::Unlock 受信時に「ロックされた直後の解除要求か」の判定に使う
pub(crate) static LOCKED_STATE: AtomicBool = AtomicBool::new(false);

/// 「設定 > 個人用設定 > 色」のアプリモード(0=ダーク)をレジストリから読む。読めなければライト。
fn system_dark() -> bool {
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: *mut core::ffi::c_void,
            sub: *const u16,
            name: *const u16,
            flags: u32,
            value_type: *mut u32,
            data: *mut u8,
            data_len: *mut u32,
        ) -> i32;
    }
    const HKEY_CURRENT_USER: *mut core::ffi::c_void = 0x8000_0001usize as _;
    const RRF_RT_REG_DWORD: u32 = 0x10;
    let mut v: u32 = 1;
    let mut cb: u32 = 4;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
            wide("AppsUseLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut v as *mut u32 as *mut u8,
            &mut cb,
        )
    };
    ok == 0 && v == 0
}
/// テーマを OS 設定へ同期し、変わったら true(呼び出し元は再描画する)。
pub(super) fn sync_theme() -> bool {
    let d = system_dark();
    DARK_MODE.swap(d, Ordering::Relaxed) != d
}
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
    fn SelectObject(
        hdc: *mut core::ffi::c_void,
        obj: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn DeleteObject(obj: *mut core::ffi::c_void) -> i32;
    fn SetTextColor(hdc: *mut core::ffi::c_void, color: u32) -> u32;
    fn SetBkColor(hdc: *mut core::ffi::c_void, color: u32) -> u32;
    fn SetBkMode(hdc: *mut core::ffi::c_void, mode: i32) -> i32;
    fn FillRect(
        hdc: *mut core::ffi::c_void,
        rect: *const Rect,
        brush: *mut core::ffi::c_void,
    ) -> i32;
    fn RoundRect(
        hdc: *mut core::ffi::c_void,
        l: i32,
        t: i32,
        r: i32,
        b: i32,
        ew: i32,
        eh: i32,
    ) -> i32;
    fn DrawTextW(
        hdc: *mut core::ffi::c_void,
        text: *mut u16,
        count: i32,
        rect: *mut Rect,
        flags: u32,
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

// DWM のダークタイトルバー(DwmSetWindowAttribute)は現状未使用(無地のまま)
unsafe extern "system" {
    #[allow(dead_code)]
    fn DwmSetWindowAttribute(
        hwnd: HWND,
        attr: u32,
        val: *const core::ffi::c_void,
        size: u32,
    ) -> i32;
}

const MENU_QUIT: u32 = 1001;
const MENU_STATUS: u32 = 1002;
const MENU_AUDIO: u32 = 1003;
const MENU_OPENLOG: u32 = 1004;
const MENU_RESTART: u32 = 1005;
const MENU_SAVEHOST: u32 = 1006;
const MENU_BACKMAC: u32 = 1007;
const MENU_OPENFOLDER: u32 = 1008;
const MENU_DIAGNOSE: u32 = 1017;
const MENU_OPENSETDIR: u32 = 1018;
/// 「その他」ページの「すべての設定を初期化…」(Mac 側の同名ボタンと対)
const MENU_RESETSETTINGS: u32 = 1019;
/// 「その他」ページの「すべての登録を初期化…」(Mac 側の同名ボタンと対。
/// 接続キーを削除して初回登録へ戻す)
const MENU_RESETREG: u32 = 1021;
/// 「その他」ページの「ロック解除の連動」トグル(チェックボックス)
const MENU_UNLOCK_SYNC: u32 = 1022;
/// 「その他」ページの「ロック解除の連動」のパスワード保存ボタン
const MENU_UNLOCK_SAVE: u32 = 1023;
const MENU_SHARE_CLIP: u32 = 1014;
const MENU_SHARE_FILES: u32 = 1015;
const MENU_UPDATE: u32 = 1013;
// 設定ページのコンボ・スライダ(Mac の設定を遠隔で変える部品のID)
const ID_HOTKEY_COMBO: u32 = 3104;
const ID_DELAY_TRACK: u32 = 3204;
// 共有ページの「相手側の設定」トグル(Mac の共有ページの項目と対)
const MENU_MAC_CLIP: u32 = 3110;
const MENU_MAC_FILES: u32 = 3111;
const MENU_MAC_HISTORY: u32 = 3112;
const MENU_MAC_AUDIO: u32 = 3113;
const MENU_MAC_SPK: u32 = 3114;
const MENU_INPUT_HELPER: u32 = 1090;
const MENU_HOSTMODE: u32 = 1016;
const MENU_ROLE_CLIENT: u32 = 3120;
const MENU_ROLE_HOST: u32 = 3121;
/// クリップボード履歴の項目(MENU_HISTORY_FIRST + 表示順 index)。
/// index→履歴 id の対応は開くたびに MENU_HISTORY_IDS へ保存する
const MENU_HISTORY_FIRST: u32 = 1100;
const MENU_HISTORY_CLEAR: u32 = 1110;
static MENU_HISTORY_IDS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

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
static EDIT_HOST: AtomicUsize = AtomicUsize::new(0);
/// 現在接続先としているホスト(サーバー編集欄の初期値)
pub static HOST_NOW: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// UTF-16 への変換(固定幅バッファ用)。満杯時に終端 NUL が欠けないよう
/// 1 手前まで書き、末尾は必ず 0 のまま残す
fn wide_into(buf: &mut [u16], s: &str) {
    let last = buf.len().saturating_sub(1);
    for (dst, src) in buf[..last].iter_mut().zip(s.encode_utf16()) {
        *dst = src;
    }
    if let Some(tail) = buf.last_mut() {
        *tail = 0;
    }
}

fn audio_line() -> String {
    if crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed) {
        // 音声の受け手は現状 Mac 的な相手(キーボード入力と同じ制約)だが、文言は
        // 相手の種類を固定しない(将来の一般化でそのまま使えるように)
        "音声転送: ON(Windowsの音を相手側で再生)".to_string()
    } else {
        "音声転送: OFF".to_string()
    }
}
/// exe と同じフォルダの .env の KNIT_HOST 行を書き換える(無ければ追記)。
/// host が空のときは行を書かない=LAN 自動発見へ戻す(Mac 側 imp_save_host の
/// 「空欄で KNIT_HOST 行を消す」と同じ挙動)。
/// 共通の envutil::set_env_value へ統一した: Mac 側と同じ一時ファイル+リネームの
/// 安全な書き込み(直書きは書込み途中の失敗で .env が壊れるため)と、旧名称
/// (TSUNAGU_HOST/SEAMLESS_HOST)行の新しい 1 行への統合を併せ持つ。改行は
/// CRLF から LF へ変わるが、読み込み側(lines())はどちらも同じ扱い
fn save_host_to_env(host: &str) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("exe directory is unavailable"))?;
    knit_common::envutil::set_env_value(&dir.join(".env"), "KNIT_HOST", host)
}

fn set_text(h: usize, s: &str) {
    if h == 0 {
        return;
    }
    let mut w: Vec<u16> = s.encode_utf16().collect();
    w.push(0);
    unsafe { SetWindowTextW(h as _, w.as_ptr()) };
}

/// 一度でも接続キーの保存(登録)を確認できたら立てる(未接続時の案内分岐用)。
/// 未登録の間は呼び出しのたびに見に行くが、未登録ならファイル自体が無く
/// すぐ返るため負荷にならない
static REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn registered() -> bool {
    if REGISTERED.load(Ordering::Relaxed) {
        return true;
    }
    let ok = matches!(knit_common::credentials::load(), Ok(Some(_)));
    if ok {
        REGISTERED.store(true, Ordering::Relaxed);
    }
    ok
}

fn tray_status_text() -> String {
    let conn = if crate::CONNECTED.load(Ordering::Relaxed) {
        "接続済み"
    } else if registered() {
        // 登録済みなら再接続は自動。初回ユーザーに「自動でつながる」という
        // 不正確な期待を与えない(Mac 側の PAIRED 分岐と同じ方針)
        "未接続・自動再接続中"
    } else {
        "未接続"
    };
    // 遅延と経路(接続中のみ。履歴件数は接続の有無に関係なく役立つ)
    let rtt = crate::RTT_MS.load(Ordering::Relaxed);
    let rtt_s = if crate::CONNECTED.load(Ordering::Relaxed) && rtt > 0 {
        format!("・遅延{}ms", rtt)
    } else {
        String::new()
    };
    let route = crate::PEER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map(|ip| {
            if knit_common::net::is_tailscale(ip) {
                "・Tailscale"
            } else {
                "・LAN 直"
            }
        })
        .unwrap_or_default();
    let history = crate::HISTORY
        .lock()
        .map(|h| h.entries().len())
        .unwrap_or(0);
    let history_s = if history > 0 {
        format!("・履歴{history}件")
    } else {
        String::new()
    };
    // 転送中は進捗を常に見える場所へ(ツールチップとステータス窓の状態行で共用)
    let xfer_s = match crate::xfer_line() {
        Some(x) => format!("・{x}"),
        None => String::new(),
    };
    format!("Knit・{conn}{rtt_s}{route}{history_s}{xfer_s}")
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

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    const WM_WTSSESSION_CHANGE2: u32 = 0x02B1;
    // 電源イベント: 復帰後の再接続をソケットの読み出しタイムアウト(最長9秒)待たずに
    // すぐ始める。スリープ入りでも古い接続を綺麗に切っておく
    const WM_POWERBROADCAST: u32 = 0x218;
    if msg == WM_POWERBROADCAST {
        const PBT_APMSUSPEND: usize = 0x4;
        const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
        const PBT_APMRESUMESUSPEND: usize = 0x7;
        return match wparam {
            PBT_APMSUSPEND => {
                crate::on_power_event(false);
                1
            }
            PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                crate::on_power_event(true);
                1
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        };
    }
    match msg {
        WM_TRAY => {
            let mouse = (lparam & 0xFFFF) as u32;
            if mouse == WM_LBUTTONUP || mouse == 0x0203
            /*WM_LBUTTONDBLCLK*/
            {
                open_status_window(); // 左クリック/ダブルクリック=アプリ画面(Windows 標準操作)
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
        WM_WTSSESSION_CHANGE2 => {
            // WTS_SESSION_LOCK(0x7)=このセッションがロックされた。Mac へ既存の
            // Msg::Lock を送り、Mac 側で ⌘Ctrl+Q を発生させる(双方向のロック連動)。
            // ロック済みで再送が返ってきても受信側の LockWorkStation は無害。
            // WTS_SESSION_UNLOCK(0x8)=ロックが解除された。Msg::Unlock の受信側
            //(session.rs)が「ロック中か」の判定に使うため LOCKED_STATE を管理する
            const WTS_SESSION_LOCK: WPARAM = 0x7;
            const WTS_SESSION_UNLOCK: WPARAM = 0x8;
            match wparam {
                WTS_SESSION_LOCK => {
                    LOCKED_STATE.store(true, Ordering::Relaxed);
                    if crate::CONNECTED.load(Ordering::Relaxed) && lock_sync_enabled() {
                        if !crate::state::send_main_msg(&knit_common::proto::Msg::Lock) {
                            eprintln!("[lock] Mac へのロック指示を送れませんでした(未接続)");
                        }
                    }
                }
                WTS_SESSION_UNLOCK => {
                    LOCKED_STATE.store(false, Ordering::Relaxed);
                }
                _ => {}
            }
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
/// 相手(Mac)の役割切替の適用完了(RoleAck)。再起動を待っていたメニュー処理が参照する
static ROLE_ACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 本線受信ループから呼ぶ: 相手の適用が済んだ合図を立てる
pub(crate) fn note_role_ack() {
    ROLE_ACK.store(true, Ordering::Relaxed);
}
/// 相手の適用完了を待つ(旧版相手は返さないためタイムアウトで諦める)
fn wait_role_ack(timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if ROLE_ACK.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// 実効的な役割(このPCがホストか)。環境変数 KNIT_ROLE=server が GUI 設定に優先する
///(main の起動判定と同じ条件。表示と実役割が食い違わないように一元化する)
pub(crate) fn effective_host_mode() -> bool {
    role_env_fixed() || preferences::host_mode_pref()
}

/// 環境変数 KNIT_ROLE=server で役割が固定されているか(GUI からの切替が効かない)
pub(crate) fn role_env_fixed() -> bool {
    knit_common::envutil::get("KNIT_ROLE").as_deref() == Some("server")
}

/// 相手から接続の方向の切替を知らされたときの対応。相手がホストになるなら自分は接続側へ、
/// 相手が接続側へ戻るなら自分がホストへ。すでに合っていれば何もしない
pub(crate) fn apply_peer_role(peer_is_host: bool) {
    let want_host = !peer_is_host;
    // 実効役割(GUI 設定+環境変数)が既に相手の指示と一致していれば何も要らない
    if effective_host_mode() == want_host {
        return;
    }
    // 環境変数で役割を固定しているときは追従できない(再起動しても env が優先する)。
    // 黙って再起動すると役割が変わらないまま切れるだけのため、案内で留める
    if role_env_fixed() {
        notify(
            "Knit",
            "環境変数 KNIT_ROLE=server で役割を固定しているため、相手の切り替えには追従しません",
        );
        println!("[role] KNIT_ROLE 固定中のため相手の切替指示を無視しました");
        return;
    }
    preferences::set_host_mode(want_host);
    if let Err(e) = preferences::save() {
        eprintln!("[prefs] save failed: {e}");
    }
    println!("[role] 相手の切替に合わせて {} へ変更します", if want_host { "ホスト" } else { "接続側" });
    notify(
        "Knit",
        if want_host {
            "相手がホストをやめたため、このPCをホスト(待受側)に切り替えて再起動します"
        } else {
            "相手がホストになったため、このPCを接続側に切り替えて再起動します"
        },
    );
    crate::audio::speaker_disconnect();
    crate::release_all_input();
    std::thread::sleep(std::time::Duration::from_millis(600));
    restart_self();
}

/// 自分を終了して、すぐ起こし直す。終了だけだと毎分の自動復帰タスクが起こすまで
/// 最長 1 分アプリが消えたままになる。古いプロセスが抜ける(単一起動の排他が空く)のを
/// 少し待ってから、導入済みの起動タスク(knit_run)を走らせる
fn restart_self() -> ! {
    use std::os::windows::process::CommandExt;
    // 自発的切替と相手からの切替要求の両方から呼ばれるため、二重進入で
    // 起動タスクを二重発火させないよう1回だけ通す
    static RESTARTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RESTARTING.swap(true, Ordering::Relaxed) {
        std::process::exit(0);
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let _ = std::process::Command::new("cmd")
        .args(["/c", "ping -n 3 127.0.0.1 >nul & schtasks /Run /TN knit_run"])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn();
    std::process::exit(0);
}

unsafe fn handle_command(id: u32) {
    match id {
        id if (MENU_HISTORY_FIRST..MENU_HISTORY_CLEAR).contains(&id) => {
            // 履歴からの復元。index→id はメニューを開いた時点の対応を使う
            let idx = (id - MENU_HISTORY_FIRST) as usize;
            let target = MENU_HISTORY_IDS
                .lock()
                .ok()
                .and_then(|g| g.get(idx).copied());
            if let Some(entry_id) = target {
                crate::history_restore_by_id(entry_id);
                update_tip();
            }
        }
        MENU_HISTORY_CLEAR => {
            // 戻せない操作のため確認を挟む(Mac 側「履歴をすべて消す…」と対称。
            // 既定(Enter/フォーカス)は「いいえ」=キャンセル側に置く)
            let text = wide(
                "クリップボードの履歴すべて(最大50件)が消え、元に戻せません。\n履歴をすべて消しますか?",
            );
            let caption = wide("履歴をすべて消す");
            let choice = MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                0x0001_0134 /*MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_SETFOREGROUND*/,
            );
            if choice != 6 {
                /*IDYES 以外 = キャンセル*/
                return;
            }
            crate::history_clear();
            update_tip();
            update_labels();
        }
        3000..=3004 => settings_ui::select((id - settings_ui::NAV_FIRST) as usize),
        MENU_STATUS => open_status_window(),
        MENU_AUDIO if !knit_common::share::env_cap().audio => {
            notify("このPCでは音声を共有できません", "KNIT_SHARE の設定で制限されています。");
        }
        MENU_AUDIO => {
            // 設定ページのチェックボックスからならそのチェック状態を、トレイメニュー
            // からならトグルを設定する(チェックボックスはクリック時に Win32 が先に
            // 状態を切り替えてから通知してくるため、読み取った値が新しい状態)
            let next = settings_ui::checked_state(MENU_AUDIO)
                .unwrap_or_else(|| !crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed));
            crate::audio::AUDIO_ENABLED.store(next, Ordering::Relaxed);
            if !UI_PREVIEW.load(Ordering::Relaxed) {
                if let Err(e) = preferences::save() {
                    eprintln!("[prefs] save failed: {e}");
                    notify(
                        "設定を保存できません",
                        "変更は今回の起動中のみ有効です。ログを確認してください。",
                    );
                }
            }
            println!("[tray] 音声転送 -> {next}");
            update_labels();
            update_tip();
        }
        MENU_UNLOCK_SYNC => {
            // ロック解除の連動トグル(MENU_AUDIO と同じ自動保存)。チェックボックスは
            // クリック時に Win32 が先に状態を切り替えてから通知してくるため、
            // 読み取った値=利用者が選んだ新しい状態
            let next = settings_ui::checked_state(MENU_UNLOCK_SYNC)
                .unwrap_or_else(|| !crate::tray::UNLOCK_SYNC.load(Ordering::Relaxed));
            crate::tray::UNLOCK_SYNC.store(next, Ordering::Relaxed);
            if !UI_PREVIEW.load(Ordering::Relaxed) {
                if let Err(e) = preferences::save() {
                    eprintln!("[prefs] save failed: {e}");
                    notify(
                        "設定を保存できません",
                        "変更は今回の起動中のみ有効です。ログを確認してください。",
                    );
                }
            }
            println!("[tray] ロック解除の連動 -> {next}");
            update_labels();
        }
        MENU_UNLOCK_SAVE => {
            // パスワード欄の内容を DPAPI で保護して保存。空欄はパスワードのクリア。
            // DPAPI 失敗時は設定を拒否してエラー表示(平文を保存しない)
            if UI_PREVIEW.load(Ordering::Relaxed) {
                return;
            }
            let h = settings_ui::EDIT_UNLOCK_SECRET.load(Ordering::Relaxed);
            let text = settings_ui::edit_text(h);
            match preferences::protect_and_set_unlock_secret(&text) {
                Ok(()) => {
                    if let Err(e) = preferences::save() {
                        eprintln!("[prefs] save failed: {e}");
                        notify(
                            "設定を保存できません",
                            "変更は今回の起動中のみ有効です。ログを確認してください。",
                        );
                    } else if text.is_empty() {
                        notify("Knit", "ロック解除のパスワードを消去しました(次回から Enter のみ送ります)");
                    } else {
                        notify("Knit", "ロック解除のパスワードを保存しました(このPC内に暗号化保存)");
                    }
                    // 保存済みのパスワードを画面に残さない(状態行は sync が更新する)
                    set_text(h, "");
                }
                Err(e) => {
                    eprintln!("[prefs] unlock secret save failed: {e}");
                    notify(
                        "ロック解除のパスワードを保存できません",
                        &format!("設定を保存しませんでした。{e}"),
                    );
                }
            }
            update_labels();
        }
        MENU_DIAGNOSE => {
            // 接続診断(Mac の「接続を診断…」と同じ)。実測に最大3秒かかるため
            // 別スレッドで回し、結果をテキストボックスで表示する。
            // 連打で診断と MessageBox が重ならないよう1つだけ動かす
            static DIAG_RUNNING: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if DIAG_RUNNING.swap(true, Ordering::Relaxed) {
                return;
            }
            std::thread::spawn(|| {
                let report = crate::diag::run();
                DIAG_RUNNING.store(false, Ordering::Relaxed);
                let w: Vec<u16> = report.encode_utf16().chain(std::iter::once(0)).collect();
                let caption = wide("Knit接続診断");
                unsafe {
                    MessageBoxW(
                        std::ptr::null_mut(),
                        w.as_ptr(),
                        caption.as_ptr(),
                        0x0001_0040 /*MB_ICONINFORMATION | MB_SETFOREGROUND*/,
                    );
                }
            });
        }
        MENU_OPENSETDIR => {
            // 設定の置き場を開く: .env(exe と同じフォルダ)と端末固有データ
            // (%LOCALAPPDATA%\Knit)の両方。無いフォルダは作ってから開く
            //(MENU_OPENFOLDER と同じ ShellExecuteW の導線)
            let mut dirs: Vec<std::path::PathBuf> = Vec::new();
            if let Ok(exe) = std::env::current_exe() {
                if let Some(d) = exe.parent() {
                    dirs.push(d.to_path_buf());
                }
            }
            if let Some(d) = knit_common::envutil::data_dir() {
                let _ = std::fs::create_dir_all(&d);
                dirs.push(d);
            }
            for dir in dirs {
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
                    5, /*SW_SHOW*/
                );
            }
        }
        MENU_RESETSETTINGS => {
            // 設定を初期化(Mac の「その他」の「すべての設定を初期化…」と対になる操作)。
            // .env の KNIT_* 行を消し、配置を既定へ戻す。履歴と端末の登録は
            // 消さない。環境変数として設定された KNIT_* はここからは消えない
            let text = wide(
                "このPCの設定をすべて消して、最初の状態に戻します。\n対象: .env の KNIT_* 行・画面配置\n履歴と端末の登録は消えません(それぞれ専用の操作があります)。\n\n環境変数として設定されている KNIT_* は消えません。\nこの操作は取り消せません。",
            );
            let caption = wide("すべての設定を初期化しますか?");
            let choice = MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                0x0004_0030 /*MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND*/,
            );
            if choice != 6 {
                /*IDYES 以外 = キャンセル*/
                return;
            }
            let env_path = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(".env")));
            if let Some(path) = env_path {
                if let Err(e) = knit_common::envutil::clear_knit_lines(&path) {
                    eprintln!("[tray] 設定の初期化に失敗(.env): {e}");
                    notify(
                        "設定を初期化できませんでした",
                        "アプリのフォルダへの書込み権限を確認してください。",
                    );
                    return;
                }
            }
            // 配置の既定化(辺とモニター指定の両方を戻す)
            crate::state::set_mac_pref("side", serde_json::json!(0));
            crate::state::set_mac_pref("peer_layout_reset", serde_json::json!(true));
            eprintln!("[tray] 設定を初期化しました(.env の KNIT_* 行を削除し配置を既定化)");
            notify(
                "設定を初期化しました",
                "Knitを再起動すると、最初の状態で起動します。",
            );
        }
        MENU_RESETREG => {
            // 登録を初期化(Mac の「その他」の「すべての登録を初期化…」と対になる操作)。
            // 接続キーを削除して再起動=次回起動で初回登録の画面からやり直す。
            // 取り消せないため確認を挟み、既定は「いいえ」に置く
            if UI_PREVIEW.load(Ordering::Relaxed) {
                return;
            }
            // .env の KNIT_TOKEN(直接つなぐ)は接続キーより優先され、この初期化では
            // 消せない。誤って「初期化したのに締め出せない」状態を作らないため、
            // ここで止めて、通常の登録へ戻す導線(設定の初期化で KNIT_* 行を消す)へ
            // 案内する(Mac 側 reset_registration と同じ対応)
            if knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty()) {
                notify(
                    "このPCでは登録を初期化できません",
                    "直接つなぐ(共通トークン)で運用中です。「すべての設定を初期化…」で KNIT_* 行を消すと、通常の登録へ戻せます。",
                );
                return;
            }
            let text = wide(
                "このPCの接続キーを削除して、登録を最初からやり直します。\nいま登録済みの端末はすべて接続できなくなり、使うには再登録が必要です。\n\nこの操作は取り消せません。",
            );
            let caption = wide("すべての登録を初期化しますか?");
            let choice = MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                0x0004_0030 /*MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND*/,
            );
            if choice != 6 {
                /*IDYES 以外 = キャンセル*/
                return;
            }
            if let Err(e) = knit_common::credentials::delete() {
                eprintln!("[tray] 登録の初期化に失敗: {e}");
                notify(
                    "登録を初期化できませんでした",
                    "接続キーは変更していません。ログを確認してください。",
                );
                return;
            }
            eprintln!("[tray] すべての登録を初期化しました(接続キーを削除)");
            notify(
                "登録をすべて初期化しました",
                "Knitを再起動すると、初回登録の画面からやり直せます。",
            );
            crate::audio::speaker_disconnect();
            crate::release_all_input();
            restart_self();
        }
        MENU_OPENLOG => {
            // ログは exe と同じフォルダ(run_knit.bat が書き出す)
            let path = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("knit-win.log")))
                .unwrap_or_default();
            let mut log: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
            log.push(0);
            let verb = wide("open");
            let np = wide("notepad.exe");
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                np.as_ptr(),
                log.as_ptr(),
                std::ptr::null(),
                5, /*SW_SHOW*/
            );
        }
        MENU_BACKMAC => {
            // Mac へ制御を返す(Return を送る=左端到達と同じ経路)
            let guard = crate::WTX.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(crate::proto_return());
                println!("[tray] Mac へ戻る");
            } else {
                println!("[tray] Mac へ戻る: 未接続のため何も起きません");
                notify(
                    "Knit",
                    "未接続のため戻れません(相手側アプリが起動していれば自動で再接続します)",
                );
            }
        }
        MENU_OPENFOLDER => {
            // 受信フォルダ(DOWNLOADS\\Knit)をエクスプローラーで開く
            let dir = std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
                .join("Downloads")
                .join("Knit");
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
            // サーバー(Mac)アドレスを .env へ保存して再起動(自動復帰が起こす)。
            // 空欄は KNIT_HOST 行を消して LAN 自動発見へ戻す(Mac 側と同じ挙動)
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
                if UI_PREVIEW.load(Ordering::Relaxed) {
                    return;
                }
                // 形式の確認(Mac 側 imp_save_host と同じ基準の共通関数):
                // 各要素が IP アドレスとして書けているか。不正なら通知して保存しない
                if let Err(bad) = knit_common::connect::validate_host_input(&host) {
                    eprintln!("[prefs] host save rejected: {bad}");
                    notify(
                        "接続先を保存できません",
                        &format!("192.168.1.23の形式で入力してください({bad}はIPアドレスとして読めません)"),
                    );
                    return;
                }
                if let Err(e) = save_host_to_env(&host) {
                    eprintln!("[prefs] host save failed: {e}");
                    notify(
                        "接続先を保存できません",
                        "アプリのフォルダへの書込み権限を確認してください。",
                    );
                    return;
                }
                // 保存先(.env)より優先される指定(起動時の環境変数)があるときは
                // 保存が効かないため、上書き方法を併せて案内する(Mac 側と同じ)
                let mut message = if host.is_empty() {
                    "接続先の指定を外しました(自動発見へ戻します)。Knitを再起動します".to_string()
                } else {
                    format!("接続先を {host} に保存しました。Knitを再起動します")
                };
                if std::env::var("KNIT_HOST").map(|v| !v.is_empty()).unwrap_or(false) {
                    message.push_str("。なお環境変数 KNIT_HOST が設定されているため、そちらが優先されます");
                }
                eprintln!(
                    "[tray] サーバーを {} へ変更し再起動します",
                    if host.is_empty() { "(自動発見)" } else { host.as_str() }
                );
                notify("Knit", &message);
                // 接続中ミュートの状態を復帰させてから終わる(ミュート恒久化の防止)。
                // 終了だけだと毎分の自動復帰タスクが起こすまで最長 1 分消えたままに
                // なるため、restart_self で即座に起こし直す(MENU_RESTART と同じ)
                crate::audio::speaker_disconnect();
                crate::release_all_input();
                restart_self();
            }
        }
        MENU_MAC_CLIP | MENU_MAC_FILES | MENU_MAC_HISTORY | MENU_MAC_AUDIO | MENU_MAC_SPK => {
            let key = match id {
                MENU_MAC_CLIP => "clip_share",
                MENU_MAC_FILES => "share_files",
                MENU_MAC_HISTORY => "local_history",
                MENU_MAC_AUDIO => "audio_muted",
                _ => "spk_mute",
            };
            // Mac から一覧が届いていない間は何も変えない(現在値が分からない)
            if let Some(cur) = crate::state::mac_pref(key).and_then(|v| v.as_bool()) {
                crate::state::set_mac_pref(key, serde_json::json!(!cur));
                update_labels();
            }
        }
        MENU_SHARE_CLIP | MENU_SHARE_FILES => {
            // 環境変数 KNIT_SHARE が禁じた項目は、設定画面から許可できない。
            // チェックボックス(Mac の「渡すもの」と同じ項目)からの通知ならその
            // チェック状態を、トレイメニューならトグルを設定する
            let cap = knit_common::share::env_cap();
            if id == MENU_SHARE_CLIP && cap.clip {
                let next = settings_ui::checked_state(MENU_SHARE_CLIP)
                    .unwrap_or_else(|| !knit_common::share::user_clip());
                knit_common::share::set_user_clip(next);
                println!("[tray] テキストと画像の共有 -> {next}");
            } else if id == MENU_SHARE_FILES && cap.files {
                let next = settings_ui::checked_state(MENU_SHARE_FILES)
                    .unwrap_or_else(|| !knit_common::share::user_files());
                knit_common::share::set_user_files(next);
                println!("[tray] ファイルの受け渡し -> {next}");
            }
            if !UI_PREVIEW.load(Ordering::Relaxed) {
                if let Err(e) = preferences::save() {
                    eprintln!("[prefs] save failed: {e}");
                    notify(
                        "設定を保存できません",
                        "変更は今回の起動中のみ有効です。ログを確認してください。",
                    );
                }
            }
            update_labels();
        }
        MENU_HOSTMODE | MENU_ROLE_CLIENT | MENU_ROLE_HOST => {
            // 接続の方向は起動時に決まるため、切り替えたら再起動して即反映する
            if UI_PREVIEW.load(Ordering::Relaxed) {
                return;
            }
            // 環境変数 KNIT_ROLE=server で固定中は GUI から切り替えられない
            if role_env_fixed() {
                notify(
                    "Knit",
                    "環境変数 KNIT_ROLE=server で役割を固定しています。切り替えるには .env の KNIT_ROLE の指定を外して再起動してください",
                );
                update_labels();
                return;
            }
            let want = match id {
                MENU_ROLE_HOST => true,
                MENU_ROLE_CLIENT => false,
                _ => !HOST_MODE.load(Ordering::Relaxed),
            };
            if want == HOST_MODE.load(Ordering::Relaxed) {
                update_labels(); // すでにその役割。選択表示だけ合わせ直す
                return;
            }
            preferences::set_host_mode(want);
            let next = want;
            if let Err(e) = preferences::save() {
                eprintln!("[prefs] save failed: {e}");
            }
            notify(
                "Knit",
                if next {
                    "このPCをホスト(待受側)に切り替えました。Knitを再起動します"
                } else {
                    "このPCを接続側に戻しました。Knitを再起動します"
                },
            );
            // 相手(Mac)にも反対の役割へ合わせさせる(双方が待ち受け/双方が接続側になり、
            // つながらなくなるのを防ぐ)。版 15 以降の相手は適用済みの RoleAck を返す
            // ので、それを確認してから再起動する(行き損ねで双方が同役割のまま沈黙するのを
            // 防ぐ)。旧版は応答しないため時間経過で再起動する。
            // この処理はウィンドウプロシージャ(メッセージループ)から呼ばれるため、
            // 最大2秒の待ちをここで行うと描画・トレイ操作が固まる。別スレッドで待つ
            ROLE_ACK.store(false, Ordering::Relaxed);
            // 切替の進行を設定画面の注記行へ出す(確認待ち→確認済み/タイムアウト。
            // sync() がこの状態を文言へ反映する。Mac 側と同じ仕組み)
            settings_ui::ROLE_NOTE_KIND.store(2, Ordering::Relaxed);
            crate::state::send_main_msg(&knit_common::proto::Msg::Role { host: next });
            crate::audio::speaker_disconnect();
            crate::release_all_input();
            std::thread::spawn(|| {
                if wait_role_ack(std::time::Duration::from_millis(2000)) {
                    println!("[role] 相手の適用を確認しました");
                    settings_ui::ROLE_NOTE_KIND.store(3, Ordering::Relaxed);
                } else {
                    println!("[role] 相手の適用確認が取れないため時間経過で再起動します");
                    settings_ui::ROLE_NOTE_KIND.store(4, Ordering::Relaxed);
                }
                restart_self();
            });
        }
        MENU_UPDATE => crate::updater::on_click(),
        MENU_INPUT_HELPER => {
            // 管理者権限が要るため UAC の確認を出して自分自身を昇格起動する(一度だけ)
            if let Ok(exe) = std::env::current_exe() {
                let verb = wide("runas");
                let file = wide(&exe.to_string_lossy());
                let args = wide("--install-input-helper");
                ShellExecuteW(
                    std::ptr::null_mut(),
                    verb.as_ptr(),
                    file.as_ptr(),
                    args.as_ptr(),
                    std::ptr::null(),
                    0, /*SW_HIDE*/
                );
            }
        }
        MENU_RESTART => {
            // exe を止めると毎分の自動復帰タスクが起こす=確実な再起動
            eprintln!("[tray] 再起動します");
            crate::audio::speaker_disconnect();
            crate::release_all_input();
            restart_self();
        }
        MENU_QUIT => {
            eprintln!("[tray] メニューから終了しました");
            crate::audio::speaker_disconnect();
            crate::release_all_input();
            std::process::exit(0);
        }
        _ => {}
    }
}

unsafe extern "system" fn status_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    const WM_PAINT2: u32 = 0x000F;
    const WM_ERASEBKGND2: u32 = 0x0014;
    const WM_CTLCOLORSTATIC2: u32 = 0x0138;
    const WM_CTLCOLOREDIT2: u32 = 0x0133;
    const WM_DRAWITEM2: u32 = 0x002B;
    const WM_SETTINGCHANGE2: u32 = 0x001A;
    match msg {
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u32;
            const CBN_SELCHANGE: usize = 1;
            if (wparam >> 16) & 0xFFFF == CBN_SELCHANGE && id == ID_HOTKEY_COMBO {
                let sel = windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(lparam as HWND, 0x147 /*CB_GETCURSEL*/, 0, 0);
                if sel >= 0 {
                    settings_ui::combo_changed(id, sel as usize);
                }
            } else {
                handle_command(id);
            }
            0
        }
        0x0114 /*WM_HSCROLL*/ => {
            settings_ui::delay_changed(wparam, lparam as HWND);
            0
        }
        0x0201 /*WM_LBUTTONDOWN*/ | 0x0200 /*WM_MOUSEMOVE*/ | 0x0202 /*WM_LBUTTONUP*/ => {
            if settings_ui::layout_mouse(hwnd, msg, lparam) {
                0
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_CLOSE => {
            // 閉じても破棄せず隠すだけ(常駐アプリの標準動作)
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_SETTINGCHANGE2 => {
            // 「アプリをダークにする」の切替に追従して全面を再描画する
            if sync_theme() {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 1);
            }
            0
        }
        WM_ERASEBKGND2 => 1, // 背景は WM_PAINT で全描き(ちらつき防止)
        WM_PAINT2 => {
            unsafe { paint_status(hwnd) };
            0
        }
        WM_CTLCOLOREDIT2 => {
            // 入力欄: テーマの面と文字色(ライト=白地+黒文字、ダーク=暗面+明文字)
            unsafe {
                let hdc = wparam as *mut core::ffi::c_void;
                let t = theme();
                SetTextColor(hdc, rgb(t.edit_text));
                SetBkColor(hdc, rgb(t.edit_bg));
                static EDIT_LIGHT_BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
                static EDIT_DARK_BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
                let b = if DARK_MODE.load(Ordering::Relaxed) {
                    *EDIT_DARK_BRUSH.get_or_init(|| CreateSolidBrush(rgb(DARK.edit_bg)) as usize)
                } else {
                    *EDIT_LIGHT_BRUSH.get_or_init(|| CreateSolidBrush(rgb(LIGHT.edit_bg)) as usize)
                };
                b as LRESULT
            }
        }
        WM_CTLCOLORSTATIC2 => {
            // ラベルの文字色をテーマへ(見出し/状態=強調、本文=本文色、補助=補足色)。
            // 背景は透過(WM_PAINT のカード面がそのまま見える)
            unsafe {
                let hdc = wparam as *mut core::ffi::c_void;
                let child = lparam as HWND;
                extern "system" {
                    fn GetDlgCtrlID(hwnd: HWND) -> i32;
                }
                let id = GetDlgCtrlID(child);
                let t = theme();
                // 色: 見出し(ブランド・ページ見出し・セクション見出し・状態行)=強調、
                // 補足(サブタイトル・注記・ヒント)=補助色、他=本文色
                let color = match id as u32 {
                    210 | 220 | 223 | 226 => rgb(t.head),
                    221 | 222 | 224 | 225 | 227 | 230 | 231 | 232 | 233 | 234 => rgb(t.sub),
                    _ => rgb(t.text),
                };
                SetTextColor(hdc, color);
                SetBkMode(hdc, TRANSPARENT_BK);
                // 背景ブラシを窓背景色で返す: 透過(NULL_BRUSH)だと文字更新時に
                // 古い文字が残って重なって見える(ゴースト)ため不透明で塗る。
                // カード内(210..=230 の状態カードまわり)だけカード面の色を返す
                static BG_BRUSHES: std::sync::OnceLock<[usize; 2]> = std::sync::OnceLock::new();
                static CARD_BRUSHES: std::sync::OnceLock<[usize; 2]> = std::sync::OnceLock::new();
                let brushes = BG_BRUSHES.get_or_init(|| {
                    [
                        CreateSolidBrush(rgb(LIGHT.bg)) as usize,
                        CreateSolidBrush(rgb(DARK.bg)) as usize,
                    ]
                });
                let cards = CARD_BRUSHES.get_or_init(|| {
                    [
                        CreateSolidBrush(rgb(LIGHT.card)) as usize,
                        CreateSolidBrush(rgb(DARK.card)) as usize,
                    ]
                });
                let dark = DARK_MODE.load(Ordering::Relaxed) as usize;
                if (210..=230).contains(&id) {
                    cards[dark] as LRESULT
                } else {
                    brushes[dark] as LRESULT
                }
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
                let t = theme();
                let nav = (settings_ui::NAV_FIRST
                    ..settings_ui::NAV_FIRST + settings_ui::PAGE_COUNT as u32)
                    .contains(&d.ctl_id);
                let selected = nav
                    && d.ctl_id as usize - settings_ui::NAV_FIRST as usize
                        == settings_ui::PAGE.load(Ordering::Relaxed);
                // 主要ボタン(保存して再接続)=アクセント、初期化=赤系(Mac の
                // mark_destructive と同じ「破壊的操作は赤で区別する」)
                let primary = d.ctl_id == MENU_SAVEHOST;
                let danger = matches!(d.ctl_id, MENU_RESETREG | MENU_RESETSETTINGS);
                let disabled = d.item_state & 0x4 != 0; // ODS_DISABLED
                let brush_color = if nav {
                    if selected {
                        t.nav_sel
                    } else {
                        t.bg
                    }
                } else if disabled {
                    t.btn_idle
                } else if d.item_state & 1 != 0 {
                    if danger {
                        t.danger_pressed
                    } else if primary {
                        t.btn_primary_pressed
                    } else {
                        t.btn_pressed
                    }
                } else if danger {
                    t.danger
                } else if primary {
                    t.accent
                } else {
                    t.btn_idle
                };
                let base = CreateSolidBrush(rgb(if nav { t.bg } else { t.card }));
                FillRect(d.hdc, &d.rc_item, base);
                DeleteObject(base);
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
                SetTextColor(
                    d.hdc,
                    if nav {
                        rgb(if selected { t.accent } else { t.text })
                    } else if disabled {
                        rgb(t.sub)
                    } else if danger || primary {
                        0x00FFFFFF
                    } else {
                        rgb(t.head)
                    },
                );
                if d.item_state & 0x10 != 0 {
                    windows_sys::Win32::Graphics::Gdi::DrawFocusRect(
                        d.hdc,
                        &d.rc_item as *const Rect as *const windows_sys::Win32::Foundation::RECT,
                    );
                }
                SetBkMode(d.hdc, TRANSPARENT_BK);
                // ボタン文字はウィンドウテキストから取る(タブはアイコン無しの中央寄せ)
                let mut buf = [0u16; 64];
                extern "system" {
                    fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
                }
                let font = windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                    d.hwnd_item as HWND,
                    0x0031, /*WM_GETFONT*/
                    0,
                    0,
                );
                if font != 0 {
                    SelectObject(d.hdc, font as _);
                }
                let len = GetWindowTextW(d.hwnd_item as HWND, buf.as_mut_ptr(), 64);
                if len > 0 {
                    let mut r = d.rc_item;
                    r.left += 4;
                    r.right -= 4;
                    DrawTextW(
                        d.hdc,
                        buf.as_mut_ptr(),
                        len,
                        &mut r,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
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
        let bg = CreateSolidBrush(rgb(theme().bg));
        FillRect(hdc, &rc, bg);
        DeleteObject(bg);
        settings_ui::paint_groups(hdc);
        settings_ui::paint_layout(hdc);
        EndPaint(hwnd, &ps);
    }
}

/// モダンな見た目のための Segoe UI フォント生成(通常/太字)。
/// 既定の DEFAULT_GUI_FONT は古いシステムフォントになるため使わない。
unsafe fn segoe_font(bold: bool, height: i32) -> *mut core::ffi::c_void {
    unsafe {
        let mut name: Vec<u16> = "Meiryo UI".encode_utf16().collect();
        name.push(0);
        extern "system" {
            fn CreateFontW(
                height: i32,
                width: i32,
                escapement: i32,
                orientation: i32,
                weight: i32,
                italic: u32,
                underline: u32,
                strikeout: u32,
                charset: u32,
                outprecision: u32,
                clipprecision: u32,
                quality: u32,
                pitchandfamily: u32,
                face: *const u16,
            ) -> *mut core::ffi::c_void;
        }
        CreateFontW(
            -height,
            0,
            0,
            0,
            if bold { 700 } else { 400 },
            0,
            0,
            0,
            1, /*DEFAULT_CHARSET*/
            0,
            0,
            5, /*CLEARTYPE_QUALITY*/
            0,
            name.as_ptr(),
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
    settings_ui::build();
}

/// ラベル類の定期更新(WM_TIMER から)。状態行は Mac の設定「接続」と同じ文言形式
unsafe fn update_labels() {
    settings_ui::sync();
    set_text(
        LABEL_STATE.load(Ordering::Relaxed),
        &settings_ui::connection_state_text(
            UI_PREVIEW.load(Ordering::Relaxed),
            crate::CONNECTED.load(Ordering::Relaxed),
            crate::RTT_MS.load(Ordering::Relaxed),
            if crate::CONNECTED.load(Ordering::Relaxed) {
                settings_ui::route_label()
            } else {
                None
            },
            settings_ui::token_manual(),
            registered(),
            crate::next_retry_line().as_deref(),
            crate::conn::not_found_line().as_deref(),
        ),
    );
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
    let open_w = wide("設定を開く…");
    AppendMenuW(menu, MF_STRING, MENU_STATUS as usize, open_w.as_ptr());
    // クリップボード履歴(送信・受信したテキストから選んで復元)
    {
        let now = knit_common::history::now_epoch_ms();
        let count = crate::HISTORY
            .lock()
            .map(|h| h.entries().len())
            .unwrap_or(0);
        let entries: Vec<(u64, String)> = crate::HISTORY
            .lock()
            .map(|h| {
                h.recent(knit_common::history::MENU_ITEMS)
                    .into_iter()
                    .map(|e| (e.id, knit_common::history::label(e, now, 34)))
                    .collect()
            })
            .unwrap_or_default();
        if !entries.is_empty() {
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            let head = wide(&format!("クリップボード履歴 {count}件(クリックでクリップボードへ戻します)"));
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, head.as_ptr());
            let mut ids = Vec::new();
            for (i, (entry_id, label)) in entries.iter().enumerate() {
                let w = wide(label);
                AppendMenuW(
                    menu,
                    MF_STRING,
                    (MENU_HISTORY_FIRST + i as u32) as usize,
                    w.as_ptr(),
                );
                ids.push(*entry_id);
            }
            // 平文保存の常時通知(利用者が気づけるように履歴がある間は常に表示する)
            let note = wide("※履歴は平文で保存されています(残したくない場合は「履歴をすべて消す…」)");
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, note.as_ptr());
            let clear = wide("履歴をすべて消す…");
            AppendMenuW(menu, MF_STRING, MENU_HISTORY_CLEAR as usize, clear.as_ptr());
            if let Ok(mut g) = MENU_HISTORY_IDS.lock() {
                *g = ids;
            }
        } else {
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            let head = wide("クリップボード履歴(まだありません。コピーすると記録されます)");
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, head.as_ptr());
        }
    }
    let audio_w = wide(&audio_line());
    AppendMenuW(menu, MF_STRING, MENU_AUDIO as usize, audio_w.as_ptr());
    let bm = wide("相手へ戻る");
    AppendMenuW(
        menu,
        MF_STRING | if crate::CONNECTED.load(Ordering::Relaxed) {
            0
        } else {
            MF_GRAYED
        },
        MENU_BACKMAC as usize,
        bm.as_ptr(),
    );
    let fo = wide("受信フォルダを開く");
    AppendMenuW(menu, MF_STRING, MENU_OPENFOLDER as usize, fo.as_ptr());
    let sd = wide("設定フォルダを開く");
    AppendMenuW(menu, MF_STRING, MENU_OPENSETDIR as usize, sd.as_ptr());
    let log_w = wide("ログを開く");
    AppendMenuW(menu, MF_STRING, MENU_OPENLOG as usize, log_w.as_ptr());
    let rs_set = wide("設定を初期化…");
    AppendMenuW(menu, MF_STRING, MENU_RESETSETTINGS as usize, rs_set.as_ptr());
    let up = wide(&crate::updater::menu_title());
    AppendMenuW(menu, MF_STRING, MENU_UPDATE as usize, up.as_ptr());
    let ih = wide(if crate::helper::is_connected() {
        "UAC・管理者アプリの操作補助を更新…"
    } else {
        "UAC・管理者アプリの操作を有効にする…"
    });
    AppendMenuW(menu, MF_STRING, MENU_INPUT_HELPER as usize, ih.as_ptr());
    let rs = wide("再起動");
    AppendMenuW(menu, MF_STRING, MENU_RESTART as usize, rs.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    let quit = wide("終了");
    AppendMenuW(menu, MF_STRING, MENU_QUIT as usize, quit.as_ptr());
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        std::ptr::null(),
    );
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
        // 埋め込みアイコン(build.rs が `1 ICON "app.ico"` でリソース ID 1 を
        // 埋める)。dangling ポインタはアラインメント(=2)を指すため ID 2 を要求する
        // ことになり、常に失敗していた。ID 1 を明示する
        let embedded = LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            1usize as *const u16, /*MAKEINTRESOURCEW(1)*/
            IMAGE_ICON,
            size,
            size,
            0,
        );
        if !embedded.is_null() {
            return embedded;
        }
        LoadIconW(
            std::ptr::null_mut(),
            windows_sys::Win32::UI::WindowsAndMessaging::IDI_APPLICATION,
        )
    }
}

unsafe fn load_tray_icon() -> *mut core::ffi::c_void {
    load_tray_icon_size(24)
}

unsafe fn tray_loop() {
    let mut class: Vec<u16> = "SDWinTray".encode_utf16().collect();
    class.push(0);
    let hinst = GetModuleHandleW(std::ptr::null());
    TRAY_HINST.store(hinst as usize, Ordering::Relaxed);
    let icon = load_tray_icon();
    TRAY_HICON.store(icon as usize, Ordering::Relaxed);
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
    let mut title: Vec<u16> = "Knit".encode_utf16().collect();
    title.push(0);
    // 可視化しないメッセージウィンドウ(トレイのコールバック受け)
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        title.as_ptr(),
        0, // WS_OVERLAPPED(非表示のまま ShowWindow しない)
        0,
        0,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinst,
        std::ptr::null(),
    );
    if hwnd.is_null() {
        eprintln!("[tray] CreateWindowExW 失敗(トレイなしで継続)");
        return;
    }
    TRAY_HWND.store(hwnd as usize, Ordering::Relaxed);

    // 画面ロック連動(Windows→Mac 方向): このセッションのロック(Win+L 等)を
    // 受け取り、Mac へ既存の Msg::Lock(ワイヤ変更なし)を送る。KNIT_LOCK_SYNC=0 で無効
    #[link(name = "wtsapi32")]
    unsafe extern "system" {
        fn WTSRegisterSessionNotification(hwnd: HWND, flags: u32) -> i32;
    }
    const NOTIFY_FOR_THIS_SESSION: u32 = 0;
    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        eprintln!("[tray] セッション通知の登録に失敗(ロック連動は無効)");
    }

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
    // デバッグ/スクリーンショット検証用: KNIT_STATUS_SHOW=1 で起動時に窓を開く
    if UI_PREVIEW.load(Ordering::Relaxed)
        || crate::JUST_REGISTERED.load(Ordering::Relaxed)
        || knit_common::envutil::get("KNIT_STATUS_SHOW").as_deref() == Some("1")
    {
        open_status_window();
    }

    let mut msg: windows_sys::Win32::UI::WindowsAndMessaging::MSG = std::mem::zeroed();
    loop {
        let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
        if r <= 0 {
            break;
        }
        let status = STATUS_HWND.load(Ordering::Relaxed) as HWND;
        if !status.is_null()
            && windows_sys::Win32::UI::WindowsAndMessaging::IsDialogMessageW(status, &msg) != 0
        {
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

/// トレイを開始(別スレッドでメッセージループ)。失敗しても本体は継続する
pub fn start() {
    preferences::restore();
    std::thread::spawn(|| unsafe { tray_loop() });
}
