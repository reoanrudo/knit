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
static HINT: AtomicUsize = AtomicUsize::new(0);
static DRAW_FONT: AtomicUsize = AtomicUsize::new(0);
static INPUT_HINT: AtomicUsize = AtomicUsize::new(0);

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
    set_text(
        AUDIO_BUTTON.load(Ordering::Relaxed),
        if crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed) {
            "音声転送をオフにする"
        } else {
            "音声転送をオンにする"
        },
    );
    set_text(
        HINT.load(Ordering::Relaxed),
        if crate::CONNECTED.load(Ordering::Relaxed) {
            "接続できています。MacからこのWindowsを操作できます。"
        } else {
            "MacでTsunaguを開き、同じネットワークへの接続を確認。"
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

pub(super) unsafe fn build() {
    let existing = STATUS_HWND.load(Ordering::Relaxed) as HWND;
    if !existing.is_null() {
        ShowWindow(existing, SW_SHOW);
        SetForegroundWindow(existing);
        return;
    }
    let hinst = TRAY_HINST.load(Ordering::Relaxed) as *mut core::ffi::c_void;
    let class = wide("TsunaguSettings");
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
        bottom: 590,
    };
    AdjustWindowRect(&mut r, style, 0);
    let hwnd = CreateWindowExW(
        0x00010000, /*WS_EX_CONTROLPARENT*/
        class.as_ptr(),
        wide("Tsunagu 設定").as_ptr(),
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
    make(None, "STATIC", "Tsunagu", 0, 68, 32, 114, 32, 220, segoe_font(true, 22));
    make(
        None,
        "STATIC",
        "2台を、ひとつの手元で。",
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
        552,
        150,
        20,
        221,
        small,
    );
    let titles = [
        ("接続", "2台の接続と、現在の状態。"),
        ("画面配置", "机の上と同じ並びで、カーソルを移動。"),
        ("操作", "いつものキーボードを、両方のPCで。"),
        ("共有", "作業に必要なものを、2台のあいだで。"),
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
        |p, text: &str, x, y, w, id| make(Some(p), "STATIC", text, 0, x, y, w, 26, id, font);
    let note =
        |p, text: &str, x, y, w| make(Some(p), "STATIC", text, 0, x, y, w, 32, ID_LBL_BUILD, small);
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
    // Connection is a device relationship, followed by a separate editable endpoint.
    LABEL_STATE.store(
        label(0, &tray_status_text(), 236, 245, 520, ID_LBL_STATE),
        Ordering::Relaxed,
    );
    HINT.store(note(0, "", 236, 278, 520), Ordering::Relaxed);
    label(0, "接続先", 236, 345, 200, ID_HEAD_ACT);
    note(0, "MacのIPアドレス", 236, 374, 200);
    let host = HOST_NOW.lock().unwrap_or_else(|e| e.into_inner()).clone();
    EDIT_HOST.store(
        make(
            Some(0),
            "EDIT",
            &host,
            0x0080 | 0x00800000 | WS_TABSTOP,
            236,
            410,
            310,
            30,
            230,
            font,
        ),
        Ordering::Relaxed,
    );
    btn(0, "保存して再接続", 562, 407, 190, MENU_SAVEHOST);
    LABEL_RTT.store(
        label(0, &rtt_line(), 236, 480, 150, ID_LBL_RTT),
        Ordering::Relaxed,
    );
    btn(0, "キーを再入力", 400, 476, 136, MENU_REGISTER);
    btn(0, "ログ", 548, 476, 96, MENU_OPENLOG);
    btn(0, "再起動", 656, 476, 96, MENU_RESTART);
    LABEL_MACCFG.store(
        label(1, &maccfg_line(), 236, 416, 520, ID_HEAD_ACT),
        Ordering::Relaxed,
    );
    note(
        1,
        "配置を変更するには、Macの「設定 → 画面配置」を開きます。",
        236,
        455,
        520,
    );
    label(2, "キーボード", 236, 142, 520, ID_HEAD_ACT);
    label(2, "修飾キー", 236, 194, 180, ID_HEAD_ACT);
    INPUT_HINT.store(label(2, "", 494, 194, 258, ID_HEAD_ACT), Ordering::Relaxed);
    label(2, "コピー / 貼り付け", 236, 249, 240, ID_HEAD_ACT);
    label(2, "⌘ C     /     ⌘ V", 494, 249, 258, ID_HEAD_ACT);
    label(2, "日本語入力", 236, 304, 220, ID_HEAD_ACT);
    label(2, "かな / 英数", 494, 304, 258, ID_HEAD_ACT);
    label(2, "操作するPC", 236, 393, 250, ID_HEAD_ACT);
    note(2, "このWindowsの操作を終え、Macに戻ります。", 236, 426, 320);
    btn(2, "Macへ戻る", 582, 399, 170, MENU_BACKMAC);
    note(
        2,
        "切替キー・スクロール速度はMac側で変更できます。",
        236,
        478,
        520,
    );
    label(3, "音声", 236, 142, 300, ID_HEAD_ACT);
    LABEL_AUDIO.store(
        label(3, &audio_line(), 236, 183, 520, ID_LBL_AUDIO),
        Ordering::Relaxed,
    );
    LABEL_SPK.store(
        label(3, &spk_line(), 236, 220, 520, ID_LBL_SPK),
        Ordering::Relaxed,
    );
    AUDIO_BUTTON.store(
        btn(3, "音声転送をオフにする", 492, 264, 260, MENU_AUDIO),
        Ordering::Relaxed,
    );
    label(3, "ファイル", 236, 360, 250, ID_HEAD_ACT);
    LABEL_FILES.store(
        label(3, &files_line(), 236, 400, 260, ID_LBL_FILES),
        Ordering::Relaxed,
    );
    btn(3, "受信フォルダを開く", 492, 399, 260, MENU_OPENFOLDER);
    note(
        3,
        "テキスト・画像の共有設定は、Mac側で変更できます。",
        236,
        475,
        520,
    );
    LABEL_FOOTER.store(
        make(
            None,
            "STATIC",
            &footer_line(),
            0,
            208,
            553,
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
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

pub(super) unsafe fn paint_layout(hdc: *mut core::ffi::c_void) {
    let font = DRAW_FONT.load(Ordering::Relaxed) as *mut core::ffi::c_void;
    if !font.is_null() {
        SelectObject(hdc, font);
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
        return;
    }
    let side = crate::SIDE_W.load(Ordering::Relaxed);
    let (mx, my, wx, wy) = match side {
        1 => (525, 236, 320, 236),
        2 => (420, 304, 420, 163),
        3 => (420, 163, 420, 304),
        4 => (320, 260, 525, 204),
        5 => (320, 204, 525, 260),
        6 => (525, 260, 320, 204),
        7 => (525, 204, 320, 260),
        _ => (320, 236, 525, 236),
    };
    for (x, y, text, color) in [(mx, my, "Mac", 0x7C869C), (wx, wy, "Windows", CLR_ACCENT)] {
        let brush = CreateSolidBrush(rgb(0xF2F4FB));
        let pen = CreatePen(0, 2, rgb(color));
        let ob = SelectObject(hdc, brush);
        let op = SelectObject(hdc, pen);
        RoundRect(hdc, x, y, x + 144, y + 88, 12, 12);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(brush);
        DeleteObject(pen);
        SetTextColor(hdc, rgb(CLR_TEXT));
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
    SetTextColor(hdc, rgb(if selected { CLR_ACCENT } else { CLR_TEXT }));
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
    let brush = CreateSolidBrush(rgb(CLR_CARD));
    let pen = CreatePen(0, 1, rgb(0xE3E6ED));
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
        0 => &[(120, 320), (332, 458), (468, 530)],
        1 => &[(120, 388), (400, 506)],
        2 => &[(120, 350), (370, 530)],
        _ => &[(120, 324), (342, 446), (462, 518)],
    };
    for &(top, bottom) in groups {
        card(hdc, top, bottom);
    }
    if page == 2 {
        use windows_sys::Win32::Graphics::Gdi::{LineTo, MoveToEx};
        let pen = CreatePen(0, 1, rgb(0xECEEF3));
        let old = SelectObject(hdc, pen);
        for y in [180, 235, 290] {
            MoveToEx(hdc, 236, y, std::ptr::null_mut());
            LineTo(hdc, 752, y);
        }
        SelectObject(hdc, old);
        DeleteObject(pen);
    }
    if page == 0 {
        let old = SelectObject(hdc, DRAW_FONT.load(Ordering::Relaxed) as _);
        SetBkMode(hdc, TRANSPARENT_BK);
        SetTextColor(hdc, rgb(CLR_HEAD));
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
        SetTextColor(hdc, rgb(CLR_ACCENT));
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
        let pen = CreatePen(0, 1, rgb(0xCDD2E4));
        let op = SelectObject(hdc, pen);
        MoveToEx(hdc, 418, 170, std::ptr::null_mut());
        LineTo(hdc, 568, 170);
        SelectObject(hdc, op);
        DeleteObject(pen);
        SelectObject(hdc, old);
    }
}
