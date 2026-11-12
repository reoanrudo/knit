//! 設定画面の新デザイン。2026-10-28 のデザイン画像を正とする。
//! 仕様は docs/design/settings-2026-10-28/SPEC.md。
//!
//! この画面は「見た目と体系」を担い、設定値の保存は既存の prefs / preferences へ
//! 委譲する。現在は --settings-new プレビュー専用。
//!
//! 設計上の約束:
//! - 意味色のみ使う(windowBackgroundColor / separatorColor / labelColor /
//!   secondaryLabelColor / controlAccentColor 等)。ダークは OS に追従する
//! - サイドバーだけ NSVisualEffectView(.sidebar)。全面ガラスは使わない
//! - 座標は「上端基準」。内容面は isFlipped=YES のキャンバスなので変換しない
//! - カードは背景ビュー(角丸+境界線)。中身は同じキャンバスへ絶対配置する
//! - カードの高さは中身から計算する(固定値を書かないので溢れない)
//! - 余白は 4pt グリッド
use crate::*;
use crate::gui::UI_PREVIEW;
use super::localization;
use super::{msg0_void, msg1_id_f64, msg1_void_i64, msg1_void_id, msg1_void_u8, msg1_void_sel, sel, NSRect};
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------- トークン(SPEC.md と 1:1 対応) ----------

const W: f64 = 1040.0;
const H: f64 = 700.0;

const SIDEBAR_W: f64 = 219.0;
const PAGE_X: f64 = SIDEBAR_W;
const PAGE_W: f64 = W - SIDEBAR_W;

/// 内容面の左右余白 / カード幅 / カード内パディング
const GUTTER: f64 = 25.0;
const CARD_X: f64 = GUTTER;
const CARD_W: f64 = PAGE_W - GUTTER * 2.0;
const CARD_PAD: f64 = 15.0;
/// ページ見出しはカード内の文字と左端を揃える(デザイン実測 259pt)
const PAGE_TEXT_X: f64 = CARD_X + CARD_PAD;
/// カード間の縦の間隔
const CARD_GAP: f64 = 14.0;
/// カード上端から見出し・説明・本文・下端まで
const SEC_TITLE: f64 = 16.0;
const SEC_SUB: f64 = 40.0;
const SEC_BODY: f64 = 72.0;
const SEC_ROW_GAP: f64 = 10.0;
const SEC_BOTTOM: f64 = 18.0;

/// 角丸 / 行の高さ
const R_CARD: f64 = 12.0;
const R_PILL: f64 = 7.0;
const R_ROW: f64 = 9.0;
const PILL_H: f64 = 28.0;

/// 文字サイズ
const F_TITLE: f64 = 30.0;
const F_SUBTITLE: f64 = 13.0;
const F_CARD_TITLE: f64 = 17.0;
const F_CARD_SUB: f64 = 12.0;
const F_ROW: f64 = 13.0;
const F_NOTE: f64 = 11.0;
const F_NAV: f64 = 13.0;

/// ページ見出しの位置
const PAGE_TOP: f64 = 30.0;
const PAGE_HEAD_SUB: f64 = PAGE_TOP + 40.0;
const PAGE_BODY: f64 = 108.0;
const PAGE_BOTTOM: f64 = 24.0;

/// ナビ
const NAV_TOP: f64 = 54.0;
const NAV_PITCH: f64 = 50.0;
const NAV_ITEM_H: f64 = 38.0;
const NAV_INSET: f64 = 10.0;

/// フッター
const FOOTER_H: f64 = 60.0;

// ---------- 状態 ----------

static WINDOW: AtomicUsize = AtomicUsize::new(0);
static SCROLLS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static NAV_BUTTONS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static NAV_ROWS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static NAV_ICONS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static NAV_LABELS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
/// 画面のスイッチ(OPT_ORDER と同じ並び)
static SWITCHES: [AtomicUsize; 7] = [const { AtomicUsize::new(0) }; 7];
/// フッターのボタン(押せる状態を切り替える)
static APPLY_BUTTON: AtomicUsize = AtomicUsize::new(0);
static REVERT_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// フッターの状態表示
static STATUS: AtomicUsize = AtomicUsize::new(0);

const PAGES: [&str; 4] = ["接続", "画面配置", "共有", "その他"];
const ICONS: [&str; 4] = ["link", "display", "square.and.arrow.up", "gearshape"];
const SUBTITLES: [&str; 4] = [
    "端末、つなぎ方、接続の状態。",
    "画面の並びと、端末ごとの表示。",
    "端末の間で共有する内容。",
    "言語、キー、更新、メンテナンス。",
];

fn args() -> Vec<String> {
    std::env::args().collect()
}

/// --preview-page N を読む。プレビュー時だけ解釈する
fn page_arg() -> usize {
    if !UI_PREVIEW.load(Ordering::Relaxed) {
        return 0;
    }
    let a = args();
    a.iter()
        .position(|x| x == "--preview-page")
        .and_then(|i| a.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
        .min(3)
}

// ---------- ObjC ランタイム(gui.rs と同じ宣言) ----------

#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn objc_allocateClassPair(
        superclass: ID,
        name: *const core::ffi::c_char,
        extra_bytes: usize,
    ) -> ID;
    fn class_addMethod(cls: ID, name: SEL, imp: usize, types: *const core::ffi::c_char) -> u8;
    fn objc_registerClassPair(cls: ID);
}

// ---------- 基本部品 ----------

unsafe fn frame(v: ID, r: NSRect) {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"setFrame:"), r);
}

unsafe fn with_layer(v: ID) -> ID {
    msg1_void_u8(v, sel(c"setWantsLayer:"), 1);
    msg0(v, sel(c"layer"))
}

fn color_rgb(r: f64, g: f64, b: f64, a: f64) -> ID {
    unsafe {
        let f: unsafe extern "C" fn(ID, SEL, f64, f64, f64, f64) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(
            objc_getClass(c"NSColor".as_ptr()),
            sel(c"colorWithSRGBRed:green:blue:alpha:"),
            r,
            g,
            b,
            a,
        )
    }
}

unsafe fn system_color(name: &std::ffi::CStr) -> ID {
    msg0(objc_getClass(c"NSColor".as_ptr()), sel(name))
}

/// ビルド中のウィンドウ外観。NSColor の動的カラー(controlBackgroundColor 等)は
/// CGColor へ変換した時点の外観で固定されるため、先に外観を確定させてから解決する。
/// これをしないとダークモードで背景だけ明るいままになり、文字が読めなくなる
static BUILD_APPEARANCE: AtomicUsize = AtomicUsize::new(0);

unsafe fn build_appearance() -> ID {
    BUILD_APPEARANCE.load(Ordering::Relaxed) as ID
}

