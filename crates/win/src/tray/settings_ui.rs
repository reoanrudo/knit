//! Windows 設定画面。Mac 側(gui/prefs.rs)と同じ 5 ページ構成(接続・画面配置・切替・
//! 共有・その他)・同じ文言で構築し、上部タブで切り替える。Mac に UI が無い調整
//! (スクロール・タブレット操作)はここにも置かない(env と設定ファイルが担う)。
//! 接続先のIP入力と「保存して再接続」は Mac と同じ導線で、IP アドレス指定での
//! 直接接続をこの画面から完結させる。
use super::*;
use std::sync::Mutex;
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRect, SendMessageW, WS_CAPTION, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP,
};

pub(super) const NAV_FIRST: u32 = 3000;
/// ページ数。Mac 側と同じ 5 ページ(接続・画面配置・切替・共有・その他)
pub(super) const PAGE_COUNT: usize = 5;
pub(super) static PAGE: AtomicUsize = AtomicUsize::new(0);
static CONTROLS: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
/// 役割ラジオ(0=相手がホスト=このPCが接続しに行く、1=このPCがホスト)
static ROLE_CLIENT_RADIO: AtomicUsize = AtomicUsize::new(0);
static ROLE_HOST_RADIO: AtomicUsize = AtomicUsize::new(0);
/// 役割カードの注記行(通常の再起動案内・KNIT_ROLE 固定中・切替進行で文言が変わる)
static ROLE_NOTE: AtomicUsize = AtomicUsize::new(0);
/// 役割注記のモード(0=通常 1=Env固定 2=確認待ち 3=確認済み 4=タイムアウト)。
/// 切替ハンドラと Ack 待ちスレッドが書き、sync() が文言へ反映する(Mac と同じ仕組み)
pub(super) static ROLE_NOTE_KIND: AtomicUsize = AtomicUsize::new(0);
/// 接続先の保存ボタン(ホスト役割の間は押せなくする)
static SAVEHOST_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 接続先の下のヒント行(Mac 側 HOST_HINT と同じ 2 文言を役割で切替)
static HOST_HINT: AtomicUsize = AtomicUsize::new(0);
/// 状態カード内の「このPCのアドレス」行(IP で直接つなぐときの確認用)
static OWN_IP: AtomicUsize = AtomicUsize::new(0);
/// 配置図・カード描画で使う文字フォント(build で作り、paint で選択する)
static DRAW_FONT: AtomicUsize = AtomicUsize::new(0);
/// KNIT_TOKEN(直接つなぐ)が設定済みか。保存→再起動でしか変わらないため
/// 起動時に 1 回だけ .env を見る(毎秒の sync() で読み直さない)
pub(super) fn token_manual() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty()))
}
// このPCの共有設定のチェック(Mac の「渡すもの」と同じ項目)
static CHK_CLIP: AtomicUsize = AtomicUsize::new(0);
static CHK_FILES: AtomicUsize = AtomicUsize::new(0);
static CHK_AUDIO: AtomicUsize = AtomicUsize::new(0);
// ロック解除の連動(その他ページ)。トグル+パスワード欄+保存ボタン
static CHK_UNLOCK: AtomicUsize = AtomicUsize::new(0);
pub(super) static EDIT_UNLOCK_SECRET: AtomicUsize = AtomicUsize::new(0);
static UNLOCK_SECRET_STATE: AtomicUsize = AtomicUsize::new(0);
// Mac の設定を Windows から変える部品(接続中のみ有効)
pub(super) static HOTKEY_COMBO: AtomicUsize = AtomicUsize::new(0);
/// 切替キーコンボの「現在のキー(コードN)」項目に入れているキーコード。
/// 候補外のキーが設定されているときだけ項目を足す(-1 = 無し)
static HOTKEY_CUSTOM_KC: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);
/// 滞在時間スライダ(50-1000ms。Mac の設定「切替」と同じ範囲)
pub(super) static DELAY_TRACK: AtomicUsize = AtomicUsize::new(0);
static DELAY_VALUE: AtomicUsize = AtomicUsize::new(0);
pub(super) static TRACK_DRAGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 画面配置のドラッグ中フラグとドラッグ中のこのPC四角の位置
static LAY_DRAGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LAY_DRAG_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static LAY_DRAG_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// 相手側の設定(Mac の共有ページの項目)のトグル。接続中のみ有効
static MAC_CLIP_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_FILES_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_HISTORY_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_AUDIO_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_SPK_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 切替ページの未接続注記(接続中は空)
static SWITCH_OFFLINE_NOTE: AtomicUsize = AtomicUsize::new(0);
/// 画面配置の現在値行(「現在: このPCは相手の画面の右」)
static LAYOUT_VALUES: AtomicUsize = AtomicUsize::new(0);

// ---------- ラベルID(WM_CTLCOLORSTATIC の配色分けに使う) ----------
/// 固定部・カード外のラベル(220..=227 は窓背景の面)
pub(super) const ID_BRAND: u32 = 220;
const ID_CONTEXT: u32 = 221;
const ID_PAGE_SUB: u32 = 222;
const ID_PAGE_TITLE: u32 = 223;
const ID_SAVE_NOTE: u32 = 224;
const ID_VERSION: u32 = 225;
/// セクション見出し(「接続の方向」「渡すもの」等)
const ID_SECTION: u32 = 226;
/// 注記・ヒント(カード外)
const ID_NOTE: u32 = 227;
/// 状態カード内(210..=230 はカードの面)
const ID_STATE_HEAD: u32 = 211;
pub(super) const ID_STATE_LINE: u32 = 210;
const ID_OWN_IP: u32 = 230;
const ID_LAYOUT_VALUE: u32 = 231;
const ID_DELAY_VALUE: u32 = 232;
const ID_SWITCH_NOTE: u32 = 233;
/// ロック解除の連動のパスワード状態行(保存済み/未設定)
const ID_UNLOCK_STATE: u32 = 234;

