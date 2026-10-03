//! 設定の分類と画面構築。入力・通信の処理は既存アクションへ委譲する。
use super::*;
use std::sync::Mutex;
static PAGES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static TABS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
/// 「操作」ページのタブレット項目(タブレットが無い時は隠す)
static TABLET_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
/// 設定画面のアップデートボタン(状態に応じて文言が変わる)
static UPDATE_BUTTON: AtomicUsize = AtomicUsize::new(0);
static SAVE_LABEL: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_POP: AtomicUsize = AtomicUsize::new(0);
static SWITCH_POP: AtomicUsize = AtomicUsize::new(0);
static PEER_POP: AtomicUsize = AtomicUsize::new(0);
static PEER_MENU_OPEN: AtomicBool = AtomicBool::new(false);
static PEER_CHOICES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static AUDIO_LABEL: AtomicUsize = AtomicUsize::new(0);
static SPEAKER_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 音声の再生音量スライダ(Windows から届く音をこの Mac で鳴らす大きさ)
static AUDIO_GAIN_SLIDER: AtomicUsize = AtomicUsize::new(0);
static AUDIO_GAIN_LABEL: AtomicUsize = AtomicUsize::new(0);
static SHARE_HINT: AtomicUsize = AtomicUsize::new(0);
static CONNECTION_HINT: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの受け入れ範囲の行(KNIT_ALLOW_ANY/TS の緩和を見える化する)
static ACCEPT_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの相手側の名前(Android 接続中は端末名へ書き換わる)
static PEER_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの Android タブレットの状態行(端末が無ければ空)
static ANDROID_STATE: AtomicUsize = AtomicUsize::new(0);
static NAV_SWITCH: AtomicUsize = AtomicUsize::new(0);
static GESTURE_HINT: AtomicUsize = AtomicUsize::new(0);
static PINCH_SWITCH: AtomicUsize = AtomicUsize::new(0);
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
unsafe fn group(parent: ID, y: f64, h: f64) -> ID {
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
    v
}
unsafe fn divider(parent: ID, y: f64) -> ID {
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
    v
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
    msg1_void_i64(v, sel(c"setLineBreakMode:"), 4);
    msg1_void_id(v, sel(c"setToolTip:"), nsstring(text));
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
pub(super) static ROLE_RADIO: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

unsafe fn radio(parent: ID, target: ID, title: &str, action: &std::ffi::CStr, tag: i64, r: NSRect) -> ID {
    let b = msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"new"));
    frame(b, r);
    msg1_void_id(b, sel(c"setTitle:"), nsstring(title));
    msg1_void_i64(b, sel(c"setButtonType:"), 4); // NSButtonTypeRadio
    msg1_void_i64(b, sel(c"setTag:"), tag);
    msg1_void_id(b, sel(c"setTarget:"), target);
    let action_fn: unsafe extern "C" fn(ID, SEL, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    action_fn(b, sel(c"setAction:"), sel(action));
    msg1_void_id(b, sel(c"setAccessibilityLabel:"), nsstring(title));
    msg1_void_id(parent, sel(c"addSubview:"), b);
    b
}

/// 選択表示を現在の役割に合わせる(0=この Mac がホスト / 1=Windows がホスト)
pub(super) unsafe fn sync_role() {
    let client = crate::CLIENT_ROLE.load(Ordering::Relaxed);
    for (i, slot) in ROLE_RADIO.iter().enumerate() {
        let b = slot.load(Ordering::Relaxed) as ID;
        if !b.is_null() {
            msg1_void_i64(b, sel(c"setState:"), ((i == 1) == client) as i64);
        }
    }
}

unsafe fn check(
    parent: ID,
    target: ID,
    title: &str,
    action: &std::ffi::CStr,
    slot: &AtomicUsize,
    y: f64,
) -> ID {
    let caption = label(parent, title, 40.0, y + 1.0, 460.0, 13.0, false);
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
    caption
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
#[allow(clippy::too_many_arguments)] // UI 部品の生成は引数が本質的に多い
unsafe fn slider(
    parent: ID,
    target: ID,
    title: &str,
    value: f64,
    min: f64,
    max: f64,
    action: &std::ffi::CStr,
    slot: &AtomicUsize,
    value_slot: &AtomicUsize,
    y: f64,
) -> ID {
    let caption = label(parent, title, 40.0, y, 240.0, 13.0, false);
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
            x: 282.0,
            y,
            w: 228.0,
            h: 24.0,
        },
    );
    msg1_void_u8(s, sel(c"setContinuous:"), 0);
    msg1_void_id(s, sel(c"setAccessibilityLabel:"), nsstring(title));
    msg1_void_id(parent, sel(c"addSubview:"), s);
    slot.store(s as usize, Ordering::Relaxed);
    let value = label(parent, "", 518.0, y, 60.0, 11.0, true);
    msg1_void_i64(value, sel(c"setAlignment:"), 2);
    value_slot.store(value as usize, Ordering::Relaxed);
    caption
}

