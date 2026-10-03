//! 両OSで同じ分類を使う。Windowsから変更できる項目だけ操作部品として出す。
use super::*;
use std::sync::Mutex;
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRect, SendMessageW, WS_CAPTION, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP,
};
pub(super) const NAV_FIRST: u32 = 3000;
pub(super) static PAGE: AtomicUsize = AtomicUsize::new(0);
static CONTROLS: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
static AUDIO_BUTTON: AtomicUsize = AtomicUsize::new(0);
static SHARE_CLIP_BUTTON: AtomicUsize = AtomicUsize::new(0);
static SHARE_FILES_BUTTON: AtomicUsize = AtomicUsize::new(0);
static BACK_BUTTON: AtomicUsize = AtomicUsize::new(0);
static ROLE_CLIENT_RADIO: AtomicUsize = AtomicUsize::new(0);
static ROLE_HOST_RADIO: AtomicUsize = AtomicUsize::new(0);
static HINT: AtomicUsize = AtomicUsize::new(0);
static DRAW_FONT: AtomicUsize = AtomicUsize::new(0);
static INPUT_HINT: AtomicUsize = AtomicUsize::new(0);
// Mac の設定を Windows から変える部品(接続中のみ有効)
pub(super) static SIDE_COMBO: AtomicUsize = AtomicUsize::new(0);
static SIDE_RESET_BUTTON: AtomicUsize = AtomicUsize::new(0);
pub(super) static METHOD_COMBO: AtomicUsize = AtomicUsize::new(0);
pub(super) static HOTKEY_COMBO: AtomicUsize = AtomicUsize::new(0);
static FLIP_BUTTON: AtomicUsize = AtomicUsize::new(0);
pub(super) static SCROLL_TRACK: AtomicUsize = AtomicUsize::new(0);
static NAV_BUTTON: AtomicUsize = AtomicUsize::new(0);
static PINCH_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_CLIP_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_FILES_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_HISTORY_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_AUDIO_BUTTON: AtomicUsize = AtomicUsize::new(0);
static MAC_SPK_BUTTON: AtomicUsize = AtomicUsize::new(0);
pub(super) static TRACK_DRAGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) unsafe fn select(index: usize) {
    PAGE.store(index.min(3), Ordering::Relaxed);
    for &(h, page) in CONTROLS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        ShowWindow(h as HWND, if page == index { SW_SHOW } else { SW_HIDE });
    }
    InvalidateRect(
        STATUS_HWND.load(Ordering::Relaxed) as HWND,
        std::ptr::null(),
        1,
    );
}
pub(super) unsafe fn sync() {
    sync_mac_prefs();
    let audio_allowed = knit_common::share::env_cap().audio;
    set_text(
        AUDIO_BUTTON.load(Ordering::Relaxed),
        if !audio_allowed {
            "環境変数 KNIT_SHARE で制限中"
        } else if crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed) {
            "音声転送をオフにする"
        } else {
            "音声転送をオンにする"
        },
    );
    let audio_button = AUDIO_BUTTON.load(Ordering::Relaxed);
    if audio_button != 0 {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
            audio_button as HWND,
            audio_allowed as i32,
        );
    }
    // 共有範囲(このPCが相手へ渡す内容)。KNIT_SHARE で禁じられた項目は押せない
    let cap = knit_common::share::env_cap();
    for (slot, allowed, on) in [
        (&SHARE_CLIP_BUTTON, cap.clip, knit_common::share::user_clip()),
        (&SHARE_FILES_BUTTON, cap.files, knit_common::share::user_files()),
    ] {
        let h = slot.load(Ordering::Relaxed);
        set_text(
            h,
            if !allowed {
                "環境変数 KNIT_SHARE で制限中"
            } else if on {
                "共有中(押すと停止)"
            } else {
                "停止中(押すと共有)"
            },
        );
        if h != 0 {
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h as HWND, allowed as i32);
        }
    }
    set_text(
        HINT.load(Ordering::Relaxed),
        if crate::CONNECTED.load(Ordering::Relaxed) {
            "接続できています。MacからこのWindowsを操作できます。"
        } else {
            "MacでKnitを開き、同じネットワークまたは直結であることを確認。"
        },
    );
    static LAST_SIDE: AtomicUsize = AtomicUsize::new(usize::MAX);
    let side = crate::SIDE_W.load(Ordering::Relaxed) as usize;
    if LAST_SIDE.swap(side, Ordering::Relaxed) != side {
        let hwnd = STATUS_HWND.load(Ordering::Relaxed) as HWND;
        if !hwnd.is_null() {
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }
    // 接続の方向の選択表示
    let host = crate::tray::HOST_MODE.load(Ordering::Relaxed);
    for (slot, on) in [(&ROLE_CLIENT_RADIO, !host), (&ROLE_HOST_RADIO, host)] {
        let h = slot.load(Ordering::Relaxed) as HWND;
        if !h.is_null() && (SendMessageW(h, 0xF2 /*BM_GETCHECK*/, 0, 0) != 0) != on {
            SendMessageW(h, 0xF1 /*BM_SETCHECK*/, on as usize, 0);
        }
    }
    // 「Macへ戻る」は接続中だけ押せる(未接続では相手に届かない)
    let back = BACK_BUTTON.load(Ordering::Relaxed);
    if back != 0 {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
            back as HWND,
            crate::CONNECTED.load(Ordering::Relaxed) as i32,
        );
    }
    set_text(
        INPUT_HINT.load(Ordering::Relaxed),
        &format!(
            "⌘キーの割当：{}",
            if crate::CMD_ALT.load(Ordering::Relaxed) {
                "Alt"
            } else {
                "Ctrl"
            }
        ),
    );
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
    let mut r = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 820,
        bottom: 740,
    };
    AdjustWindowRect(&mut r, style, 0);
    let hwnd = CreateWindowExW(
        0x00010000, /*WS_EX_CONTROLPARENT*/
        class.as_ptr(),
        wide("Knit 設定").as_ptr(),
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
    DRAW_FONT.store(font as usize, Ordering::Relaxed);
    let small = segoe_font(false, 14);
    let heading = segoe_font(true, 25);
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
    make(
        None,
        "STATIC",
        "Knit",
        0,
        68,
        32,
        114,
        32,
        220,
        segoe_font(true, 22),
    );
    make(
        None,
        "STATIC",
        "机を、ひとつの手元で。",
        0,
        20,
        83,
        150,
        20,
        221,
        small,
    );
    for (i, title) in ["接続", "画面配置", "操作", "共有"].iter().enumerate() {
        make(
            None,
            "BUTTON",
            title,
            0xB | WS_TABSTOP,
            16,
            160 + i as i32 * 48,
            140,
            36,
            NAV_FIRST + i as u32,
            font,
        );
    }
    make(
        None,
        "STATIC",
        &format!("バージョン {}", crate::VERSION_STR),
        0,
        20,
        722,
        150,
        20,
        221,
        small,
    );
    // サブタイトルは Mac 設定画面と共通の文言に揃える(同じページ構成のため)
    let titles = [
        ("接続", "つながっている端末と、その状態です。"),
        ("画面配置", "接続先の位置を、実際の画面配置に合わせます。"),
        ("操作", "画面を移る方法と、スクロールの感触です。"),
        ("共有", "このPCが渡すものと、受け取るものを選びます。"),
    ];
    for (page, (title, sub)) in titles.iter().enumerate() {
        make(
            Some(page),
            "STATIC",
            title,
            0,
            212,
            32,
            560,
            36,
            223,
            heading,
        );
        make(Some(page), "STATIC", sub, 0, 212, 76, 560, 24, 222, small);
    }
    let label =
        |p, text: &str, x, y, w, id| make(Some(p), "STATIC", text, 0x200, x, y, w, 26, id, font);
    let note =
        |p, text: &str, x, y, w| make(Some(p), "STATIC", text, 0x200, x, y, w, 32, ID_LBL_BUILD, small);
    let btn = |p, text: &str, x, y, w, id| {
        make(
            Some(p),
            "BUTTON",
            text,
            0xB | WS_TABSTOP,
            x,
            y,
            w,
            36,
            id,
            font,
        )
    };
    let combo = |p, items: &[&str], x, y, w, id| {
        let h = make(Some(p), "COMBOBOX", "", 0x0003 | 0x0020_0000 | WS_TABSTOP, x, y, w, 240, id, font);
        for it in items {
            SendMessageW(h as HWND, 0x143 /*CB_ADDSTRING*/, 0, wide(it).as_ptr() as isize);
        }
        h
    };
    // Connection is a device relationship, followed by a separate editable endpoint.
    LABEL_STATE.store(
        label(0, &tray_status_text(), 236, 234, 520, ID_LBL_STATE),
        Ordering::Relaxed,
    );
    HINT.store(note(0, "", 236, 268, 520), Ordering::Relaxed);
    label(0, "接続先", 236, 350, 200, ID_HEAD_ACT);
    note(0, "MacのIPアドレス", 236, 380, 200);
    let host = HOST_NOW.lock().unwrap_or_else(|e| e.into_inner()).clone();
    EDIT_HOST.store(
        make(
            Some(0),
            "EDIT",
            &host,
            0x0080 | 0x00800000 | WS_TABSTOP,
            236,
            426,
            310,
            30,
            230,
            font,
        ),
        Ordering::Relaxed,
    );
    btn(0, "保存して再接続", 562, 424, 190, MENU_SAVEHOST);
    // 接続の方向: このPCが待ち受ける(ホスト)か、Mac へ接続しに行くか。再起動で反映
    // 役割は 2 択の選択(ラジオ)。選ぶと相手にも伝えて両方が再起動する
    let radio = |text: &str, y, id| make(Some(0), "BUTTON", text, 0x9 | WS_TABSTOP, 236, y, 516, 26, id, font);
    ROLE_CLIENT_RADIO.store(
        radio("Macがホスト: このPCがMacへ接続しに行く(既定)", 462, MENU_ROLE_CLIENT),
        Ordering::Relaxed,
    );
    ROLE_HOST_RADIO.store(
        radio("このPCがホスト: Macがこのパソコンへ接続しに来る", 490, MENU_ROLE_HOST),
        Ordering::Relaxed,
    );
    LABEL_RTT.store(
        label(0, &rtt_line(), 236, 561, 150, ID_LBL_RTT),
        Ordering::Relaxed,
    );
    btn(0, "登録情報", 236, 596, 150, MENU_REGISTER);
    btn(0, "ログ", 392, 596, 130, MENU_OPENLOG);
    btn(0, "診断", 528, 596, 100, MENU_DIAGNOSE);
    btn(0, "再起動", 634, 596, 100, MENU_RESTART);
    // 画面配置: Mac の設定を Windows から変える(Mac から見たこのPCの位置)
    label(1, "このPCの位置(Macから見て)", 236, 424, 250, ID_HEAD_ACT);
    SIDE_COMBO.store(
        combo(1, &["右", "左", "上", "下", "右上", "右下", "左上", "左下"], 492, 420, 260, ID_SIDE_COMBO),
        Ordering::Relaxed,
    );
    label(1, "配置を初期状態に戻す", 236, 472, 250, ID_HEAD_ACT);
    SIDE_RESET_BUTTON.store(btn(1, "既定(右)に戻す", 492, 468, 260, MENU_SIDE_RESET), Ordering::Relaxed);
    label(2, "キーボード", 236, 132, 520, ID_HEAD_ACT);
    label(2, "修飾キー", 236, 166, 180, ID_HEAD_ACT);
    INPUT_HINT.store(label(2, "", 494, 166, 258, ID_HEAD_ACT), Ordering::Relaxed);
    label(2, "コピー / 貼り付け", 236, 206, 240, ID_HEAD_ACT);
    label(2, "⌘ C     /     ⌘ V", 494, 206, 258, ID_HEAD_ACT);
    label(2, "日本語入力", 236, 246, 220, ID_HEAD_ACT);
    label(2, "かな / 英数", 494, 246, 258, ID_HEAD_ACT);
    // Mac の設定を Windows から変える(接続中のみ)
    label(2, "切替方式", 236, 318, 220, ID_HEAD_ACT);
    METHOD_COMBO.store(
        combo(2, &["端に2回触れる", "端で少し待つ", "ショートカットのみ", "端に1回触れる"], 440, 314, 312, ID_METHOD_COMBO),
        Ordering::Relaxed,
    );
    label(2, "切替キー", 236, 362, 220, ID_HEAD_ACT);
    HOTKEY_COMBO.store(
        combo(2, &["F6（必要に応じてfnと併用）", "F8（必要に応じてfnと併用）", "F13"], 440, 358, 312, ID_HOTKEY_COMBO),
        Ordering::Relaxed,
    );
    label(2, "スクロール方向", 236, 406, 220, ID_HEAD_ACT);
    FLIP_BUTTON.store(btn(2, "", 440, 402, 312, MENU_SCROLL_FLIP), Ordering::Relaxed);
    label(2, "スクロール速度", 236, 450, 200, ID_HEAD_ACT);
    SCROLL_TRACK.store(
        make(Some(2), "msctls_trackbar32", "", WS_TABSTOP, 440, 448, 312, 34, ID_SCROLL_TRACK, font),
        Ordering::Relaxed,
    );
    SendMessageW(SCROLL_TRACK.load(Ordering::Relaxed) as HWND, 0x406, 1, (20 | (240 << 16)) as isize);
    label(2, "タブレットのナビゲーション", 236, 494, 200, ID_HEAD_ACT);
    NAV_BUTTON.store(btn(2, "", 440, 490, 312, MENU_PAD_NAV), Ordering::Relaxed);
    label(2, "ピンチで拡大・縮小", 236, 538, 200, ID_HEAD_ACT);
    PINCH_BUTTON.store(btn(2, "", 440, 534, 312, MENU_PAD_PINCH), Ordering::Relaxed);
    label(2, "操作するPC", 236, 612, 250, ID_HEAD_ACT);
    note(2, "このWindowsの操作を終え、Macに戻ります。", 236, 646, 320);
    BACK_BUTTON.store(btn(2, "Macへ戻る", 582, 608, 170, MENU_BACKMAC), Ordering::Relaxed);
    label(3, "音声", 236, 140, 300, ID_HEAD_ACT);
    LABEL_AUDIO.store(
        label(3, &audio_line(), 236, 172, 520, ID_LBL_AUDIO),
        Ordering::Relaxed,
    );
    LABEL_SPK.store(
        label(3, &spk_line(), 236, 206, 520, ID_LBL_SPK),
        Ordering::Relaxed,
    );
    AUDIO_BUTTON.store(
        btn(3, "音声転送をオフにする", 492, 240, 260, MENU_AUDIO),
        Ordering::Relaxed,
    );
    label(3, "このPCが共有する内容", 236, 328, 300, ID_HEAD_ACT);
    label(3, "テキストと画像", 236, 367, 240, ID_LBL_AUDIO);
    SHARE_CLIP_BUTTON.store(
        btn(3, "", 492, 362, 260, MENU_SHARE_CLIP),
        Ordering::Relaxed,
    );
    label(3, "ファイルの受け渡し", 236, 411, 240, ID_LBL_AUDIO);
    SHARE_FILES_BUTTON.store(
        btn(3, "", 492, 406, 260, MENU_SHARE_FILES),
        Ordering::Relaxed,
    );
    LABEL_FILES.store(
        label(3, &files_line(), 236, 457, 260, ID_LBL_FILES),
        Ordering::Relaxed,
    );
    btn(3, "受信フォルダを開く", 492, 452, 260, MENU_OPENFOLDER);
    note(
        3,
        "この設定は、Macの設定にかかわらず、このPCで常に優先されます。",
        236,
        496,
        520,
    );
    // 下のブロックは渡すもの(テキスト・ファイル)だけでなく、Mac 自身の記録
    //(コピーの履歴)や受け取る設定(Macで音声を再生)、相手への指示(スピーカー
    // ミュート)が混ざるため、「共有する内容」と言い切らず Mac 側の設定とする
    label(3, "Mac 側の設定", 236, 538, 300, ID_HEAD_ACT);
    MAC_CLIP_BUTTON.store(btn(3, "", 236, 574, 250, MENU_MAC_CLIP), Ordering::Relaxed);
    MAC_FILES_BUTTON.store(btn(3, "", 502, 574, 250, MENU_MAC_FILES), Ordering::Relaxed);
    MAC_HISTORY_BUTTON.store(btn(3, "", 236, 618, 250, MENU_MAC_HISTORY), Ordering::Relaxed);
    MAC_AUDIO_BUTTON.store(btn(3, "", 502, 618, 250, MENU_MAC_AUDIO), Ordering::Relaxed);
    // 「相手のスピーカーをミュート: オン」等のトグル文言が 250px を超えるため
    // この列だけ 260px にする(右隣は 502px 開始なので重ならない)
    MAC_SPK_BUTTON.store(btn(3, "", 236, 662, 260, MENU_MAC_SPK), Ordering::Relaxed);
    LABEL_FOOTER.store(
        make(
            None,
            "STATIC",
            &footer_line(),
            0,
            208,
            722,
            580,
            23,
            222,
            small,
        ),
        Ordering::Relaxed,
    );
    let args: Vec<String> = std::env::args().collect();
    let page = if UI_PREVIEW.load(Ordering::Relaxed) {
        args.iter()
            .position(|a| a == "--preview-page")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(3)
    } else {
        0
    };
    select(page);
    update_labels();
    bring_to_front(hwnd);
}