/// 画面配置キャンバスの領域(left, top, w, h)と、相手・このPCの四角
const LAY_AREA: (i32, i32, i32, i32) = (36, 180, 648, 396);
const BOX_W: i32 = 144;
const BOX_H: i32 = 88;
/// キャンバス中央(相手の四角の中心)
fn lay_center() -> (i32, i32) {
    (LAY_AREA.0 + LAY_AREA.2 / 2, LAY_AREA.1 + LAY_AREA.3 / 2)
}
/// side(0=右 1=左 2=上 3=下 4=右上 5=右下 6=左上 7=左下)→このPC四角の左上座標
fn box_pos(side: u8) -> (i32, i32) {
    let (cx, cy) = lay_center();
    let (dx, dy) = match side {
        1 => (-190, 0),
        2 => (0, -150),
        3 => (0, 150),
        4 => (190, -150),
        5 => (190, 150),
        6 => (-190, -150),
        7 => (-190, 150),
        _ => (190, 0),
    };
    (cx + dx - BOX_W / 2, cy + dy - BOX_H / 2)
}
/// side の日本語名(Mac 側 side_label と同じ語彙)
fn side_label(side: u8) -> &'static str {
    match side {
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
/// ドロップ位置(このPC四角の中心)から side を定める(純粋関数)。
/// 相手の中心に重ねた(縦横とも 90px 未満)は配置と見なさず None
fn side_from_center(x: i32, y: i32) -> Option<u8> {
    let (cx, cy) = lay_center();
    let (dx, dy) = (x - cx, y - cy);
    const TH: i32 = 90;
    if dx.abs() <= TH && dy.abs() <= TH {
        return None;
    }
    let horiz = if dx > TH {
        0b01
    } else if dx < -TH {
        0b10
    } else {
        0
    };
    let vert = if dy > TH {
        0b01
    } else if dy < -TH {
        0b10
    } else {
        0
    };
    Some(match (horiz, vert) {
        (0b01, 0b00) => 0,
        (0b10, 0b00) => 1,
        (0b00, 0b10) => 2,
        (0b00, 0b01) => 3,
        (0b01, 0b10) => 4,
        (0b01, 0b01) => 5,
        (0b10, 0b10) => 6,
        (0b10, 0b01) => 7,
        _ => return None,
    })
}

pub(super) unsafe fn select(index: usize) {
    PAGE.store(index.min(PAGE_COUNT - 1), Ordering::Relaxed);
    for &(h, page) in CONTROLS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        ShowWindow(h as HWND, if page == index { SW_SHOW } else { SW_HIDE });
    }
    InvalidateRect(
        STATUS_HWND.load(Ordering::Relaxed) as HWND,
        std::ptr::null(),
        1,
    );
}

/// 接続ページの状態行の文言(Mac 側 connection_state_text と同じ形式・語彙)。
/// registered は接続キーの保存が確認できているか(未登録なら登録への導線を出す)。
/// manual は KNIT_TOKEN(直接つなぐ)運用中: 待ち状態の文言を分ける(トークン不一致に
/// 気づけるように)。retry/missing は未接続が続くときの補足(Mac と同じ文言を流用)
pub(super) fn connection_state_text(
    preview: bool,
    connected: bool,
    rtt: u64,
    route: Option<&str>,
    manual: bool,
    registered: bool,
    retry: Option<&str>,
    missing: Option<&str>,
) -> String {
    if preview {
        return "設定画面のプレビュー".into();
    }
    if connected {
        return match (rtt > 0, route) {
            (true, Some(r)) => format!("接続済み({}・遅延 {rtt}ms)", r),
            (true, None) => format!("接続済み(遅延 {rtt}ms)"),
            (false, _) => "接続済み".into(),
        };
    }
    // 手動接続(トークン)運用中は登録よりトークンの待機を優先(Mac と同じ)
    if !manual && !registered {
        return "未接続(相手側の設定「接続」の「端末を登録…」で登録できます)".into();
    }
    let inner = if manual { "直接つなぐで待機中" } else { "自動で再接続します" };
    match missing.or(retry) {
        Some(extra) => format!("未接続({inner}・{extra})"),
        None => format!("未接続({inner})"),
    }
}

/// 役割カードの注記行の文言(ROLE_NOTE_KIND の状態から。Mac 側と同じ文言)
fn role_note_text() -> String {
    match ROLE_NOTE_KIND.load(Ordering::Relaxed) {
        1 => "環境変数 KNIT_ROLE で役割を固定中のため、ここでは切り替えられません".into(),
        2 => "切り替えを相手へ送りました。相手の適用を確認しています…".into(),
        3 => "相手の適用を確認しました。再起動します…".into(),
        4 => "相手の適用確認が取れないため、時間経過で再起動します…".into(),
        _ => "切り替えると、両方のPCでKnitが自動で再起動します".into(),
    }
}

/// 接続の経路ラベル(LAN 直 / Tailscale)
pub(super) fn route_label() -> Option<&'static str> {
    crate::peer_ip().map(|ip| {
        if knit_common::net::is_tailscale(ip) {
            "Tailscale"
        } else {
            "LAN 直"
        }
    })
}

/// 設定ページのチェックボックス(このPCの共有設定)の現在のチェック状態。
/// チェックボックスはクリック時に Win32 が先に状態を切り替えてから通知して
/// くるため、読み取った値=利用者が選んだ新しい状態。そのIDのチェックボックスが
/// 無ければ None(トレイメニューからの通知)=呼び出し側はトグルとして扱う
pub(super) unsafe fn checked_state(id: u32) -> Option<bool> {
    let slot = match id {
        MENU_SHARE_CLIP => &CHK_CLIP,
        MENU_SHARE_FILES => &CHK_FILES,
        MENU_AUDIO => &CHK_AUDIO,
        MENU_UNLOCK_SYNC => &CHK_UNLOCK,
        _ => return None,
    };
    let h = slot.load(Ordering::Relaxed) as HWND;
    (!h.is_null()).then(|| SendMessageW(h, 0xF2 /*BM_GETCHECK*/, 0, 0) != 0)
}

