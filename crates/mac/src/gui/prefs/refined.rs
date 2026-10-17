//! 設定の編集値はここに閉じ、適用するまで入力や音声の実行値へ渡さない。
use super::*;
use std::ffi::CStr;
use std::sync::atomic::AtomicUsize;

static BUILT: AtomicBool = AtomicBool::new(false);
static PAGE: AtomicUsize = AtomicUsize::new(0);
static WINDOW: AtomicUsize = AtomicUsize::new(0);
static TARGET: AtomicUsize = AtomicUsize::new(0);
static PANELS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static NAV: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static APPLY: AtomicUsize = AtomicUsize::new(0);
static REVERT: AtomicUsize = AtomicUsize::new(0);
static ERROR: AtomicUsize = AtomicUsize::new(0);
static SIDEBAR: AtomicUsize = AtomicUsize::new(0);
static LAST_THEME: AtomicUsize = AtomicUsize::new(usize::MAX);
static PEER_LIST: AtomicUsize = AtomicUsize::new(0);
static MANUAL: AtomicUsize = AtomicUsize::new(0);
static MANUAL_OPEN: AtomicBool = AtomicBool::new(false);
static HOST: AtomicUsize = AtomicUsize::new(0);
static HOST_ROLE: AtomicUsize = AtomicUsize::new(0);
static SENS: AtomicUsize = AtomicUsize::new(0);
static SENS_VALUE: AtomicUsize = AtomicUsize::new(0);
static GAIN: AtomicUsize = AtomicUsize::new(0);
static GAIN_VALUE: AtomicUsize = AtomicUsize::new(0);
static OUTPUT: AtomicUsize = AtomicUsize::new(0);
static FOLDER: AtomicUsize = AtomicUsize::new(0);
static LANGUAGE: AtomicUsize = AtomicUsize::new(0);
static SWITCHES: [AtomicUsize; 10] = [const { AtomicUsize::new(0) }; 10];
/// その他ページの切替キーポップアップ(既存4候補+「ほかのキー…」)
static HOTKEY: AtomicUsize = AtomicUsize::new(0);
/// 切替キーの env 固定(KNIT_HOTKEY_KC)の注記行(固定が無いときは空=見えない)
static HOTKEY_ENV_NOTE: AtomicUsize = AtomicUsize::new(0);
/// その他ページのキー変換プレビューの行(現在の CMD_ALT で sync が書き換える)
static KEY_CONV_LINES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
/// 接続ページの端末一覧スクロール(診断パネル表示中は隠す)
static PEER_SCROLL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページ内診断パネル(peer_scroll と同位置に重ねる。初期hidden)
static DIAG_PANEL: AtomicUsize = AtomicUsize::new(0);
static DIAG_TEXT: AtomicUsize = AtomicUsize::new(0);
static DIAG_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 接続ページのRTT表示行
static RTT: AtomicUsize = AtomicUsize::new(0);
/// 履歴の保存件数ポップアップ
static HISTORY_LIMIT: AtomicUsize = AtomicUsize::new(0);
static OUTPUTS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static PEER_IDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PEER_SIGNATURE: Mutex<String> = Mutex::new(String::new());
static DRAFT: Mutex<Option<Draft>> = Mutex::new(None);
static BASELINE: Mutex<Option<Draft>> = Mutex::new(None);

fn reset_draft() {
    let current = Draft::current();
    *DRAFT.lock().unwrap_or_else(|e| e.into_inner()) = Some(current.clone());
    *BASELINE.lock().unwrap_or_else(|e| e.into_inner()) = Some(current);
}
fn merge_runtime() -> bool {
    let current = Draft::current();
    let mut draft = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    let mut baseline = BASELINE.lock().unwrap_or_else(|e| e.into_inner());
    let (Some(d), Some(b)) = (draft.as_mut(), baseline.as_mut()) else {
        return false;
    };
    merge_values(d, b, &current)
}
fn merge_values(d: &mut Draft, b: &mut Draft, current: &Draft) -> bool {
    let before = d.clone();
    macro_rules! merge {
        ($field:ident) => {
            if d.$field == b.$field {
                d.$field = current.$field.clone();
                b.$field = current.$field.clone();
            }
        };
    }
    merge!(playback);
    merge!(gain);
    merge!(speaker);
    merge!(clip);
    merge!(files);
    merge!(history);
    merge!(output);
    merge!(language);
    merge!(startup);
    merge!(hotkey);
    merge!(update_check);
    if d.desktop.notify_connection == b.desktop.notify_connection {
        d.desktop.notify_connection = current.desktop.notify_connection;
        b.desktop.notify_connection = current.desktop.notify_connection;
    }
    if d.desktop.notify_transfer == b.desktop.notify_transfer {
        d.desktop.notify_transfer = current.desktop.notify_transfer;
        b.desktop.notify_transfer = current.desktop.notify_transfer;
    }
    if d.desktop.receive_folder == b.desktop.receive_folder {
        d.desktop.receive_folder = current.desktop.receive_folder.clone();
        b.desktop.receive_folder = current.desktop.receive_folder.clone();
    }
    if d.desktop.open_receive_folder == b.desktop.open_receive_folder {
        d.desktop.open_receive_folder = current.desktop.open_receive_folder;
        b.desktop.open_receive_folder = current.desktop.open_receive_folder;
    }
    if d.desktop.history_limit == b.desktop.history_limit {
        d.desktop.history_limit = current.desktop.history_limit;
        b.desktop.history_limit = current.desktop.history_limit;
    }
    *d != before
}
fn draft_dirty() -> bool {
    let draft = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    let baseline = BASELINE.lock().unwrap_or_else(|e| e.into_inner());
    *draft != *baseline
}
static RANGES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static OUTPUT_RESELECT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, PartialEq)]
struct Draft {
    playback: bool,
    gain: f64,
    speaker: bool,
    clip: bool,
    files: bool,
    history: bool,
    output: String,
    language: Language,
    desktop: crate::desktop_settings::Settings,
    startup: bool,
    /// 切替キー(Mac keycode・1..=127)。適用で preferences の hotkey へ保存され、
    /// tap は HOTKEY_KC を毎回読むため再起動不要で反映される
    hotkey: i64,
    /// 更新の自動確認(メニューの手動確認は常に可能)
    update_check: bool,
}
impl Draft {
    fn current() -> Self {
        Self {
            playback: !crate::audio::MUTED.load(Ordering::Relaxed),
            gain: crate::audio::gain() as f64,
            speaker: crate::SPK_MUTE.load(Ordering::Relaxed),
            clip: crate::CLIP_SHARE.load(Ordering::Relaxed),
            files: knit_common::share::user_files(),
            history: crate::LOCAL_HISTORY.load(Ordering::Relaxed),
            output: crate::audio::output_uid(),
            language: localization::language(),
            desktop: crate::desktop_settings::snapshot(),
            startup: crate::desktop_settings::startup_enabled(),
            hotkey: crate::hotkey_kc(),
            update_check: crate::UPDATE_CHECK.load(Ordering::Relaxed),
        }
    }
}
fn with_draft<T>(f: impl FnOnce(&mut Draft) -> T) -> T {
    let mut d = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    f(d.get_or_insert_with(Draft::current))
}
pub(in crate::gui) fn is_built() -> bool {
    BUILT.load(Ordering::Relaxed)
}
pub(in crate::gui) fn host_field() -> usize {
    HOST.load(Ordering::Relaxed)
}
unsafe fn object(slot: &AtomicUsize) -> ID {
    slot.load(Ordering::Relaxed) as ID
}
unsafe fn value(v: ID) -> f64 {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"doubleValue"))
}
unsafe fn number(v: ID, n: f64) {
    let f: unsafe extern "C" fn(ID, SEL, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"setDoubleValue:"), n);
}
unsafe fn string(v: ID) -> String {
    let s = msg0(v, sel(c"stringValue"));
    let p = msg0_cstr(s, sel(c"UTF8String"));
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}
unsafe fn text(v: ID, s: &str) {
    if !v.is_null() {
        msg1_void_id(v, sel(c"setStringValue:"), nsstring(s));
    }
}
unsafe fn enabled(v: ID, on: bool) {
    if !v.is_null() {
        msg1_void_u8(v, sel(c"setEnabled:"), on as u8);
    }
}
fn surface_rgb(dark: bool, sidebar: bool) -> (f64, f64, f64) {
    match (dark, sidebar) {
        (false, false) => (0.945, 0.950, 0.958),
        (false, true) => (0.890, 0.901, 0.918),
        (true, false) => (0.145, 0.154, 0.171),
        (true, true) => (0.110, 0.120, 0.138),
    }
}
pub(super) unsafe fn surface_color(view: ID, sidebar: bool) -> ID {
    let appearance = msg0(view, sel(c"effectiveAppearance"));
    let name = msg0(appearance, sel(c"name"));
    let p = msg0_cstr(name, sel(c"UTF8String"));
    let dark = !p.is_null() && CStr::from_ptr(p).to_string_lossy().contains("Dark");
    let (r, g, b) = surface_rgb(dark, sidebar);
    let make: unsafe extern "C" fn(ID, SEL, f64, f64, f64, f64) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    make(
        objc_getClass(c"NSColor".as_ptr()),
        sel(c"colorWithSRGBRed:green:blue:alpha:"),
        r,
        g,
        b,
        1.0,
    )
}
unsafe fn color(v: ID, sidebar: bool) {
    msg1_void_u8(v, sel(c"setWantsLayer:"), 1);
    let c = surface_color(v, sidebar);
    msg1_void_id(
        msg0(v, sel(c"layer")),
        sel(c"setBackgroundColor:"),
        msg0(c, sel(c"CGColor")),
    );
}
unsafe fn refresh_surface() {
    let win = object(&WINDOW);
    if win.is_null() {
        return;
    }
    let appearance = msg0(win, sel(c"effectiveAppearance"));
    let p = msg0_cstr(msg0(appearance, sel(c"name")), sel(c"UTF8String"));
    let dark = !p.is_null() && CStr::from_ptr(p).to_string_lossy().contains("Dark");
    if LAST_THEME.swap(dark as usize, Ordering::Relaxed) == dark as usize {
        return;
    }
    let cv = msg0(win, sel(c"contentView"));
    color(cv, false);
    color(object(&SIDEBAR), true);
    msg1_void_id(win, sel(c"setBackgroundColor:"), surface_color(cv, false));
}
unsafe fn line(parent: ID, x: f64, y: f64, w: f64) {
    let v = msg0(objc_getClass(c"NSBox".as_ptr()), sel(c"new"));
    frame(v, NSRect { x, y, w, h: 1.0 });
    msg1_void_i64(v, sel(c"setBoxType:"), 2);
    msg1_void_id(parent, sel(c"addSubview:"), v);
}
unsafe fn symbol(name: &str, title: &str) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(
        objc_getClass(c"NSImage".as_ptr()),
        sel(c"imageWithSystemSymbolName:accessibilityDescription:"),
        nsstring(name),
        nsstring(title),
    )
}
unsafe fn icon(parent: ID, name: &str, title: &str, r: NSRect) -> ID {
    let v = msg0(objc_getClass(c"NSImageView".as_ptr()), sel(c"new"));
    frame(v, r);
    msg1_void_id(v, sel(c"setImage:"), symbol(name, title));
    msg1_void_i64(v, sel(c"setImageScaling:"), 0);
    msg1_void_id(v, sel(c"setAccessibilityLabel:"), nsstring(title));
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn command(parent: ID, title: &str, action: &CStr, r: NSRect) -> ID {
    let b = button(parent, object(&TARGET), title, action, r);
    msg1_void_i64(b, sel(c"setBezelStyle:"), 1);
    b
}
unsafe fn toggle(parent: ID, title: &str, index: usize, x: f64, y: f64, w: f64) {
    label(parent, title, x, y + 3.0, w - 58.0, 14.0, false);
    let s = msg0(objc_getClass(c"NSSwitch".as_ptr()), sel(c"new"));
    frame(
        s,
        NSRect {
            x: x + w - 44.0,
            y,
            w: 44.0,
            h: 28.0,
        },
    );
    msg1_void_id(s, sel(c"setTarget:"), object(&TARGET));
    msg1_void_sel(s, sel(c"setAction:"), sel(c"sdRefinedToggle:"));
    msg1_void_i64(s, sel(c"setTag:"), index as i64);
    msg1_void_id(
        s,
        sel(c"setAccessibilityLabel:"),
        nsstring(&localization::copy(title)),
    );
    bind_copy(s, c"setAccessibilityLabel:", title);
    msg1_void_id(parent, sel(c"addSubview:"), s);
    SWITCHES[index].store(s as usize, Ordering::Relaxed);
}
unsafe fn range_slider(
    parent: ID,
    title: &str,
    r: NSRect,
    min: f64,
    max: f64,
    action: &CStr,
) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let s = f(
        objc_getClass(c"NSSlider".as_ptr()),
        sel(c"sliderWithValue:minValue:maxValue:target:action:"),
        min,
        min,
        max,
        object(&TARGET),
        sel(action),
    );
    frame(s, r);
    msg1_void_u8(s, sel(c"setContinuous:"), 1);
    msg1_void_id(
        s,
        sel(c"setAccessibilityLabel:"),
        nsstring(&localization::copy(title)),
    );
    msg1_void_id(parent, sel(c"addSubview:"), s);
    s
}
unsafe fn alert(title: &str, body: &str, accept: Option<&str>) -> bool {
    let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(a, sel(c"setMessageText:"), nsstring(title));
    msg1_void_id(a, sel(c"setInformativeText:"), nsstring(body));
    msg1_void_id(
        a,
        sel(c"addButtonWithTitle:"),
        nsstring(if accept.is_some() {
            "キャンセル"
        } else {
            "閉じる"
        }),
    );
    if let Some(s) = accept {
        msg1_void_id(a, sel(c"addButtonWithTitle:"), nsstring(s));
    }
    crate::msg0_isize(a, sel(c"runModal")) == 1001
}
unsafe fn error(s: &str) {
    text(object(&ERROR), s);
    msg1_void_u8(object(&ERROR), sel(c"setHidden:"), s.is_empty() as u8);
    msg1_void_id(object(&ERROR), sel(c"setToolTip:"), nsstring(s));
}
unsafe fn make_page(parent: ID, x: f64, y: f64, w: f64, h: f64) -> ID {
    let scroll = msg0(objc_getClass(c"NSScrollView".as_ptr()), sel(c"new"));
    frame(scroll, NSRect { x, y, w, h });
    msg1_void_u8(scroll, sel(c"setDrawsBackground:"), 0);
    msg1_void_u8(scroll, sel(c"setHasVerticalScroller:"), 1);
    msg1_void_u8(scroll, sel(c"setAutohidesScrollers:"), 1);
    msg1_void_id(parent, sel(c"addSubview:"), scroll);
    let p = view(
        scroll,
        NSRect {
            x: 0.0,
            y: 0.0,
            w,
            h: 560.0,
        },
    );
    msg1_void_id(scroll, sel(c"setDocumentView:"), p);
    p
}