pub(super) unsafe fn paint_layout(hdc: *mut core::ffi::c_void) {
    let font = DRAW_FONT.load(Ordering::Relaxed) as *mut core::ffi::c_void;
    // 選択したフォントは必ず戻す(BeginPaint の DC は共有のため、選択が
    // DC キャッシュへ残留しないようにする)
    let mut old_font: *mut core::ffi::c_void = std::ptr::null_mut();
    if !font.is_null() {
        old_font = SelectObject(hdc, font);
    }
    static ICON: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let icon = *ICON.get_or_init(|| load_tray_icon_size(256) as usize);
    windows_sys::Win32::UI::WindowsAndMessaging::DrawIconEx(
        hdc,
        22,
        28,
        icon as _,
        36,
        36,
        0,
        std::ptr::null_mut(),
        3,
    );
    if PAGE.load(Ordering::Relaxed) != 1 {
        if !old_font.is_null() {
            SelectObject(hdc, old_font);
        }
        return;
    }
    let side = crate::SIDE_W.load(Ordering::Relaxed);
    let (mx, my, wx, wy) = match side {
        1 => (525, 216, 320, 216),
        2 => (420, 284, 420, 143),
        3 => (420, 143, 420, 284),
        4 => (320, 240, 525, 184),
        5 => (320, 184, 525, 240),
        6 => (525, 240, 320, 184),
        7 => (525, 184, 320, 240),
        _ => (320, 216, 525, 216),
    };
    let t = theme();
    for (x, y, text, color) in [(mx, my, "Mac", t.diagram_mac), (wx, wy, "Windows", t.accent)] {
        let brush = CreateSolidBrush(rgb(t.diagram_fill));
        let pen = CreatePen(0, 2, rgb(color));
        let ob = SelectObject(hdc, brush);
        let op = SelectObject(hdc, pen);
        RoundRect(hdc, x, y, x + 144, y + 88, 12, 12);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(brush);
        DeleteObject(pen);
        SetTextColor(hdc, rgb(t.text));
        SetBkMode(hdc, TRANSPARENT_BK);
        let mut rect = Rect {
            left: x,
            top: y,
            right: x + 144,
            bottom: y + 88,
        };
        let mut text = wide(text);
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

/// Segoe MDL2 Assets contains scalable OS glyphs; no pixel-coordinate icon drawing.
pub(super) unsafe fn nav_icon(hdc: *mut core::ffi::c_void, page: usize, selected: bool) {
    static FONT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let font = *FONT.get_or_init(|| {
        use windows_sys::Win32::Graphics::Gdi::CreateFontW;
        CreateFontW(
            -20,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            wide("Segoe MDL2 Assets").as_ptr(),
        ) as usize
    });
    let old = SelectObject(hdc, font as _);
    SetBkMode(hdc, TRANSPARENT_BK);
    let t = theme();
    SetTextColor(hdc, rgb(if selected { t.accent } else { t.text }));
    let glyphs = [0xE774u16, 0xE7F4, 0xE765, 0xE72D];
    let mut text = [glyphs[page.min(3)], 0];
    let mut rect = Rect {
        left: 12,
        top: 0,
        right: 34,
        bottom: 36,
    };
    DrawTextW(
        hdc,
        text.as_mut_ptr(),
        1,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(hdc, old);
}

unsafe fn card(hdc: *mut core::ffi::c_void, top: i32, bottom: i32) {
    let brush = CreateSolidBrush(rgb(theme().card));
    let pen = CreatePen(0, 1, rgb(theme().edge));
    let ob = SelectObject(hdc, brush);
    let op = SelectObject(hdc, pen);
    RoundRect(hdc, 212, top, 776, bottom, 18, 18);
    SelectObject(hdc, ob);
    SelectObject(hdc, op);
    DeleteObject(brush);
    DeleteObject(pen);
}
pub(super) unsafe fn paint_groups(hdc: *mut core::ffi::c_void) {
    let page = PAGE.load(Ordering::Relaxed);
    let groups: &[(i32, i32)] = match page {
        0 => &[(120, 318), (330, 524), (536, 622)],
        1 => &[(120, 392), (404, 520)],
        2 => &[(120, 290), (302, 596), (608, 700)],
        _ => &[(120, 290), (302, 530), (534, 708)],
    };
    for &(top, bottom) in groups {
        card(hdc, top, bottom);
    }
    if page == 2 {
        use windows_sys::Win32::Graphics::Gdi::{LineTo, MoveToEx};
        let pen = CreatePen(0, 1, rgb(theme().divider));
        let old = SelectObject(hdc, pen);
        for y in [196, 236] {
            MoveToEx(hdc, 236, y, std::ptr::null_mut());
            LineTo(hdc, 752, y);
        }
        SelectObject(hdc, old);
        DeleteObject(pen);
    }
    if page == 0 {
        let old = SelectObject(hdc, DRAW_FONT.load(Ordering::Relaxed) as _);
        SetBkMode(hdc, TRANSPARENT_BK);
        SetTextColor(hdc, rgb(theme().head));
        for (x, name) in [(264, "このWindows"), (574, "Mac")] {
            let mut text = wide(name);
            let mut r = Rect {
                left: x,
                top: 196,
                right: x + 150,
                bottom: 224,
            };
            DrawTextW(
                hdc,
                text.as_mut_ptr(),
                -1,
                &mut r,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        // Scalable display glyphs use the same outline family as navigation.
        use windows_sys::Win32::Graphics::Gdi::{CreateFontW, LineTo, MoveToEx};
        static DISPLAY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let f = *DISPLAY.get_or_init(|| {
            CreateFontW(
                -42,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                wide("Segoe MDL2 Assets").as_ptr(),
            ) as usize
        });
        SelectObject(hdc, f as _);
        SetTextColor(hdc, rgb(theme().accent));
        for x in [264, 574] {
            let mut t = [0xE7F4u16, 0];
            let mut r = Rect {
                left: x,
                top: 143,
                right: x + 150,
                bottom: 191,
            };
            DrawTextW(
                hdc,
                t.as_mut_ptr(),
                1,
                &mut r,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        let pen = CreatePen(0, 1, rgb(theme().diagram_line));
        let op = SelectObject(hdc, pen);
        MoveToEx(hdc, 418, 170, std::ptr::null_mut());
        LineTo(hdc, 568, 170);
        SelectObject(hdc, op);
        DeleteObject(pen);
        SelectObject(hdc, old);
    }
}

// ---------- Mac の設定を Windows から変える ----------
fn pref_bool(key: &str) -> Option<bool> {
    crate::state::mac_pref(key).and_then(|v| v.as_bool())
}

fn method_index() -> Option<usize> {
    let only = pref_bool("hotkey_only")?;
    let taps = crate::state::mac_pref("edge_taps").and_then(|v| v.as_u64()).unwrap_or(2);
    let delay = crate::state::mac_pref("delay").and_then(|v| v.as_u64()).unwrap_or(0);
    // Mac 側(method_index)と同じ優先順位: 待ち時間の指定が 1 回触れるより先
    Some(if only {
        2
    } else if delay > 0 {
        1
    } else if taps == 1 {
        3
    } else {
        0
    })
}

pub(super) unsafe fn combo_changed(id: u32, sel: usize) {
    use crate::state::set_mac_pref;
    use serde_json::json;
    match id {
        ID_SIDE_COMBO if sel <= 7 => set_mac_pref("side", json!(sel)),
        ID_METHOD_COMBO => {
            set_mac_pref("hotkey_only", json!(sel == 2));
            set_mac_pref("edge_taps", json!(if sel == 3 { 1 } else { 2 }));
            set_mac_pref("delay", json!(if sel == 1 { 300 } else { 0 }));
        }
        ID_HOTKEY_COMBO => {
            if let Some(k) = [97, 100, 105].get(sel) {
                set_mac_pref("hotkey", json!(k));
            }
        }
        _ => {}
    }
}

/// スクロール速度のスライダー。値は Mac と同じ向き(右ほど速い = 除数 260 - 位置)
pub(super) unsafe fn scroll_changed(wparam: usize, track: HWND) {
    if track as usize != SCROLL_TRACK.load(Ordering::Relaxed) {
        return;
    }
    const TB_ENDTRACK: usize = 8;
    const TB_THUMBTRACK: usize = 5;
    let code = wparam & 0xFFFF;
    let pos = SendMessageW(track, 0x400 /*TBM_GETPOS*/, 0, 0) as i64;
    TRACK_DRAGGING.store(code == TB_THUMBTRACK, Ordering::Relaxed);
    if code == TB_THUMBTRACK {
        // ドラッグ中は 100ms に 1 回だけ送る(Mac へ連打しない)
        static LAST: AtomicUsize = AtomicUsize::new(0);
        let now = crate::state::now_ms() as usize;
        if now.saturating_sub(LAST.load(Ordering::Relaxed)) < 100 {
            return;
        }
        LAST.store(now, Ordering::Relaxed);
    }
    if code == TB_ENDTRACK || code <= 5 {
        crate::state::set_mac_pref("scroll_div", serde_json::json!(260 - pos));
    }
}

fn toggle_text(on: bool, label: &str) -> String {
    format!("{label}: {}", if on { "オン" } else { "オフ" })
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
    let all = [
        &SIDE_COMBO, &SIDE_RESET_BUTTON, &METHOD_COMBO, &HOTKEY_COMBO, &FLIP_BUTTON, &SCROLL_TRACK,
        &NAV_BUTTON, &PINCH_BUTTON, &MAC_CLIP_BUTTON, &MAC_FILES_BUTTON, &MAC_HISTORY_BUTTON,
        &MAC_AUDIO_BUTTON, &MAC_SPK_BUTTON,
    ];
    for slot in all {
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
    select(&SIDE_COMBO, prefs("side").and_then(|v| v.as_u64()).map(|n| n as usize).filter(|n| *n <= 7));
    select(&METHOD_COMBO, method_index());
    select(
        &HOTKEY_COMBO,
        prefs("hotkey").and_then(|v| v.as_i64()).and_then(|k| [97, 100, 105].iter().position(|x| *x == k)),
    );
    let track = SCROLL_TRACK.load(Ordering::Relaxed) as HWND;
    if !track.is_null() && !TRACK_DRAGGING.load(Ordering::Relaxed) {
        if let Some(div) = prefs("scroll_div").and_then(|v| v.as_f64()) {
            let pos = (260.0 - div).clamp(20.0, 240.0) as isize;
            if SendMessageW(track, 0x400, 0, 0) != pos {
                SendMessageW(track, 0x405 /*TBM_SETPOS*/, 1, pos);
            }
        }
    }
    let state = |key: &str, label: &str, invert: bool| -> String {
        match pref_bool(key) {
            Some(b) => toggle_text(b != invert, label),
            None => format!("{label}: —"),
        }
    };
    // scroll_flip=true は「Windows 標準に固定」のため、「Macの向きに合わせる」は
    // 値を反転して表示する(Mac 側チェックボックスの ON と一致させる)
    set_text(FLIP_BUTTON.load(Ordering::Relaxed), &state("scroll_flip", "Macの向きに合わせる", true));
    set_text(NAV_BUTTON.load(Ordering::Relaxed), &state("android_navigation", "ナビゲーション", false));
    set_text(PINCH_BUTTON.load(Ordering::Relaxed), &state("android_pinch", "ピンチ", false));
    set_text(MAC_CLIP_BUTTON.load(Ordering::Relaxed), &state("clip_share", "テキストと画像", false));
    set_text(MAC_FILES_BUTTON.load(Ordering::Relaxed), &state("share_files", "ファイル", false));
    set_text(MAC_HISTORY_BUTTON.load(Ordering::Relaxed), &state("local_history", "コピーの履歴", false));
    set_text(MAC_AUDIO_BUTTON.load(Ordering::Relaxed), &state("audio_muted", "Macで音声を再生", true));
    set_text(MAC_SPK_BUTTON.load(Ordering::Relaxed), &state("spk_mute", "相手のスピーカーをミュート", false));
}