/// EDIT コントロールの現在テキスト(接続先の保存と同じ読み取り方式)。
/// ハンドルが 0(まだ作られていない)なら空
pub(super) unsafe fn edit_text(h: usize) -> String {
    if h == 0 {
        return String::new();
    }
    extern "system" {
        fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
        fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    }
    let len = GetWindowTextLengthW(h as HWND);
    let mut buf = vec![0u16; len as usize + 1];
    GetWindowTextW(h as HWND, buf.as_mut_ptr(), len + 1);
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// このPCの IPv4 一覧の表示文字列(10 秒キャッシュ)。IP で直接つなぐとき、
/// 相手側に入力するこのPCのアドレスをこの行で確認できる
fn own_ip_text() -> String {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, String)>> =
        std::sync::Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, text)) = cache.as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(10) {
            return text.clone();
        }
    }
    let ips = knit_common::discover::local_ipv4s();
    let text = if ips.is_empty() {
        "このPCのアドレス: (取得できません)".to_string()
    } else {
        format!(
            "このPCのアドレス: {}",
            ips.iter()
                .map(|ip| ip.to_string())
                .collect::<Vec<_>>()
                .join("、")
        )
    };
    *cache = Some((std::time::Instant::now(), text.clone()));
    text
}

pub(super) unsafe fn sync() {
    sync_mac_prefs();
    let preview = UI_PREVIEW.load(Ordering::Relaxed);
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    set_text(ROLE_NOTE.load(Ordering::Relaxed), &role_note_text());
    set_text(OWN_IP.load(Ordering::Relaxed), &own_ip_text());
    // 接続の方向の選択表示。実役割は環境変数 KNIT_ROLE=server が GUI 設定に
    // 優先するため表示にも反映し、固定中は GUI から切り替えられないように
    // ラジオを無効化する(Mac 側の設定と同じ対応)
    let env_fixed = crate::tray::role_env_fixed();
    let host = env_fixed || crate::tray::HOST_MODE.load(Ordering::Relaxed);
    for (slot, on) in [(&ROLE_CLIENT_RADIO, !host), (&ROLE_HOST_RADIO, host)] {
        let h = slot.load(Ordering::Relaxed) as HWND;
        if !h.is_null() {
            if (SendMessageW(h, 0xF2 /*BM_GETCHECK*/, 0, 0) != 0) != on {
                SendMessageW(h, 0xF1 /*BM_SETCHECK*/, on as usize, 0);
            }
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h, (!env_fixed || preview) as i32);
        }
    }
    // 接続先IPと保存は、この PC が接続しに行く側の間だけ有効(ホストでは不要)。
    // Mac 側の HOST_FIELD/HOST_SAVE_BUTTON と同じ条件
    let client_side = !host || preview;
    for slot in [&EDIT_HOST, &SAVEHOST_BUTTON] {
        let h = slot.load(Ordering::Relaxed);
        if h != 0 {
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h as HWND, client_side as i32);
        }
    }
    set_text(
        HOST_HINT.load(Ordering::Relaxed),
        if client_side {
            "空欄なら自動発見。変更は「保存して再接続」で反映します。"
        } else {
            "ホストでは相手側から接続します。接続先の指定は不要です。"
        },
    );
    // このPCの共有設定のチェック(Mac の「渡すもの」と同じ項目)
    let set_check = |slot: &AtomicUsize, on: bool, allowed: bool| {
        let h = slot.load(Ordering::Relaxed) as HWND;
        if !h.is_null() {
            SendMessageW(h, 0xF1 /*BM_SETCHECK*/, on as usize, 0);
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h, allowed as i32);
        }
    };
    let cap = knit_common::share::env_cap();
    set_check(&CHK_CLIP, knit_common::share::user_clip(), cap.clip && !preview);
    set_check(&CHK_FILES, knit_common::share::user_files(), cap.files && !preview);
    set_check(
        &CHK_AUDIO,
        crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed),
        cap.audio && !preview,
    );
    // ロック解除の連動(このPCの設定のため共有範囲の制限を受けない)
    set_check(&CHK_UNLOCK, crate::tray::UNLOCK_SYNC.load(Ordering::Relaxed), !preview);
    set_text(
        UNLOCK_SECRET_STATE.load(Ordering::Relaxed),
        if crate::tray::unlock_secret_stored() {
            "パスワード: 保存済み"
        } else {
            "パスワード: 未設定(Enter のみ送ります)"
        },
    );
    // 切替ページの未接続注記(接続中は空=見えない)
    set_text(
        SWITCH_OFFLINE_NOTE.load(Ordering::Relaxed),
        if connected || preview {
            ""
        } else {
            "接続すると、相手側の切替キーと滞在時間を変えられます。"
        },
    );
    // 画面配置の現在値行
    set_text(
        LAYOUT_VALUES.load(Ordering::Relaxed),
        &format!(
            "現在: このPCは相手の画面の{}",
            side_label(crate::SIDE_W.load(Ordering::Relaxed) as u8)
        ),
    );
    static LAST_SIDE: AtomicUsize = AtomicUsize::new(usize::MAX);
    let side = crate::SIDE_W.load(Ordering::Relaxed) as usize;
    if LAST_SIDE.swap(side, Ordering::Relaxed) != side {
        let hwnd = STATUS_HWND.load(Ordering::Relaxed) as HWND;
        if !hwnd.is_null() {
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }
}