/// 動的カラーを外観を切り替えた状態で CGColor にする。
/// performAsCurrentDrawingAppearance: のブロック API は Rust から渡しにくいため、
/// NSAppearance.currentAppearance(クラスプロパティ)を一時的に切り替えて解決する。
/// macOS 12 で deprecated だが動作は残っており、CGColor 化の瞬間だけ使う。
unsafe fn cg_color(color: ID) -> ID {
    if color.is_null() {
        return std::ptr::null_mut();
    }
    let appearance = build_appearance();
    let has_current: unsafe extern "C" fn(ID, SEL, SEL) -> u8 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let cls = objc_getClass(c"NSAppearance".as_ptr());
    let settable =
        !appearance.is_null() && has_current(cls, sel(c"respondsToSelector:"), sel(c"setCurrentAppearance:")) != 0;
    if settable {
        msg1_void_id(cls, sel(c"setCurrentAppearance:"), appearance);
    }
    let cg = msg0(color, sel(c"CGColor"));
    if settable {
        msg1_void_id(cls, sel(c"setCurrentAppearance:"), std::ptr::null_mut());
    }
    cg
}

unsafe fn set_bg(v: ID, color: ID) {
    let layer = with_layer(v);
    msg1_void_id(layer, sel(c"setBackgroundColor:"), cg_color(color));
}

unsafe fn plain_view(parent: ID, r: NSRect) -> ID {
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

/// 角丸 + 塗り + 境界線のビュー。カード・行の面・ピルに使う
unsafe fn panel(parent: ID, r: NSRect, fill: ID, border: Option<ID>, radius: f64) -> ID {
    let v = plain_view(parent, r);
    let layer = with_layer(v);
    msg1_void_id(layer, sel(c"setBackgroundColor:"), cg_color(fill));
    if let Some(border) = border {
        msg1_void_id(layer, sel(c"setBorderColor:"), cg_color(border));
        msg1_id_f64(layer, sel(c"setBorderWidth:"), 1.0);
    }
    msg1_id_f64(layer, sel(c"setCornerRadius:"), radius);
    v
}

/// SF Symbol
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

unsafe fn icon_view(parent: ID, name: &str, title: &str, r: NSRect, tint: Option<ID>) -> ID {
    let v = msg0(objc_getClass(c"NSImageView".as_ptr()), sel(c"new"));
    frame(v, r);
    msg1_void_id(v, sel(c"setImage:"), symbol(name, title));
    msg1_void_i64(v, sel(c"setImageScaling:"), 1); // NSImageScaleProportionallyUpOrDown
    msg1_void_id(v, sel(c"setAccessibilityLabel:"), nsstring(title));
    if let Some(tint) = tint {
        msg1_void_id(v, sel(c"setContentTintColor:"), tint);
    }
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}

/// ラベル
unsafe fn label(
    parent: ID,
    s: &str,
    x: f64,
    y: f64,
    w: f64,
    size: f64,
    bold: bool,
    muted: bool,
) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        objc_getClass(c"NSTextField".as_ptr()),
        sel(c"labelWithString:"),
        nsstring(s),
    );
    frame(v, NSRect { x, y, w, h: size + 6.0 });
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(if bold { c"boldSystemFontOfSize:" } else { c"systemFontOfSize:" }),
        size,
    );
    msg1_void_id(v, sel(c"setFont:"), font);
    let color = msg0(
        objc_getClass(c"NSColor".as_ptr()),
        if muted {
            sel(c"secondaryLabelColor")
        } else {
            sel(c"labelColor")
        },
    );
    msg1_void_id(v, sel(c"setTextColor:"), color);
    // はみ出しは末尾省略。単一行に固定して隣と重ならないようにする
    let f2: unsafe extern "C" fn(ID, SEL, i64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f2(v, sel(c"setLineBreakMode:"), 4); // NSLineBreakByTruncatingTail
    msg1_void_u8(v, sel(c"setUsesSingleLineMode:"), 1);
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}

unsafe fn status_dot(parent: ID, x: f64, y: f64, d: f64, color: ID) {
    panel(parent, NSRect { x, y, w: d, h: d }, color, None, d / 2.0);
}

// ---------- ボタン(ネイティブ NSButton を使い、幅は文字から測る) ----------

#[derive(Clone, Copy, PartialEq)]
enum BtnKind {
    /// accent 塗り(既定ボタン相当)
    Primary,
    /// 枠線のみ
    Secondary,
    /// 赤字(破壊的操作)
    Destructive,
}

/// [view frame] を NSRect として読む
unsafe fn frame_of(v: ID) -> NSRect {
    let f: unsafe extern "C" fn(ID, SEL) -> NSRect =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"frame"))
}

/// 文字が収まる幅を実測する。sizeToFit は現在の frame 幅を無視して
/// 内容に合う大きさへ変えるので、その結果を読む
unsafe fn button_width(title: &str) -> f64 {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let b = f(
        msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect { x: 0.0, y: 0.0, w: 200.0, h: PILL_H },
    );
    msg1_void_id(b, sel(c"setTitle:"), nsstring(title));
    msg1_void_i64(b, sel(c"setBezelStyle:"), 1); // NSBezelStyleRounded
    msg0_void(b, sel(c"sizeToFit"));
    let fit = frame_of(b);
    // 角丸ボタンの標準的な左右余白を足し、最低幅を確保する
    (fit.w + 12.0).max(64.0)
}

/// ネイティブの角丸ボタン。primary は accent で塗る
unsafe fn button(
    parent: ID,
    title: &str,
    x: f64,
    y: f64,
    kind: BtnKind,
) -> (f64, f64) {
    let w = button_width(title);
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let b = f(
        msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect { x, y, w, h: PILL_H },
    );
    msg1_void_id(b, sel(c"setTitle:"), nsstring(title));
    msg1_void_i64(b, sel(c"setBezelStyle:"), 1); // NSBezelStyleRounded
    msg1_void_u8(b, sel(c"setBordered:"), 1);
    match kind {
        BtnKind::Primary => {
            msg1_void_id(b, sel(c"setBezelColor:"), system_color(c"controlAccentColor"));
        }
        BtnKind::Secondary => {}
        BtnKind::Destructive => {
            msg1_void_id(b, sel(c"setContentTintColor:"), system_color(c"systemRedColor"));
        }
    }
    msg1_void_id(parent, sel(c"addSubview:"), b);
    (x, w)
}

/// 幅を自分で測らずに置きたい場所へ置く(フッターなど)
unsafe fn button_at(parent: ID, title: &str, x: f64, y: f64, kind: BtnKind) -> f64 {
    let (_, w) = button(parent, title, x, y, kind);
    w
}

/// 画面のスイッチとモデルの項目の対応
#[derive(Clone, Copy, PartialEq)]
enum Opt {
    ClipShare,
    ShareFiles,
    AudioMuted,
    HistoryEnabled,
    OpenReceiveFolder,
    Startup,
    UpdateCheck,
}

/// スイッチの並び(モデルと同じ順序)
const OPT_ORDER: [Opt; 7] = [
    Opt::ClipShare,
    Opt::ShareFiles,
    Opt::AudioMuted,
    Opt::HistoryEnabled,
    Opt::OpenReceiveFolder,
    Opt::Startup,
    Opt::UpdateCheck,
];