pub(in crate::gui) unsafe fn build(target: ID) -> ID {
    TARGET.store(target as usize, Ordering::Relaxed);
    COPY_BINDINGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    let args: Vec<_> = std::env::args().collect();
    let preview = UI_PREVIEW.load(Ordering::Relaxed);
    if preview {
        if let Some(lang) = args
            .iter()
            .position(|a| a == "--preview-language")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| Language::parse(s))
        {
            localization::set_language(lang);
        }
    }
    let width = 1040.0;
    let height = 700.0;
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let win = f(
        msg0(objc_getClass(c"NSWindow".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentRect:styleMask:backing:defer:"),
        NSRect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        },
        1 | 2 | 4 | 8,
        2,
        0,
    );
    if win.is_null() {
        return win;
    }
    WINDOW.store(win as usize, Ordering::Relaxed);
    msg1_void_id(win, sel(c"setTitle:"), nsstring("Knit"));
    msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
    msg1_void_u8(win, sel(c"setTitlebarAppearsTransparent:"), 1);
    let size: unsafe extern "C" fn(ID, SEL, crate::CGSize) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    size(
        win,
        sel(c"setContentMinSize:"),
        crate::CGSize {
            w: width,
            h: height,
        },
    );
    size(
        win,
        sel(c"setContentMaxSize:"),
        crate::CGSize {
            w: width,
            h: height,
        },
    );
    if preview
        && args
            .iter()
            .any(|a| a == "--preview-light" || a == "--preview-dark")
    {
        let name = if args.iter().any(|a| a == "--preview-dark") {
            "NSAppearanceNameDarkAqua"
        } else {
            "NSAppearanceNameAqua"
        };
        let appearance = crate::msg1_id(
            objc_getClass(c"NSAppearance".as_ptr()),
            sel(c"appearanceNamed:"),
            nsstring(name),
        );
        msg1_void_id(win, sel(c"setAppearance:"), appearance);
    }
    msg0_void(win, sel(c"center"));
    let cv = msg0(win, sel(c"contentView"));
    color(cv, false);
    msg1_void_id(win, sel(c"setBackgroundColor:"), surface_color(cv, false));
    let sidebar = view(
        cv,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 180.0,
            h: height,
        },
    );
    SIDEBAR.store(sidebar as usize, Ordering::Relaxed);
    color(sidebar, true);
    for (i, (title, image)) in [
        ("接続", "link"),
        ("画面配置", "display"),
        ("共有", "square.and.arrow.up"),
        ("その他", "gearshape"),
    ]
    .iter()
    .enumerate()
    {
        let b = button(
            sidebar,
            target,
            title,
            c"sdNavigate:",
            NSRect {
                x: 12.0,
                y: height - 66.0 - i as f64 * 46.0,
                w: 156.0,
                h: 36.0,
            },
        );
        msg1_void_u8(b, sel(c"setBordered:"), 0);
        msg1_void_i64(b, sel(c"setTag:"), i as i64);
        msg1_void_i64(b, sel(c"setAlignment:"), 0);
        msg1_void_id(b, sel(c"setImage:"), symbol(image, title));
        msg1_void_i64(b, sel(c"setImagePosition:"), 2);
        NAV[i].store(b as usize, Ordering::Relaxed);
    }
    let main_x = 180.0;
    for i in 0..4 {
        let p = make_page(cv, main_x, 68.0, width - main_x, height - 68.0);
        PANELS[i].store(
            msg0(p, sel(c"enclosingScrollView")) as usize,
            Ordering::Relaxed,
        );
        match i {
            0 => build_connection(p),
            1 => build_layout(p),
            2 => build_sharing(p),
            _ => build_general(p),
        }
    }
    line(cv, 180.0, 66.0, 860.0);
    ERROR.store(
        label(cv, "", 202.0, 21.0, 560.0, 12.0, false) as usize,
        Ordering::Relaxed,
    );
    msg1_void_id(
        object(&ERROR),
        sel(c"setTextColor:"),
        msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"systemRedColor")),
    );
    REVERT.store(
        command(
            cv,
            "元に戻す",
            c"sdRefinedRevert:",
            NSRect {
                x: 800.0,
                y: 18.0,
                w: 108.0,
                h: 32.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    APPLY.store(
        command(
            cv,
            "適用",
            c"sdRefinedApply:",
            NSRect {
                x: 920.0,
                y: 18.0,
                w: 96.0,
                h: 32.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    msg1_void_id(
        object(&APPLY),
        sel(c"setBezelColor:"),
        msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"systemBlueColor")),
    );
    let close = command(
        cv,
        "",
        c"sdClosePrefs:",
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        },
    );
    msg1_void_id(close, sel(c"setKeyEquivalent:"), nsstring("w"));
    msg1_void_i64(close, sel(c"setKeyEquivalentModifierMask:"), 1 << 20);
    refined_layout::reset();
    reset_draft();
    BUILT.store(true, Ordering::Relaxed);
    refresh_devices();
    populate_outputs();
    sync_controls();
    let page = args
        .iter()
        .position(|a| a == "--preview-page")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    select_page(page);
    win
}
unsafe fn build_connection(p: ID) {
    icon(
        p,
        "laptopcomputer",
        "このMac",
        NSRect {
            x: 32.0,
            y: 494.0,
            w: 32.0,
            h: 28.0,
        },
    );
    label(p, &crate::hostname_label(), 78.0, 498.0, 420.0, 18.0, false);
    DIAG_BUTTON.store(
        command(
            p,
            "診断",
            c"sdDiagnose:",
            NSRect {
                x: 720.0,
                y: 492.0,
                w: 104.0,
                h: 32.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    RTT.store(
        label(p, "", 78.0, 470.0, 560.0, 14.0, false) as usize,
        Ordering::Relaxed,
    );
    let peer_scroll = msg0(objc_getClass(c"NSScrollView".as_ptr()), sel(c"new"));
    frame(
        peer_scroll,
        NSRect {
            x: 32.0,
            y: 228.0,
            w: 792.0,
            h: 228.0,
        },
    );
    msg1_void_u8(peer_scroll, sel(c"setDrawsBackground:"), 0);
    msg1_void_u8(peer_scroll, sel(c"setHasVerticalScroller:"), 1);
    msg1_void_u8(peer_scroll, sel(c"setAutohidesScrollers:"), 1);
    msg1_void_id(p, sel(c"addSubview:"), peer_scroll);
    let peer_doc = view(
        peer_scroll,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 792.0,
            h: 228.0,
        },
    );
    msg1_void_id(peer_scroll, sel(c"setDocumentView:"), peer_doc);
    PEER_LIST.store(peer_doc as usize, Ordering::Relaxed);
    PEER_SCROLL.store(peer_scroll as usize, Ordering::Relaxed);
    // 接続ページ内診断: peer_scroll と同じ枠に重ねるパネル(初期hidden)。
    // 結果は set_inline_diag で本文を受け取り、閉じるボタンで一覧へ戻る。
    let diag_panel = msg0(objc_getClass(c"NSView".as_ptr()), sel(c"new"));
    frame(
        diag_panel,
        NSRect {
            x: 32.0,
            y: 228.0,
            w: 792.0,
            h: 228.0,
        },
    );
    msg1_void_id(p, sel(c"addSubview:"), diag_panel);
    msg1_void_u8(diag_panel, sel(c"setHidden:"), 1);
    DIAG_PANEL.store(diag_panel as usize, Ordering::Relaxed);
    // 編集不可・選択可のテキストビュー(text_input と同じ作法でスクロールに載せる)
    let init_frame: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let diag_text = init_frame(
        msg0(objc_getClass(c"NSTextView".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 792.0,
            h: 184.0,
        },
    );
    msg1_void_u8(diag_text, sel(c"setEditable:"), 0);
    msg1_void_u8(diag_text, sel(c"setSelectable:"), 1);
    msg1_void_u8(diag_text, sel(c"setRichText:"), 0);
    let diag_scroll = init_frame(
        msg0(objc_getClass(c"NSScrollView".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect {
            x: 0.0,
            y: 44.0,
            w: 792.0,
            h: 184.0,
        },
    );
    msg1_void_u8(diag_scroll, sel(c"setHasVerticalScroller:"), 1);
    msg1_void_u8(diag_scroll, sel(c"setAutohidesScrollers:"), 1);
    msg1_void_id(diag_scroll, sel(c"setDocumentView:"), diag_text);
    msg1_void_id(diag_panel, sel(c"addSubview:"), diag_scroll);
    DIAG_TEXT.store(diag_text as usize, Ordering::Relaxed);
    command(
        diag_panel,
        "閉じる",
        c"sdRefinedDiagClose:",
        NSRect {
            x: 672.0,
            y: 4.0,
            w: 100.0,
            h: 32.0,
        },
    );
    command(
        p,
        "端末を追加…",
        c"sdRegistration:",
        NSRect {
            x: 32.0,
            y: 184.0,
            w: 140.0,
            h: 32.0,
        },
    );
    command(
        p,
        "手動接続",
        c"sdRefinedManual:",
        NSRect {
            x: 32.0,
            y: 132.0,
            w: 150.0,
            h: 32.0,
        },
    );
    let manual = view(
        p,
        NSRect {
            x: 32.0,
            y: 10.0,
            w: 792.0,
            h: 110.0,
        },
    );
    MANUAL.store(manual as usize, Ordering::Relaxed);
    msg1_void_u8(manual, sel(c"setHidden:"), 1);
    HOST_ROLE.store(
        popup(
            manual,
            object(&TARGET),
            &["このMacで待ち受ける", "相手へ接続する"],
            c"sdRefinedRole:",
            NSRect {
                x: 0.0,
                y: 70.0,
                w: 240.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    let host = if UI_PREVIEW.load(Ordering::Relaxed) {
        String::new()
    } else {
        crate::envutil::get("KNIT_HOST").unwrap_or_default()
    };
    HOST.store(
        text_field(
            manual,
            object(&TARGET),
            None,
            "接続先のIPアドレス",
            "192.168.1.10",
            &host,
            NSRect {
                x: 0.0,
                y: 25.0,
                w: 380.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    command(
        manual,
        "保存して再接続",
        c"sdRefinedSaveHost:",
        NSRect {
            x: 400.0,
            y: 22.0,
            w: 180.0,
            h: 32.0,
        },
    );
    msg1_void_i64(
        object(&HOST_ROLE),
        sel(c"selectItemAtIndex:"),
        crate::effective_client_role() as i64,
    );
}
unsafe fn build_layout(p: ID) {
    command(
        p,
        "画面を識別",
        c"sdRefinedIdentify:",
        NSRect {
            x: 548.0,
            y: 508.0,
            w: 126.0,
            h: 30.0,
        },
    );
    command(
        p,
        "数値で指定",
        c"sdRefinedRanges:",
        NSRect {
            x: 686.0,
            y: 508.0,
            w: 138.0,
            h: 30.0,
        },
    );
    refined_layout::make_view(
        p,
        NSRect {
            x: 24.0,
            y: 102.0,
            w: 808.0,
            h: 390.0,
        },
    );
    line(p, 32.0, 86.0, 792.0);
    icon(
        p,
        "computermouse",
        "選択した端末",
        NSRect {
            x: 32.0,
            y: 38.0,
            w: 25.0,
            h: 26.0,
        },
    );
    label(p, "ポインター感度", 66.0, 40.0, 160.0, 14.0, false);
    label(p, "0.2", 246.0, 40.0, 36.0, 12.0, true);
    SENS.store(
        range_slider(
            p,
            "選択した端末のポインター感度",
            NSRect {
                x: 286.0,
                y: 36.0,
                w: 286.0,
                h: 28.0,
            },
            0.2,
            3.0,
            c"sdRefinedSensitivity:",
        ) as usize,
        Ordering::Relaxed,
    );
    label(p, "3.0", 582.0, 40.0, 36.0, 12.0, true);
    SENS_VALUE.store(
        text_field(
            p,
            object(&TARGET),
            Some(c"sdRefinedSensitivity:"),
            "ポインター感度の倍率",
            "1.0",
            "1.0",
            NSRect {
                x: 628.0,
                y: 36.0,
                w: 62.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    command(
        p,
        "標準に戻す",
        c"sdRefinedSensitivityReset:",
        NSRect {
            x: 704.0,
            y: 34.0,
            w: 120.0,
            h: 32.0,
        },
    );
}
unsafe fn build_sharing(p: ID) {
    label(p, "Windows音声", 32.0, 494.0, 360.0, 22.0, false);
    label(p, "コピーとファイル", 458.0, 494.0, 370.0, 22.0, false);
    toggle(p, "Macで再生", 0, 32.0, 434.0, 360.0);
    label(p, "音量", 32.0, 375.0, 100.0, 14.0, false);
    GAIN.store(
        range_slider(
            p,
            "Windows音声の音量",
            NSRect {
                x: 132.0,
                y: 372.0,
                w: 200.0,
                h: 28.0,
            },
            0.0,
            2.0,
            c"sdRefinedGain:",
        ) as usize,
        Ordering::Relaxed,
    );
    GAIN_VALUE.store(
        label(p, "", 337.0, 375.0, 58.0, 13.0, true) as usize,
        Ordering::Relaxed,
    );
    label(p, "出力先", 32.0, 316.0, 100.0, 14.0, false);
    OUTPUT.store(
        dynamic_copy_scope(|| {
            popup(
                p,
                object(&TARGET),
                &["システム既定"],
                c"sdRefinedOutput:",
                NSRect {
                    x: 132.0,
                    y: 312.0,
                    w: 260.0,
                    h: 30.0,
                },
            )
        }) as usize,
        Ordering::Relaxed,
    );
    toggle(p, "Windowsスピーカーを消音", 1, 32.0, 254.0, 360.0);
    command(
        p,
        "出力を確認…",
        c"sdRefinedTestOutput:",
        NSRect {
            x: 32.0,
            y: 194.0,
            w: 148.0,
            h: 32.0,
        },
    );
    toggle(p, "コピー", 2, 458.0, 434.0, 366.0);
    toggle(p, "ファイル", 3, 458.0, 380.0, 366.0);
    line(p, 458.0, 362.0, 366.0);
    label(p, "受信フォルダ", 458.0, 316.0, 366.0, 14.0, false);
    FOLDER.store(
        label(p, "", 458.0, 276.0, 245.0, 13.0, false) as usize,
        Ordering::Relaxed,
    );
    command(
        p,
        "変更…",
        c"sdRefinedFolder:",
        NSRect {
            x: 700.0,
            y: 270.0,
            w: 72.0,
            h: 30.0,
        },
    );
    command(
        p,
        "開く",
        c"sdRefinedOpenFolder:",
        NSRect {
            x: 778.0,
            y: 270.0,
            w: 48.0,
            h: 30.0,
        },
    );
    line(p, 458.0, 246.0, 366.0);
    toggle(p, "受信後にフォルダを開く", 8, 458.0, 214.0, 366.0);
    label(p, "履歴", 458.0, 196.0, 366.0, 15.0, false);
    toggle(p, "コピーを保存", 4, 458.0, 150.0, 250.0);
    label(p, "保存件数", 716.0, 153.0, 100.0, 14.0, false);
    HISTORY_LIMIT.store(
        popup(
            p,
            object(&TARGET),
            &["10件", "50件", "100件", "200件"],
            c"sdRefinedHistoryLimit:",
            NSRect {
                x: 690.0,
                y: 148.0,
                w: 126.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    command(
        p,
        "履歴を消去…",
        c"sdHistoryClear:",
        NSRect {
            x: 458.0,
            y: 94.0,
            w: 150.0,
            h: 32.0,
        },
    );
    // 履歴の保存件数を減らしても画像の実体は images/ に最大 512MiB 残る
    // (刈り込みは 60 件/512MiB までしか見ない)。置き場と上限を消去ボタンの隣に
    // 出す(幅 366pt に収まる長さにする。消える量は全消去の確認文に実数で出す)
    let store_note = format!(
        "画像の実体: ~/.config/knit/images(最大 {})",
        crate::human_bytes(knit_common::history::MAX_IMAGE_STORE_BYTES)
    );
    label(p, &store_note, 458.0, 126.0, 366.0, 12.0, true);
}
unsafe fn build_general(p: ID) {
    // 切替キー・更新の自動確認・キー変換プレビューを増やした分だけ文書を伸ばす
    // (ページはスクロールビューのため、高さだけ変えればよい)
    frame(
        p,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 860.0,
            h: 830.0,
        },
    );
    label(p, "一般", 74.0, 788.0, 700.0, 15.0, false);
    label(p, "言語", 90.0, 742.0, 420.0, 14.0, false);
    LANGUAGE.store(
        popup(
            p,
            object(&TARGET),
            &["日本語", "English"],
            c"sdRefinedChanged:",
            NSRect {
                x: 580.0,
                y: 738.0,
                w: 188.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    toggle(p, "ログイン時に起動", 5, 90.0, 690.0, 678.0);
    // 切替キー: 既存4候補+「ほかのキー…」(キーコード入力ダイアログ)。
    // 適用で preferences の hotkey へ保存される(tap は毎回 HOTKEY_KC を読む)
    label(p, "切替キー", 90.0, 644.0, 420.0, 14.0, false);
    HOTKEY.store(
        popup(
            p,
            object(&TARGET),
            &[
                "F6(必要に応じてfnと併用)",
                "F8(必要に応じてfnと併用)",
                "F13(既定)",
                "右⌘",
                "ほかのキー…",
            ],
            c"sdRefinedHotkey:",
            NSRect {
                x: 580.0,
                y: 640.0,
                w: 188.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    // sync() が毎秒選択位置を書き換えるため、開示中保護を付ける(旧UIと同じ作法)
    let hotkey_menu = msg0(object(&HOTKEY), sel(c"menu"));
    if !hotkey_menu.is_null() {
        msg1_void_id(hotkey_menu, sel(c"setDelegate:"), object(&TARGET));
        super::HOTKEY_MENU.store(hotkey_menu as usize, Ordering::Relaxed);
    }
    // env 固定(KNIT_HOTKEY_KC)の注記行。固定が無いときは空=見えない
    HOTKEY_ENV_NOTE.store(
        label(p, "", 90.0, 616.0, 678.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    let hotkey_env_fixed =
        !UI_PREVIEW.load(Ordering::Relaxed) && crate::envutil::get("KNIT_HOTKEY_KC").is_some();
    if hotkey_env_fixed {
        let note = localization::copy("環境変数 KNIT_HOTKEY_KC で固定中のため変更できません");
        text(object(&HOTKEY_ENV_NOTE), &note);
        msg1_void_id(
            object(&HOTKEY_ENV_NOTE),
            sel(c"setToolTip:"),
            nsstring(&note),
        );
        enabled(object(&HOTKEY), false);
    }
    toggle(p, "更新を自動で確認", 9, 90.0, 584.0, 678.0);
    line(p, 74.0, 560.0, 710.0);
    // キー変換のプレビュー(現在の CMD_ALT で sync が組み立て直す)
    let conv_caption = label(p, "キー変換", 74.0, 526.0, 700.0, 15.0, false);
    if !conv_caption.is_null() {
        tooltip(
            conv_caption,
            "Windows 操作中の Mac 流キー操作の対応です。⌘の対応先は環境変数 KNIT_CMD_ALT と設定(cmd_alt)で Ctrl/Alt に切り替わります。右⌘は常に右Ctrlです",
        );
    }
    for (i, y) in [492.0, 470.0, 448.0, 426.0].iter().enumerate() {
        KEY_CONV_LINES[i].store(
            label(p, "", 90.0, *y, 700.0, 13.0, true) as usize,
            Ordering::Relaxed,
        );
    }
    line(p, 74.0, 402.0, 710.0);
    label(p, "通知", 74.0, 368.0, 700.0, 15.0, false);
    toggle(p, "接続", 6, 90.0, 322.0, 678.0);
    toggle(p, "転送完了", 7, 90.0, 274.0, 678.0);
    line(p, 74.0, 252.0, 710.0);
    label(p, "設定", 74.0, 218.0, 700.0, 15.0, false);
    command(
        p,
        "書き出す…",
        c"sdRefinedExport:",
        NSRect {
            x: 90.0,
            y: 174.0,
            w: 330.0,
            h: 30.0,
        },
    );
    command(
        p,
        "読み込む…",
        c"sdRefinedImport:",
        NSRect {
            x: 438.0,
            y: 174.0,
            w: 330.0,
            h: 30.0,
        },
    );
    label(p, "メンテナンス", 74.0, 130.0, 700.0, 15.0, false);
    label(p, "ログ", 90.0, 90.0, 420.0, 14.0, false);
    command(
        p,
        "保存…",
        c"sdRefinedLog:",
        NSRect {
            x: 642.0,
            y: 84.0,
            w: 126.0,
            h: 30.0,
        },
    );
    label(p, "Knit", 90.0, 50.0, 420.0, 14.0, false);
    command(
        p,
        "再起動…",
        c"sdRefinedRestart:",
        NSRect {
            x: 642.0,
            y: 44.0,
            w: 126.0,
            h: 30.0,
        },
    );
    command(
        p,
        "初期化…",
        c"sdRefinedReset:",
        NSRect {
            x: 74.0,
            y: 8.0,
            w: 134.0,
            h: 30.0,
        },
    );
    // build 時点でポップアップの選択とプレビュー本文を整える(毎秒の sync() が
    // 同じ値を保つため、初回表示が空にならない)
    sync_hotkey_popup(crate::hotkey_kc());
    let cmd_alt_now = crate::CMD_ALT.load(Ordering::Relaxed);
    for (slot, (from, to)) in KEY_CONV_LINES
        .iter()
        .zip(key_conversion_preview(cmd_alt_now))
    {
        text(object(slot), &format!("{from} → {to}"));
    }
}

pub(in crate::gui) unsafe fn select_page(index: usize) {
    let index = index.min(3);
    PAGE.store(index, Ordering::Relaxed);
    for i in 0..4 {
        msg1_void_u8(object(&PANELS[i]), sel(c"setHidden:"), (i != index) as u8);
        let b = object(&NAV[i]);
        msg1_void_i64(b, sel(c"setState:"), (i == index) as i64);
        let c = msg0(
            objc_getClass(c"NSColor".as_ptr()),
            sel(if i == index {
                c"controlAccentColor"
            } else {
                c"labelColor"
            }),
        );
        msg1_void_id(b, sel(c"setContentTintColor:"), c);
        msg1_void_u8(b, sel(c"setWantsLayer:"), 1);
        let f: unsafe extern "C" fn(ID, SEL, f64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(msg0(b, sel(c"layer")), sel(c"setCornerRadius:"), 8.0);
        let alpha = msg1_id_f64(
            msg0(
                objc_getClass(c"NSColor".as_ptr()),
                sel(c"controlAccentColor"),
            ),
            sel(c"colorWithAlphaComponent:"),
            if i == index { 0.12 } else { 0.0 },
        );
        msg1_void_id(
            msg0(b, sel(c"layer")),
            sel(c"setBackgroundColor:"),
            msg0(alpha, sel(c"CGColor")),
        );
    }
    update_buttons();
}
unsafe fn update_buttons() {
    let changed =
        draft_dirty() || refined_layout::dirty() || OUTPUT_RESELECT.load(Ordering::Relaxed);
    enabled(object(&APPLY), changed);
    enabled(object(&REVERT), changed);
}
unsafe fn populate_outputs() {
    let mut devices = vec![(String::new(), localization::copy("システム既定"))];
    if !UI_PREVIEW.load(Ordering::Relaxed) {
        devices.extend(crate::audio::output_devices());
    }
    let desired = with_draft(|d| d.output.clone());
    if !desired.is_empty() && !devices.iter().any(|(id, _)| *id == desired) {
        devices.push((desired.clone(), localization::copy("未接続の出力先")));
    }
    if *OUTPUTS.lock().unwrap_or_else(|e| e.into_inner()) == devices {
        msg1_void_i64(
            object(&OUTPUT),
            sel(c"selectItemAtIndex:"),
            devices
                .iter()
                .position(|(id, _)| *id == desired)
                .unwrap_or(0) as i64,
        );
        return;
    }
    let p = object(&OUTPUT);
    msg0_void(p, sel(c"removeAllItems"));
    for (_, name) in &devices {
        msg1_void_id(p, sel(c"addItemWithTitle:"), nsstring(name));
    }
    let index = devices
        .iter()
        .position(|(id, _)| *id == desired)
        .unwrap_or(0);
    msg1_void_i64(p, sel(c"selectItemAtIndex:"), index as i64);
    *OUTPUTS.lock().unwrap_or_else(|e| e.into_inner()) = devices;
}
unsafe fn sync_controls() {
    let d = with_draft(|d| d.clone());
    let states = [
        d.playback,
        d.speaker,
        d.clip,
        d.files,
        d.history,
        d.startup,
        d.desktop.notify_connection,
        d.desktop.notify_transfer,
        d.desktop.open_receive_folder,
        d.update_check,
    ];
    for (slot, on) in SWITCHES.iter().zip(states) {
        msg1_void_i64(object(slot), sel(c"setState:"), on as i64);
    }
    number(object(&GAIN), d.gain);
    text(
        object(&GAIN_VALUE),
        &format!("{}%", (d.gain * 100.0).round() as i32),
    );
    msg1_void_i64(
        object(&LANGUAGE),
        sel(c"selectItemAtIndex:"),
        (d.language == Language::English) as i64,
    );
    let folder = if d.desktop.receive_folder.is_empty() {
        "Downloads / Knit".into()
    } else {
        d.desktop.receive_folder
    };
    text(object(&FOLDER), &folder);
    // 範囲外の既存値は読み込み時 50 扱いで表示する
    let limits = [10usize, 50, 100, 200];
    let index = limits
        .iter()
        .position(|&n| n == d.desktop.history_limit)
        .unwrap_or(1);
    msg1_void_i64(
        object(&HISTORY_LIMIT),
        sel(c"selectItemAtIndex:"),
        index as i64,
    );
    // 切替キーのポップアップも下書きへ揃える(適用・元に戻すの直後に反映。
    // 開示中は毎秒の sync() と同じ保護を付ける)
    if !super::HOTKEY_MENU_OPEN.load(Ordering::Relaxed) {
        sync_hotkey_popup(d.hotkey);
    }
    layout_changed();
    update_buttons();
}
pub(in crate::gui) unsafe fn layout_changed() {
    if let Some(s) = refined_layout::settings_for_selected() {
        enabled(object(&SENS), true);
        enabled(object(&SENS_VALUE), true);
        number(object(&SENS), s.sensitivity);
        text(object(&SENS_VALUE), &format!("{:.1}", s.sensitivity));
        if let Some((id, _)) = refined_layout::selected() {
            let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
            let name = peers
                .iter()
                .find(|p| p.id == id)
                .map(|p| crate::conn::peer_label(p).to_owned())
                .unwrap_or(id);
            msg1_void_id(
                object(&SENS),
                sel(c"setAccessibilityLabel:"),
                nsstring(&format!("{name} ポインター感度")),
            );
        }
    } else {
        enabled(object(&SENS), false);
        enabled(object(&SENS_VALUE), false);
    }
    update_buttons();
}
unsafe fn refresh_devices() {
    dynamic_copy_scope(|| refresh_devices_inner());
}
unsafe fn refresh_devices_inner() {
    let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
    // alias を含める: 「名前…」でのエイリアス変更もこの signature で再描画される
    let signature = peers
        .iter()
        .map(|p| format!("{}:{}:{}:{:?}", p.id, p.gen, p.name, p.alias))
        .collect::<Vec<_>>()
        .join("|");
    let mut old = PEER_SIGNATURE.lock().unwrap_or_else(|e| e.into_inner());
    if *old == signature
        && !PEER_IDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    {
        return;
    }
    *old = signature;
    let parent = object(&PEER_LIST);
    let children = msg0(parent, sel(c"subviews"));
    let copy = msg0(children, sel(c"copy"));
    let n = crate::msg0_isize(copy, sel(c"count")).max(0);
    let at: unsafe extern "C" fn(ID, SEL, isize) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    for i in 0..n {
        msg0_void(
            at(copy, sel(c"objectAtIndex:"), i),
            sel(c"removeFromSuperview"),
        );
    }
    msg0_void(copy, sel(c"release"));
    let mut ids = Vec::new();
    if peers.is_empty() {
        label(
            parent,
            localization::text("未接続", "Not connected"),
            0.0,
            166.0,
            600.0,
            15.0,
            true,
        );
    }
    let doc_height = (peers.len() as f64 * 72.0 + 12.0).max(228.0);
    frame(
        parent,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 792.0,
            h: doc_height,
        },
    );
    for (i, p) in peers.iter().enumerate() {
        let y = doc_height - 62.0 - i as f64 * 72.0;
        icon(
            parent,
            if p.id.starts_with("android-") {
                "ipad"
            } else {
                "display"
            },
            crate::conn::peer_label(p),
            NSRect {
                x: 4.0,
                y: y - 4.0,
                w: 34.0,
                h: 34.0,
            },
        );
        label(
            parent,
            crate::conn::peer_label(p),
            56.0,
            y,
            360.0,
            16.0,
            false,
        );
        label(
            parent,
            localization::text("接続中", "Connected"),
            440.0,
            y,
            140.0,
            13.0,
            true,
        );
        let b = command(
            parent,
            "再接続",
            c"sdRefinedReconnect:",
            NSRect {
                x: 670.0,
                y: y - 6.0,
                w: 110.0,
                h: 30.0,
            },
        );
        msg1_void_i64(b, sel(c"setTag:"), i as i64);
        // 名前…(エイリアス): 空欄で確定するとコンピュータ名へ戻る。peer_label が
        // エイリアス優先で表示するため、保存後は refresh_devices で行を作り直す
        let n = command(
            parent,
            "名前…",
            c"sdRefinedRename:",
            NSRect {
                x: 584.0,
                y: y - 6.0,
                w: 80.0,
                h: 30.0,
            },
        );
        msg1_void_i64(n, sel(c"setTag:"), i as i64);
        ids.push(p.id.clone());
        line(parent, 0.0, y - 20.0, 792.0);
    }
    *PEER_IDS.lock().unwrap_or_else(|e| e.into_inner()) = ids;
    let point: unsafe extern "C" fn(ID, SEL, CGPoint2) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    point(
        parent,
        sel(c"scrollPoint:"),
        CGPoint2 {
            x: 0.0,
            y: (doc_height - 228.0).max(0.0),
        },
    );
}
pub(in crate::gui) unsafe fn sync() {
    refresh_surface();
    if merge_runtime() {
        sync_controls();
    }
    refresh_devices();
    // RTT表示行: 接続中のみ。直近3回以上なら中央値・最大も添える
    let rtt = crate::RTT_MS.load(Ordering::Relaxed);
    let line = if !UI_PREVIEW.load(Ordering::Relaxed)
        && crate::CONNECTED.load(Ordering::Relaxed)
        && rtt > 0
    {
        let samples = crate::RTT_SAMPLES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if samples.len() >= 3 {
            let (med, max, n) = crate::summarize_rtt(&samples).unwrap_or((rtt, rtt, 1));
            localization::text(
                &format!("遅延 {rtt}ms・直近{n}回 中央値 {med}ms・最大 {max}ms"),
                &format!("Latency {rtt}ms · last {n}: median {med}ms · max {max}ms"),
            )
            .to_string()
        } else {
            localization::text(&format!("遅延 {rtt}ms"), &format!("Latency {rtt}ms")).to_string()
        }
    } else {
        String::new()
    };
    text(object(&RTT), &line);
    msg1_void_u8(object(&RTT), sel(c"setHidden:"), line.is_empty() as u8);
    // 診断の実行中はボタン名を「診断中…」へ戻す(メニュー側と同じ作法)
    let diag_title = if super::super::diag_running() {
        localization::copy("診断中…")
    } else {
        localization::copy("診断")
    };
    let b = object(&DIAG_BUTTON);
    if !b.is_null() {
        msg1_void_id(b, sel(c"setTitle:"), nsstring(&diag_title));
    }
    if !UI_PREVIEW.load(Ordering::Relaxed) {
        crate::audio::check_output_topology();
    }
    static LAST_OUTPUT_REFRESH: AtomicU64 = AtomicU64::new(0);
    let now = crate::now_ms();
    if now.saturating_sub(LAST_OUTPUT_REFRESH.load(Ordering::Relaxed)) > 3000 {
        LAST_OUTPUT_REFRESH.store(now, Ordering::Relaxed);
        populate_outputs();
    }
    let cap = knit_common::share::env_cap();
    enabled(object(&SWITCHES[2]), cap.clip);
    enabled(object(&SWITCHES[3]), cap.files);
    enabled(object(&SWITCHES[4]), cap.clip);
    let audio = crate::PEERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|p| !p.id.starts_with("android-"))
        || UI_PREVIEW.load(Ordering::Relaxed);
    enabled(object(&SWITCHES[0]), cap.audio && audio);
    enabled(
        object(&SWITCHES[1]),
        cap.audio && audio && crate::envutil::get("KNIT_MUTE_SPK").is_none(),
    );
    enabled(
        object(&GAIN),
        cap.audio && with_draft(|d| d.playback) && audio,
    );
    if let Some(s) = crate::audio::output_status() {
        error(&s);
    }
    // その他ページ: 切替キーポップアップ(開示中は書き換えない)とキー変換の
    // プレビュー(現在の CMD_ALT・env 固定を含む実行値から毎秒組み立て直す)
    if !super::HOTKEY_MENU_OPEN.load(Ordering::Relaxed) {
        sync_hotkey_popup(with_draft(|d| d.hotkey));
    }
    let cmd_alt_now = crate::CMD_ALT.load(Ordering::Relaxed);
    for (slot, (from, to)) in KEY_CONV_LINES
        .iter()
        .zip(key_conversion_preview(cmd_alt_now))
    {
        text(object(slot), &format!("{from} → {to}"));
    }
    let note = SAVE_ERROR_NOTE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(s) = save_error_display(localization::language(), &note) {
        error(&s);
    }
    refined_layout::redraw();
    update_buttons();
}

pub(in crate::gui) unsafe extern "C" fn noop(_s: ID, _c: SEL, _sender: ID) {}
pub(in crate::gui) unsafe extern "C" fn role_changed(_s: ID, _c: SEL, sender: ID) {
    let want = crate::msg0_isize(sender, sel(c"indexOfSelectedItem")) == 1;
    if !UI_PREVIEW.load(Ordering::Relaxed) && !super::super::role_env_guard() {
        super::super::set_role(want);
    }
    msg1_void_i64(
        sender,
        sel(c"selectItemAtIndex:"),
        crate::effective_client_role() as i64,
    );
}
pub(in crate::gui) unsafe extern "C" fn save_host(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    if alert(
        "保存して再接続",
        "接続先を保存し、Knitを再起動します。現在の接続はいったん切れます。",
        Some("再接続"),
    ) {
        super::super::imp_save_host(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}
pub(in crate::gui) unsafe extern "C" fn changed(_s: ID, _c: SEL, _sender: ID) {
    with_draft(|d| {
        d.language = if crate::msg0_isize(object(&LANGUAGE), sel(c"indexOfSelectedItem")) == 1 {
            Language::English
        } else {
            Language::Japanese
        }
    });
    update_buttons();
}
pub(in crate::gui) unsafe extern "C" fn toggle_changed(_s: ID, _c: SEL, sender: ID) {
    let i = crate::msg0_isize(sender, sel(c"tag"));
    let on = crate::msg0_isize(sender, sel(c"state")) != 0;
    with_draft(|d| match i {
        0 => d.playback = on,
        1 => d.speaker = on,
        2 => d.clip = on,
        3 => d.files = on,
        4 => d.history = on,
        5 => d.startup = on,
        6 => d.desktop.notify_connection = on,
        7 => d.desktop.notify_transfer = on,
        8 => d.desktop.open_receive_folder = on,
        9 => d.update_check = on,
        _ => {}
    });
    sync();
}
pub(in crate::gui) unsafe extern "C" fn history_limit_changed(_s: ID, _c: SEL, sender: ID) {
    let index = crate::msg0_isize(sender, sel(c"indexOfSelectedItem")).max(0) as usize;
    if let Some(limit) = [10usize, 50, 100, 200].get(index) {
        with_draft(|d| d.desktop.history_limit = *limit);
    }
    update_buttons();
}
/// 切替キーポップアップの選択と「ほかのキー…」項目の表示を下書きへ揃える。
/// sync() が毎秒呼ぶため、開示中(HOTKEY_MENU_OPEN)は書き換えない
unsafe fn sync_hotkey_popup(kc: i64) {
    let pop = object(&HOTKEY);
    if pop.is_null() {
        return;
    }
    let item_at: unsafe extern "C" fn(ID, SEL, isize) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let other = item_at(
        pop,
        sel(c"itemAtIndex:"),
        super::HOTKEY_CHOICES.len() as isize,
    );
    let index = match super::HOTKEY_CHOICES.iter().position(|k| *k == kc) {
        Some(i) => {
            if !other.is_null() {
                msg1_void_id(
                    other,
                    sel(c"setTitle:"),
                    nsstring(&localization::copy("ほかのキー…")),
                );
            }
            i
        }
        // 未知のキー(env・コード入力)は末尾の「ほかのキー…(コードN)」を選ぶ
        None => {
            if !other.is_null() {
                let title = localization::copy(&format!(
                    "ほかのキー…({})",
                    knit_common::keymap::mac_key_label(kc)
                ));
                msg1_void_id(other, sel(c"setTitle:"), nsstring(&title));
            }
            super::HOTKEY_CHOICES.len()
        }
    };
    msg1_void_i64(pop, sel(c"selectItemAtIndex:"), index as i64);
}
/// 切替キーポップアップ。既存4候補は即座に下書きへ。「ほかのキー…」は
/// キーコード入力のダイアログを開く(1..=127検証。キャンセル・失敗時は
/// 選択を下書きの値へ戻す)
pub(in crate::gui) unsafe extern "C" fn hotkey_changed(_s: ID, _c: SEL, _sender: ID) {
    let index = crate::msg0_isize(object(&HOTKEY), sel(c"indexOfSelectedItem")).max(0) as usize;
    if let Some(k) = super::HOTKEY_CHOICES.get(index) {
        with_draft(|d| d.hotkey = *k);
    } else {
        dynamic_copy_scope(|| {
            let current = with_draft(|d| d.hotkey);
            let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
            msg1_void_id(
                a,
                sel(c"setMessageText:"),
                nsstring(&localization::copy("切替キー")),
            );
            let v = msg0(objc_getClass(c"NSView".as_ptr()), sel(c"new"));
            frame(
                v,
                NSRect {
                    x: 0.0,
                    y: 0.0,
                    w: 380.0,
                    h: 36.0,
                },
            );
            let field = text_field(
                v,
                object(&TARGET),
                None,
                "切替キーのキーコード(HID)",
                "例: 97",
                &current.to_string(),
                NSRect {
                    x: 0.0,
                    y: 4.0,
                    w: 380.0,
                    h: 28.0,
                },
            );
            msg1_void_id(a, sel(c"setAccessoryView:"), v);
            msg1_void_id(
                a,
                sel(c"setInformativeText:"),
                nsstring(&localization::copy(
                    "キーコード(HID)を1〜127の数値で入力してください。主なキー: F6=97・F8=100・F13=105(既定)・F14=107・右⌘=54",
                )),
            );
            msg1_void_id(
                a,
                sel(c"addButtonWithTitle:"),
                nsstring(&localization::copy("キャンセル")),
            );
            msg1_void_id(
                a,
                sel(c"addButtonWithTitle:"),
                nsstring(&localization::copy("登録")),
            );
            // モーダル表示時にコード入力欄へ直接打てるようにする
            let win = msg0(a, sel(c"window"));
            if !win.is_null() {
                msg1_void_id(win, sel(c"setInitialFirstResponder:"), field);
            }
            if crate::msg0_isize(a, sel(c"runModal")) == 1001 {
                match string(field).trim().parse::<i64>() {
                    Ok(n) if (1..=127).contains(&n) => with_draft(|d| d.hotkey = n),
                    _ => error("キーコードは1〜127の数値で指定してください"),
                }
            }
        });
        // ダイアログの結果(成功・失敗・キャンセル)にかかわらず選択を下書きへ戻す
        sync_hotkey_popup(with_draft(|d| d.hotkey));
    }
    update_buttons();
}
/// 接続ページの「名前…」(エイリアス編集)。初期値は現在のエイリアスまたは
/// コンピュータ名。空欄で確定=エイリアス削除(コンピュータ名へ戻る)。
/// 保存は tap::set_peer_alias(peer-sides.json)へ即座に反映し、一覧を作り直す
pub(in crate::gui) unsafe extern "C" fn rename_peer(_s: ID, _c: SEL, sender: ID) {
    let index = crate::msg0_isize(sender, sel(c"tag")).max(0) as usize;
    let id = PEER_IDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(index)
        .cloned();
    let Some(id) = id else {
        return;
    };
    if UI_PREVIEW.load(Ordering::Relaxed) {
        error("プレビューでは名前を変更しません");
        return;
    }
    let initial = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        peers
            .iter()
            .find(|p| p.id == id)
            // 初期値=エイリアス(無ければコンピュータ名)
            .map(|p| p.alias.clone().unwrap_or_else(|| p.name.clone()))
            .unwrap_or_default()
    };
    dynamic_copy_scope(|| {
        let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
        msg1_void_id(
            a,
            sel(c"setMessageText:"),
            nsstring(&localization::copy("端末の名前")),
        );
        let v = msg0(objc_getClass(c"NSView".as_ptr()), sel(c"new"));
        frame(
            v,
            NSRect {
                x: 0.0,
                y: 0.0,
                w: 420.0,
                h: 36.0,
            },
        );
        let field = text_field(
            v,
            object(&TARGET),
            None,
            "端末の名前",
            "例: 書斎のWindows",
            &initial,
            NSRect {
                x: 0.0,
                y: 4.0,
                w: 420.0,
                h: 28.0,
            },
        );
        msg1_void_id(a, sel(c"setAccessoryView:"), v);
        msg1_void_id(
            a,
            sel(c"setInformativeText:"),
            nsstring(&localization::copy(
                "空欄で確定すると名前を削除し、コンピュータ名へ戻ります",
            )),
        );
        msg1_void_id(
            a,
            sel(c"addButtonWithTitle:"),
            nsstring(&localization::copy("キャンセル")),
        );
        msg1_void_id(
            a,
            sel(c"addButtonWithTitle:"),
            nsstring(&localization::copy("変更")),
        );
        // モーダル表示時に名前入力欄へ直接打てるようにする
        let win = msg0(a, sel(c"window"));
        if !win.is_null() {
            msg1_void_id(win, sel(c"setInitialFirstResponder:"), field);
        }
        if crate::msg0_isize(a, sel(c"runModal")) != 1001 {
            return;
        }
        let value = string(field);
        let alias = if value.trim().is_empty() {
            None
        } else {
            Some(value.as_str())
        };
        match crate::tap::set_peer_alias(&id, alias) {
            Ok(()) => refresh_devices(),
            Err(e) => error(&e),
        }
    });
}
pub(in crate::gui) unsafe extern "C" fn diag_close(_s: ID, _c: SEL, _sender: ID) {
    // 診断パネルを閉じて端末一覧へ戻す
    msg1_void_u8(object(&DIAG_PANEL), sel(c"setHidden:"), 1);
    msg1_void_u8(object(&PEER_SCROLL), sel(c"setHidden:"), 0);
}
/// 診断結果を接続ページ内のパネルへ出す(メインスレッドからのみ呼ぶ)。
/// peer_scroll を隠してパネルを表示する。閉じるのはユーザー操作(diag_close)。
pub(in crate::gui) unsafe fn set_inline_diag(text: &str) {
    let panel = object(&DIAG_PANEL);
    if panel.is_null() {
        return;
    }
    let tv = object(&DIAG_TEXT);
    msg1_void_id(tv, sel(c"setString:"), nsstring(text));
    let scroll = msg0(tv, sel(c"enclosingScrollView"));
    if !scroll.is_null() {
        let top: unsafe extern "C" fn(ID, SEL, CGPoint2) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        top(scroll, sel(c"scrollPoint:"), CGPoint2 { x: 0.0, y: 0.0 });
    }
    msg1_void_u8(object(&PEER_SCROLL), sel(c"setHidden:"), 1);
    msg1_void_u8(panel, sel(c"setHidden:"), 0);
}
/// 設定画面(refined)のウィンドウが今あるか(診断結果の表示先の判定に使う)
pub(in crate::gui) unsafe fn refined_window_visible() -> bool {
    if !BUILT.load(Ordering::Relaxed) {
        return false;
    }
    let win = object(&WINDOW);
    !win.is_null() && crate::msg0_isize(win, sel(c"isVisible")) != 0
}
pub(in crate::gui) unsafe extern "C" fn gain_changed(_s: ID, _c: SEL, sender: ID) {
    let gain = value(sender).clamp(0.0, 2.0);
    with_draft(|d| d.gain = gain);
    text(
        object(&GAIN_VALUE),
        &format!("{}%", (gain * 100.0).round() as i32),
    );
    update_buttons();
}
pub(in crate::gui) unsafe extern "C" fn sensitivity_changed(_s: ID, _c: SEL, sender: ID) {
    let n = value(sender);
    if n.is_finite() && (0.2..=3.0).contains(&n) {
        refined_layout::set_sensitivity(n);
    } else {
        error("感度は0.2〜3.0で指定してください");
    }
    layout_changed();
}
pub(in crate::gui) unsafe extern "C" fn sensitivity_reset(_s: ID, _c: SEL, _sender: ID) {
    refined_layout::set_sensitivity(1.0);
    layout_changed();
}
pub(in crate::gui) unsafe extern "C" fn output_changed(_s: ID, _c: SEL, sender: ID) {
    let index = crate::msg0_isize(sender, sel(c"indexOfSelectedItem")).max(0) as usize;
    if let Some((uid, _)) = OUTPUTS.lock().unwrap_or_else(|e| e.into_inner()).get(index) {
        with_draft(|d| d.output = uid.clone());
    }
    OUTPUT_RESELECT.store(true, Ordering::Relaxed);
    update_buttons();
}
pub(in crate::gui) unsafe extern "C" fn revert(_s: ID, _c: SEL, _sender: ID) {
    OUTPUT_RESELECT.store(false, Ordering::Relaxed);
    reset_draft();
    refined_layout::reset();
    populate_outputs();
    sync_controls();
    error("");
}
unsafe fn rollback_general(before: &Draft) {
    let mut notes = Vec::new();
    if crate::desktop_settings::snapshot() != before.desktop {
        if let Err(e) = crate::desktop_settings::apply(before.desktop.clone()) {
            notes.push(e);
        }
    }
    if crate::desktop_settings::startup_enabled() != before.startup {
        if let Err(e) = crate::desktop_settings::set_startup_enabled(before.startup) {
            notes.push(e);
        }
    }
    if !UI_PREVIEW.load(Ordering::Relaxed) && crate::audio::output_uid() != before.output {
        if let Err(e) = crate::audio::set_output_uid(&before.output) {
            notes.push(e);
        }
    }
    if !notes.is_empty() {
        error(&format!(
            "一部の設定を元に戻せません: {}",
            notes.join(" / ")
        ));
    }
}
pub(in crate::gui) unsafe extern "C" fn apply(_s: ID, _c: SEL, _sender: ID) {
    merge_runtime();
    let d = with_draft(|d| d.clone());
    let before = Draft::current();
    let before_preferences = preferences::snapshot();
    if crate::WIN_MODE.load(Ordering::Relaxed) {
        error("Macへ戻ってから適用してください");
        return;
    }
    if let Err(e) = refined_layout::validate_pending() {
        error(&e);
        return;
    }
    let folder_changed = crate::desktop_settings::receive_dir_change_requires_restart(&d.desktop);
    if folder_changed
        && d.desktop.receive_folder != before.desktop.receive_folder
        && !UI_PREVIEW.load(Ordering::Relaxed)
        && !alert(
            "受信フォルダを変更",
            "新しい保存先はKnitの再起動後から使います。今の接続と転送先はそのままです。",
            Some("保存"),
        )
    {
        return;
    }
    if d.desktop != before.desktop {
        if let Err(e) = crate::desktop_settings::apply(d.desktop.clone()) {
            error(&e);
            return;
        }
        // 保存件数の変更は保存後すぐ履歴へ反映する(超過分は古い方から消える)
        if d.desktop.history_limit != before.desktop.history_limit
            && !UI_PREVIEW.load(Ordering::Relaxed)
        {
            crate::history_set_limit(d.desktop.history_limit);
        }
    }
    if d.startup != before.startup {
        if let Err(e) = crate::desktop_settings::set_startup_enabled(d.startup) {
            if d.desktop != before.desktop {
                let _ = crate::desktop_settings::apply(before.desktop.clone());
            }
            error(&e);
            return;
        }
    }
    if (d.output != before.output
        || OUTPUT_RESELECT.load(Ordering::Relaxed)
        || (d.playback && !before.playback && crate::audio::output_status().is_some()))
        && !UI_PREVIEW.load(Ordering::Relaxed)
    {
        if let Err(e) = crate::audio::set_output_uid(&d.output) {
            rollback_general(&before);
            error(&e);
            return;
        }
    }
    let patch = serde_json::json!({"ui_language":d.language.code(),"audio_muted":!d.playback,"audio_gain":d.gain,"spk_mute":d.speaker,"clip_share":d.clip,"share_files":d.files,"local_history":d.history,"audio_output_uid":d.output,"hotkey":d.hotkey,"update_check":d.update_check});
    if let Err(e) = preferences::commit_patch(&patch) {
        rollback_general(&before);
        error(&e);
        return;
    }
    if let Err(e) = refined_layout::apply() {
        let restore = preferences::commit_patch(&before_preferences);
        rollback_general(&before);
        error(&if let Err(r) = restore {
            format!("{e}。一般設定の復元にも失敗しました: {r}")
        } else {
            e
        });
        return;
    }
    if !UI_PREVIEW.load(Ordering::Relaxed) {
        crate::send_cfg();
    }
    for binding in COPY_BINDINGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
    {
        msg1_void_id(
            binding.view as ID,
            sel(binding.property),
            nsstring(localization::language().pick(&binding.japanese, &binding.english)),
        );
    }
    reset_draft();
    OUTPUT_RESELECT.store(false, Ordering::Relaxed);
    populate_outputs();
    sync_controls();
    if folder_changed {
        error("保存しました。受信先の変更は再起動後に反映されます");
    } else {
        error("");
    }
}
pub(in crate::gui) unsafe extern "C" fn manual(_s: ID, _c: SEL, _sender: ID) {
    let open = !MANUAL_OPEN.fetch_xor(true, Ordering::Relaxed);
    msg1_void_u8(object(&MANUAL), sel(c"setHidden:"), !open as u8);
}
pub(in crate::gui) unsafe extern "C" fn identify(_s: ID, _c: SEL, _sender: ID) {
    refined_layout::identify();
}
pub(in crate::gui) unsafe extern "C" fn reconnect(_s: ID, _c: SEL, sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    let index = crate::msg0_isize(sender, sel(c"tag")).max(0) as usize;
    let id = PEER_IDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(index)
        .cloned();
    let Some(id) = id else {
        return;
    };
    if !alert(
        "端末へ再接続",
        "接続をいったん終了し、自動で接続し直します。",
        Some("再接続"),
    ) {
        return;
    }
    let active = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        peers.get(act).is_some_and(|p| p.id == id)
    };
    if active {
        if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
            crate::leave_win_mode_cursor_unlock(None);
        }
        crate::conn::drop_stream("設定から再接続");
    } else {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = peers.iter().find(|p| p.id == id) {
            if let Some(w) = &p.writer {
                w.shutdown();
            }
        }
    }
}
unsafe fn choose_path(save: bool, folder: bool, name: &str) -> Option<std::path::PathBuf> {
    let panel = msg0(
        objc_getClass(if save {
            c"NSSavePanel".as_ptr()
        } else {
            c"NSOpenPanel".as_ptr()
        }),
        sel(if save { c"savePanel" } else { c"openPanel" }),
    );
    if save {
        msg1_void_id(panel, sel(c"setNameFieldStringValue:"), nsstring(name));
    } else {
        msg1_void_u8(panel, sel(c"setCanChooseDirectories:"), folder as u8);
        msg1_void_u8(panel, sel(c"setCanChooseFiles:"), !folder as u8);
        msg1_void_u8(panel, sel(c"setAllowsMultipleSelection:"), 0);
    }
    if crate::msg0_isize(panel, sel(c"runModal")) != 1 {
        return None;
    }
    let url = msg0(panel, sel(c"URL"));
    let path = msg0(url, sel(c"path"));
    let s = msg0_cstr(path, sel(c"UTF8String"));
    if s.is_null() {
        None
    } else {
        Some(std::path::PathBuf::from(
            CStr::from_ptr(s).to_string_lossy().into_owned(),
        ))
    }
}
pub(in crate::gui) unsafe extern "C" fn folder(_s: ID, _c: SEL, _sender: ID) {
    if let Some(p) = choose_path(false, true, "") {
        with_draft(|d| d.desktop.receive_folder = p.to_string_lossy().into_owned());
        sync_controls();
    }
}
pub(in crate::gui) unsafe extern "C" fn open_folder(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    let path = crate::desktop_settings::receive_dir();
    if let Err(e) = std::process::Command::new("open").arg(&path).spawn() {
        error(&format!("フォルダを開けません: {e}"));
    }
}
pub(in crate::gui) unsafe extern "C" fn export(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        error("プレビューでは書き出しません");
        return;
    }
    if let Some(path) = choose_path(true, false, "Knit-settings.json") {
        if let Err(e) = super::super::settings_archive::export_to(&path) {
            error(&format!("書き出せません: {e}"));
        }
    }
}
pub(in crate::gui) unsafe extern "C" fn import(_s: ID, _c: SEL, _sender: ID) {
    let Some(path) = choose_path(false, false, "") else {
        return;
    };
    if !alert(
        "設定を読み込む",
        "共有・一般・端末別の感度と範囲を読み込みます。未適用の編集は破棄します。接続方向、端末の位置、ログイン時起動、認証情報は変更しません。受信先の変更は再起動後に反映されます。",
        Some("読み込む"),
    ) {
        return;
    }
    if UI_PREVIEW.load(Ordering::Relaxed) {
        error("プレビューでは設定ファイルを読み込みません");
        return;
    }
    match super::super::settings_archive::import_from(&path) {
        Ok(()) => {
            for binding in COPY_BINDINGS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
            {
                msg1_void_id(
                    binding.view as ID,
                    sel(binding.property),
                    nsstring(localization::language().pick(&binding.japanese, &binding.english)),
                );
            }
            if !UI_PREVIEW.load(Ordering::Relaxed) {
                crate::send_cfg();
            }
            reset_draft();
            refined_layout::reset();
            populate_outputs();
            sync_controls();
        }
        Err(e) => error(&e),
    }
}
pub(in crate::gui) unsafe extern "C" fn log(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        error("プレビューではログを保存しません");
        return;
    }
    if let Some(path) = choose_path(true, false, "Knit.log") {
        if let Err(e) = std::fs::copy("/tmp/knit-mac.log", &path) {
            error(&format!("ログを保存できません: {e}"));
        }
    }
}
pub(in crate::gui) unsafe extern "C" fn restart(_s: ID, _c: SEL, _sender: ID) {
    if !UI_PREVIEW.load(Ordering::Relaxed)
        && alert(
            "Knitを再起動",
            "接続はいったん切れます。未適用の編集は保存されません。",
            Some("再起動"),
        )
    {
        super::super::imp_restart(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}
pub(in crate::gui) unsafe extern "C" fn reset(_s: ID, _c: SEL, _sender: ID) {
    let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(a, sel(c"setMessageText:"), nsstring("初期化する対象"));
    for s in ["キャンセル", "端末の登録…", "すべての設定…"] {
        msg1_void_id(a, sel(c"addButtonWithTitle:"), nsstring(s));
    }
    let n = crate::msg0_isize(a, sel(c"runModal"));
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    if n == 1001 {
        setup::reset_registration(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    } else if n == 1002 {
        setup::reset_all_settings(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}
pub(in crate::gui) unsafe extern "C" fn test_output(_s: ID, _c: SEL, _sender: ID) {
    if !alert("出力を確認", "選択したMacの出力先で確認音を再生します。Windowsからの受信経路全体を確認する操作ではありません。",Some("再生")){return;}
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    let draft = with_draft(|d| d.clone());
    if draft.output != crate::audio::output_uid() {
        error("出力先の変更を適用してから確認してください");
        return;
    }
    let uid = match crate::audio::confirmed_output_uid() {
        Ok(uid) => uid,
        Err(note) => {
            error(&note);
            return;
        }
    };
    let init: unsafe extern "C" fn(ID, SEL, ID, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let sound = init(
        msg0(objc_getClass(c"NSSound".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentsOfFile:byReference:"),
        nsstring("/System/Library/Sounds/Pop.aiff"),
        1,
    );
    if sound.is_null() {
        error("確認音を読み込めません");
        return;
    }
    msg1_void_id(sound, sel(c"setPlaybackDeviceIdentifier:"), nsstring(&uid));
    match crate::audio::confirmed_output_uid() {
        Ok(current) if current == uid => {}
        _ => {
            msg0_void(sound, sel(c"release"));
            error("出力先が変わったため確認音を中止しました");
            return;
        }
    }
    let volume: unsafe extern "C" fn(ID, SEL, f32) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    volume(sound, sel(c"setVolume:"), 0.12);
    let play: unsafe extern "C" fn(ID, SEL) -> u8 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    if play(sound, sel(c"play")) == 0 {
        error("確認音を再生できません");
    }
    static LAST_SOUND: AtomicUsize = AtomicUsize::new(0);
    let previous = LAST_SOUND.swap(sound as usize, Ordering::Relaxed) as ID;
    if !previous.is_null() {
        msg0_void(previous, sel(c"stop"));
        msg0_void(previous, sel(c"release"));
    }
}
pub(in crate::gui) unsafe extern "C" fn ranges(_s: ID, _c: SEL, _sender: ID) {
    dynamic_copy_scope(|| ranges_inner());
}
unsafe fn ranges_inner() {
    let choices = refined_layout::choices();
    if choices.is_empty() {
        error("接続している端末がありません");
        return;
    }
    let select = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(select, sel(c"setMessageText:"), nsstring("編集する端末"));
    let names: Vec<&str> = choices.iter().map(|(_, n)| n.as_str()).collect();
    let box_view = msg0(objc_getClass(c"NSView".as_ptr()), sel(c"new"));
    frame(
        box_view,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 45.0,
        },
    );
    let pick = popup(
        box_view,
        object(&TARGET),
        &names,
        c"sdRefinedNoop:",
        NSRect {
            x: 0.0,
            y: 5.0,
            w: 400.0,
            h: 28.0,
        },
    );
    if let Some((id, _)) = refined_layout::selected() {
        msg1_void_i64(
            pick,
            sel(c"selectItemAtIndex:"),
            choices.iter().position(|(p, _)| *p == id).unwrap_or(0) as i64,
        );
    }
    msg1_void_id(select, sel(c"setAccessoryView:"), box_view);
    for s in ["キャンセル", "次へ"] {
        msg1_void_id(select, sel(c"addButtonWithTitle:"), nsstring(s));
    }
    if crate::msg0_isize(select, sel(c"runModal")) != 1001 {
        return;
    }
    let i = crate::msg0_isize(pick, sel(c"indexOfSelectedItem")).max(0) as usize;
    if let Some((id, _)) = choices.get(i) {
        refined_layout::select_id(id);
    }
    let Some(settings) = refined_layout::settings_for_selected() else {
        return;
    };
    let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(a, sel(c"setMessageText:"), nsstring("境界範囲"));
    let v = msg0(objc_getClass(c"NSView".as_ptr()), sel(c"new"));
    frame(
        v,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 440.0,
            h: 240.0,
        },
    );
    label(v, "位置", 0.0, 212.0, 100.0, 13.0, false);
    let side = popup(
        v,
        object(&TARGET),
        &["右", "左", "上", "下", "右上", "右下", "左上", "左下"],
        c"sdRefinedNoop:",
        NSRect {
            x: 120.0,
            y: 204.0,
            w: 300.0,
            h: 28.0,
        },
    );
    label(v, "Mac画面", 0.0, 166.0, 100.0, 13.0, false);
    let displays = crate::mac_displays();
    let mut names = vec!["全画面".to_string()];
    names.extend(
        displays
            .iter()
            .enumerate()
            .map(|(i, d)| format!("{} · {}", i + 1, d.name)),
    );
    if UI_PREVIEW.load(Ordering::Relaxed) {
        names = vec![
            "全画面".into(),
            "1 · MacBook Pro".into(),
            "2 · 外部ディスプレイ".into(),
        ];
    }
    let titles: Vec<&str> = names.iter().map(String::as_str).collect();
    let monitor = popup(
        v,
        object(&TARGET),
        &titles,
        c"sdRefinedNoop:",
        NSRect {
            x: 120.0,
            y: 158.0,
            w: 300.0,
            h: 28.0,
        },
    );
    if let Some((s, m)) = refined_layout::placement_for_selected() {
        msg1_void_i64(side, sel(c"selectItemAtIndex:"), s as i64);
        msg1_void_i64(
            monitor,
            sel(c"selectItemAtIndex:"),
            m.and_then(|i| i.checked_add(1))
                .unwrap_or(0)
                .min(names.len() - 1) as i64,
        );
    }
    for (i, (name, n)) in [
        ("Mac 開始", settings.source.0),
        ("Mac 終了", settings.source.1),
        ("相手 開始", settings.target.0),
        ("相手 終了", settings.target.1),
    ]
    .iter()
    .enumerate()
    {
        let x = (i % 2) as f64 * 220.0;
        let y = 90.0 - (i / 2) as f64 * 60.0;
        label(v, name, x, y + 12.0, 120.0, 13.0, false);
        RANGES[i].store(
            text_field(
                v,
                object(&TARGET),
                None,
                name,
                "",
                &format!("{:.1}", n * 100.0),
                NSRect {
                    x: x + 120.0,
                    y: y + 8.0,
                    w: 78.0,
                    h: 26.0,
                },
            ) as usize,
            Ordering::Relaxed,
        );
    }
    msg1_void_id(a, sel(c"setAccessoryView:"), v);
    msg1_void_id(
        a,
        sel(c"setInformativeText:"),
        nsstring("範囲は0〜100%。上から下、または左から右です。"),
    );
    msg1_void_id(a, sel(c"addButtonWithTitle:"), nsstring("キャンセル"));
    msg1_void_id(a, sel(c"addButtonWithTitle:"), nsstring("変更"));
    if crate::msg0_isize(a, sel(c"runModal")) == 1001 {
        let parsed: RResult = RANGES
            .iter()
            .map(|s| string(object(s)).parse::<f64>().map(|n| n / 100.0))
            .collect();
        match parsed {
            Ok(n) => {
                if !crate::screen_settings::valid_range((n[0], n[1]))
                    || !crate::screen_settings::valid_range((n[2], n[3]))
                {
                    error("範囲は0〜100%で、開始より終了を大きくしてください");
                    return;
                }
                let s = crate::msg0_isize(side, sel(c"indexOfSelectedItem")).max(0) as u8;
                let m = crate::msg0_isize(monitor, sel(c"indexOfSelectedItem")).max(0) as usize;
                if let Err(e) = refined_layout::set_placement(s, m.checked_sub(1)) {
                    error(&e);
                    return;
                }
                if let Err(e) = refined_layout::set_ranges((n[0], n[1]), (n[2], n[3])) {
                    error(&e);
                } else {
                    layout_changed();
                }
            }
            Err(_) => error("範囲を数値で指定してください"),
        }
    }
}
type RResult = Result<Vec<f64>, std::num::ParseFloatError>;

/// キー変換のプレビュー行(純粋関数・単体テストで守る)。Windows 側の実際の変換
/// (win/input.rs ModState::apply・tap の CMD_ALT 交換と同じ規則)から組み立てる:
/// ⌘→Ctrl(既定)/Alt(CMD_ALT=true)、⌥→もう一方、⌃→Winキー、⇧→Shift。
/// 右⌘→右Ctrl は CMD_ALT の影響を受けないためツールチップ側で補完する。
/// セクション見出し「キー変換」の下に 3〜5 行で表示する
fn key_conversion_preview(cmd_alt: bool) -> Vec<(String, String)> {
    let (cmd_to, opt_to) = if cmd_alt {
        ("Alt", "Ctrl")
    } else {
        ("Ctrl", "Alt")
    };
    vec![
        ("⌘C".into(), format!("{cmd_to}+C")),
        ("⌘⇧Z".into(), format!("{cmd_to}+Shift+Z")),
        ("⌥F4".into(), format!("{opt_to}+F4")),
        // ⌃→Winキーは CMD_ALT と無関係に固定(端末/profile による例外は除く)
        ("⌃C".into(), "Win+C".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Draft {
        Draft {
            playback: false,
            gain: 1.0,
            speaker: true,
            clip: true,
            files: true,
            history: true,
            output: String::new(),
            language: Language::Japanese,
            desktop: crate::desktop_settings::Settings::default(),
            startup: false,
            hotkey: 105,
            update_check: true,
        }
    }
    #[test]
    fn untouched_fields_follow_remote_changes_without_becoming_edits() {
        let mut baseline = base();
        let mut draft = baseline.clone();
        let mut current = baseline.clone();
        current.clip = false;
        current.gain = 0.7;
        assert!(merge_values(&mut draft, &mut baseline, &current));
        assert!(!draft.clip);
        assert_eq!(draft.gain, 0.7);
        assert!(draft == baseline);
    }
    #[test]
    fn editing_language_cannot_restore_old_clipboard_permission() {
        let mut baseline = base();
        let mut draft = baseline.clone();
        draft.language = Language::English;
        let mut current = baseline.clone();
        current.clip = false;
        current.files = false;
        merge_values(&mut draft, &mut baseline, &current);
        assert!(!draft.clip);
        assert!(!draft.files);
        assert_eq!(draft.language, Language::English);
        assert_eq!(baseline.language, Language::Japanese);
    }
    #[test]
    fn independent_desktop_fields_merge_without_losing_folder_draft() {
        let mut baseline = base();
        let mut draft = baseline.clone();
        draft.desktop.receive_folder = "/chosen".into();
        let mut current = baseline.clone();
        current.desktop.notify_transfer = false;
        merge_values(&mut draft, &mut baseline, &current);
        assert_eq!(draft.desktop.receive_folder, "/chosen");
        assert!(!draft.desktop.notify_transfer);
    }
    #[test]
    fn new_desktop_fields_merge_independently_of_edits() {
        // 未編集の項目は外部変更へ追従し、編集中の項目は下書きを守る
        let mut baseline = base();
        let mut draft = baseline.clone();
        draft.desktop.history_limit = 100;
        let mut current = baseline.clone();
        current.desktop.open_receive_folder = true;
        current.desktop.history_limit = 200;
        merge_values(&mut draft, &mut baseline, &current);
        assert!(draft.desktop.open_receive_folder, "未編集は追従");
        assert_eq!(draft.desktop.history_limit, 100, "編集中は下書きを維持");
        assert_eq!(baseline.desktop.history_limit, 50, "baselineも編集前のまま");
    }

    /// 切替キー(hotkey)と更新の自動確認(update_check)も他の下書き項目と同じ
    /// 規則で merge される: 未編集なら実行値へ追従し、編集中は下書きを守る
    #[test]
    fn hotkey_and_update_check_merge_like_other_draft_fields() {
        let mut baseline = base();
        let mut draft = baseline.clone();
        draft.hotkey = 54;
        let mut current = baseline.clone();
        current.hotkey = 97;
        current.update_check = false;
        merge_values(&mut draft, &mut baseline, &current);
        assert_eq!(draft.hotkey, 54, "編集中の切替キーは下書きを維持");
        assert!(!draft.update_check, "未編集の update_check は実行値へ追従");
        assert_eq!(baseline.hotkey, 105, "baseline は編集前のまま");
        assert!(!baseline.update_check);
    }

    /// キー変換プレビュー: cmd_alt で出力が変わり、Windows 側の実際の変換
    /// (win/input.rs ModState::apply と同じ規則: ⌘→Ctrl/Alt・⌥→もう一方・
    /// ⌃→Winキー)と矛盾しない。タップ翻訳表(mac_shortcut_translation)の
    /// 送信時 CMD_ALT 交換はこの規則の上に乗るため、ここでは一般則を守る
    #[test]
    fn key_conversion_preview_matches_runtime_modifier_map() {
        let off = key_conversion_preview(false);
        let on = key_conversion_preview(true);
        assert_ne!(off, on, "cmd_alt で出力が変わる");
        // ⌘は CMD_ALT で Ctrl⇄Alt、⌥はもう一方へ入れ替わる
        assert!(
            off.contains(&("⌘C".to_string(), "Ctrl+C".to_string())),
            "{off:?}"
        );
        assert!(
            on.contains(&("⌘C".to_string(), "Alt+C".to_string())),
            "{on:?}"
        );
        assert!(
            off.contains(&("⌥F4".to_string(), "Alt+F4".to_string())),
            "{off:?}"
        );
        assert!(
            on.contains(&("⌥F4".to_string(), "Ctrl+F4".to_string())),
            "{on:?}"
        );
        // ⇧は常に Shift、⌃→Winキーは CMD_ALT の影響を受けない
        for rows in [&off, &on] {
            assert!(rows.contains(&("⌘⇧Z".to_string(), {
                let cmd_to = if rows[0].1.starts_with("Alt") { "Alt" } else { "Ctrl" };
                format!("{cmd_to}+Shift+Z")
            })));
            assert!(
                rows.contains(&("⌃C".to_string(), "Win+C".to_string())),
                "{rows:?}"
            );
        }
        // 表示は 3〜5 行
        assert!((3..=5).contains(&off.len()) && (3..=5).contains(&on.len()));
    }
}