/// 最小化を戻して前面へ出す。トレイのメニュー操作直後は前面権限がなく
/// SetForegroundWindow 単独では点滅だけで終わるため、前面スレッドへ入力を接続して通す
unsafe fn bring_to_front(hwnd: HWND) {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SW_RESTORE,
    };
    ShowWindow(hwnd, if IsIconic(hwnd) != 0 { SW_RESTORE } else { SW_SHOW });
    let fg = GetForegroundWindow();
    let fg_thread = if fg.is_null() {
        0
    } else {
        GetWindowThreadProcessId(fg, std::ptr::null_mut())
    };
    let me = GetCurrentThreadId();
    let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, 1) != 0;
    BringWindowToTop(hwnd);
    SetForegroundWindow(hwnd);
    if attached {
        AttachThreadInput(me, fg_thread, 0);
    }
}

pub(super) unsafe fn build() {
    sync_theme();
    let existing = STATUS_HWND.load(Ordering::Relaxed) as HWND;
    if !existing.is_null() {
        bring_to_front(existing);
        return;
    }
    let hinst = TRAY_HINST.load(Ordering::Relaxed) as *mut core::ffi::c_void;
    let class = wide("KnitSettings");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(status_wndproc),
        hInstance: hinst,
        hIcon: TRAY_HICON.load(Ordering::Relaxed) as _,
        hCursor: windows_sys::Win32::UI::WindowsAndMessaging::LoadCursorW(
            std::ptr::null_mut(),
            windows_sys::Win32::UI::WindowsAndMessaging::IDC_ARROW,
        ),
        lpszClassName: class.as_ptr(),
        ..std::mem::zeroed()
    };
    RegisterClassW(&wc);
    let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    // Mac 側と同じ画面サイズ(720×700)
    let mut r = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 720,
        bottom: 700,
    };
    AdjustWindowRect(&mut r, style, 0);
    let hwnd = CreateWindowExW(
        0x00010000, /*WS_EX_CONTROLPARENT*/
        class.as_ptr(),
        wide("Knit設定").as_ptr(),
        style,
        windows_sys::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
        windows_sys::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
        r.right - r.left,
        r.bottom - r.top,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinst,
        std::ptr::null(),
    );
    if hwnd.is_null() {
        eprintln!("[tray] settings window failed");
        return;
    }
    STATUS_HWND.store(hwnd as usize, Ordering::Relaxed);
    let font = segoe_font(false, 16);
    let small = segoe_font(false, 14);
    let heading = segoe_font(true, 25);
    DRAW_FONT.store(small as usize, Ordering::Relaxed);
    {
        use windows_sys::Win32::UI::Controls::{InitCommonControlsEx, INITCOMMONCONTROLSEX};
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: 0x4 /*ICC_BAR_CLASSES*/ };
        InitCommonControlsEx(&icc);
    }
    // The hidden window is reused, so fonts and children live for the process lifetime.
    let make = |page: Option<usize>,
                class: &str,
                text: &str,
                style: u32,
                x: i32,
                y: i32,
                w: i32,
                h: i32,
                id: u32,
                f: *mut core::ffi::c_void|
     -> usize {
        let child = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as _,
            hinst,
            std::ptr::null(),
        );
        SendMessageW(child, WM_SETFONT, f as usize, 1);
        if let Some(page) = page {
            CONTROLS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((child as usize, page));
        }
        child as usize
    };
    // ---------- 固定部: ブランド・コンテキスト・上部タブ(Mac と同じ構成) ----------
    make(None, "STATIC", "Knit", 0, 24, 20, 160, 32, ID_BRAND, segoe_font(true, 22));
    make(None, "STATIC", "設定", 0, 620, 28, 76, 20, ID_CONTEXT, small);
    for (i, title) in ["接続", "画面配置", "切替", "共有", "その他"].iter().enumerate() {
        make(
            None,
            "BUTTON",
            title,
            0xB | WS_TABSTOP,
            24 + i as i32 * 134,
            64,
            128,
            36,
            NAV_FIRST + i as u32,
            font,
        );
    }
    make(
        None,
        "STATIC",
        "スイッチとチェックは自動保存・接続先は「保存して再接続」",
        0,
        24,
        664,
        430,
        20,
        ID_SAVE_NOTE,
        small,
    );
    make(
        None,
        "STATIC",
        &format!("v{}", crate::VERSION_STR),
        0x200,
        560,
        664,
        136,
        20,
        ID_VERSION,
        small,
    );
    // ページ見出しとサブタイトルは Mac と同じ文言
    let titles = [
        ("接続", "端末どうしをつなぎ、いまの状態を確かめます。"),
        ("画面配置", "画面の並びを、机の上の配置と揃えます。"),
        ("切替", "画面を越える方法を、自分の操作に合わせます。"),
        ("共有", "境界を越えて渡すものと、受け取るもの。"),
        ("その他", "表示言語と、アプリのメンテナンス。"),
    ];
    for (page, (title, sub)) in titles.iter().enumerate() {
        make(Some(page), "STATIC", title, 0, 24, 108, 560, 40, ID_PAGE_TITLE, heading);
        make(Some(page), "STATIC", sub, 0, 24, 156, 640, 24, ID_PAGE_SUB, small);
    }
    let section = |p, text: &str, y| make(Some(p), "STATIC", text, 0, 36, y, 300, 26, ID_SECTION, font);
    let note = |p, text: &str, x, y, w, id| make(Some(p), "STATIC", text, 0x200, x, y, w, 24, id, small);
    let btn = |p, text: &str, x, y, w, id| {
        make(Some(p), "BUTTON", text, 0xB | WS_TABSTOP, x, y, w, 32, id, font)
    };
    let combo = |p, items: &[&str], x, y, w, id| {
        let h = make(Some(p), "COMBOBOX", "", 0x0003 | 0x0020_0000 | WS_TABSTOP, x, y, w, 240, id, font);
        for it in items {
            SendMessageW(h as HWND, 0x143 /*CB_ADDSTRING*/, 0, wide(it).as_ptr() as isize);
        }
        h
    };
    // ---------- ページ0: 接続(Mac と同じ項目順: 方向→状態→接続先) ----------
    section(0, "接続の方向", 190);
    ROLE_CLIENT_RADIO.store(
        make(Some(0), "BUTTON", "相手の端末がホスト(既定) — このPCが接続しに行きます", 0x9 | WS_TABSTOP | 0x0002_0000 /*WS_GROUP*/, 36, 216, 648, 24, MENU_ROLE_CLIENT, font),
        Ordering::Relaxed,
    );
    ROLE_HOST_RADIO.store(
        make(Some(0), "BUTTON", "このPCがホスト — 相手の端末が接続しに来ます", 0x9, 36, 242, 648, 24, MENU_ROLE_HOST, font),
        Ordering::Relaxed,
    );
    ROLE_NOTE.store(note(0, "", 36, 270, 648, ID_NOTE), Ordering::Relaxed);
    note(0, "接続状態", 60, 312, 200, ID_STATE_HEAD);
    LABEL_STATE.store(
        note(0, "接続を確認しています…", 60, 338, 636, ID_STATE_LINE),
        Ordering::Relaxed,
    );
    OWN_IP.store(note(0, "", 60, 376, 636, ID_OWN_IP), Ordering::Relaxed);
    section(0, "接続先の端末", 452);
    let host = HOST_NOW.lock().unwrap_or_else(|e| e.into_inner()).clone();
    EDIT_HOST.store(
        make(
            Some(0),
            "EDIT",
            &host,
            0x0080 | 0x00800000 | WS_TABSTOP,
            36,
            482,
            400,
            30,
            230,
            font,
        ),
        Ordering::Relaxed,
    );
    // IP アドレスの直接指定は 511 文字まで(IPv6 をカンマ区切りで複数書ける長さ。
    // 形式の検証は保存時の validate_host_input が担う)
    SendMessageW(EDIT_HOST.load(Ordering::Relaxed) as HWND, 0x00C5 /*EM_SETLIMITTEXT*/, 511, 0);
    SAVEHOST_BUTTON.store(btn(0, "保存して再接続", 448, 478, 236, MENU_SAVEHOST), Ordering::Relaxed);
    HOST_HINT.store(note(0, "", 36, 522, 648, ID_NOTE), Ordering::Relaxed);
    // ---------- ページ1: 画面配置(ドラッグで置く。Mac の配置キャンバスと同じ操作) ----------
    // キャンバス本体は描画だけなので子コントロールは注記と現在値行のみ
    note(1, "グレー:相手/ブルー:このPC。ドラッグで配置(斜めも可)", 36, 588, 648, ID_NOTE);
    LAYOUT_VALUES.store(note(1, "", 36, 614, 648, ID_LAYOUT_VALUE), Ordering::Relaxed);
    // ---------- ページ2: 切替(Mac の「切替」と同じ2項目の遠隔設定) ----------
    section(2, "切替キー", 196);
    HOTKEY_COMBO.store(
        combo(2, &["F6(必要に応じてfnと併用)", "F8(必要に応じてfnと併用)", "F13(既定)", "右⌘"], 280, 190, 404, ID_HOTKEY_COMBO),
        Ordering::Relaxed,
    );
    section(2, "滞在時間(短い↔長い)", 252);
    DELAY_TRACK.store(
        make(Some(2), "msctls_trackbar32", "", WS_TABSTOP, 280, 246, 320, 34, ID_DELAY_TRACK, font),
        Ordering::Relaxed,
    );
    // 50-1000ms(Mac のスライダと同じ範囲)
    SendMessageW(DELAY_TRACK.load(Ordering::Relaxed) as HWND, 0x406 /*TBM_SETRANGE*/, 1, (50 | (1000 << 16)) as isize);
    DELAY_VALUE.store(note(2, "", 612, 252, 96, ID_DELAY_VALUE), Ordering::Relaxed);
    note(2, "切替キーは、どの方式でも使えます。", 36, 316, 648, ID_NOTE);
    SWITCH_OFFLINE_NOTE.store(note(2, "", 36, 344, 648, ID_SWITCH_NOTE), Ordering::Relaxed);
    // ---------- ページ3: 共有(Mac と同じ分類: 渡すもの/音声/履歴+相手側の設定) ----------
    section(3, "渡すもの", 190);
    CHK_CLIP.store(make(Some(3), "BUTTON", "テキストと画像", 0x3 | WS_TABSTOP | 0x0002_0000, 36, 216, 340, 24, MENU_SHARE_CLIP, font), Ordering::Relaxed);
    CHK_FILES.store(make(Some(3), "BUTTON", "ファイルの受け渡し", 0x3, 36, 244, 340, 24, MENU_SHARE_FILES, font), Ordering::Relaxed);
    section(3, "音声", 288);
    CHK_AUDIO.store(make(Some(3), "BUTTON", "このPCの音声を相手側で再生", 0x3, 36, 314, 440, 24, MENU_AUDIO, font), Ordering::Relaxed);
    section(3, "履歴", 356);
    note(3, "メニューのクリップボード履歴から選び直せます", 36, 382, 648, ID_NOTE);
    note(3, "履歴は平文で保存されます。残したくない場合は「履歴をすべて消す」で消せます", 36, 406, 648, ID_NOTE);
    section(3, "相手側の設定", 448);
    MAC_CLIP_BUTTON.store(btn(3, "", 36, 476, 310, MENU_MAC_CLIP), Ordering::Relaxed);
    MAC_FILES_BUTTON.store(btn(3, "", 380, 476, 304, MENU_MAC_FILES), Ordering::Relaxed);
    MAC_HISTORY_BUTTON.store(btn(3, "", 36, 512, 310, MENU_MAC_HISTORY), Ordering::Relaxed);
    MAC_AUDIO_BUTTON.store(btn(3, "", 380, 512, 304, MENU_MAC_AUDIO), Ordering::Relaxed);
    MAC_SPK_BUTTON.store(btn(3, "", 36, 548, 310, MENU_MAC_SPK), Ordering::Relaxed);
    note(3, "相手側の設定は、接続中のみ変更できます。", 36, 588, 648, ID_NOTE);
    // ---------- ページ4: その他(Mac と同じ構成: 表示言語/メンテナンス/初期化) ----------
    section(4, "表示言語", 190);
    note(4, "日本語(Windows版は日本語のみです)", 280, 190, 404, ID_NOTE);
    section(4, "メンテナンス", 244);
    btn(4, "Knitを再起動", 36, 272, 200, MENU_RESTART);
    btn(4, "ログ", 248, 272, 150, MENU_OPENLOG);
    btn(4, "診断", 412, 272, 150, MENU_DIAGNOSE);
    section(4, "初期化", 324);
    make(Some(4), "STATIC", "端末の登録", 0, 36, 356, 240, 26, ID_SECTION, font);
    btn(4, "すべての登録を初期化…", 448, 350, 236, MENU_RESETREG);
    make(Some(4), "STATIC", "すべての設定", 0, 36, 408, 240, 26, ID_SECTION, font);
    btn(4, "すべての設定を初期化…", 448, 402, 236, MENU_RESETSETTINGS);
    note(4, "初期化すると全端末が締め出され、再登録が必要です(取り消せません)。", 36, 448, 648, ID_NOTE);
    // ---------- ロック解除の連動(版 16 以降の Mac との解除連動。既定 OFF) ----------
    section(4, "ロック解除の連動", 484);
    CHK_UNLOCK.store(
        make(Some(4), "BUTTON", "Mac のロック解除に合わせてこの PC も解除する", 0x3 | WS_TABSTOP | 0x0002_0000, 36, 510, 640, 24, MENU_UNLOCK_SYNC, font),
        Ordering::Relaxed,
    );
    UNLOCK_SECRET_STATE.store(note(4, "", 36, 546, 110, ID_UNLOCK_STATE), Ordering::Relaxed);
    EDIT_UNLOCK_SECRET.store(
        make(
            Some(4),
            "EDIT",
            "",
            0x0080 | 0x00800000 | 0x20 /*ES_PASSWORD(マスク表示)*/ | WS_TABSTOP,
            150,
            542,
            286,
            30,
            235,
            font,
        ),
        Ordering::Relaxed,
    );
    // 入力は 1 行・長さ上限(preferences の平文上限と同じ)
    SendMessageW(
        EDIT_UNLOCK_SECRET.load(Ordering::Relaxed) as HWND,
        0x00C5 /*EM_SETLIMITTEXT*/,
        crate::tray::preferences::MAX_UNLOCK_SECRET_CHARS,
        0,
    );
    btn(4, "保存", 448, 538, 150, MENU_UNLOCK_SAVE);
    note(4, "Macのロック解除に合わせてこのPCも解除する。パスワードはこのPC内に暗号化保存。", 36, 582, 648, ID_NOTE);
    note(4, "操作補助(UAC・管理者アプリの操作)の導入が必要です。", 36, 606, 648, ID_NOTE);
    let args: Vec<String> = std::env::args().collect();
    let page = if UI_PREVIEW.load(Ordering::Relaxed) {
        args.iter()
            .position(|a| a == "--preview-page")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(PAGE_COUNT - 1)
    } else {
        0
    };
    select(page);
    sync();
    bring_to_front(hwnd);
}

