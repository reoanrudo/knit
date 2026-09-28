//! 設定の分類と画面構築。入力・通信の処理は既存アクションへ委譲する。
use super::*;
static PAGES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static TABS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static SAVE_LABEL: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_POP: AtomicUsize = AtomicUsize::new(0);
static SWITCH_POP: AtomicUsize = AtomicUsize::new(0);
static SWITCH_BUTTON: AtomicUsize = AtomicUsize::new(0);
static CONNECTION_HINT: AtomicUsize = AtomicUsize::new(0);
static LAYOUT_CANVAS: AtomicUsize = AtomicUsize::new(0);

unsafe fn frame(v: ID, r: NSRect) {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"setFrame:"), r);
}
unsafe fn view(parent: ID, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        msg0(objc_getClass(c"NSView".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        r,
    );
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn surface(v: ID, color: &std::ffi::CStr, radius: f64) {
    msg1_void_u8(v, sel(c"setWantsLayer:"), 1);
    let layer = msg0(v, sel(c"layer"));
    let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(color));
    msg1_void_id(
        layer,
        sel(c"setBackgroundColor:"),
        msg0(color, sel(c"CGColor")),
    );
    let f: unsafe extern "C" fn(ID, SEL, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(layer, sel(c"setCornerRadius:"), radius);
}
unsafe fn group(parent: ID, y: f64, h: f64) {
    let v = view(
        parent,
        NSRect {
            x: 24.0,
            y,
            w: 572.0,
            h,
        },
    );
    surface(v, c"controlBackgroundColor", 12.0);
    let layer = msg0(v, sel(c"layer"));
    let f: unsafe extern "C" fn(ID, SEL, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(layer, sel(c"setBorderWidth:"), 0.5);
    let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"separatorColor"));
    msg1_void_id(layer, sel(c"setBorderColor:"), msg0(color, sel(c"CGColor")));
}
unsafe fn divider(parent: ID, y: f64) {
    let v = view(
        parent,
        NSRect {
            x: 40.0,
            y,
            w: 540.0,
            h: 0.5,
        },
    );
    surface(v, c"separatorColor", 0.0);
}
unsafe fn symbol(parent: ID, name: &str, x: f64, y: f64, size: f64) {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let image = f(
        objc_getClass(c"NSImage".as_ptr()),
        sel(c"imageWithSystemSymbolName:accessibilityDescription:"),
        nsstring(name),
        std::ptr::null_mut(),
    );
    let v = msg0(objc_getClass(c"NSImageView".as_ptr()), sel(c"new"));
    frame(
        v,
        NSRect {
            x,
            y,
            w: size,
            h: size,
        },
    );
    msg1_void_id(v, sel(c"setImage:"), image);
    msg1_void_i64(v, sel(c"setImageScaling:"), 3);
    let color = msg0(
        objc_getClass(c"NSColor".as_ptr()),
        sel(c"controlAccentColor"),
    );
    msg1_void_id(v, sel(c"setContentTintColor:"), color);
    msg1_void_id(parent, sel(c"addSubview:"), v);
}
unsafe fn label(parent: ID, text: &str, x: f64, y: f64, w: f64, size: f64, muted: bool) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        objc_getClass(c"NSTextField".as_ptr()),
        sel(c"labelWithString:"),
        nsstring(text),
    );
    frame(
        v,
        NSRect {
            x,
            y,
            w,
            h: size + 10.0,
        },
    );
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(if size >= 20.0 {
            c"boldSystemFontOfSize:"
        } else {
            c"systemFontOfSize:"
        }),
        size,
    );
    msg1_void_id(v, sel(c"setFont:"), font);
    let color = msg0(
        objc_getClass(c"NSColor".as_ptr()),
        sel(if muted {
            c"secondaryLabelColor"
        } else {
            c"labelColor"
        }),
    );
    msg1_void_id(v, sel(c"setTextColor:"), color);
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn button(parent: ID, target: ID, title: &str, action: &std::ffi::CStr, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID, SEL) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let b = f(
        objc_getClass(c"NSButton".as_ptr()),
        sel(c"buttonWithTitle:target:action:"),
        nsstring(title),
        target,
        sel(action),
    );
    frame(b, r);
    msg1_void_id(parent, sel(c"addSubview:"), b);
    b
}
unsafe fn check(
    parent: ID,
    target: ID,
    title: &str,
    action: &std::ffi::CStr,
    slot: &AtomicUsize,
    y: f64,
) {
    label(parent, title, 40.0, y + 1.0, 460.0, 13.0, false);
    let b = msg0(objc_getClass(c"NSSwitch".as_ptr()), sel(c"new"));
    frame(
        b,
        NSRect {
            x: 532.0,
            y,
            w: 44.0,
            h: 28.0,
        },
    );
    msg1_void_id(b, sel(c"setTarget:"), target);
    let action_fn: unsafe extern "C" fn(ID, SEL, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    action_fn(b, sel(c"setAction:"), sel(action));
    msg1_void_id(b, sel(c"setAccessibilityLabel:"), nsstring(title));
    msg1_void_id(parent, sel(c"addSubview:"), b);
    slot.store(b as usize, Ordering::Relaxed);
}
unsafe fn popup(parent: ID, target: ID, titles: &[&str], action: &std::ffi::CStr, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let p = f(
        msg0(objc_getClass(c"NSPopUpButton".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:pullsDown:"),
        r,
        0,
    );
    for t in titles {
        msg1_void_id(p, sel(c"addItemWithTitle:"), nsstring(t));
    }
    msg1_void_id(p, sel(c"setTarget:"), target);
    msg1_void_sel(p, sel(c"setAction:"), sel(action));
    msg1_void_id(parent, sel(c"addSubview:"), p);
    p
}
unsafe fn slider(
    parent: ID,
    target: ID,
    title: &str,
    value: f64,
    min: f64,
    max: f64,
    action: &std::ffi::CStr,
    slot: &AtomicUsize,
    y: f64,
) {
    label(parent, title, 40.0, y, 285.0, 13.0, false);
    let f: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let s = f(
        objc_getClass(c"NSSlider".as_ptr()),
        sel(c"sliderWithValue:minValue:maxValue:target:action:"),
        value,
        min,
        max,
        target,
        sel(action),
    );
    frame(
        s,
        NSRect {
            x: 322.0,
            y,
            w: 215.0,
            h: 24.0,
        },
    );
    msg1_void_u8(s, sel(c"setContinuous:"), 0);
    msg1_void_id(s, sel(c"setAccessibilityLabel:"), nsstring(title));
    msg1_void_id(parent, sel(c"addSubview:"), s);
    slot.store(s as usize, Ordering::Relaxed);
}

pub(super) unsafe fn select_page(index: usize) {
    for i in 0..4 {
        let p = PAGES[i].load(Ordering::Relaxed) as ID;
        if !p.is_null() {
            msg1_void_u8(p, sel(c"setHidden:"), (i != index) as u8);
        }
        let b = TABS[i].load(Ordering::Relaxed) as ID;
        if !b.is_null() {
            msg1_void_i64(b, sel(c"setState:"), (i == index) as i64);
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            let color: unsafe extern "C" fn(ID, SEL, f64, f64, f64, f64) -> ID =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            let accent = color(
                color_cls,
                sel(c"colorWithCalibratedRed:green:blue:alpha:"),
                0.32,
                0.38,
                0.82,
                if i == index { 0.16 } else { 0.0 },
            );
            msg1_void_u8(b, sel(c"setWantsLayer:"), 1);
            let radius: unsafe extern "C" fn(ID, SEL, f64) =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            radius(msg0(b, sel(c"layer")), sel(c"setCornerRadius:"), 7.0);
            msg1_void_id(
                msg0(b, sel(c"layer")),
                sel(c"setBackgroundColor:"),
                msg0(accent, sel(c"CGColor")),
            );
            msg1_void_id(
                b,
                sel(c"setContentTintColor:"),
                msg0(
                    color_cls,
                    sel(if i == index {
                        c"controlAccentColor"
                    } else {
                        c"labelColor"
                    }),
                ),
            );
        }
    }
    let page = PAGES[index.min(3)].load(Ordering::Relaxed) as ID;
    if !page.is_null() {
        let window = msg0(page, sel(c"window"));
        if !window.is_null() {
            msg1_void_u8(
                msg0(window, sel(c"contentView")),
                sel(c"setNeedsDisplay:"),
                1,
            );
            msg0_void(window, sel(c"display"));
        }
    }
}
pub(super) unsafe extern "C" fn navigate(_s: ID, _c: SEL, sender: ID) {
    select_page(crate::msg0_isize(sender, sel(c"tag")).clamp(0, 3) as usize);
}
pub(super) unsafe extern "C" fn hotkey(_s: ID, _c: SEL, sender: ID) {
    let i = crate::msg0_isize(sender, sel(c"indexOfSelectedItem"));
    if let Some(k) = [97, 100, 105].get(i.max(0) as usize) {
        crate::HOTKEY_KC.store(*k, Ordering::Relaxed);
        preferences::save();
    }
}
pub(super) unsafe extern "C" fn switch_method(_s: ID, _c: SEL, sender: ID) {
    let i = crate::msg0_isize(sender, sel(c"indexOfSelectedItem"));
    crate::HOTKEY_ONLY.store(i == 2, Ordering::Relaxed);
    crate::EDGE_TAPS.store(if i == 3 { 1 } else { 2 }, Ordering::Relaxed);
    crate::SWITCH_DELAY_MS.store(if i == 1 { 300 } else { 0 }, Ordering::Relaxed);
    let slider = PREFS_DELAY_SLIDER.load(Ordering::Relaxed) as ID;
    if !slider.is_null() {
        let f: unsafe extern "C" fn(ID, SEL, f64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(
            slider,
            sel(c"setDoubleValue:"),
            if i == 1 { 300.0 } else { 0.0 },
        );
    }
    preferences::save();
    refresh_status();
}
pub(super) unsafe extern "C" fn return_mac(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
        crate::leave_win_mode_cursor_unlock(None);
    }
    refresh_status();
}
pub(super) unsafe extern "C" fn scroll_speed(_s: ID, _c: SEL, sender: ID) {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    crate::set_scroll_div(260.0 - f(sender, sel(c"doubleValue")));
    preferences::save();
}
pub(super) unsafe fn set_save_status(text: &str) {
    let l = SAVE_LABEL.load(Ordering::Relaxed) as ID;
    if !l.is_null() {
        msg1_void_id(l, sel(c"setStringValue:"), nsstring(text));
    }
}
pub(super) unsafe fn sync() {
    let p = SWITCH_POP.load(Ordering::Relaxed) as ID;
    if !p.is_null() {
        let i = if crate::HOTKEY_ONLY.load(Ordering::Relaxed) {
            2
        } else if crate::SWITCH_DELAY_MS.load(Ordering::Relaxed) > 0 {
            1
        } else if crate::EDGE_TAPS.load(Ordering::Relaxed) == 1 {
            3
        } else {
            0
        };
        msg1_void_i64(p, sel(c"selectItemAtIndex:"), i);
    }
    let b = SWITCH_BUTTON.load(Ordering::Relaxed) as ID;
    if !b.is_null() {
        msg1_void_u8(
            b,
            sel(c"setEnabled:"),
            crate::CONNECTED.load(Ordering::Relaxed) as u8,
        );
    }
    let hint = CONNECTION_HINT.load(Ordering::Relaxed) as ID;
    if !hint.is_null() {
        let text = if UI_PREVIEW.load(Ordering::Relaxed) {
            "デザイン確認モード · 入力・通信・音声は動作しません"
        } else if crate::CONNECTED.load(Ordering::Relaxed) {
            "接続できています。画面の端からWindowsへ移動できます。"
        } else {
            "WindowsでKnitを開き、同じネットワークへの接続を確認。"
        };
        msg1_void_id(hint, sel(c"setStringValue:"), nsstring(text));
    }
    let canvas = LAYOUT_CANVAS.load(Ordering::Relaxed) as ID;
    if !canvas.is_null() {
        msg1_void_u8(canvas, sel(c"setNeedsDisplay:"), 1);
    }
}

pub(super) unsafe fn build(target: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let win = f(
        msg0(objc_getClass(c"NSWindow".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentRect:styleMask:backing:defer:"),
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 820.0,
            h: 590.0,
        },
        1 | 2 | 4,
        2,
        0,
    );
    if win.is_null() {
        return win;
    }
    msg1_void_id(win, sel(c"setTitle:"), nsstring("Knit 設定"));
    msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
    msg0_void(win, sel(c"center"));
    let cv = msg0(win, sel(c"contentView"));
    let sidebar = view(
        cv,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 172.0,
            h: 590.0,
        },
    );
    surface(sidebar, c"windowBackgroundColor", 0.0);
    let bytes = include_bytes!("../../../../assets/AppIcon.iconset/icon_128x128.png");
    let data_fn: unsafe extern "C" fn(ID, SEL, *const u8, usize) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let data = data_fn(
        objc_getClass(c"NSData".as_ptr()),
        sel(c"dataWithBytes:length:"),
        bytes.as_ptr(),
        bytes.len(),
    );
    let init_image: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let icon = init_image(
        msg0(objc_getClass(c"NSImage".as_ptr()), sel(c"alloc")),
        sel(c"initWithData:"),
        data,
    );
    let iv = msg0(objc_getClass(c"NSImageView".as_ptr()), sel(c"new"));
    frame(
        iv,
        NSRect {
            x: 20.0,
            y: 535.0,
            w: 32.0,
            h: 32.0,
        },
    );
    msg1_void_id(iv, sel(c"setImage:"), icon);
    msg1_void_id(sidebar, sel(c"addSubview:"), iv);
    label(sidebar, "Knit", 59.0, 537.0, 110.0, 20.0, false);
    label(
        sidebar,
        "2台を、ひとつの手元で。",
        20.0,
        508.0,
        150.0,
        10.0,
        true,
    );
    for (i, title) in ["接続", "画面配置", "操作", "共有"].iter().enumerate() {
        let b = button(
            sidebar,
            target,
            title,
            c"sdNavigate:",
            NSRect {
                x: 16.0,
                y: 416.0 - i as f64 * 48.0,
                w: 140.0,
                h: 36.0,
            },
        );
        msg1_void_u8(b, sel(c"setBordered:"), 0);
        msg1_void_i64(b, sel(c"setAlignment:"), 0);
        let image_fn: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let symbol = image_fn(
            objc_getClass(c"NSImage".as_ptr()),
            sel(c"imageWithSystemSymbolName:accessibilityDescription:"),
            nsstring(["link", "display.2", "keyboard", "square.and.arrow.up"][i]),
            nsstring(title),
        );
        msg1_void_id(b, sel(c"setImage:"), symbol);
        msg1_void_i64(b, sel(c"setImagePosition:"), 2);
        msg1_void_i64(b, sel(c"setTag:"), i as i64);
        msg1_void_i64(b, sel(c"setButtonType:"), 1); // push-on/push-off
        TABS[i].store(b as usize, Ordering::Relaxed);
    }
    label(
        sidebar,
        &format!("バージョン {}", crate::VERSION_STR),
        20.0,
        24.0,
        150.0,
        10.0,
        true,
    );
    let pages: Vec<ID> = (0..4)
        .map(|i| {
            let p = view(
                cv,
                NSRect {
                    x: 184.0,
                    y: 48.0,
                    w: 620.0,
                    h: 530.0,
                },
            );
            surface(p, c"controlBackgroundColor", 0.0);
            PAGES[i].store(p as usize, Ordering::Relaxed);
            p
        })
        .collect();
    let titles = [
        ("接続", "接続先と操作する画面を確認できます。"),
        ("画面配置", "Windowsの位置を、実際の画面配置に合わせます。"),
        ("操作", "切替とスクロールを、使いやすく。"),
        ("共有", "コピーした内容とWindowsの音声をつなぎます。"),
    ];
    for (p, (title, sub)) in pages.iter().zip(titles) {
        label(*p, title, 28.0, 470.0, 565.0, 24.0, false);
        label(*p, sub, 28.0, 441.0, 565.0, 12.0, true);
    }
    let p = pages[0];
    group(p, 186.0, 232.0);
    group(p, 32.0, 136.0);
    symbol(p, "laptopcomputer", 106.0, 338.0, 54.0);
    symbol(p, "desktopcomputer", 434.0, 338.0, 54.0);
    label(p, "このMac", 107.0, 309.0, 145.0, 14.0, false);
    label(p, "Windows", 427.0, 309.0, 145.0, 14.0, false);
    label(p, "接続先", 280.0, 354.0, 100.0, 11.0, true);
    PREFS_STATE.store(
        label(p, "接続を確認しています…", 40.0, 266.0, 540.0, 13.0, false) as usize,
        Ordering::Relaxed,
    );
    CONNECTION_HINT.store(
        label(p, "", 40.0, 235.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    SWITCH_BUTTON.store(
        button(
            p,
            target,
            "Windowsへ切替",
            c"sdToggle:",
            NSRect {
                x: 40.0,
                y: 195.0,
                w: 170.0,
                h: 30.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    button(
        p,
        target,
        "このMacに戻る",
        c"sdReturnMac:",
        NSRect {
            x: 222.0,
            y: 195.0,
            w: 170.0,
            h: 30.0,
        },
    );
    label(p, "接続の確認", 40.0, 128.0, 500.0, 14.0, false);
    label(
        p,
        "両方のアプリの起動と、ネットワークを確認してください。",
        40.0,
        98.0,
        540.0,
        12.0,
        true,
    );
    button(
        p,
        target,
        "ログを開く",
        c"sdOpenLog:",
        NSRect {
            x: 40.0,
            y: 47.0,
            w: 130.0,
            h: 32.0,
        },
    );
    button(
        p,
        target,
        "再起動",
        c"sdRestart:",
        NSRect {
            x: 182.0,
            y: 47.0,
            w: 130.0,
            h: 32.0,
        },
    );
    button(
        p,
        target,
        "Windowsを登録…",
        c"sdRegistration:",
        NSRect {
            x: 380.0,
            y: 47.0,
            w: 192.0,
            h: 32.0,
        },
    );
    let p = pages[1];
    let lay = make_layout_view(p);
    frame(
        lay,
        NSRect {
            x: 28.0,
            y: 128.0,
            w: LAY_VW,
            h: LAY_VH,
        },
    );
    msg1_void_id(p, sel(c"addSubview:"), lay);
    LAYOUT_CANVAS.store(lay as usize, Ordering::Relaxed);
    label(
        p,
        "グレー：Mac    ブルー：Windows（ドラッグで調整）",
        28.0,
        98.0,
        560.0,
        12.0,
        true,
    );
    let pop = popup(
        p,
        target,
        &[
            "Windowsは右",
            "Windowsは左",
            "Windowsは上",
            "Windowsは下",
            "Windowsは右上",
            "Windowsは右下",
            "Windowsは左上",
            "Windowsは左下",
        ],
        c"sdSide:",
        NSRect {
            x: 28.0,
            y: 48.0,
            w: 240.0,
            h: 30.0,
        },
    );
    PREFS_SIDE_POP.store(pop as usize, Ordering::Relaxed);
    let p = pages[2];
    group(p, 330.0, 88.0);
    group(p, 203.0, 118.0);
    group(p, 30.0, 164.0);
    for y in [376.0, 282.0, 247.0, 154.0, 111.0, 77.0] {
        divider(p, y);
    }

    label(p, "切替方式", 40.0, 386.0, 220.0, 13.0, false);
    SWITCH_POP.store(
        popup(
            p,
            target,
            &[
                "端に2回触れる",
                "端で少し待つ",
                "ショートカットのみ",
                "端に1回触れる",
            ],
            c"sdSwitchMethod:",
            NSRect {
                x: 300.0,
                y: 384.0,
                w: 252.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    label(p, "切替キー", 40.0, 345.0, 260.0, 13.0, false);
    let keys = [97, 100, 105];
    let current = crate::hotkey_kc();
    let custom = format!("現在のキー（コード{current}）");
    let titles = [
        "F6（必要に応じてfnと併用）",
        "F8（必要に応じてfnと併用）",
        "F13",
        custom.as_str(),
    ];
    let pop = popup(
        p,
        target,
        &titles[..if keys.contains(&current) { 3 } else { 4 }],
        c"sdHotkey:",
        NSRect {
            x: 300.0,
            y: 342.0,
            w: 252.0,
            h: 28.0,
        },
    );
    msg1_void_i64(
        pop,
        sel(c"selectItemAtIndex:"),
        keys.iter().position(|k| *k == current).unwrap_or(3) as i64,
    );
    HOTKEY_POP.store(pop as usize, Ordering::Relaxed);
    check(
        p,
        target,
        "スクロール方向をMacに合わせる",
        c"sdScroll:",
        &PREFS_CHK_SCROLL,
        287.0,
    );
    slider(
        p,
        target,
        "スクロール速度   遅い / 速い",
        260.0 - crate::scroll_div(),
        20.0,
        240.0,
        c"sdScrollSpeed:",
        &PREFS_SLIDER,
        256.0,
    );
    slider(
        p,
        target,
        "カーソル速度",
        crate::mouse_scale(),
        0.2,
        3.0,
        c"sdMouseScale:",
        &PREFS_MSCALE_SLIDER,
        215.0,
    );
    check(
        p,
        target,
        "⌘キーをAltに割り当てる（オフ：Ctrl）",
        c"sdCmdMap:",
        &PREFS_CHK_CMD,
        158.0,
    );
    check(
        p,
        target,
        "スクロール互換モード",
        c"sdScrollCompat:",
        &PREFS_CHK_SCOMPAT,
        116.0,
    );
    slider(
        p,
        target,
        "切替の待ち時間（0〜1000ms）",
        crate::SWITCH_DELAY_MS.load(Ordering::Relaxed) as f64,
        0.0,
        1000.0,
        c"sdDelay:",
        &PREFS_DELAY_SLIDER,
        83.0,
    );
    slider(
        p,
        target,
        "ダブルタップ間隔（100〜1500ms）",
        crate::DOUBLE_TAP_MS.load(Ordering::Relaxed) as f64,
        100.0,
        1500.0,
        c"sdDblTap:",
        &PREFS_DBL_SLIDER,
        42.0,
    );
    let p = pages[3];
    group(p, 260.0, 158.0);
    group(p, 76.0, 168.0);
    divider(p, 326.0);
    label(p, "ファイル", 40.0, 285.0, 220.0, 13.0, false);
    divider(p, 202.0);
    divider(p, 155.0);

    check(
        p,
        target,
        "テキストと画像",
        c"sdClipShare:",
        &PREFS_CHK_CLIP,
        378.0,
    );
    label(
        p,
        "コピーした内容を、もう1台でも貼り付けられます。",
        40.0,
        348.0,
        540.0,
        12.0,
        true,
    );
    button(
        p,
        target,
        "Windowsへファイルを送る…",
        c"sdSendFile:",
        NSRect {
            x: 324.0,
            y: 280.0,
            w: 255.0,
            h: 34.0,
        },
    );
    check(
        p,
        target,
        "Windowsの音声を再生",
        c"sdAudio:",
        &PREFS_CHK_AUDIO,
        210.0,
    );
    check(
        p,
        target,
        "Windowsスピーカーをミュート",
        c"sdSpkMute:",
        &PREFS_CHK_SPK,
        166.0,
    );
    for (i, (title, tag)) in [("音量を下げる", 2), ("音量を上げる", 1), ("ミュート", 3)]
        .iter()
        .enumerate()
    {
        let b = button(
            p,
            target,
            title,
            c"sdVol:",
            NSRect {
                x: 40.0 + i as f64 * 180.0,
                y: 100.0,
                w: 156.0,
                h: 32.0,
            },
        );
        msg1_void_i64(b, sel(c"setTag:"), *tag);
    }
    label(
        p,
        "音声の有効・無効は、このMacでの再生に反映されます。",
        28.0,
        48.0,
        565.0,
        12.0,
        true,
    );
    SAVE_LABEL.store(
        label(
            cv,
            if UI_PREVIEW.load(Ordering::Relaxed) {
                "デザイン確認モード · 設定は保存しません"
            } else {
                if preferences::has_overrides() {
                    "自動保存 · 起動時は環境変数・envファイルの指定が優先されます"
                } else {
                    "変更はこのMacに自動保存されます"
                }
            },
            210.0,
            14.0,
            590.0,
            11.0,
            true,
        ) as usize,
        Ordering::Relaxed,
    );
    let args: Vec<String> = std::env::args().collect();
    let page = if UI_PREVIEW.load(Ordering::Relaxed) {
        args.iter()
            .position(|v| v == "--preview-page")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(3)
    } else {
        0
    };
    select_page(page);
    win
}