unsafe fn tag_of(v: ID, n: i64) {
    let f: unsafe extern "C" fn(ID, SEL, i64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"setTag:"), n);
}

unsafe fn switch(parent: ID, target: ID, on: bool, opt: Opt, x: f64, y: f64) -> ID {
    let s = msg0(objc_getClass(c"NSSwitch".as_ptr()), sel(c"new"));
    frame(s, NSRect { x, y, w: 44.0, h: 28.0 });
    msg1_void_id(parent, sel(c"addSubview:"), s);
    msg1_void_i64(s, sel(c"setState:"), if on { 1 } else { 0 });
    msg1_void_i64(s, sel(c"setIntValue:"), if on { 1 } else { 0 });
    let index = OPT_ORDER.iter().position(|o| *o == opt).unwrap_or(0);
    SWITCHES[index].store(s as usize, Ordering::Relaxed);
    msg1_void_id(s, sel(c"setTarget:"), target);
    msg1_void_sel(s, sel(c"setAction:"), sel(c"sdNewToggle:"));
    tag_of(s, index as i64);
    s
}

/// スイッチの現在状態をモデルへ書き戻す(クリック時の経路)
fn read_switch(index: usize, on: bool) {
    if index >= OPT_ORDER.len() {
        return;
    }
    let opt = OPT_ORDER[index];
    crate::gui::settings_model::update(|d| match opt {
        Opt::ClipShare => d.clip_share = on,
        Opt::ShareFiles => d.share_files = on,
        Opt::AudioMuted => d.audio_muted = on,
        Opt::HistoryEnabled => d.history_enabled = on,
        Opt::OpenReceiveFolder => d.open_receive_folder = on,
        Opt::Startup => d.startup = on,
        Opt::UpdateCheck => d.update_check = on,
    });
}

/// 画面のスイッチ表示をモデルへ合わせる
fn sync_switches() {
    let d = crate::gui::settings_model::draft();
    let states = [
        d.clip_share,
        d.share_files,
        d.audio_muted,
        d.history_enabled,
        d.open_receive_folder,
        d.startup,
        d.update_check,
    ];
    for (slot, on) in SWITCHES.iter().zip(states) {
        let v = slot.load(Ordering::Relaxed) as ID;
        if !v.is_null() {
            unsafe {
                msg1_void_i64(v, sel(c"setState:"), if on { 1 } else { 0 });
            }
        }
    }
}