pub(super) unsafe fn select_page(index: usize) {
    for i in 0..PAGES.len() {
        let p = PAGES[i].load(Ordering::Relaxed) as ID;
        if !p.is_null() {
            msg1_void_u8(p, sel(c"setHidden:"), (i != index) as u8);
        }
        let b = TABS[i].load(Ordering::Relaxed) as ID;
        if !b.is_null() {
            msg1_void_i64(b, sel(c"setState:"), (i == index) as i64);
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            let accent = msg1_id_f64(
                msg0(color_cls, sel(c"controlAccentColor")),
                sel(c"colorWithAlphaComponent:"),
                if i == index { 0.14 } else { 0.0 },
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
    let page = PAGES[index.min(PAGES.len()-1)].load(Ordering::Relaxed) as ID;
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
    preferences::save();
    refresh_status();
}
pub(super) unsafe extern "C" fn enter_peer(_s: ID, _c: SEL, _sender: ID) {
    if !UI_PREVIEW.load(Ordering::Relaxed) && !crate::WIN_MODE.load(Ordering::Relaxed) {
        crate::do_toggle("設定画面");
    }
    refresh_status();
}

pub(super) unsafe extern "C" fn select_peer(s: ID, c: SEL, sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) { return; }
    let item = msg0(sender, sel(c"selectedItem"));
    super::imp_peer_activate(s, c, item);
}

pub(super) unsafe extern "C" fn peer_menu_open(_s: ID, _c: SEL, _menu: ID) {
    PEER_MENU_OPEN.store(true, Ordering::Relaxed);
}

pub(super) unsafe extern "C" fn peer_menu_close(_s: ID, _c: SEL, _menu: ID) {
    PEER_MENU_OPEN.store(false, Ordering::Relaxed);
}

unsafe fn set_label(slot: &AtomicUsize, text: &str) {
    let label = slot.load(Ordering::Relaxed) as ID;
    if !label.is_null() {
        msg1_void_id(label, sel(c"setStringValue:"), nsstring(text));
        msg1_void_id(label, sel(c"setToolTip:"), nsstring(text));
    }
}

fn method_index() -> i64 {
    if crate::HOTKEY_ONLY.load(Ordering::Relaxed) { 2 }
    else if crate::SWITCH_DELAY_MS.load(Ordering::Relaxed) > 0 { 1 }
    else if crate::EDGE_TAPS.load(Ordering::Relaxed) == 1 { 3 }
    else { 0 }
}

unsafe fn sync_peer_picker() {
    let pop = PEER_POP.load(Ordering::Relaxed) as ID;
    if pop.is_null() || PEER_MENU_OPEN.load(Ordering::Relaxed) { return; }
    let (mut entries, selected) = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let active = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        (peers.iter().map(|p| (p.id.clone(), p.name.clone())).collect::<Vec<_>>(), active)
    };
    let selectable = entries.len() > 1 && !UI_PREVIEW.load(Ordering::Relaxed);
    if entries.is_empty() {
        entries.push((String::new(), if crate::CONNECTED.load(Ordering::Relaxed) {
            crate::active_peer_label()
        } else { "接続先を待っています".into() }));
    }
    let mut seen = PEER_CHOICES.lock().unwrap_or_else(|e| e.into_inner());
    if *seen != entries {
        msg0_void(pop, sel(c"removeAllItems"));
        for (index, (id, name)) in entries.iter().enumerate() {
            msg1_void_id(pop, sel(c"addItemWithTitle:"), nsstring(name));
            let item_at: unsafe extern "C" fn(ID, SEL, isize) -> ID =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            let item = item_at(pop, sel(c"itemAtIndex:"), index as isize);
            msg1_void_id(item, sel(c"setRepresentedObject:"), nsstring(id));
        }
        *seen = entries;
    }
    msg1_void_i64(pop, sel(c"selectItemAtIndex:"), selected.min(seen.len() - 1) as i64);
    msg1_void_u8(pop, sel(c"setEnabled:"), selectable as u8);
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

/// 音量スライダの値表示(1.0 = 100%)
fn gain_text(g: f32) -> String {
    format!("{}%", (g * 100.0).round() as i32)
}

/// 音声の再生音量スライダ(受信サンプルへのソフトゲイン。Windows 操作中は
/// Mac の音量キーが Windows 側へ転送されるため、Knit 内で完結する調整口)
pub(super) unsafe extern "C" fn audio_gain(_s: ID, _c: SEL, sender: ID) {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(sender, sel(c"doubleValue"));
    crate::audio::set_gain(v);
    set_label(&AUDIO_GAIN_LABEL, &gain_text(crate::audio::gain()));
    preferences::save();
}
pub(super) unsafe fn set_save_status(text: &str) {
    let l = SAVE_LABEL.load(Ordering::Relaxed) as ID;
    if !l.is_null() {
        msg1_void_id(l, sel(c"setStringValue:"), nsstring(text));
    }
}
pub(super) unsafe extern "C" fn navigation_toggle(_s: ID, _c: SEL, sender: ID) {
    crate::trackpad::set_navigation(crate::msg0_isize(sender,sel(c"state")) != 0);
    preferences::save();
}
pub(super) unsafe extern "C" fn pinch_toggle(_s: ID, _c: SEL, sender: ID) {
    crate::trackpad::set_enabled(crate::msg0_isize(sender,sel(c"state")) != 0);
    preferences::save();
}

pub(super) unsafe fn sync() {
    sync_role();
    let preview = UI_PREVIEW.load(Ordering::Relaxed);
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    let remote = crate::WIN_MODE.load(Ordering::Relaxed);
    let android = !preview && crate::active_peer_is_android();
    let android_app = !preview && crate::active_peer_is_android_app();
    let peer = crate::active_peer_label();
    let enable = |slot: &AtomicUsize, enabled: bool| {
        let view = slot.load(Ordering::Relaxed) as ID;
        if !view.is_null() { msg1_void_u8(view, sel(c"setEnabled:"), enabled as u8); }
    };
    let method = method_index();
    let pop = SWITCH_POP.load(Ordering::Relaxed) as ID;
    if !pop.is_null() { msg1_void_i64(pop, sel(c"selectItemAtIndex:"), method); }
    let keys = HOTKEY_POP.load(Ordering::Relaxed) as ID;
    if !keys.is_null() {
        let current = crate::hotkey_kc();
        let index = [97, 100, 105].iter().position(|k| *k == current).unwrap_or(3);
        msg1_void_i64(keys, sel(c"selectItemAtIndex:"), index as i64);
    }
    sync_peer_picker();
    let pinch = PINCH_SWITCH.load(Ordering::Relaxed) as ID;
    if !pinch.is_null() { msg1_void_u8(pinch,sel(c"setState:"),crate::trackpad::ENABLED.load(Ordering::Relaxed) as u8); }
    enable(&PINCH_SWITCH, preview || crate::trackpad::AVAILABLE.load(Ordering::Relaxed));
    let nav = NAV_SWITCH.load(Ordering::Relaxed) as ID;
    if !nav.is_null() { msg1_void_i64(nav,sel(c"setState:"),crate::trackpad::NAV_ENABLED.load(Ordering::Relaxed) as i64); }
    enable(&NAV_SWITCH, preview || crate::trackpad::navigation_available());
    let hint=GESTURE_HINT.load(Ordering::Relaxed) as ID;
    if !hint.is_null() {
        let text=if preview {"タブレット操作中に有効 · 上で一時停止すると最近のタスクを表示"}
            else if !crate::trackpad::navigation_available() {"指の位置を取得できません。ピンチと通常のスクロールを利用できます"}
            else if !crate::trackpad::NAV_ENABLED.load(Ordering::Relaxed) {"ナビゲーションはオフです。スクロールとピンチは個別に利用できます"}
            else if android && remote {"タブレットを操作中 · 上で一時停止すると最近のタスクを表示"}
            else {"タブレットへ操作を切り替えると有効になります"};
        set_label(&GESTURE_HINT,text);
    }
    // KNIT_SHARE で禁じられた項目は、設定画面から許可できない
    let cap = knit_common::share::env_cap();
    enable(&super::PREFS_CHK_CLIP, cap.clip && !preview);
    enable(&super::PREFS_CHK_FILES, cap.files && !preview);
    enable(&super::PREFS_CHK_HISTORY, cap.clip && !preview);
    let audio_ok = knit_common::share::env_cap().audio;
    enable(&super::PREFS_CHK_SPK, !android && !preview && audio_ok);
    enable(&super::PREFS_CHK_AUDIO, !android_app && !preview && audio_ok);
    // 相手側ラベルは接続状態によらず「どの端末との組み合わせか」を示し続ける。
    // 状態(接続済み/未接続)は下の PREFS_STATE が担うため二重に書き換えない
    set_label(&PEER_LABEL, &peer);
    let rtt = crate::RTT_MS.load(Ordering::Relaxed);
    let state = if preview { "設定画面のプレビュー".into() }
        else if connected && rtt > 0 { format!("接続済み（{}・遅延 {rtt}ms）", crate::route_label()) }
        else if connected { "接続済み".into() }
        else { "未接続(自動で再接続します)".into() };
    set_label(&PREFS_STATE, &state);
    let state_label = PREFS_STATE.load(Ordering::Relaxed) as ID;
    if !state_label.is_null() {
        let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(if connected { c"labelColor" } else { c"secondaryLabelColor" }));
        msg1_void_id(state_label, sel(c"setTextColor:"), color);
    }
    // 受け入れ範囲(KNIT_ALLOW_ANY/TS で緩んでいるときは警告色で目立たせる)
    let scope = knit_common::net::accept_scope();
    let accept_text = if scope.is_wide_open() {
        format!("受け入れ範囲: {}。信頼できるネットワークでのみ使ってください", scope.label())
    } else {
        format!("受け入れ範囲: {}", scope.label())
    };
    set_label(&ACCEPT_LABEL, &accept_text);
    let accept_label = ACCEPT_LABEL.load(Ordering::Relaxed) as ID;
    if !accept_label.is_null() {
        let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(if scope.is_wide_open() { c"systemRedColor" } else { c"secondaryLabelColor" }));
        msg1_void_id(accept_label, sel(c"setTextColor:"), color);
    }
    let hint = if preview { "入力・通信・音声は動作せず、設定も保存しません。".into() }
        else if !connected { "接続する端末でKnitを開いてください。初めてなら、この画面の「端末を登録…」から始めます。".into() }
        else if let Some((false,_))=crate::active_android_app_permissions() { "接続済みです。タブレットのKnitアプリで画面操作を許可してください。".into() }
        else { match method {
            2 => format!("切替キーで{peer}へ移ります。画面の端では切り替わりません。"),
            1 => format!("画面の端で{}ms待つと{peer}へ移ります。速く動かしたときは1回の到達で切り替わります。切替キーも使えます。", crate::SWITCH_DELAY_MS.load(Ordering::Relaxed)),
            3 => format!("画面の端に1回触れると{peer}へ移ります。切替キーも使えます。"),
            _ => format!("画面の端に2回触れると{peer}へ移ります。速く動かしたときは1回の到達で切り替わります(誤発火が続く場合は環境変数 KNIT_FAST_EDGE=0 で無効化)。切替キーも使えます。"),
        } };
    set_label(&CONNECTION_HINT, &hint);
    let android_hint = if preview { String::new() }
        else if android_app {
            match crate::active_android_app_permissions() {
                Some((true,true))=>"Androidアプリ：画面操作・キーボードを許可済み".into(),
                Some((true,false))=>"Androidアプリ：文字入力にはKnitキーボードを選択してください".into(),
                _=>"Androidアプリ：タブレットで画面操作を許可してください".into(),
            }
        } else {
        let (_, status) = crate::android::state::snapshot();
        if let Some(problem) = &status.problem { format!("Android: {problem}") }
        else if status.tablets.is_empty() { String::new() }
        else { let tablet = &status.tablets[0]; format!("Android: {} — {}（{}台）", tablet.name, tablet.phase.summary(), status.tablets.len()) }
    };
    set_label(&ANDROID_STATE, &android_hint);
    set_label(&AUDIO_LABEL, if android_app { "音声共有（Androidアプリ版は未対応）" } else { "接続先の音声をこのMacで再生" });
    set_label(&SPEAKER_LABEL, if android { "相手のスピーカーをミュート（PCのみ）" } else { "接続中は相手のスピーカーをミュート（相手が音声転送をオフの間は適用されません）" });
    set_label(&SHARE_HINT, if android_app { "ファイルはDownload/Knitへ保存します。日本語はタブレット操作中に自動で出る入力欄から送れます。" } else if android { "ファイルはDownloadへ保存します。タブレットの音はそのまま残ります。" } else if knit_common::share::env_cap() != knit_common::share::Scope::ALL { "環境変数 KNIT_SHARE で制限中のため、一部の項目は変更できません。" } else { "切断すると元に戻ります。異常終了したときも、Windows 側は次回起動時に自動で戻します。" });
    // 再生音量スライダ(起動時の設定復元・リモート適用を画面へ反映)
    let gain_slider = AUDIO_GAIN_SLIDER.load(Ordering::Relaxed) as ID;
    if !gain_slider.is_null() {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let set: unsafe extern "C" fn(ID, SEL, f64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let g = crate::audio::gain() as f64;
        if get(gain_slider, sel(c"doubleValue")) != g {
            set(gain_slider, sel(c"setDoubleValue:"), g);
        }
    }
    set_label(&AUDIO_GAIN_LABEL, &gain_text(crate::audio::gain()));
    let div = crate::scroll_div();
    set_label(&PREFS_GAIN_LABEL, if div <= 40.0 { "速め" } else if div >= 140.0 { "遅め" } else { "標準" });
    // タブレット項目は、タブレットが見つかっている(または接続中の)時だけ見せる
    let tablets = !crate::android::state::snapshot().1.tablets.is_empty();
    let show_tablet = preview || tablets || android || android_app;
    for v in TABLET_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), !show_tablet as u8); }
    }
    let update = UPDATE_BUTTON.load(Ordering::Relaxed) as ID;
    if !update.is_null() {
        msg1_void_id(update, sel(c"setTitle:"), nsstring(&crate::updater::menu_title()));
        msg1_void_u8(update, sel(c"setEnabled:"), !preview as u8);
    }
    let canvas = LAYOUT_CANVAS.load(Ordering::Relaxed) as ID;
    if !canvas.is_null() && !LAY_DRAG.load(Ordering::Relaxed) { msg1_void_u8(canvas, sel(c"setNeedsDisplay:"), 1); }
}