/// 画面配置ページのマウス操作(ドラッグでこのPCの四角を動かす)。
/// wndproc の WM_LBUTTONDOWN/WM_MOUSEMOVE/WM_LBUTTONUP から呼ぶ。
/// true=処理した(メッセージをこれ以上流さない)
pub(super) unsafe fn layout_mouse(hwnd: HWND, msg: u32, lp: LPARAM) -> bool {
    if PAGE.load(Ordering::Relaxed) != 1 {
        LAY_DRAGGING.store(false, Ordering::Relaxed);
        return false;
    }
    let x = (lp & 0xFFFF) as u16 as i32;
    let y = ((lp >> 16) & 0xFFFF) as u16 as i32;
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    match msg {
        0x0201 /*WM_LBUTTONDOWN*/ => {
            // ドラッグは接続中のみ(相手の設定を変えるため)。このPCの四角の内側で開始
            if !connected || UI_PREVIEW.load(Ordering::Relaxed) {
                return false;
            }
            let (bx, by) = box_pos(crate::SIDE_W.load(Ordering::Relaxed) as u8);
            if x >= bx && x < bx + BOX_W && y >= by && y < by + BOX_H {
                LAY_DRAGGING.store(true, Ordering::Relaxed);
                LAY_DRAG_X.store(x - BOX_W / 2, Ordering::Relaxed);
                LAY_DRAG_Y.store(y - BOX_H / 2, Ordering::Relaxed);
                InvalidateRect(hwnd, std::ptr::null(), 0);
                true
            } else {
                false
            }
        }
        0x0200 /*WM_MOUSEMOVE*/ if LAY_DRAGGING.load(Ordering::Relaxed) => {
            LAY_DRAG_X.store(x - BOX_W / 2, Ordering::Relaxed);
            LAY_DRAG_Y.store(y - BOX_H / 2, Ordering::Relaxed);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            true
        }
        0x0202 /*WM_LBUTTONUP*/ if LAY_DRAGGING.swap(false, Ordering::Relaxed) => {
            // 中心からの方向で side を定めて相手へ適用。中央に重ねた場合は元のまま
            let cx = LAY_DRAG_X.load(Ordering::Relaxed) + BOX_W / 2;
            let cy = LAY_DRAG_Y.load(Ordering::Relaxed) + BOX_H / 2;
            if let Some(side) = side_from_center(cx, cy) {
                crate::SIDE_W.store(side, Ordering::Relaxed);
                crate::state::set_mac_pref("side", serde_json::json!(side));
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
            true
        }
        _ => false,
    }
}

pub(super) unsafe fn paint_layout(hdc: *mut core::ffi::c_void) {
    if PAGE.load(Ordering::Relaxed) != 1 {
        return;
    }
    let t = theme();
    let (cx, cy) = lay_center();
    // キャンバスの面(Mac の NSBox と同じ「まとまりだけを枠で示す」役割)
    let brush = CreateSolidBrush(rgb(t.card));
    let pen = CreatePen(0, 1, rgb(t.edge));
    let ob = SelectObject(hdc, brush);
    let op = SelectObject(hdc, pen);
    RoundRect(
        hdc,
        LAY_AREA.0,
        LAY_AREA.1,
        LAY_AREA.0 + LAY_AREA.2,
        LAY_AREA.1 + LAY_AREA.3,
        12,
        12,
    );
    SelectObject(hdc, ob);
    SelectObject(hdc, op);
    DeleteObject(brush);
    DeleteObject(pen);
    let mut old_font: *mut core::ffi::c_void = std::ptr::null_mut();
    let font = DRAW_FONT.load(Ordering::Relaxed) as *mut core::ffi::c_void;
    if !font.is_null() {
        old_font = SelectObject(hdc, font);
    }
    // 相手(グレー)とこのPC(アクセント色)の四角。ドラッグ中はこのPCの四角が追従する
    let (mx, my) = (cx - BOX_W / 2, cy - BOX_H / 2);
    let (wx, wy) = if LAY_DRAGGING.load(Ordering::Relaxed) {
        (LAY_DRAG_X.load(Ordering::Relaxed), LAY_DRAG_Y.load(Ordering::Relaxed))
    } else {
        box_pos(crate::SIDE_W.load(Ordering::Relaxed) as u8)
    };
    let mut peer: Vec<char> = crate::conn::peer_display().chars().collect();
    if peer.len() > 8 {
        peer.truncate(8);
        peer.push('…');
    }
    let peer_label: String = peer.into_iter().collect();
    // 相手とこのPCを結ぶ線(どちらの画面とつながっているかの視覚的な導線)
    {
        use windows_sys::Win32::Graphics::Gdi::{LineTo, MoveToEx};
        let pen = CreatePen(0, 1, rgb(t.diagram_line));
        let old = SelectObject(hdc, pen);
        MoveToEx(hdc, mx + BOX_W / 2, my + BOX_H / 2, std::ptr::null_mut());
        LineTo(hdc, wx + BOX_W / 2, wy + BOX_H / 2);
        SelectObject(hdc, old);
        DeleteObject(pen);
    }
    for (x, y, text, color, fill) in [
        (mx, my, peer_label, t.diagram_mac, t.diagram_fill),
        (wx, wy, "このPC".to_string(), t.accent, t.diagram_fill),
    ] {
        let brush = CreateSolidBrush(rgb(fill));
        let pen = CreatePen(0, 2, rgb(color));
        let ob = SelectObject(hdc, brush);
        let op = SelectObject(hdc, pen);
        RoundRect(hdc, x, y, x + BOX_W, y + BOX_H, 12, 12);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(brush);
        DeleteObject(pen);
        SetTextColor(hdc, rgb(t.text));
        SetBkMode(hdc, TRANSPARENT_BK);
        let mut rect = Rect {
            left: x,
            top: y,
            right: x + BOX_W,
            bottom: y + BOX_H,
        };
        let mut text = wide(&text);
        DrawTextW(
            hdc,
            text.as_mut_ptr(),
            -1,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
    if !old_font.is_null() {
        SelectObject(hdc, old_font);
    }
}

pub(super) unsafe fn paint_groups(hdc: *mut core::ffi::c_void) {
    // ページ0の状態カードだけ(Mac と同じ「状態のまとまりだけを枠で示す」)。
    // ページ1のキャンバス面は paint_layout が描く
    if PAGE.load(Ordering::Relaxed) != 0 {
        return;
    }
    let brush = CreateSolidBrush(rgb(theme().card));
    let pen = CreatePen(0, 1, rgb(theme().edge));
    let ob = SelectObject(hdc, brush);
    let op = SelectObject(hdc, pen);
    RoundRect(hdc, 36, 300, 36 + 648, 300 + 130, 12, 12);
    SelectObject(hdc, ob);
    SelectObject(hdc, op);
    DeleteObject(brush);
    DeleteObject(pen);
}

// ---------- Mac の設定を Windows から変える ----------
fn pref_bool(key: &str) -> Option<bool> {
    crate::state::mac_pref(key).and_then(|v| v.as_bool())
}

pub(super) unsafe fn combo_changed(id: u32, sel: usize) {
    use crate::state::set_mac_pref;
    use serde_json::json;
    if id == ID_HOTKEY_COMBO {
        // 候補は Mac 側と共通(F6/F8/F13/右⌘)。末尾の「現在のキー」項目(候補外)は
        // 選んも変更しない(候補 .get の範囲外のため自然に無視される)
        if let Some(k) = [97, 100, 105, 54].get(sel) {
            set_mac_pref("hotkey", json!(k));
        }
    }
}

/// 滞在時間スライダ(50-1000ms・Mac の設定「切替」と同じ経路で遠隔適用)。
/// ドラッグ中は 100ms に 1 回だけ送る(Mac へ連打しない)
pub(super) unsafe fn delay_changed(wparam: usize, track: HWND) {
    if track as usize != DELAY_TRACK.load(Ordering::Relaxed) {
        return;
    }
    const TB_ENDTRACK: usize = 8;
    const TB_THUMBTRACK: usize = 5;
    let code = wparam & 0xFFFF;
    let pos = SendMessageW(track, 0x400 /*TBM_GETPOS*/, 0, 0) as i64;
    TRACK_DRAGGING.store(code == TB_THUMBTRACK, Ordering::Relaxed);
    if code == TB_THUMBTRACK {
        static LAST: AtomicUsize = AtomicUsize::new(0);
        let now = crate::state::now_ms() as usize;
        if now.saturating_sub(LAST.load(Ordering::Relaxed)) < 100 {
            return;
        }
        LAST.store(now, Ordering::Relaxed);
    }
    if code == TB_ENDTRACK || code <= 5 {
        crate::state::set_mac_pref("delay", serde_json::json!(pos.clamp(50, 1000)));
    }
}

fn toggle_text(on: bool, label: &str) -> String {
    format!("{label}: {}", if on { "オン" } else { "オフ" })
}

/// 滞在時間の値表示(Mac の delay_text と同じ: 既定 300ms にだけ目印)
fn delay_text(ms: i64) -> String {
    if ms == 300 {
        "300ms(既定)".to_string()
    } else {
        format!("{ms}ms")
    }
}

pub(super) unsafe fn sync_mac_prefs() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    // 設定画面が見えている間だけ、3 秒おきに Mac の最新の設定を取りに行く
    let hwnd = STATUS_HWND.load(Ordering::Relaxed) as HWND;
    if connected && !hwnd.is_null() && windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) != 0 {
        static LAST_REQ: AtomicUsize = AtomicUsize::new(0);
        let now = crate::state::now_ms() as usize;
        if now.saturating_sub(LAST_REQ.load(Ordering::Relaxed)) >= 3_000 {
            LAST_REQ.store(now, Ordering::Relaxed);
            crate::state::request_mac_prefs();
        }
    }
    let have = connected && crate::state::MAC_PREFS.lock().unwrap_or_else(|e| e.into_inner()).is_some();
    for slot in [&HOTKEY_COMBO, &DELAY_TRACK, &MAC_CLIP_BUTTON, &MAC_FILES_BUTTON, &MAC_HISTORY_BUTTON, &MAC_AUDIO_BUTTON, &MAC_SPK_BUTTON] {
        let h = slot.load(Ordering::Relaxed);
        if h != 0 {
            EnableWindow(h as HWND, have as i32);
        }
    }
    let select = |slot: &AtomicUsize, idx: Option<usize>| {
        let h = slot.load(Ordering::Relaxed) as HWND;
        if h.is_null() {
            return;
        }
        let want = idx.map(|i| i as isize).unwrap_or(-1);
        if SendMessageW(h, 0x147, 0, 0) != want {
            SendMessageW(h, 0x14E /*CB_SETCURSEL*/, want as usize, 0);
        }
    };
    let prefs = |k: &str| crate::state::mac_pref(k);
    let kc = prefs("hotkey").and_then(|v| v.as_i64());
    // 切替キー: 既知の候補(F6/F8/F13/右⌘)はその位置を選ぶ。Mac 側で env 等により
    // 候補外のキーが設定されているときは末尾へ「現在のキー(コードN)」項目を足して
    // 選ぶ(空選択だと実際のキーと見た目が食い違うため)。候補に戻れば項目を外す
    {
        let h = HOTKEY_COMBO.load(Ordering::Relaxed) as HWND;
        if !h.is_null() {
            let current = kc.unwrap_or(105);
            match [97, 100, 105, 54].iter().position(|x| *x == current) {
                Some(i) => {
                    if HOTKEY_CUSTOM_KC.load(Ordering::Relaxed) != -1 {
                        if SendMessageW(h, 0x146 /*CB_GETCOUNT*/, 0, 0) == 5 {
                            SendMessageW(h, 0x144 /*CB_DELETESTRING*/, 4, 0);
                        }
                        HOTKEY_CUSTOM_KC.store(-1, Ordering::Relaxed);
                    }
                    select(&HOTKEY_COMBO, Some(i));
                }
                None => {
                    if HOTKEY_CUSTOM_KC.load(Ordering::Relaxed) != current {
                        if SendMessageW(h, 0x146 /*CB_GETCOUNT*/, 0, 0) == 5 {
                            SendMessageW(h, 0x144 /*CB_DELETESTRING*/, 4, 0);
                        }
                        let text = wide(&format!("現在のキー(コード{current})"));
                        SendMessageW(h, 0x143 /*CB_ADDSTRING*/, 0, text.as_ptr() as isize);
                        HOTKEY_CUSTOM_KC.store(current, Ordering::Relaxed);
                    }
                    select(&HOTKEY_COMBO, Some(4));
                }
            }
        }
    }
    // 滞在時間トラックバー(Mac の delay と同じ範囲 50-1000ms)
    let track = DELAY_TRACK.load(Ordering::Relaxed) as HWND;
    let delay_ms = prefs("delay").and_then(|v| v.as_i64()).unwrap_or(300);
    if !track.is_null() && !TRACK_DRAGGING.load(Ordering::Relaxed) {
        if SendMessageW(track, 0x400, 0, 0) != delay_ms as isize {
            SendMessageW(track, 0x405 /*TBM_SETPOS*/, 1, delay_ms.clamp(50, 1000) as isize);
        }
    }
    set_text(DELAY_VALUE.load(Ordering::Relaxed), &delay_text(delay_ms));
    let state = |key: &str, label: &str, invert: bool| -> String {
        match pref_bool(key) {
            Some(b) => toggle_text(b != invert, label),
            None => format!("{label}: —"),
        }
    };
    set_text(MAC_CLIP_BUTTON.load(Ordering::Relaxed), &state("clip_share", "テキストと画像", false));
    set_text(MAC_FILES_BUTTON.load(Ordering::Relaxed), &state("share_files", "ファイル", false));
    set_text(MAC_HISTORY_BUTTON.load(Ordering::Relaxed), &state("local_history", "コピーの履歴", false));
    set_text(MAC_AUDIO_BUTTON.load(Ordering::Relaxed), &state("audio_muted", "相手側で音声を再生", true));
    set_text(MAC_SPK_BUTTON.load(Ordering::Relaxed), &state("spk_mute", "相手のスピーカーをミュート", false));
}