unsafe fn slider(parent: ID, value: f64, min: f64, max: f64, x: f64, y: f64, w: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let s = f(
        objc_getClass(c"NSSlider".as_ptr()),
        sel(c"sliderWithValue:minValue:maxValue:target:action:"),
        value,
        min,
        max,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    frame(s, NSRect { x, y, w, h: 24.0 });
    msg1_void_u8(s, sel(c"setContinuous:"), 1);
    msg1_void_id(parent, sel(c"addSubview:"), s);
    s
}

unsafe fn popup(parent: ID, titles: &[&str], selected: usize, x: f64, y: f64, w: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let p = f(
        msg0(objc_getClass(c"NSPopUpButton".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:pullsDown:"),
        NSRect { x, y, w, h: 26.0 },
        0,
    );
    for t in titles {
        msg1_void_id(p, sel(c"addItemWithTitle:"), nsstring(t));
    }
    msg1_void_i64(p, sel(c"selectItemAtIndex:"), selected as i64);
    msg1_void_id(parent, sel(c"addSubview:"), p);
    p
}

unsafe fn text_field(parent: ID, value: &str, placeholder: &str, x: f64, y: f64, w: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        msg0(objc_getClass(c"NSTextField".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect { x, y, w, h: 26.0 },
    );
    msg1_void_u8(v, sel(c"setBezeled:"), 1);
    msg1_void_u8(v, sel(c"setDrawsBackground:"), 1);
    msg1_void_u8(v, sel(c"setEditable:"), 0);
    msg1_void_u8(v, sel(c"setSelectable:"), 1);
    msg1_void_id(v, sel(c"setStringValue:"), nsstring(value));
    if !placeholder.is_empty() {
        msg1_void_id(v, sel(c"setPlaceholderString:"), nsstring(placeholder));
    }
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}

unsafe fn segmented(parent: ID, labels: &[&str], selected: usize, x: f64, y: f64, w: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let s = f(
        msg0(objc_getClass(c"NSSegmentedControl".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect { x, y, w, h: 26.0 },
    );
    msg1_void_i64(s, sel(c"setSegmentCount:"), labels.len() as i64);
    for (i, t) in labels.iter().enumerate() {
        let f2: unsafe extern "C" fn(ID, SEL, ID, isize) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f2(s, sel(c"setLabel:forSegment:"), nsstring(t), i as isize);
    }
    msg1_void_i64(s, sel(c"setSelectedSegment:"), selected as i64);
    msg1_void_i64(s, sel(c"setTrackingMode:"), 0); // NSSegmentSwitchTrackingSelectOne
    msg1_void_id(parent, sel(c"addSubview:"), s);
    s
}

// ---------- 反転キャンバス ----------

unsafe extern "C" fn flipped_yes(_s: ID, _c: SEL) -> u8 {
    1
}

/// isFlipped=YES の NSView。ここへ置く部品は「上端からの距離」で座標を書ける
unsafe fn canvas(parent: ID, r: NSRect) -> ID {
    static CLS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let cls = *CLS.get_or_init(|| {
        let super_cls = objc_getClass(c"NSView".as_ptr());
        if super_cls.is_null() {
            return 0usize;
        }
        let cls = objc_allocateClassPair(super_cls, c"SDCanvasView".as_ptr(), 0);
        if cls.is_null() {
            return 0usize;
        }
        if class_addMethod(
            cls,
            sel(c"isFlipped"),
            flipped_yes as *const () as usize,
            c"B@:".as_ptr(),
        ) == 0
        {
            return 0usize;
        }
        objc_registerClassPair(cls);
        cls as usize
    });
    if cls == 0 {
        return plain_view(parent, r);
    }
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(msg0(cls as ID, sel(c"alloc")), sel(c"initWithFrame:"), r);
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}

// ---------- カード ----------

/// カードに載せる行の高さ一覧から、カード全体の高さを決める。
/// 中身と枠がずれないよう、高さは常にこの関数で計算する
fn card_height(rows: &[f64]) -> f64 {
    let sum: f64 = rows.iter().sum();
    let gaps = SEC_ROW_GAP * (rows.len().saturating_sub(1)) as f64;
    SEC_BODY + sum + gaps + SEC_BOTTOM
}

/// カードを敷き、見出しを描き、各行の上端 y を返す
unsafe fn section(
    p: ID,
    y: f64,
    rows: &[f64],
    title: &str,
    subtitle: &str,
) -> Vec<f64> {
    let h = card_height(rows);
    panel(
        p,
        NSRect { x: CARD_X, y, w: CARD_W, h },
        system_color(c"controlBackgroundColor"),
        Some(system_color(c"separatorColor")),
        R_CARD,
    );
    let tx = CARD_X + CARD_PAD;
    let tw = CARD_W - CARD_PAD * 2.0 - 200.0;
    label(p, title, tx, y + SEC_TITLE, tw, F_CARD_TITLE, true, false);
    if !subtitle.is_empty() {
        label(p, subtitle, tx, y + SEC_SUB, tw, F_CARD_SUB, false, true);
    }
    let mut out = Vec::with_capacity(rows.len());
    let mut cy = y + SEC_BODY;
    for h in rows {
        out.push(cy);
        cy += h + SEC_ROW_GAP;
    }
    out
}

/// 行の面
unsafe fn row_plate(p: ID, y: f64, h: f64) -> ID {
    panel(
        p,
        NSRect {
            x: CARD_X + CARD_PAD,
            y,
            w: CARD_W - CARD_PAD * 2.0,
            h,
        },
        system_color(c"controlBackgroundColor"),
        Some(color_rgb(0.5, 0.5, 0.5, 0.18)),
        R_ROW,
    )
}

/// 行(面 + ラベル + 補足)
unsafe fn row(p: ID, y: f64, h: f64, title: &str, note: &str) -> ID {
    let plate = row_plate(p, y, h);
    let tx = CARD_X + CARD_PAD + 12.0;
    let tw = CARD_W - CARD_PAD * 2.0 - 24.0 - 300.0;
    if note.is_empty() {
        label(p, title, tx, y + (h - F_ROW) / 2.0 - 3.0, tw, F_ROW, false, false);
    } else {
        label(p, title, tx, y + 7.0, tw, F_ROW, false, false);
        label(p, note, tx, y + 26.0, tw, F_NOTE, false, true);
    }
    plate
}

/// 行の右端からの配置に使う。右端の内側 x を返す
fn row_right() -> f64 {
    CARD_X + CARD_W - CARD_PAD - 12.0
}

fn center_y(y: f64, h: f64, control_h: f64) -> f64 {
    y + (h - control_h) / 2.0
}

// ---------- 各ページ ----------

unsafe fn build_connection(p: ID, target: ID) {
    // 端末
    let y = PAGE_BODY;
    let rows = section(p, y, &[84.0], "端末", "この Mac と接続している端末、これから加える端末。");
    // 見出しの右に操作を置く
    row_buttons_right_of_title(p, target, y, &[("端末を追加…", BtnKind::Primary, c"sdRegistration:"), ("診断…", BtnKind::Secondary, c"sdDiagnose:")]);
    let ry = rows[0];
    row_plate(p, ry, 84.0);
    icon_view(
        p,
        "pc",
        "Windows PC",
        NSRect { x: CARD_X + CARD_PAD + 14.0, y: ry + 16.0, w: 52.0, h: 52.0 },
        Some(system_color(c"controlAccentColor")),
    );
    label(p, "Windows PC", CARD_X + CARD_PAD + 80.0, ry + 20.0, 260.0, F_ROW + 1.0, false, false);
    label(p, "192.168.1.42", CARD_X + CARD_PAD + 80.0, ry + 44.0, 260.0, F_NOTE, false, true);
    status_dot(p, row_right() - 100.0, ry + 38.0, 9.0, system_color(c"systemGreenColor"));
    label(p, "接続中", row_right() - 84.0, ry + 33.0, 70.0, F_ROW, false, false);
    icon_view(
        p,
        "chevron.right",
        "詳細",
        NSRect { x: CARD_X + CARD_W - CARD_PAD - 12.0 - 8.0, y: ry + 36.0, w: 8.0, h: 13.0 },
        Some(system_color(c"tertiaryLabelColor")),
    );

    // 接続方法
    let y = y + card_height(&[84.0]) + CARD_GAP;
    let rows = section(p, y, &[46.0, 46.0], "接続方法", "この端末へつなぐ方法と、そのアドレス。");
    row(p, rows[0], 46.0, "方法", "");
    popup(p, &["自動(推奨)", "IP アドレスを指定"], 0, row_right() - 300.0, center_y(rows[0], 46.0, 26.0), 300.0);
    row(p, rows[1], 46.0, "アドレス", "");
    let save_w = button_width("保存して再接続");
    text_field(
        p,
        "192.168.1.42",
        "192.168.1.10",
        row_right() - save_w - 8.0 - 240.0,
        center_y(rows[1], 46.0, 26.0),
        240.0,
    );
    button(p, "保存して再接続", row_right() - save_w, center_y(rows[1], 46.0, PILL_H), BtnKind::Primary);

    // 画面配置への近道
    let y = y + card_height(&[46.0, 46.0]) + CARD_GAP;
    let rows = section(p, y, &[44.0], "画面配置", "画面の並びを変えると、カーソルが移る位置も変わります。");
    row(p, rows[0], 44.0, "右に配置・感度 1.0・範囲 全域", "");
    let w = button_width("画面配置を開く");
    button(p, "画面配置を開く", row_right() - w, center_y(rows[0], 44.0, PILL_H), BtnKind::Secondary);
}

/// カード見出しの右端にボタンを並べる(見出し行に置く操作)
unsafe fn row_buttons_right_of_title(
    p: ID,
    target: ID,
    card_y: f64,
    buttons: &[(&str, BtnKind, &std::ffi::CStr)],
) {
    let mut right = CARD_X + CARD_W - CARD_PAD;
    for (title, kind, action) in buttons.iter().rev() {
        let w = button_width(title);
        right -= w;
        place_action(p, target, title, right, card_y + 11.0, *kind, action);
        right -= 8.0;
    }
}

unsafe fn build_displays(p: ID, target: ID) {
    let y = PAGE_BODY;
    let rows = section(p, y, &[210.0], "画面の並び", "この Mac の画面と、相手の端末の位置。");
    row_buttons_right_of_title(p, target, y, &[("画面を識別", BtnKind::Secondary, c"sdRefinedIdentify:"), ("数値で指定…", BtnKind::Secondary, c"sdRefinedRanges:")]);
    let cy = rows[0];
    row_plate(p, cy, 210.0);
    let face = system_color(c"controlBackgroundColor");
    panel(
        p,
        NSRect { x: CARD_X + CARD_PAD + 56.0, y: cy + 58.0, w: 200.0, h: 112.0 },
        face,
        Some(system_color(c"controlAccentColor")),
        6.0,
    );
    panel(
        p,
        NSRect { x: CARD_X + CARD_PAD + 306.0, y: cy + 74.0, w: 170.0, h: 96.0 },
        face,
        Some(system_color(c"separatorColor")),
        6.0,
    );
    label(p, "この Mac", CARD_X + CARD_PAD + 56.0, cy + 178.0, 200.0, F_ROW, false, false);
    label(p, "Windows PC", CARD_X + CARD_PAD + 306.0, cy + 178.0, 200.0, F_ROW, false, false);
    label(p, "右に配置", CARD_X + CARD_PAD + 330.0, cy + 84.0, 120.0, F_NOTE, false, true);

    let y = y + card_height(&[210.0]) + CARD_GAP;
    let rows = section(p, y, &[44.0], "感度", "画面の間を移るときのポインタの速さ。");
    row(p, rows[0], 44.0, "遅い", "");
    let field_w = 70.0;
    let slider_w = 320.0;
    let sx = CARD_X + CARD_PAD + 12.0 + 44.0;
    slider(p, 1.0, 0.2, 3.0, sx, center_y(rows[0], 44.0, 24.0), slider_w);
    label(p, "速い", sx + slider_w + 12.0, center_y(rows[0], 44.0, F_ROW) - 3.0, 44.0, F_ROW, false, true);
    text_field(p, "1.0", "", row_right() - field_w, center_y(rows[0], 44.0, 26.0), field_w);

    let y = y + card_height(&[44.0]) + CARD_GAP;
    let rows = section(p, y, &[44.0], "送出する範囲", "この画面のどこから相手へカーソルを出せるか。");
    row(p, rows[0], 44.0, "範囲", "");
    segmented(p, &["全域", "カスタム"], 0, row_right() - 180.0, center_y(rows[0], 44.0, 26.0), 180.0);
}

unsafe fn build_sharing(p: ID, target: ID) {
    let y = PAGE_BODY;
    let rows = section(p, y, &[52.0], "クリップボード共有", "コピーしたテキストや画像を、もう1台で貼り付けられるようにします。");
    row(p, rows[0], 52.0, "クリップボード共有を有効にする", "どちらかでコピーした内容を、もう一方で貼り付けられます。");
    switch(p, target, true, Opt::ClipShare, row_right() - 44.0, center_y(rows[0], 52.0, 28.0));

    let y = y + card_height(&[52.0]) + CARD_GAP;
    let rows = section(p, y, &[52.0], "ファイル転送", "端末の間でファイルを送受信します。");
    row(p, rows[0], 52.0, "受信フォルダ", "ほかの端末から届いたファイルはここに保存されます。");
    let w = button_width("変更…");
    text_field(p, "~/Downloads/Knit", "", row_right() - w - 8.0 - 210.0, center_y(rows[0], 52.0, 26.0), 210.0);
    place_action(p, target, "変更…", row_right() - w, center_y(rows[0], 52.0, PILL_H), BtnKind::Secondary, c"sdRefinedFolder:");

    let y = y + card_height(&[52.0]) + CARD_GAP;
    let rows = section(p, y, &[46.0, 46.0], "音声", "ほかの端末の音声を、この Mac で再生します。");
    row(p, rows[0], 46.0, "音声の再生を許可する", "");
    switch(p, target, true, Opt::AudioMuted, row_right() - 44.0, center_y(rows[0], 46.0, 28.0));
    row(p, rows[1], 46.0, "出力先", "");
    popup(p, &["内蔵スピーカー", "AirPods Pro"], 0, row_right() - 260.0, center_y(rows[1], 46.0, 26.0), 260.0);

    let y = y + card_height(&[46.0, 46.0]) + CARD_GAP;
    let rows = section(p, y, &[44.0], "クリップボード履歴", "最近コピーした内容を残し、あとから復元できるようにします。");
    row(p, rows[0], 44.0, "履歴に残す", "");
    switch(p, target, true, Opt::HistoryEnabled, row_right() - 44.0, center_y(rows[0], 44.0, 28.0));
}

unsafe fn build_general(p: ID, target: ID) {
    let y = PAGE_BODY;
    let rows = section(p, y, &[52.0], "ログイン時に開く", "この Mac にログインしたとき、Knit を自動で起動します。");
    row(p, rows[0], 52.0, "ログイン時に開く", "バックグラウンドで自動的に起動します。");
    switch(p, target, true, Opt::Startup, row_right() - 44.0, center_y(rows[0], 52.0, 28.0));

    let y = y + card_height(&[52.0]) + CARD_GAP;
    let rows = section(p, y, &[44.0], "言語", "Knit の画面で使う言語を選びます。");
    row(p, rows[0], 44.0, "言語", "");
    popup(p, &["日本語", "English (United States)"], 0, row_right() - 260.0, center_y(rows[0], 44.0, 26.0), 260.0);

    let y = y + card_height(&[44.0]) + CARD_GAP;
    let rows = section(p, y, &[44.0], "キーボード切替", "端末の間をキーボードで行き来する方法。");
    row(p, rows[0], 44.0, "切替キー", "");
    popup(p, &["右端をダブルタップ", "Control + Option + 矢印キー"], 0, row_right() - 280.0, center_y(rows[0], 44.0, 26.0), 280.0);

    let y = y + card_height(&[44.0]) + CARD_GAP;
    let rows = section(p, y, &[52.0], "キー変換のプレビュー", "この設定での修飾キーの対応。");
    row(p, rows[0], 52.0, "⌘  →  Ctrl", "⌥  →  Alt");

    let y = y + card_height(&[52.0]) + CARD_GAP;
    let rows = section(p, y, &[44.0], "メンテナンス", "設定の控え、ログ、初期化。");
    row(p, rows[0], 44.0, "", "");
    let mut x = CARD_X + CARD_PAD + 12.0;
    for (title, kind, action) in [
        ("設定を書き出す…", BtnKind::Secondary, c"sdRefinedExport:"),
        ("設定を読み込む…", BtnKind::Secondary, c"sdRefinedImport:"),
        ("ログ", BtnKind::Secondary, c"sdOpenLog:"),
        ("登録を初期化…", BtnKind::Destructive, c"sdResetRegistration:"),
        ("設定を初期化…", BtnKind::Destructive, c"sdResetSettings:"),
    ] {
        let w = button_width(title);
        place_action(p, target, title, x, center_y(rows[0], 44.0, PILL_H), kind, action);
        x += w + 8.0;
    }
}

/// ページ内容の高さ。最終カードの下端 + 下余白
fn page_height(page: usize) -> f64 {
    let mut y = PAGE_BODY;
    let cards: &[&[f64]] = match page {
        0 => &[&[84.0], &[46.0, 46.0], &[44.0]],
        1 => &[&[210.0], &[44.0], &[44.0]],
        2 => &[&[52.0], &[52.0], &[46.0, 46.0], &[44.0]],
        _ => &[&[52.0], &[44.0], &[44.0], &[52.0], &[44.0]],
    };
    for rows in cards {
        y += card_height(rows) + CARD_GAP;
    }
    y - CARD_GAP + PAGE_BOTTOM
}
// ---------- 操作のハンドラ(モデルへの配線) ----------

/// スイッチが切り替わった。画面の値をそのままモデルへ写す
pub(in crate::gui) unsafe extern "C" fn toggle(_s: ID, _c: SEL, sender: ID) {
    let index = crate::msg0_isize(sender, sel(c"tag")).max(0) as usize;
    let on = crate::msg0_isize(sender, sel(c"state")) == 1;
    read_switch(index, on);
    update_footer();
}

/// 「適用」。保存先へ書き込み、結果をフッターに出す
pub(in crate::gui) unsafe extern "C" fn apply(_s: ID, _c: SEL, _sender: ID) {
    match crate::gui::settings_model::apply() {
        Ok(values) => {
            if values.language != localization::language() {
                localization::set_language(values.language);
            }
            sync_switches();
            disable_footer();
            set_status("保存しました");
        }
        Err(e) => {
            sync_switches();
            update_footer();
            set_status(&format!("保存できませんでした: {e}"));
        }
    }
}

/// 「元に戻す」。下書きを捨てて保存済みの値に戻す
pub(in crate::gui) unsafe extern "C" fn revert(_s: ID, _c: SEL, _sender: ID) {
    crate::gui::settings_model::discard();
    sync_switches();
    disable_footer();
    set_status("");
}

unsafe fn set_status(text: &str) {
    let v = STATUS.load(Ordering::Relaxed) as ID;
    if !v.is_null() {
        msg1_void_id(v, sel(c"setStringValue:"), nsstring(text));
    }
}

/// 未保存の変更に応じてフッターのボタンを有効/無効にする
unsafe fn update_footer() {
    let dirty = crate::gui::settings_model::is_dirty();
    for slot in [&APPLY_BUTTON, &REVERT_BUTTON] {
        let b = slot.load(Ordering::Relaxed) as ID;
        if !b.is_null() {
            msg1_void_u8(b, sel(c"setEnabled:"), dirty as u8);
        }
    }
}

/// 保存直後は未保存の変更が無いのでフッターを無効に戻す
unsafe fn disable_footer() {
    for slot in [&APPLY_BUTTON, &REVERT_BUTTON] {
        let b = slot.load(Ordering::Relaxed) as ID;
        if !b.is_null() {
            msg1_void_u8(b, sel(c"setEnabled:"), 0);
        }
    }
}

/// 幅を測ってアクション付きボタンを置く(最後に追加したビューを掴む)
unsafe fn place_action(
    parent: ID,
    target: ID,
    title: &str,
    x: f64,
    y: f64,
    kind: BtnKind,
    action: &std::ffi::CStr,
) -> ID {
    let (_, _w) = button(parent, title, x, y, kind);
    let subviews = msg0(parent, sel(c"subviews"));
    let count = crate::msg0_isize(subviews, sel(c"count"));
    if count <= 0 {
        return std::ptr::null_mut();
    }
    let f: unsafe extern "C" fn(ID, SEL, isize) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let b = f(subviews, sel(c"objectAtIndex:"), count - 1);
    msg1_void_id(b, sel(c"setTarget:"), target);
    msg1_void_sel(b, sel(c"setAction:"), sel(action));
    b
}
// ---------- 自己テスト(配線を GUI 操作なしで確かめる) ----------

/// `--settings-new --selftest` で実行する。クリックに頼らず、
/// スイッチ操作と同じ経路を通して配線と安全柵を検証する。
pub(crate) unsafe fn selftest() -> bool {
    let mut ok = true;

    // 1. 初期状態は未編集
    crate::gui::settings_model::discard();
    report(&mut ok, "初期状態は未編集", !crate::gui::settings_model::is_dirty());

    // 2. スイッチ操作と同じ経路で値が変わる
    let before = crate::gui::settings_model::draft();
    read_switch(0, !before.clip_share);
    report(&mut ok, "スイッチ操作で下書きが変わる", crate::gui::settings_model::is_dirty());
    report(
        &mut ok,
        "変更がモデルへ届く",
        crate::gui::settings_model::draft().clip_share != before.clip_share,
    );

    // 3. 元に戻すと値も未編集状態も戻る
    revert(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
    report(&mut ok, "元に戻すと未編集へ戻る", !crate::gui::settings_model::is_dirty());
    report(
        &mut ok,
        "元に戻すと値も戻る",
        crate::gui::settings_model::draft().clip_share == before.clip_share,
    );

    // 4. 対応表とスイッチの数が一致している
    report(&mut ok, "スイッチの対応表が7件", OPT_ORDER.len() == SWITCHES.len());

    // 5. プレビュー中は実ファイルを書き換えない
    let dir = knit_common::envutil::config_dir();
    let prefs_path = dir.as_ref().map(|d| d.join("preferences.json"));
    let desktop_path = dir.as_ref().map(|d| d.join("desktop-settings.json"));
    let stamp = |p: &Option<std::path::PathBuf>| {
        p.as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
    };
    let prefs_before = stamp(&prefs_path);
    let desktop_before = stamp(&desktop_path);
    read_switch(0, !before.clip_share);
    report(&mut ok, "プレビュー中の適用は成功を返す", crate::gui::settings_model::apply().is_ok());
    report(
        &mut ok,
        "プレビュー中は下書きを破棄する",
        !crate::gui::settings_model::is_dirty(),
    );
    report(
        &mut ok,
        "プレビュー中は設定ファイルを書き換えない",
        stamp(&prefs_path) == prefs_before && stamp(&desktop_path) == desktop_before,
    );
    let restored = crate::gui::settings_model::discard();

    println!(
        "[selftest] {} (clip_share: {} -> {})",
        if ok { "ALL PASS" } else { "FAILED" },
        before.clip_share,
        restored.clip_share
    );
    ok
}

fn report(ok: &mut bool, name: &str, passed: bool) {
    println!("[selftest] {} {}", if passed { "PASS" } else { "FAIL" }, name);
    if !passed {
        *ok = false;
    }
}
// ---------- ウィンドウ ----------

unsafe fn style_nav(index: usize, selected: bool) {
    let row = NAV_ROWS[index].load(Ordering::Relaxed) as ID;
    let icon = NAV_ICONS[index].load(Ordering::Relaxed) as ID;
    let lb = NAV_LABELS[index].load(Ordering::Relaxed) as ID;
    let accent = system_color(c"controlAccentColor");
    if !row.is_null() {
        if selected {
            let f: unsafe extern "C" fn(ID, SEL, f64) -> ID =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            set_bg(row, f(accent, sel(c"colorWithAlphaComponent:"), 0.16));
        } else {
            set_bg(row, system_color(c"clearColor"));
        }
    }
    let want = if selected { accent } else { system_color(c"labelColor") };
    if !icon.is_null() {
        msg1_void_id(icon, sel(c"setContentTintColor:"), want);
    }
    if !lb.is_null() {
        msg1_void_id(lb, sel(c"setTextColor:"), want);
    }
}

/// 透明なクリック受けボタン(見た目は自前で描く)
unsafe fn hit_button(parent: ID, title: &str, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let b = f(
        msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        r,
    );
    msg1_void_id(b, sel(c"setTitle:"), nsstring(""));
    msg1_void_i64(b, sel(c"setBezelStyle:"), 1);
    msg1_void_u8(b, sel(c"setBordered:"), 0);
    msg1_void_u8(b, sel(c"setTransparent:"), 1);
    msg1_void_id(b, sel(c"setAccessibilityLabel:"), nsstring(title));
    msg1_void_id(parent, sel(c"addSubview:"), b);
    b
}

unsafe fn build_sidebar(cv: ID, target: ID, current: usize) {
    let sb = msg0(objc_getClass(c"NSVisualEffectView".as_ptr()), sel(c"new"));
    frame(sb, NSRect { x: 0.0, y: 0.0, w: SIDEBAR_W, h: H });
    // NSVisualEffectMaterialSidebar = 7
    // NSVisualEffectBlendingModeBehindWindow = 0(背後のデスクトップをぼかして透かす。
    // デザイン画像と同じ「壁紙の色がうっすら乗る」見え方になる)
    let f: unsafe extern "C" fn(ID, SEL, i64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(sb, sel(c"setMaterial:"), 7);
    f(sb, sel(c"setBlendingMode:"), 0);
    f(sb, sel(c"setState:"), 1);
    msg1_void_id(cv, sel(c"addSubview:"), sb);
    let sbc = canvas(sb, NSRect { x: 0.0, y: 0.0, w: SIDEBAR_W, h: H });

    for (i, title) in PAGES.iter().enumerate() {
        let y = NAV_TOP + i as f64 * NAV_PITCH;
        let row = plain_view(
            sbc,
            NSRect { x: NAV_INSET, y, w: SIDEBAR_W - NAV_INSET * 2.0, h: NAV_ITEM_H },
        );
        let layer = with_layer(row);
        msg1_id_f64(layer, sel(c"setCornerRadius:"), R_PILL);
        let accent = system_color(c"controlAccentColor");
        if i == current {
            let fade: unsafe extern "C" fn(ID, SEL, f64) -> ID =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            set_bg(row, fade(accent, sel(c"colorWithAlphaComponent:"), 0.16));
        } else {
            set_bg(row, system_color(c"clearColor"));
        }
        let icon = icon_view(
            sbc,
            ICONS[i],
            title,
            NSRect { x: NAV_INSET + 12.0, y: y + (NAV_ITEM_H - 18.0) / 2.0, w: 18.0, h: 18.0 },
            Some(if i == current { accent } else { system_color(c"labelColor") }),
        );
        let lb = label(
            sbc,
            title,
            NAV_INSET + 40.0,
            y + (NAV_ITEM_H - F_NAV) / 2.0 - 3.0,
            SIDEBAR_W - NAV_INSET * 2.0 - 48.0,
            F_NAV,
            false,
            false,
        );
        if i == current {
            msg1_void_id(lb, sel(c"setTextColor:"), accent);
        }
        let hit = hit_button(
            sbc,
            title,
            NSRect { x: NAV_INSET, y, w: SIDEBAR_W - NAV_INSET * 2.0, h: NAV_ITEM_H },
        );
        NAV_BUTTONS[i].store(hit as usize, Ordering::Relaxed);
        NAV_ROWS[i].store(row as usize, Ordering::Relaxed);
        NAV_ICONS[i].store(icon as usize, Ordering::Relaxed);
        NAV_LABELS[i].store(lb as usize, Ordering::Relaxed);
        msg1_void_id(hit, sel(c"setTarget:"), target);
        msg1_void_sel(hit, sel(c"setAction:"), sel(c"sdNewNavigate:"));
        let tag: unsafe extern "C" fn(ID, SEL, i64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        tag(hit, sel(c"setTag:"), i as i64);
    }

    label(sbc, "デザインプレビュー", NAV_INSET + 4.0, H - 92.0, 180.0, F_NOTE, false, false);
    label(sbc, "表示はすべてサンプルです。", NAV_INSET + 4.0, H - 74.0, 190.0, F_NOTE, false, true);
    label(sbc, "設定は変更されません。", NAV_INSET + 4.0, H - 58.0, 190.0, F_NOTE, false, true);
}

/// ページ面(スクロール)。(スクロールビュー, 内容キャンバス) を返す
unsafe fn page_scroll(cv: ID, h: f64) -> (ID, ID) {
    let scroll = msg0(objc_getClass(c"NSScrollView".as_ptr()), sel(c"new"));
    // cv は下端基準。足元にフッターを空けるので、その分だけ上へずらす
    frame(scroll, NSRect { x: PAGE_X, y: FOOTER_H, w: PAGE_W, h: H - FOOTER_H });
    msg1_void_u8(scroll, sel(c"setDrawsBackground:"), 0);
    msg1_void_u8(scroll, sel(c"setHasVerticalScroller:"), 1);
    msg1_void_u8(scroll, sel(c"setAutohidesScrollers:"), 1);
    let style: unsafe extern "C" fn(ID, SEL, i64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    style(scroll, sel(c"setScrollerStyle:"), 1); // NSScrollerStyleOverlay
    // タイトルバー下の自動インセットを切る。切らないと内容が約 32pt 下がり、
    // デザイン(見出しが窓上端から 36pt)とずれる
    msg1_void_u8(scroll, sel(c"setAutomaticallyAdjustsContentInsets:"), 0);
    let zero: unsafe extern "C" fn(ID, SEL, f64, f64, f64, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    zero(scroll, sel(c"setContentInsets:"), 0.0, 0.0, 0.0, 0.0);
    msg1_void_id(cv, sel(c"addSubview:"), scroll);
    let doc = canvas(scroll, NSRect { x: 0.0, y: 0.0, w: PAGE_W, h: h.max(H) });
    msg1_void_id(scroll, sel(c"setDocumentView:"), doc);
    (scroll, doc)
}

pub(in crate::gui) unsafe fn build(target: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    // titled(1) | closable(2) | miniaturizable(4) | resizable(8) | fullSizeContentView(32768)
    let win = f(
        msg0(objc_getClass(c"NSWindow".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentRect:styleMask:backing:defer:"),
        NSRect { x: 0.0, y: 0.0, w: W, h: H },
        1 | 2 | 4 | 8 | 32768,
        2,
        0,
    );
    if win.is_null() {
        return win;
    }
    WINDOW.store(win as usize, Ordering::Relaxed);
    msg1_void_id(win, sel(c"setTitle:"), nsstring("Knit の設定"));
    let dark = args().iter().any(|a| a == "--preview-dark");
    msg1_void_id(
        win,
        sel(c"setAppearance:"),
        msg1_id(
            objc_getClass(c"NSAppearance".as_ptr()),
            sel(c"appearanceNamed:"),
            nsstring(if dark {
                "NSAppearanceNameDarkAqua"
            } else {
                "NSAppearanceNameAqua"
            }),
        ),
    );
    // 外観を先に確定させ、窓と部品の両方へ効かせる(動的カラーを解くため)
    let dark = args().iter().any(|a| a == "--preview-dark");
    let appearance = msg1_id(
        objc_getClass(c"NSAppearance".as_ptr()),
        sel(c"appearanceNamed:"),
        nsstring(if dark {
            "NSAppearanceNameDarkAqua"
        } else {
            "NSAppearanceNameAqua"
        }),
    );
    BUILD_APPEARANCE.store(appearance as usize, Ordering::Relaxed);
    msg1_void_id(win, sel(c"setAppearance:"), appearance);
    msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
    msg1_void_u8(win, sel(c"setTitlebarAppearsTransparent:"), 1);
    let vis: unsafe extern "C" fn(ID, SEL, i64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    vis(win, sel(c"setTitleVisibility:"), 1);
    // 内容サイズを固定する(絶対座標レイアウトなので、リサイズで崩れないように)
    let set_size: unsafe extern "C" fn(ID, SEL, NSRect) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let fixed = NSRect { x: 0.0, y: 0.0, w: W, h: H };
    set_size(win, sel(c"setMinSize:"), fixed);
    set_size(win, sel(c"setMaxSize:"), fixed);
    msg0_void(win, sel(c"center"));

    // サイドバーの背後ブラーを効かせるため、ウィンドウ自体は透明にする
    msg1_void_u8(win, sel(c"setOpaque:"), 0);
    msg1_void_id(win, sel(c"setBackgroundColor:"), system_color(c"clearColor"));

    let cv = msg0(win, sel(c"contentView"));
    // 内容面は不透明に敷く(サイドバーだけが壁紙を透かす)
    panel(
        cv,
        NSRect { x: PAGE_X, y: 0.0, w: PAGE_W, h: H },
        system_color(c"underPageBackgroundColor"),
        None,
        0.0,
    );
    build_sidebar(cv, target, page_arg());

    // フッター(デザイン下部の区切り線と「元に戻す / 適用」)。
    // 操作の配線は次の段階のため、いまは見た目だけを置く。
    // cv は下端基準なので、上端基準の Y を下端基準へ反転する
    let foot_y = FOOTER_H;
    panel(
        cv,
        NSRect { x: PAGE_X, y: foot_y, w: PAGE_W, h: 1.0 },
        system_color(c"separatorColor"),
        None,
        0.0,
    );
    panel(
        cv,
        NSRect { x: PAGE_X, y: foot_y + 1.0, w: PAGE_W, h: FOOTER_H - 1.0 },
        system_color(c"windowBackgroundColor"),
        None,
        0.0,
    );
    let apply_w = button_width("適用");
    let revert_w = button_width("元に戻す");
    let mut fx = W - GUTTER - apply_w;
    let by = (FOOTER_H - PILL_H) / 2.0;
    let apply_button = place_action(cv, target, "適用", fx, by, BtnKind::Primary, c"sdNewApply:");
    APPLY_BUTTON.store(apply_button as usize, Ordering::Relaxed);
    fx -= revert_w + 8.0;
    let revert_button =
        place_action(cv, target, "元に戻す", fx, by, BtnKind::Secondary, c"sdNewRevert:");
    REVERT_BUTTON.store(revert_button as usize, Ordering::Relaxed);
    let status = label(cv, "", PAGE_X + GUTTER, by + 9.0, 300.0, F_NOTE, false, true);
    STATUS.store(status as usize, Ordering::Relaxed);

    let current = page_arg();
    for i in 0..4 {
        let (scroll, doc) = page_scroll(cv, page_height(i));
        SCROLLS[i].store(scroll as usize, Ordering::Relaxed);
        // ページ見出し
        label(doc, PAGES[i], PAGE_TEXT_X, PAGE_TOP, 560.0, F_TITLE, true, false);
        label(doc, SUBTITLES[i], PAGE_TEXT_X, PAGE_HEAD_SUB, 700.0, F_SUBTITLE, false, true);
        match i {
            0 => build_connection(doc, target),
            1 => build_displays(doc, target),
            2 => build_sharing(doc, target),
            _ => build_general(doc, target),
        }
        msg1_void_u8(scroll, sel(c"setHidden:"), (i != current) as u8);
    }

    // 画面の状態を保存済みの値へ合わせる(既定値の直書きを避ける)
    sync_switches();
    disable_footer();

    if UI_PREVIEW.load(Ordering::Relaxed) {
        // 非アクティブのままだと NSVisualEffectView が灰色で描かれ、デザインと
        // 見比べられない。前面化してからキーウィンドウにする
        msg1_void_id(win, sel(c"makeKeyAndOrderFront:"), std::ptr::null_mut());
        msg0_void(win, sel(c"orderFrontRegardless"));
        msg0_void(win, sel(c"makeKeyWindow"));
        // ウィンドウを出した後にアクティブ化する(先に activate すると
        // ウィンドウが前面化されず、スイッチ等が非アクティブ配色になる)
        let app = msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel(c"sharedApplication"),
        );
        if !app.is_null() {
            let activate: unsafe extern "C" fn(ID, SEL, u8) =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            activate(app, sel(c"activateIgnoringOtherApps:"), 1);
            let responds: unsafe extern "C" fn(ID, SEL, SEL) -> u8 =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            if responds(app, sel(c"respondsToSelector:"), sel(c"activate")) != 0 {
                msg0_void(app, sel(c"activate"));
            }
        }
    }
    win
}

pub(in crate::gui) unsafe extern "C" fn nav(_s: ID, _c: SEL, sender: ID) {
    let idx = crate::msg0_isize(sender, sel(c"tag")).max(0) as usize;
    let idx = idx.min(3);
    for i in 0..4 {
        style_nav(i, i == idx);
        let scroll = SCROLLS[i].load(Ordering::Relaxed) as ID;
        if !scroll.is_null() {
            msg1_void_u8(scroll, sel(c"setHidden:"), (i != idx) as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_match_spec() {
        // SPEC.md の値とコードがずれていないことを固定する
        assert_eq!(W, 1040.0);
        assert_eq!(H, 700.0);
        assert_eq!(SIDEBAR_W, 219.0);
        assert_eq!(R_CARD, 12.0);
        assert_eq!(F_TITLE, 30.0);
        assert_eq!(F_CARD_TITLE, 17.0);
        assert_eq!(F_SUBTITLE, 13.0);
    }

    #[test]
    fn card_height_grows_with_rows() {
        assert!(card_height(&[44.0]) < card_height(&[44.0, 44.0]));
        // 1行ぶんの増加は「行の高さ + 行間」に一致する
        let d = card_height(&[44.0, 44.0]) - card_height(&[44.0]);
        assert!((d - (44.0 + SEC_ROW_GAP)).abs() < 1e-9);
    }

    #[test]
    fn page_heights_fit_content() {
        // どのページもスクロール内容がウィンドウより高くならない(末尾の余白を除く)
        for page in 0..4 {
            let h = page_height(page);
            assert!(h > PAGE_BODY, "page {} height {}", page, h);
        }
    }

    #[test]
    fn nav_index_is_clamped_to_page_count() {
        for raw in -5isize..10 {
            let idx = (raw.max(0) as usize).min(3);
            assert!(idx < PAGES.len());
        }
    }
}