/// 配置キャンバスの再描画要求(設定の変更を即座に絵へ反映する)。
/// ドラッグ中は絵が動かなくなるため、終わるまで待つ
pub(super) unsafe fn redraw_layout() {
    let canvas = LAYOUT_CANVAS.load(Ordering::Relaxed) as ID;
    if !canvas.is_null() && !LAY_DRAG.load(Ordering::Relaxed) {
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
            h: 700.0,
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
            h: 700.0,
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
            y: 645.0,
            w: 32.0,
            h: 32.0,
        },
    );
    msg1_void_id(iv, sel(c"setImage:"), icon);
    msg1_void_id(sidebar, sel(c"addSubview:"), iv);
    label(sidebar, "Knit", 59.0, 647.0, 110.0, 20.0, false);
    label(
        sidebar,
        "机を、ひとつの手元で。",
        20.0,
        618.0,
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
                y: 526.0 - i as f64 * 48.0,
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
    let pages: Vec<ID> = (0..PAGES.len())
        .map(|i| {
            let p = view(
                cv,
                NSRect {
                    x: 184.0,
                    y: 48.0,
                    w: 620.0,
                    h: 640.0,
                },
            );
            surface(p, c"controlBackgroundColor", 0.0);
            PAGES[i].store(p as usize, Ordering::Relaxed);
            p
        })
        .collect();
    let titles = [
        ("接続", "つながっている端末と、その状態です。"),
        ("画面配置", "接続先の位置を、実際の画面配置に合わせます。"),
        ("操作", "画面を移る方法と、スクロールの感触です。"),
        ("共有", "この Mac が渡すものと、受け取るものを選びます。"),
    ];
    for (p, (title, sub)) in pages.iter().zip(titles) {
        label(*p, title, 28.0, 580.0, 565.0, 24.0, false);
        label(*p, sub, 28.0, 551.0, 565.0, 12.0, true);
    }
    let p = pages[0];
    // 上: 端末の状態(この Mac と相手、操作する端末)。下: 登録・初期化・アップデート・困った時
    group(p, 334.0, 194.0);
    group(p, 36.0, 202.0);
    symbol(p, "laptopcomputer", 106.0, 448.0, 54.0);
    symbol(p, "desktopcomputer", 434.0, 448.0, 54.0);
    label(p, "このMac", 107.0, 419.0, 145.0, 14.0, false);
    // 相手側の名前は接続先に応じて書き換わる(Android 接続中は端末名)
    PEER_LABEL.store(
        label(p, "Windows", 427.0, 419.0, 145.0, 14.0, false) as usize,
        Ordering::Relaxed,
    );
    symbol(p, "link", 282.0, 460.0, 26.0);
    label(p, "操作する端末", 40.0, 388.0, 105.0, 12.0, true);
    let peer_pop = popup(p, target, &[], c"sdSelectPeer:", NSRect { x: 150.0, y: 385.0, w: 428.0, h: 28.0 });
    msg1_void_id(peer_pop, sel(c"setAccessibilityLabel:"), nsstring("操作する接続先"));
    msg1_void_id(msg0(peer_pop, sel(c"menu")), sel(c"setDelegate:"), target);
    PEER_POP.store(peer_pop as usize, Ordering::Relaxed);
    PREFS_STATE.store(
        label(p, "接続を確認しています…", 40.0, 497.0, 540.0, 14.0, false) as usize,
        Ordering::Relaxed,
    );
    CONNECTION_HINT.store(
        label(p, "", 40.0, 360.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // Android タブレットの状態(見つかっている時だけ内容が入る)
    ANDROID_STATE.store(
        label(p, "", 40.0, 340.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 接続の受け入れ範囲(net.rs の判定結果)。KNIT_ALLOW_ANY/TS の緩和が
    // 画面から見えない問題への対策。ANY のときは sync() が警告色へ変える
    ACCEPT_LABEL.store(
        label(p, "", 40.0, 530.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 接続の方向(どちらがホストか)。選ぶと相手にも伝わり、両方が再起動して切り替わる
    group(p, 250.0, 72.0);
    label(p, "接続の方向", 40.0, 294.0, 100.0, 13.0, false);
    ROLE_RADIO[0].store(
        radio(p, target, "この Mac がホスト: Windows が接続しに来る(既定)", c"sdRole:", 0, NSRect { x: 150.0, y: 288.0, w: 430.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    ROLE_RADIO[1].store(
        radio(p, target, "Windows がホスト: この Mac が接続しに行く", c"sdRole:", 1, NSRect { x: 150.0, y: 260.0, w: 430.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    label(p, "端末の登録", 40.0, 192.0, 300.0, 13.0, false);
    button(
        p,
        target,
        "端末を登録…",
        c"sdRegistration:",
        NSRect { x: 392.0, y: 186.0, w: 188.0, h: 30.0 },
    );
    divider(p, 179.0);
    // 登録の初期化(全端末の締め出し)。確認ダイアログを挟む処理は setup 側
    label(p, "登録の管理", 40.0, 148.0, 300.0, 13.0, false);
    button(
        p,
        target,
        "すべての登録を初期化…",
        c"sdResetRegistration:",
        NSRect { x: 392.0, y: 142.0, w: 188.0, h: 30.0 },
    );
    divider(p, 135.0);
    label(p, &format!("Knit {}", crate::VERSION_STR), 40.0, 104.0, 300.0, 13.0, false);
    UPDATE_BUTTON.store(
        button(
            p,
            target,
            "アップデートを確認…",
            c"sdCheckUpdate:",
            NSRect { x: 392.0, y: 98.0, w: 188.0, h: 30.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    divider(p, 91.0);
    label(p, "困った時", 40.0, 60.0, 200.0, 13.0, false);
    button(
        p,
        target,
        "ログを開く",
        c"sdOpenLog:",
        NSRect { x: 350.0, y: 54.0, w: 108.0, h: 30.0 },
    );
    button(
        p,
        target,
        "再起動",
        c"sdRestart:",
        NSRect { x: 466.0, y: 54.0, w: 114.0, h: 30.0 },
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
        "グレー：Mac／ブルー：接続先（各モニターに機種名）。青い枠のセルへドラッグして配置（斜めも置けます）",
        28.0,
        98.0,
        560.0,
        12.0,
        true,
    );
    // 初期化ボタンは持たない: ドラッグで好きなセルへ置き直せるため
    //(Windows 側設定の「既定(右)に戻す」は Windows に配置エディタが無い分の代替)
    let p = pages[2];
    // 切替(方式とキー)/ スクロール / タブレット(ある時だけ)。タイミングの調整は持たない
    group(p, 330.0, 88.0);
    group(p, 236.0, 78.0);
    divider(p, 376.0);
    divider(p, 268.0);
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
    let scroll_caption = check(
        p,
        target,
        "スクロール方向をMacに合わせる",
        c"sdScroll:",
        &PREFS_CHK_SCROLL,
        278.0,
    );
    // 方向は起動時の macOS 設定で決まり、起動中の変更は設定画面を開き直す時に
    // 反映する(この画面を開くタイミングで再取得している)
    msg1_void_id(
        scroll_caption,
        sel(c"setToolTip:"),
        nsstring("macOS の自然スクロール設定に合わせます。起動中に切り替えた場合は、この設定画面を開き直すと反映します"),
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
        &PREFS_GAIN_LABEL,
        244.0,
    );
    // タブレット(Android)がある時だけ見せる。無い人には関係のない項目なので隠す
    let mut tablet = vec![group(p, 132.0, 84.0) as usize, divider(p, 168.0) as usize];
    tablet.push(check(p, target, "タブレットのナビゲーション", c"sdTabletNav:", &NAV_SWITCH, 178.0) as usize);
    tablet.push(check(p, target, "ピンチで拡大・縮小", c"sdPinch:", &PINCH_SWITCH, 140.0) as usize);
    tablet.push(label(p, "", 28.0, 108.0, 565.0, 11.0, true) as usize);
    GESTURE_HINT.store(*tablet.last().unwrap(), Ordering::Relaxed);
    tablet.push(NAV_SWITCH.load(Ordering::Relaxed));
    tablet.push(PINCH_SWITCH.load(Ordering::Relaxed));
    *TABLET_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = tablet;
    let p = pages[3];
    // 渡すもの(テキスト・画像・ファイル)と、音
    group(p, 280.0, 138.0);
    group(p, 152.0, 114.0);
    group(p, 22.0, 80.0);
    divider(p, 344.0);
    divider(p, 188.0);
    check(p, target, "テキストと画像", c"sdClipShare:", &PREFS_CHK_CLIP, 384.0);
    label(p, "コピーした内容を、もう1台でも貼り付けられます。", 40.0, 358.0, 540.0, 12.0, true);
    check(p, target, "ファイルの受け渡し", c"sdFileShare:", &super::PREFS_CHK_FILES, 312.0);
    label(p, "コピーしたファイルや、掴んだファイルを渡せます。", 40.0, 288.0, 540.0, 12.0, true);
    AUDIO_LABEL.store(check(
        p,
        target,
        "接続先の音声をこのMacで再生",
        c"sdAudio:",
        &PREFS_CHK_AUDIO,
        228.0,
    ) as usize, Ordering::Relaxed);
    slider(
        p,
        target,
        "再生音量   小さい / 大きい",
        crate::audio::gain() as f64,
        0.0,
        2.0,
        c"sdAudioGain:",
        &AUDIO_GAIN_SLIDER,
        &AUDIO_GAIN_LABEL,
        196.0,
    );
    SPEAKER_LABEL.store(check(
        p,
        target,
        "接続中は相手のスピーカーをミュート",
        c"sdSpkMute:",
        &PREFS_CHK_SPK,
        154.0,
    ) as usize, Ordering::Relaxed);
    check(p, target, "Macのコピーを履歴に残す", c"sdLocalHistory:", &super::PREFS_CHK_HISTORY, 64.0);
    label(p, "メニューバーの「クリップボード履歴」から選び直せます。パスワードなどの機密コピーは残しません。", 40.0, 36.0, 540.0, 12.0, true);
    SHARE_HINT.store(label(
        p,
        "切断すると元に戻ります。異常終了したときも、Windows 側は次回起動時に自動で戻します。",
        28.0,
        116.0,
        565.0,
        12.0,
        true,
    ) as usize, Ordering::Relaxed);
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
            .min(PAGES.len()-1)
    } else {
        0
    };
    select_page(page);
    win
}
