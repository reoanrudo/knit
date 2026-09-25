// メニューバー常駐 GUI(NSStatusItem + NSMenu)。
// objc_msgSend 固定シグネチャ方式(実績パターン#1。依存追加ゼロ)。
// objc2-app-kit への移行はモジュール分割(Wave3 Step6)時に検討する。
// 呼び出し規約: このモジュールの全関数はメインスレッドから呼ぶこと
// (start() は main() の末尾、IMP は AppKit のイベント配信=メインRunLoop)。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{do_toggle, msg0, msg0_cstr, nsstring, objc_getClass};

type ID = *mut core::ffi::c_void;
type SEL = *mut core::ffi::c_void;
type CLS = *mut core::ffi::c_void;

/// セレクタ登録の薄いラッパ(extern fn はそのまま import できないため)
unsafe fn sel(name: &std::ffi::CStr) -> SEL {
    crate::sel_registerName(name.as_ptr())
}

#[link(name = "AppKit", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: *mut core::ffi::c_void;
    fn objc_allocateClassPair(
        superclass: CLS,
        name: *const core::ffi::c_char,
        extra_bytes: usize,
    ) -> CLS;
    fn class_addMethod(
        cls: CLS,
        name: SEL,
        imp: usize,
        types: *const core::ffi::c_char,
    ) -> i32;
    fn objc_registerClassPair(cls: CLS);
    // メニューバーアイコンを CoreGraphics で描くための最小セット
    fn CGColorSpaceCreateDeviceRGB() -> *mut core::ffi::c_void;
    fn CGBitmapContextCreate(
        data: *mut u8, width: usize, height: usize, bits_per_component: usize,
        bytes_per_row: usize, space: *mut core::ffi::c_void, bitmap_info: u32,
    ) -> *mut core::ffi::c_void;
    fn CGBitmapContextCreateImage(ctx: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn CGContextRelease(ctx: *mut core::ffi::c_void);
    fn CGImageRelease(img: *mut core::ffi::c_void);
    fn CGContextSetRGBFillColor(
        ctx: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64,
    );
    fn CGContextSetRGBStrokeColor(
        ctx: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64,
    );
    fn CGContextSetLineWidth(ctx: *mut core::ffi::c_void, w: f64);
    fn CGContextSetLineCap(ctx: *mut core::ffi::c_void, cap: u32);
    fn CGContextBeginPath(ctx: *mut core::ffi::c_void);
    fn CGContextMoveToPoint(ctx: *mut core::ffi::c_void, x: f64, y: f64);
    fn CGContextAddLineToPoint(ctx: *mut core::ffi::c_void, x: f64, y: f64);
    fn CGContextClosePath(ctx: *mut core::ffi::c_void);
    fn CGContextFillPath(ctx: *mut core::ffi::c_void);
    fn CGContextStrokePath(ctx: *mut core::ffi::c_void);
    fn CFRelease(cf: *mut core::ffi::c_void);
}

// ---------- 固定シグネチャ呼び出しヘルパ(この画面で必要なものだけ) ----------

unsafe fn msg0_void(target: ID, cmd: SEL) {
    let f: unsafe extern "C" fn(ID, SEL) = std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd)
}
unsafe fn msg1_void_id(target: ID, cmd: SEL, a: ID) {
    let f: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_sel(target: ID, cmd: SEL, a: SEL) {
    let f: unsafe extern "C" fn(ID, SEL, SEL) = std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_u8(target: ID, cmd: SEL, a: u8) {
    let f: unsafe extern "C" fn(ID, SEL, u8) = std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_i64(target: ID, cmd: SEL, a: i64) {
    let f: unsafe extern "C" fn(ID, SEL, i64) = std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, a)
}
unsafe fn msg2_void_id_id(target: ID, cmd: SEL, a: ID, b: ID) {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) =
        std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, a, b)
}
unsafe fn msg1_id_f64(target: ID, cmd: SEL, a: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64) -> ID =
        std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, a)
}
unsafe fn msg3_id(target: ID, cmd: SEL, a: ID, b: SEL, c: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, a, b, c)
}
unsafe fn msg5_timer(target: ID, cmd: SEL, t: f64, a: ID, b: SEL, c: ID, r: u8) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64, ID, SEL, ID, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as usize);
    f(target, cmd, t, a, b, c, r)
}

// ---------- GUI 部品への参照(AtomicUsize で生ポインタを保持) ----------

static GUI_TARGET: AtomicUsize = AtomicUsize::new(0);
static GUI_BUTTON: AtomicUsize = AtomicUsize::new(0);
static GUI_STATE_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_TOGGLE_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_MODE_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_TAPS_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_AUDIO_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_CMD_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_SCROLL_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_SPK_ITEM: AtomicUsize = AtomicUsize::new(0);

// ---------- 設定ウィンドウ(メニュー「設定…」で開く) ----------
static PREFS_WIN: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_MODE: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_TAPS: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_AUDIO: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_CMD: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SCROLL: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SPK: AtomicUsize = AtomicUsize::new(0);
static PREFS_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_GAIN_LABEL: AtomicUsize = AtomicUsize::new(0);
static PREFS_SIDE_POP: AtomicUsize = AtomicUsize::new(0);
static LAYOUT_WND: AtomicUsize = AtomicUsize::new(0);
static PREFS_DELAY_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_DELAY_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_DBL_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_DBL_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_MSCALE_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_MSCALE_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_EDGE_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_EDGE_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_CLIP: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SCOMPAT: AtomicUsize = AtomicUsize::new(0);
static GUI_SIDE_ITEM: AtomicUsize = AtomicUsize::new(0);
static PREFS_STATE: AtomicUsize = AtomicUsize::new(0);
/// 起動直後に設定ウィンドウを開く(--show-prefs。1 秒タイマーの初回で処理)
pub static SHOW_AT_START: AtomicBool = AtomicBool::new(false);

/// NSRect(f64 x4)。戻り値受け取りにのみ使う(渡しは rect_args を使う)
#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Apple arm64 の objc ABI では構造体引数は汎用レジスタへ分解して渡される
/// (HFA のまま v レジスタで渡すと未認識セレクタ扱いで NSException になる)。
/// そのため NSRect は f64::to_bits した u64 x4 として渡す
#[allow(clippy::too_many_arguments)]
unsafe fn rect_call(
    target: ID,
    cmd: SEL,
    r: NSRect,
    more: &[u64],
    tr8: u8,
) -> ID {
    unsafe {
        // (id, sel, x, y, w, h, ...more...) を最大 8 整数引数まで扱う
        let mut args = [
            r.x.to_bits(),
            r.y.to_bits(),
            r.w.to_bits(),
            r.h.to_bits(),
            0u64,
            0u64,
        ];
        for (i, v) in more.iter().take(2).enumerate() {
            args[4 + i] = *v;
        }
        let f: unsafe extern "C" fn(ID, SEL, u64, u64, u64, u64, u64, u64, u8) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        f(target, cmd, args[0], args[1], args[2], args[3], args[4], args[5], tr8)
    }
}

// ---------- メニュー項目のアクション(Objective-C クラスの IMP) ----------

/// スクロール速度スライダー(値=除数。小さいほど速い)。ドラッグ中も連続で飛ぶ
unsafe extern "C" fn imp_scroll_gain(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_scroll_div(v);
        let lbl = PREFS_GAIN_LABEL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            let speed = if v <= 40.0 { "速い" } else if v >= 140.0 { "遅い" } else { "標準" };
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("スクロール速度: {speed}({v:.0})")),
            );
        }
    }
}

unsafe extern "C" fn imp_toggle(_s: ID, _c: SEL, _n: ID) {
    do_toggle("menu");
    refresh_status();
}
unsafe extern "C" fn imp_switch_mode(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::HOTKEY_ONLY.load(Ordering::Relaxed);
    crate::HOTKEY_ONLY.store(next, Ordering::Relaxed);
    eprintln!("[mode] switch_mode -> {}", if next { "hotkey(ロック)" } else { "edge" });
    refresh_status();
}
unsafe extern "C" fn imp_edge_taps(_s: ID, _c: SEL, _n: ID) {
    let next = if crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2 { 1 } else { 2 };
    crate::EDGE_TAPS.store(next, Ordering::Relaxed);
    eprintln!("[mode] edge_taps -> {next}");
    refresh_status();
}
unsafe extern "C" fn imp_open_log(_s: ID, _c: SEL, _n: ID) {
    let _ = std::process::Command::new("open")
        .args(["-a", "Console", "/tmp/tsunagu-mac.log"])
        .spawn();
}
unsafe extern "C" fn imp_restart(_s: ID, _c: SEL, _n: ID) {
    match restart_script() {
        Some(script) => {
            eprintln!("[gui] restart-mac.sh を起動します");
            let _ = std::process::Command::new("bash")
                .arg(script)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        None => {
            eprintln!("[gui] 再起動スクリプトが見つかりません(.app 配布時は終了後に LaunchAgent が再起動します)");
            crate::notify("tsunagu", "再起動スクリプトが見つかりません");
        }
    }
}
unsafe extern "C" fn imp_audio_toggle(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::audio::MUTED.load(Ordering::Relaxed);
    crate::audio::MUTED.store(next, Ordering::Relaxed);
    eprintln!("[audio] mute -> {next}");
    refresh_status();
}
unsafe extern "C" fn imp_cmd_map(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CMD_ALT.load(Ordering::Relaxed);
    crate::CMD_ALT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] ⌘キー -> {}", if next { "Alt" } else { "Ctrl" });
    crate::send_msg(&crate::Msg::Cfg {
        cmd_alt: next,
        spk_mute: crate::SPK_MUTE.load(Ordering::Relaxed),
        side: crate::SIDE.load(Ordering::Relaxed),
    });
    refresh_status();
}
unsafe extern "C" fn imp_spk_mute(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SPK_MUTE.load(Ordering::Relaxed);
    crate::SPK_MUTE.store(next, Ordering::Relaxed);
    eprintln!("[cfg] 接続中スピーカーミュート -> {}", if next { "ON" } else { "OFF" });
    crate::send_msg(&crate::Msg::Cfg {
        cmd_alt: crate::CMD_ALT.load(Ordering::Relaxed),
        spk_mute: next,
        side: crate::SIDE.load(Ordering::Relaxed),
    });
    refresh_status();
}
/// 「Windows の位置」ポップアップ(0=右/1=左/2=上/3=下)。Deskflow links 相当
unsafe extern "C" fn imp_side(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> isize =
            std::mem::transmute(crate::objc_msgSend as usize);
        let idx = get(sender, sel(c"indexOfSelectedItem"));
        crate::set_side(idx.clamp(0, 3) as u8);
    }
}

/// switchDelay スライダ(0=無効。端に N ms 滞ってから切替)
unsafe extern "C" fn imp_switch_delay(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::SWITCH_DELAY_MS.store(v as u64, Ordering::Relaxed);
        let lbl = PREFS_DELAY_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            let t = if v < 1.0 { "無効(即時/ダブルタップ)" } else { &format!("{v:.0}ms 滞って切替") };
            msg1_void_id(lbl, sel(c"setStringValue:"), nsstring(t));
        }
    }
}

/// switchDoubleTap スライダ(ダブルタップ判定窓 ms)
unsafe extern "C" fn imp_dbl_tap(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::DOUBLE_TAP_MS.store(v.max(100.0) as u64, Ordering::Relaxed);
        let lbl = PREFS_DBL_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            msg1_void_id(lbl, sel(c"setStringValue:"), nsstring(&format!("{v:.0}ms 以内の2回")));
        }
    }
}

/// カーソル速度スライダ(0.2..3.0。倍率=Windows 上の移動量)
unsafe extern "C" fn imp_mouse_scale(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_mouse_scale(v);
        let lbl = PREFS_MSCALE_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            msg1_void_id(lbl, sel(c"setStringValue:"), nsstring(&format!("速度 x{v:.1}")));
        }
    }
}

/// 境界の敏感さスライダ(0..30px。大きいほど境界に届きやすい)
unsafe extern "C" fn imp_edge_px(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_edge_px(v);
        let lbl = PREFS_EDGE_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            msg1_void_id(lbl, sel(c"setStringValue:"), nsstring(&format!("敏感さ {v:.0}px")));
        }
    }
}

/// スクロール互換モードのトグル(120 未満を無視する古いアプリ向け)
unsafe extern "C" fn imp_scroll_compat(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_COMPAT.load(Ordering::Relaxed);
    crate::SCROLL_COMPAT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] スクロール互換モード -> {next}");
}

/// clipboardSharing トグル(Deskflow 標準オプション)
unsafe extern "C" fn imp_clip_share(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CLIP_SHARE.load(Ordering::Relaxed);
    crate::CLIP_SHARE.store(next, Ordering::Relaxed);
    eprintln!("[cfg] クリップボード共有 -> {next}");
}

/// メニュー「Windows の位置」: 右→左→上→下→右 のローテート
unsafe extern "C" fn imp_rotate_side(_s: ID, _c: SEL, _n: ID) {
    let next = (crate::SIDE.load(Ordering::Relaxed) + 1) % 4;
    crate::set_side(next);
    refresh_status();
}

/// 配置エディタ(独立ウィンドウ)を開く
/// 配置エディタ(独立ウィンドウ)を開く(メニュー IMP と起動直後の両方から呼ぶ)
pub fn show_layout() {
    unsafe {
        let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
        if !app.is_null() {
            msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
        }
        let existing = LAYOUT_WND.load(Ordering::Relaxed) as ID;
        if existing.is_null() {
            let w = make_layout_window();
            if w.is_null() {
                eprintln!("[gui] 配置ウィンドウの生成に失敗");
                return;
            }
            LAYOUT_WND.store(w as usize, Ordering::Relaxed);
        }
        msg1_void_id(
            LAYOUT_WND.load(Ordering::Relaxed) as ID,
            sel(c"makeKeyAndOrderFront:"),
            std::ptr::null_mut(),
        );
    }
}

unsafe extern "C" fn imp_show_layout(_s: ID, _c: SEL, _n: ID) {
    show_layout();
}

/// 配置エディタ専用の小ウィンドウ(設定窓に収まらないため分離)
unsafe fn make_layout_window() -> ID {
    unsafe {
        let init: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        let set_frame: unsafe extern "C" fn(ID, SEL, NSRect) =
            std::mem::transmute(crate::objc_msgSend as usize);
        let alloc = msg0(objc_getClass(c"NSWindow".as_ptr()), sel(c"alloc"));
        let win = init(
            alloc,
            sel(c"initWithContentRect:styleMask:backing:defer:"),
            NSRect { x: 0.0, y: 0.0, w: 604.0, h: 400.0 },
            1 | 2, // titled | closable
            2,
            0,
        );
        if win.is_null() {
            return std::ptr::null_mut();
        }
        msg1_void_id(win, sel(c"setTitle:"), nsstring("モニター配置"));
        msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
        msg0_void(win, sel(c"center"));
        let cv = msg0(win, sel(c"contentView"));
        if cv.is_null() {
            return std::ptr::null_mut();
        }
        // 説明行(上)
        let mklabel: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        let top = mklabel(
            objc_getClass(c"NSTextField".as_ptr()),
            sel(c"labelWithString:"),
            nsstring("Windows(青)をドラッグして実際の配置へ。離すと確定"),
        );
        if !top.is_null() {
            set_frame(top, sel(c"setFrame:"), NSRect { x: 20.0, y: 372.0, w: 560.0, h: 18.0 });
            msg1_void_id(cv, sel(c"addSubview:"), top);
        }
        // 配置ビュー(中央)
        let lay = make_layout_view(cv);
        if !lay.is_null() {
            set_frame(lay, sel(c"setFrame:"), NSRect { x: 22.0, y: 44.0, w: LAY_VW, h: LAY_VH });
            msg1_void_id(cv, sel(c"addSubview:"), lay);
        }
        // 凡例(下)
        let leg = mklabel(
            objc_getClass(c"NSTextField".as_ptr()),
            sel(c"labelWithString:"),
            nsstring("灰=Mac ・ 青=Windows(大きさは実際の比)"),
        );
        if !leg.is_null() {
            set_frame(leg, sel(c"setFrame:"), NSRect { x: 20.0, y: 14.0, w: 560.0, h: 16.0 });
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            if !color_cls.is_null() {
                let get_color: unsafe extern "C" fn(ID, SEL) -> ID =
                    std::mem::transmute(crate::objc_msgSend as usize);
                let color = get_color(color_cls, sel(c"secondaryLabelColor"));
                if !color.is_null() {
                    msg1_void_id(leg, sel(c"setTextColor:"), color);
                }
            }
            msg1_void_id(cv, sel(c"addSubview:"), leg);
        }
        win
    }
}

unsafe extern "C" fn imp_scroll_flip(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_FLIP.load(Ordering::Relaxed);
    crate::SCROLL_FLIP.store(next, Ordering::Relaxed);
    eprintln!("[cfg] スクロール方向 -> {}", if next { "反転(Mac準拠)" } else { "標準(Windows準拠)" });
    refresh_status();
}
unsafe extern "C" fn imp_vol(_s: ID, _c: SEL, sender: ID) {
    // 3 つのメニュー項目(▲/▼/ミュート)から送信元 tag で判別する
    let tag: isize = {
        let f: unsafe extern "C" fn(ID, SEL) -> isize =
            std::mem::transmute(crate::objc_msgSend as usize);
        f(sender, sel(c"tag"))
    };
    let op = match tag {
        1 => 0u8, // up
        2 => 1,   // down
        _ => 2,   // mute
    };
    crate::send_msg(&crate::Msg::Vol { op });
}
unsafe extern "C" fn imp_send_file(_s: ID, _c: SEL, _n: ID) {
    // accessory アプリでも明示アクティベートすればモーダルパネルは出せる
    let app = msg0(crate::objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let panel = msg0(crate::objc_getClass(c"NSOpenPanel".as_ptr()), sel(c"openPanel"));
    if panel.is_null() {
        eprintln!("[gui] NSOpenPanel を生成できません");
        return;
    }
    msg1_void_u8(panel, sel(c"setCanChooseFiles:"), 1);
    msg1_void_u8(panel, sel(c"setCanChooseDirectories:"), 0);
    msg1_void_u8(panel, sel(c"setAllowsMultipleSelection:"), 1);
    msg1_void_id(panel, sel(c"setMessage:"), crate::nsstring("Windows へ送信します(合計 200MB まで)"));
    // runModal は選択が確定するまで戻らない(メイン RunLoop を内回りする)
    let resp = crate::msg0_isize(panel, sel(c"runModal"));
    if resp != 1 {
        return; // NSModalResponseOK 以外 = キャンセル
    }
    let urls = msg0(panel, sel(c"URLs"));
    if urls.is_null() {
        return;
    }
    let n = crate::msg0_isize(urls, sel(c"count")).max(0);
    let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
        std::mem::transmute(crate::objc_msgSend as usize);
    let mut paths = Vec::new();
    for i in 0..n.min(64) {
        let url = at(urls, sel(c"objectAtIndex:"), i as usize);
        if url.is_null() {
            continue;
        }
        let path = msg0(url, sel(c"path")); // NSURL.path -> NSString
        if path.is_null() {
            continue;
        }
        let utf8 = crate::msg0_cstr(path, sel(c"UTF8String"));
        if utf8.is_null() {
            continue;
        }
        let s = std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned();
        if !s.is_empty() {
            paths.push(s);
        }
    }
    if !paths.is_empty() {
        let pb: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
        eprintln!("[gui] ファイル送信: {} 件", pb.len());
        crate::send_files_to_win(pb);
    }
}
/// 設定ウィンドウを開く(初回のみ生成。以降は同一ウィンドウを前面化)。
/// メニューの IMP と起動直後(--show-prefs)の両方から呼ぶ
/// NSException の内容を abort 前にログへ(objc が unwind 前に呼ぶ)
unsafe extern "C" fn uncaught_exc_handler(exc: ID) {
    unsafe {
        let name = msg0_cstr(msg0(exc, sel(c"name")), sel(c"UTF8String"));
        let reason = msg0_cstr(msg0(exc, sel(c"reason")), sel(c"UTF8String"));
        eprintln!(
            "[prefs] NSException name={:?} reason={:?}",
            if name.is_null() { None } else { std::ffi::CStr::from_ptr(name).to_str().ok() },
            if reason.is_null() { None } else { std::ffi::CStr::from_ptr(reason).to_str().ok() },
        );
    }
}

pub fn show_prefs() {
    eprintln!("[prefs] enter");
    unsafe {
        let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
        if !app.is_null() {
            // メニューバー常駐型は非アクティブなので明示的に前面化する
            msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
        }
        extern "C" {
            fn NSSetUncaughtExceptionHandler(h: Option<unsafe extern "C" fn(ID)>);
        }
        unsafe { NSSetUncaughtExceptionHandler(Some(uncaught_exc_handler)) };
        eprintln!("[prefs] activated");
        let existing = PREFS_WIN.load(Ordering::Relaxed) as ID;
        if existing.is_null() {
            let target = GUI_TARGET.load(Ordering::Relaxed) as ID;
            let win = make_prefs_window(target);
            if win.is_null() {
                eprintln!("[gui] 設定ウィンドウの生成に失敗");
                return;
            }
            PREFS_WIN.store(win as usize, Ordering::Relaxed);
        }
        let win = PREFS_WIN.load(Ordering::Relaxed) as ID;
        msg1_void_id(win, sel(c"makeKeyAndOrderFront:"), std::ptr::null_mut());
        sync_prefs_state();
    }
}

unsafe extern "C" fn imp_show_prefs(_s: ID, _c: SEL, _n: ID) {
    show_prefs();
}

/// チェックボックスの見た目を本体の状態(static)へ同期する(1秒タイマーから)
fn sync_prefs_state() {
    unsafe {
        // Windows の位置ポップアップ(メニューのローテート反映。閉じた状態への
        // selectItemAtIndex はユーザー操作と競合しない)
        let pop = PREFS_SIDE_POP.load(Ordering::Relaxed) as ID;
        if !pop.is_null() {
            let select: unsafe extern "C" fn(ID, SEL, isize) =
                std::mem::transmute(crate::objc_msgSend as usize);
            select(pop, sel(c"selectItemAtIndex:"), crate::SIDE.load(Ordering::Relaxed) as isize);
        }
        // 状態行(接続・操作中・遅延)
        let st = PREFS_STATE.load(Ordering::Relaxed) as ID;
        if !st.is_null() {
            let connected = crate::CONNECTED.load(Ordering::Relaxed);
            let win = crate::WIN_MODE.load(Ordering::Relaxed);
            let rtt = crate::RTT_MS.load(Ordering::Relaxed);
            let conn = if connected {
                if rtt > 0 { format!("接続済(遅延 {rtt}ms)") } else { "接続済".into() }
            } else {
                "切断(再接続待機中)".to_string()
            };
            let mode = if win { "Windows 操作中" } else { "Mac 操作中" };
            msg1_void_id(st, sel(c"setStringValue:"), nsstring(&format!("状態: {conn} ・ {mode}")));
            // 切断時は赤で強調(接続時は標準ラベル色)
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            if !color_cls.is_null() {
                let get_color: unsafe extern "C" fn(ID, SEL) -> ID =
                    std::mem::transmute(crate::objc_msgSend as usize);
                let color = get_color(
                    color_cls,
                    sel(if connected { c"labelColor" } else { c"systemRedColor" }),
                );
                if !color.is_null() {
                    msg1_void_id(st, sel(c"setTextColor:"), color);
                }
            }
        }
        let set = |slot: &AtomicUsize, on: bool| {
            let b = slot.load(Ordering::Relaxed) as ID;
            if !b.is_null() {
                msg1_void_u8(b, sel(c"setState:"), on as u8);
            }
        };
        set(&PREFS_CHK_MODE, !crate::HOTKEY_ONLY.load(Ordering::Relaxed));
        set(&PREFS_CHK_TAPS, crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2);
        set(&PREFS_CHK_AUDIO, !crate::audio::MUTED.load(Ordering::Relaxed));
        set(&PREFS_CHK_CMD, crate::CMD_ALT.load(Ordering::Relaxed));
        set(&PREFS_CHK_SCROLL, !crate::SCROLL_FLIP.load(Ordering::Relaxed));
        set(&PREFS_CHK_SPK, crate::SPK_MUTE.load(Ordering::Relaxed));
        set(&PREFS_CHK_CLIP, crate::CLIP_SHARE.load(Ordering::Relaxed));
        set(&PREFS_CHK_SCOMPAT, crate::SCROLL_COMPAT.load(Ordering::Relaxed));
    }
}

/// 近未来アクセント色(シアン)。見出し・区切り・状態行に使う
unsafe fn accent_color() -> ID {
    unsafe {
        let color_cls = objc_getClass(c"NSColor".as_ptr());
        let f: unsafe extern "C" fn(ID, SEL, f64, f64, f64, f64) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        f(
            color_cls,
            sel(c"colorWithCalibratedRed:green:blue:alpha:"),
            0.15, 0.85, 1.0, 1.0, // シアン
        )
    }
}

/// 区切り線(アクセント色の細いライン=ホログラム風)
unsafe fn neon_rule(cv: ID, frame: NSRect) {
    unsafe {
        let cls = objc_getClass(c"NSView".as_ptr());
        if cls.is_null() {
            return;
        }
        let init_frame: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        let v = init_frame(msg0(cls, sel(c"alloc")), sel(c"initWithFrame:"), frame);
        if v.is_null() {
            return;
        }
        msg1_void_i64(v, sel(c"setWantsLayer:"), 1);
        let setbg: unsafe extern "C" fn(ID, SEL, ID) =
            std::mem::transmute(crate::objc_msgSend as usize);
        setbg(v, sel(c"setBackgroundColor:"), accent_color());
        let add: unsafe extern "C" fn(ID, SEL, ID) =
            std::mem::transmute(crate::objc_msgSend as usize);
        add(cv, sel(c"addSubview:"), v);
    }
}


// ---------- モニター配置エディタ(Mac の「ディスプレイ配置」相当) ----------
// 灰色=Mac、青=Windows の矩形を描き、Windows 側をドラッグして物理配置を再現する。
// ドロップ時に「接する辺+辺に沿った接続範囲」を算出して SIDE/LAY_RANGE へ反映
const LAY_VW: f64 = 560.0;
const LAY_VH: f64 = 320.0;

/// Mac/Win 両画面の実ピクセルサイズ(hello 受信値。未接続時は一般値)
fn lay_px() -> ((f64, f64), (f64, f64)) {
    let mac = (
        crate::SCREEN_W.get().copied().unwrap_or(2056.0),
        crate::SCREEN_H.get().copied().unwrap_or(1329.0),
    );
    let win = *crate::WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    (mac, win)
}

/// 共通縮尺: 両モニターを横並び + 縦に収める(重ならず両方必ず見える)
fn lay_scale() -> f64 {
    let ((mw, mh), (ww, wh)) = lay_px();
    let by_w = (LAY_VW - 80.0) / (mw + ww).max(1.0);
    let by_h = (LAY_VH - 40.0) / mh.max(wh).max(1.0);
    by_w.min(by_h)
}

/// Windows 矩形のサイズ(実際の大きさ比・共通縮尺)
fn lay_win_size() -> (f64, f64) {
    let (_, (ww, wh)) = lay_px();
    let sc = lay_scale();
    (ww * sc, wh * sc)
}
static LAY_WIN: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0)); // Win 矩形中心(0=既定=Macの右隣)

/// Win 矩形の中心(未設定なら Mac 右隣の既定位置)
fn lay_win_center() -> (f64, f64) {
    let c = *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner());
    if c.0 <= 0.0 {
        let m = lay_mac_rect();
        let (ww, wh) = lay_win_size();
        return (m.x + m.w + 16.0 + ww / 2.0, m.y + m.h / 2.0);
    }
    c
}
static LAY_GRAB: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));
static LAY_DRAG: AtomicBool = AtomicBool::new(false);

fn lay_mac_rect() -> NSRect {
    let ((mw, mh), _) = lay_px();
    let sc = lay_scale();
    let h = mh * sc;
    NSRect {
        x: 30.0,
        y: (LAY_VH - h) / 2.0,
        w: mw * sc,
        h,
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint2 {
    x: f64,
    y: f64,
}

unsafe fn lay_point_in_view(_self: ID, ev: ID) -> CGPoint2 {
    unsafe {
        let loc: unsafe extern "C" fn(ID, SEL) -> CGPoint2 =
            std::mem::transmute(crate::objc_msgSend as usize);
        let conv: unsafe extern "C" fn(ID, SEL, CGPoint2, ID) -> CGPoint2 =
            std::mem::transmute(crate::objc_msgSend as usize);
        let p = loc(ev, sel(c"locationInWindow"));
        conv(_self, sel(c"convertPoint:fromView:"), p, std::ptr::null_mut())
    }
}

/// 描画: CoreGraphics で直接塗る(graphicsPort 経由)
unsafe extern "C" fn lay_draw(_self: ID, _cmd: SEL, _r: NSRect) {
    unsafe {
        let ctx_cls = objc_getClass(c"NSGraphicsContext".as_ptr());
        let cur = msg0(ctx_cls, sel(c"currentContext"));
        if cur.is_null() {
            return;
        }
        let port = msg0(cur, sel(c"graphicsPort"));
        if port.is_null() {
            return;
        }
        extern "C" {
            fn CGContextSetRGBFillColor(c: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
            fn CGContextFillRect(c: *mut core::ffi::c_void, r: NSRect);
            fn CGContextSetRGBStrokeColor(c: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
            fn CGContextSetLineWidth(c: *mut core::ffi::c_void, w: f64);
            fn CGContextStrokeRect(c: *mut core::ffi::c_void, r: NSRect);
        }
        let ctx = port as *mut core::ffi::c_void;
        // 背景
        CGContextSetRGBFillColor(ctx, 0.13, 0.14, 0.16, 1.0);
        CGContextFillRect(ctx, NSRect { x: 0.0, y: 0.0, w: LAY_VW, h: LAY_VH });
        // Mac(灰+白枠: 青が重なっても輪郭が見える)
        let mr = lay_mac_rect();
        CGContextSetRGBFillColor(ctx, 0.42, 0.45, 0.50, 1.0);
        CGContextFillRect(ctx, mr);
        CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.9);
        CGContextSetLineWidth(ctx, 1.5);
        CGContextStrokeRect(ctx, mr);
        // Windows(青=標準アクセント)
        let wc = lay_win_center();
        let (ww, wh) = lay_win_size();
        CGContextSetRGBFillColor(ctx, 0.16, 0.50, 0.98, 1.0);
        CGContextFillRect(
            ctx,
            NSRect { x: wc.0 - ww / 2.0, y: wc.1 - wh / 2.0, w: ww, h: wh },
        );
    }
}

unsafe extern "C" fn lay_down(_self: ID, _cmd: SEL, ev: ID) {
    unsafe {
        let p = lay_point_in_view(_self, ev);
        let wc = lay_win_center();
        let (ww, wh) = lay_win_size();
        let inside = p.x >= wc.0 - ww / 2.0 - 4.0
            && p.x <= wc.0 + ww / 2.0 + 4.0
            && p.y >= wc.1 - wh / 2.0 - 4.0
            && p.y <= wc.1 + wh / 2.0 + 4.0;
        if inside {
            LAY_DRAG.store(true, Ordering::Relaxed);
            *LAY_GRAB.lock().unwrap_or_else(|e| e.into_inner()) = (p.x - wc.0, p.y - wc.1);
        }
    }
}

unsafe extern "C" fn lay_dragged(_self: ID, _cmd: SEL, ev: ID) {
    unsafe {
        if !LAY_DRAG.load(Ordering::Relaxed) {
            return;
        }
        let p = lay_point_in_view(_self, ev);
        let g = *LAY_GRAB.lock().unwrap_or_else(|e| e.into_inner());
        let (ww, wh) = lay_win_size();
        let nx = (p.x - g.0).clamp(ww / 2.0 + 2.0, LAY_VW - ww / 2.0 - 2.0);
        let ny = (p.y - g.1).clamp(wh / 2.0 + 2.0, LAY_VH - wh / 2.0 - 2.0);
        *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (nx, ny);
        let snd: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(crate::objc_msgSend as usize);
        snd(_self, sel(c"setNeedsDisplay:"), 1);
    }
}

/// ドロップ: 接する辺と接続範囲(LAY_RANGE)を算出して SIDE へ反映し、
/// 矩形をきれいな位置(辺にスナップ)へ揃える
unsafe extern "C" fn lay_up(_self: ID, _cmd: SEL, _ev: ID) {
    unsafe {
        if !LAY_DRAG.swap(false, Ordering::Relaxed) {
            return;
        }
        let m = lay_mac_rect();
        let mc = (m.x + m.w / 2.0, m.y + m.h / 2.0);
        let wc0 = lay_win_center();
        let (ww, wh) = lay_win_size();
        let (dx, dy) = (wc0.0 - mc.0, wc0.1 - mc.1);
        let (edge, f0, f1) = if dx.abs() >= dy.abs() {
            // 左右いずれかの辺に接続。Win の縦範囲が Mac の縦範囲のどこに来るか
            let f0 = ((wc0.1 - wh / 2.0) - m.y) / m.h;
            let f1 = ((wc0.1 + wh / 2.0) - m.y) / m.h;
            (if dx >= 0.0 { 0u8 } else { 1u8 }, f0.clamp(0.0, 1.0), f1.clamp(0.0, 1.0))
        } else {
            let f0 = ((wc0.0 - ww / 2.0) - m.x) / m.w;
            let f1 = ((wc0.0 + ww / 2.0) - m.x) / m.w;
            (if dy >= 0.0 { 2u8 } else { 3u8 }, f0.clamp(0.0, 1.0), f1.clamp(0.0, 1.0))
        };
        // 斜め(4-7)表現: 水平辺で接続範囲が半分未満なら上下の半分側へ
        let mut side = edge;
        if edge <= 1 && (f1 - f0) < 0.6 {
            side = if (f0 + f1) / 2.0 < 0.5 { edge + 4 } else { edge + 5 };
        }
        crate::set_side(side);
        // 細かい範囲で上書き(set_side は半分単位で設定するため)
        *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = (f0, f1.max(f0 + 0.05));
        let snd: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(crate::objc_msgSend as usize);
        snd(_self, sel(c"setNeedsDisplay:"), 1);
        eprintln!("[lay] 配置を更新: {}(範囲 {:.2}〜{:.2})", crate::side_name(), f0, f1);
    }
}

/// 配置エディタの NSView サブクラスを登録して生成(初回のみ)
unsafe fn make_layout_view(target_frame_host: ID) -> ID {
    unsafe {
        let _ = target_frame_host;
        static CLS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let cls = *CLS.get_or_init(|| {
            let super_cls = objc_getClass(c"NSView".as_ptr());
            if super_cls.is_null() {
                return 0usize;
            }
            let cls = objc_allocateClassPair(super_cls, c"TSLayView".as_ptr(), 0);
            if cls.is_null() {
                return 0usize;
            }
            let ok1 = class_addMethod(
                cls,
                sel(c"drawRect:"),
                lay_draw as *const () as usize,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
            );
            let ok2 = class_addMethod(cls, sel(c"mouseDown:"), lay_down as *const () as usize, c"v@:@".as_ptr());
            let ok3 = class_addMethod(
                cls,
                sel(c"mouseDragged:"),
                lay_dragged as *const () as usize,
                c"v@:@".as_ptr(),
            );
            let ok4 = class_addMethod(cls, sel(c"mouseUp:"), lay_up as *const () as usize, c"v@:@".as_ptr());
            if ok1 == 0 || ok2 == 0 || ok3 == 0 || ok4 == 0 {
                return 0usize;
            }
            objc_registerClassPair(cls);
            cls as usize
        });
        if cls == 0 {
            return std::ptr::null_mut();
        }
        let init: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        init(
            msg0(cls as ID, sel(c"alloc")),
            sel(c"initWithFrame:"),
            NSRect { x: 0.0, y: 0.0, w: LAY_VW, h: LAY_VH },
        )
    }
}

/// 見出しラベル(小さめグレーのキャプション=モダンな設定画面のセクション題)
unsafe fn section_heading(cv: ID, text: &str, frame: NSRect) {
    unsafe {
        let mk: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        let lbl = mk(
            objc_getClass(c"NSTextField".as_ptr()),
            sel(c"labelWithString:"),
            crate::nsstring(text),
        );
        if lbl.is_null() {
            return;
        }
        let set_frame: unsafe extern "C" fn(ID, SEL, NSRect) =
            std::mem::transmute(crate::objc_msgSend as usize);
        set_frame(lbl, sel(c"setFrame:"), frame);
        // 見出しは標準の補足色(Apple 純正設定画面と同じ扱い)
        let color_cls = objc_getClass(c"NSColor".as_ptr());
        if !color_cls.is_null() {
            let get_color: unsafe extern "C" fn(ID, SEL) -> ID =
                std::mem::transmute(crate::objc_msgSend as usize);
            let color = get_color(color_cls, sel(c"secondaryLabelColor"));
            if !color.is_null() {
                let setc: unsafe extern "C" fn(ID, SEL, ID) =
                    std::mem::transmute(crate::objc_msgSend as usize);
                setc(lbl, sel(c"setTextColor:"), color);
            }
        }
        // boldSystemFontOfSize: は NSFont のクラスメソッド(インスタンスへは送れない)
        let font_cls = objc_getClass(c"NSFont".as_ptr());
        if !font_cls.is_null() {
            let bold: unsafe extern "C" fn(ID, SEL, f64) -> ID =
                std::mem::transmute(crate::objc_msgSend as usize);
            let bf = bold(font_cls, sel(c"boldSystemFontOfSize:"), 11.0);
            if !bf.is_null() {
                let setf: unsafe extern "C" fn(ID, SEL, ID) =
                    std::mem::transmute(crate::objc_msgSend as usize);
                setf(lbl, sel(c"setFont:"), bf);
            }
        }
        let add: unsafe extern "C" fn(ID, SEL, ID) =
            std::mem::transmute(crate::objc_msgSend as usize);
        add(cv, sel(c"addSubview:"), lbl);
    }
}

/// 設定ウィンドウを組み立てる。トグルの action はメニュー項目と同じ IMP を
/// 共用し(sender を見ないトグル)、チェック状態は毎秒の同期で保ち直す
unsafe fn make_prefs_window(target: ID) -> ID {
    unsafe {
        let init: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        let set_frame: unsafe extern "C" fn(ID, SEL, NSRect) =
            std::mem::transmute(crate::objc_msgSend as usize);
        let label: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        let check_btn: unsafe extern "C" fn(ID, SEL, ID, ID, SEL) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        let push_btn: unsafe extern "C" fn(ID, SEL, ID, ID, SEL) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);

        // titled(1) | closable(2) | resizable(8)、backing=Buffered(2)、defer=NO
        eprintln!("[prefs] building");
        let alloc = msg0(objc_getClass(c"NSWindow".as_ptr()), sel(c"alloc"));
        eprintln!("[prefs] alloc ok");
        let win = init(
            alloc,
            sel(c"initWithContentRect:styleMask:backing:defer:"),
            NSRect { x: 0.0, y: 0.0, w: 400.0, h: 710.0 },
            1 | 2 | 8 | 0x8000, // +FullSizeContentView
            2,
            0,
        );
        if win.is_null() {
            return std::ptr::null_mut();
        }
        msg1_void_id(win, sel(c"setTitle:"), nsstring("Tsunagu 設定"));
        // タイトルバーを透過し、すりガラスを窓全面に(モダンな設定画面の見た目)
        msg1_void_u8(win, sel(c"setTitlebarAppearsTransparent:"), 1);
        // 新規 NSWindow の既定位置が画面外になることがあるため中央へ
        msg0_void(win, sel(c"center"));
        // FullSizeContentView により中央タイトル/traffic lights が内容と重なるため
        // タイトル文字は非表示にする(左上ボタンのみ残す)
        msg1_void_i64(win, sel(c"setTitleVisibility:"), 1);
        // リサイズしても崩れないよう最小サイズを固定。setContentMinSize: の引数は
        // NSSize(f64×2)のため f64 2 引数の transmute で渡す(NSRect 32byte と混同注意)
        let set_min: unsafe extern "C" fn(ID, SEL, f64, f64) =
            std::mem::transmute(crate::objc_msgSend as usize);
        set_min(win, sel(c"setContentMinSize:"), 400.0, 710.0);
        // 閉じてもオブジェクトを保持し、次回は同一ウィンドウを再表示する
        msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
        let cv = msg0(win, sel(c"contentView"));
        if cv.is_null() {
            return std::ptr::null_mut();
        }
        // ---- すりガラス背景(NSVisualEffectView)。最初に追加=最背面 ----
        let ve_cls = objc_getClass(c"NSVisualEffectView".as_ptr());
        if !ve_cls.is_null() {
            let bounds: unsafe extern "C" fn(ID, SEL) -> NSRect =
                std::mem::transmute(crate::objc_msgSend as usize);
            let b = bounds(cv, sel(c"bounds"));
            let alloc_v = msg0(ve_cls, sel(c"alloc"));
            let init_frame: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
                std::mem::transmute(crate::objc_msgSend as usize);
            let ve = init_frame(alloc_v, sel(c"initWithFrame:"), b);
            if !ve.is_null() {
                // 近未来パネル: HUDWindow(13)=黒系の濃いすりガラス+背後ブレンド
                msg1_void_i64(ve, sel(c"setMaterial:"), 2); // Sidebar(標準)
                msg1_void_i64(ve, sel(c"setBlendingMode:"), 0); // behind window
                msg1_void_i64(ve, sel(c"setState:"), 1); // active
                // 窓リサイズに追従(width|height sizable)
                msg1_void_i64(ve, sel(c"setAutoresizingMask:"), 2 | 16);
                msg1_void_id(cv, sel(c"addSubview:"), ve);
            }
        }
        let btn_cls = objc_getClass(c"NSButton".as_ptr());

        // ---- 状態行(最上部・太字。1 秒タイマーで更新・接続状態で色が変わる) ----
        let state_lbl = label(
            objc_getClass(c"NSTextField".as_ptr()),
            sel(c"labelWithString:"),
            nsstring("状態: …"),
        );
        if !state_lbl.is_null() {
            set_frame(state_lbl, sel(c"setFrame:"), NSRect { x: 20.0, y: 710.0 - 56.0, w: 360.0, h: 24.0 });
            let font_cls = objc_getClass(c"NSFont".as_ptr());
            if !font_cls.is_null() {
                let bold: unsafe extern "C" fn(ID, SEL, f64) -> ID =
                    std::mem::transmute(crate::objc_msgSend as usize);
                let bf = bold(font_cls, sel(c"boldSystemFontOfSize:"), 13.0);
                if !bf.is_null() {
                    msg1_void_id(state_lbl, sel(c"setFont:"), bf);
                }
            }
            msg1_void_id(cv, sel(c"addSubview:"), state_lbl);
            PREFS_STATE.store(state_lbl as usize, Ordering::Relaxed);
        }

        // ---- チェック項目(y は直接減らす=クロージャ借用だと見出し配置と衝突) ----
        let mut y = 710.0 - 84.0;
        let place_check = |title: &str, action: &std::ffi::CStr, slot: &AtomicUsize, yy: f64| {
            let b = check_btn(
                btn_cls,
                sel(c"checkboxWithTitle:target:action:"),
                nsstring(title),
                target,
                sel(action),
            );
            if !b.is_null() {
                set_frame(b, sel(c"setFrame:"), NSRect { x: 20.0, y: yy, w: 360.0, h: 28.0 });
                msg1_void_id(cv, sel(c"addSubview:"), b);
                slot.store(b as usize, Ordering::Relaxed);
            }
        };
        section_heading(cv, "切替", NSRect { x: 20.0, y: y + 8.0, w: 360.0, h: 18.0 });
        y -= 26.0;

        section_heading(cv, "切替", NSRect { x: 20.0, y: y + 8.0, w: 360.0, h: 18.0 });
        y -= 26.0;
        place_check("境界での切替を有効化(オフ: ホットキーロック)", c"sdSwitchMode:", &PREFS_CHK_MODE, y);
        y -= 38.0;
        place_check("境界到達はダブルタップ(オフ: 1回で切替)", c"sdEdgeTaps:", &PREFS_CHK_TAPS, y);
        y -= 38.0;
        // Windows の位置(Deskflow links 相当)。NSPopUpButton で4択
        let pop_cls = objc_getClass(c"NSPopUpButton".as_ptr());
        if !pop_cls.is_null() {
            let alloc_p = msg0(pop_cls, sel(c"alloc"));
            let init_frame: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
                std::mem::transmute(crate::objc_msgSend as usize);
            let pop = init_frame(alloc_p, sel(c"initWithFrame:"), NSRect { x: 20.0, y: y - 30.0, w: 180.0, h: 26.0 });
            if !pop.is_null() {
                for t in [
                    "Windows は右", "Windows は左", "Windows は上", "Windows は下",
                    "Windows は右上", "Windows は右下", "Windows は左上", "Windows は左下",
                ] {
                    msg1_void_id(pop, sel(c"addItemWithTitle:"), nsstring(t));
                }
                msg1_void_id(pop, sel(c"setTarget:"), target);
                msg1_void_sel(pop, sel(c"setAction:"), sel(c"sdSide:"));
                let select: unsafe extern "C" fn(ID, SEL, isize) =
                    std::mem::transmute(crate::objc_msgSend as usize);
                select(pop, sel(c"selectItemAtIndex:"), crate::SIDE.load(Ordering::Relaxed) as isize);
                msg1_void_id(cv, sel(c"addSubview:"), pop);
                PREFS_SIDE_POP.store(pop as usize, Ordering::Relaxed);
            }
        }
        // 配置エディタは独立ウィンドウ(設定窓に収まらないため別窓化)
        let lay_btn = push_btn(
            btn_cls,
            sel(c"buttonWithTitle:target:action:"),
            nsstring("配置エディタ…"),
            target,
            sel(c"sdLayout:"),
        );
        if !lay_btn.is_null() {
            set_frame(lay_btn, sel(c"setFrame:"), NSRect { x: 212.0, y: y - 30.0, w: 168.0, h: 26.0 });
            msg1_void_id(cv, sel(c"addSubview:"), lay_btn);
        }
        y -= 40.0;
        // switchDelay スライダ(0..1000ms)
        let slider_cls2 = objc_getClass(c"NSSlider".as_ptr());
        let mk_slider: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        if !slider_cls2.is_null() {
            let sl = mk_slider(
                slider_cls2, sel(c"sliderWithValue:minValue:maxValue:target:action:"),
                crate::SWITCH_DELAY_MS.load(Ordering::Relaxed) as f64, 0.0, 1000.0,
                target, sel(c"sdDelay:"),
            );
            if !sl.is_null() {
                set_frame(sl, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 28.0, w: 180.0, h: 22.0 });
                msg1_void_id(cv, sel(c"addSubview:"), sl);
                PREFS_DELAY_SLIDER.store(sl as usize, Ordering::Relaxed);
            }
        }
        let dl = label(
            objc_getClass(c"NSTextField".as_ptr()), sel(c"labelWithString:"),
            nsstring("切替までの滞在(0=無効)"),
        );
        if !dl.is_null() {
            set_frame(dl, sel(c"setFrame:"), NSRect { x: 212.0, y: y - 26.0, w: 170.0, h: 18.0 });
            msg1_void_id(cv, sel(c"addSubview:"), dl);
            PREFS_DELAY_LBL.store(dl as usize, Ordering::Relaxed);
        }
        y -= 40.0;
        // switchDoubleTap スライダ(200..1200ms)
        if !slider_cls2.is_null() {
            let sl = mk_slider(
                slider_cls2, sel(c"sliderWithValue:minValue:maxValue:target:action:"),
                crate::DOUBLE_TAP_MS.load(Ordering::Relaxed) as f64, 200.0, 1200.0,
                target, sel(c"sdDblTap:"),
            );
            if !sl.is_null() {
                set_frame(sl, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 28.0, w: 180.0, h: 22.0 });
                msg1_void_id(cv, sel(c"addSubview:"), sl);
                PREFS_DBL_SLIDER.store(sl as usize, Ordering::Relaxed);
            }
        }
        let dbl = label(
            objc_getClass(c"NSTextField".as_ptr()), sel(c"labelWithString:"),
            nsstring("ダブルタップ判定"),
        );
        if !dbl.is_null() {
            set_frame(dbl, sel(c"setFrame:"), NSRect { x: 212.0, y: y - 26.0, w: 170.0, h: 18.0 });
            msg1_void_id(cv, sel(c"addSubview:"), dbl);
            PREFS_DBL_LBL.store(dbl as usize, Ordering::Relaxed);
        }
        y -= 40.0;

        // ---- スクロール ----
        section_heading(cv, "スクロール", NSRect { x: 20.0, y: y + 8.0, w: 360.0, h: 18.0 });
        y -= 26.0;
        place_check("方向を Mac に合わせる(オフ: Windows 標準)", c"sdScroll:", &PREFS_CHK_SCROLL, y);
        y -= 38.0;
        place_check(
            "互換モード(一部のアプリでスクロールが効かない時)",
            c"sdScrollCompat:",
            &PREFS_CHK_SCOMPAT,
            y,
        );
        y -= 38.0;

        // ---- スクロール速度スライダー(右ほど遅い=除数 20..240) ----
        let slider_cls = objc_getClass(c"NSSlider".as_ptr());
        let mk_slider: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        if !slider_cls.is_null() {
            let slider = mk_slider(
                slider_cls,
                sel(c"sliderWithValue:minValue:maxValue:target:action:"),
                crate::scroll_div(),
                20.0,
                240.0,
                target,
                sel(c"sdScrollGain:"),
            );
            if !slider.is_null() {
                set_frame(slider, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 4.0, w: 200.0, h: 24.0 });
                msg1_void_id(cv, sel(c"addSubview:"), slider);
                PREFS_SLIDER.store(slider as usize, Ordering::Relaxed);
            }
        }
        let gain_lbl = label(
            objc_getClass(c"NSTextField".as_ptr()),
            sel(c"labelWithString:"),
            nsstring(&format!("速度: {:.0}", crate::scroll_div())),
        );
        if !gain_lbl.is_null() {
            set_frame(gain_lbl, sel(c"setFrame:"), NSRect { x: 232.0, y: y - 2.0, w: 130.0, h: 20.0 });
            msg1_void_id(cv, sel(c"addSubview:"), gain_lbl);
            PREFS_GAIN_LABEL.store(gain_lbl as usize, Ordering::Relaxed);
        }
        y -= 40.0;
        // 境界の敏感さスライダ(0..30px)
        let mk4: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        if !slider_cls.is_null() {
            let sl = mk4(
                slider_cls, sel(c"sliderWithValue:minValue:maxValue:target:action:"),
                crate::edge_px(), 0.0, 30.0,
                target, sel(c"sdEdgePx:"),
            );
            if !sl.is_null() {
                set_frame(sl, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 28.0, w: 180.0, h: 22.0 });
                msg1_void_id(cv, sel(c"addSubview:"), sl);
                PREFS_EDGE_SLIDER.store(sl as usize, Ordering::Relaxed);
            }
        }
        let el = label(
            objc_getClass(c"NSTextField".as_ptr()), sel(c"labelWithString:"),
            nsstring(&format!("境界の敏感さ {:.0}px", crate::edge_px())),
        );
        if !el.is_null() {
            set_frame(el, sel(c"setFrame:"), NSRect { x: 212.0, y: y - 26.0, w: 170.0, h: 18.0 });
            msg1_void_id(cv, sel(c"addSubview:"), el);
            PREFS_EDGE_LBL.store(el as usize, Ordering::Relaxed);
        }
        y -= 40.0;
        // カーソル速度スライダ(0.2..3.0)
        let mk3: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        if !slider_cls.is_null() {
            let sl = mk3(
                slider_cls, sel(c"sliderWithValue:minValue:maxValue:target:action:"),
                crate::mouse_scale(), 0.2, 3.0,
                target, sel(c"sdMouseScale:"),
            );
            if !sl.is_null() {
                set_frame(sl, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 28.0, w: 180.0, h: 22.0 });
                msg1_void_id(cv, sel(c"addSubview:"), sl);
                PREFS_MSCALE_SLIDER.store(sl as usize, Ordering::Relaxed);
            }
        }
        let ml = label(
            objc_getClass(c"NSTextField".as_ptr()), sel(c"labelWithString:"),
            nsstring(&format!("カーソル速度 x{:.1}", crate::mouse_scale())),
        );
        if !ml.is_null() {
            set_frame(ml, sel(c"setFrame:"), NSRect { x: 212.0, y: y - 26.0, w: 170.0, h: 18.0 });
            msg1_void_id(cv, sel(c"addSubview:"), ml);
            PREFS_MSCALE_LBL.store(ml as usize, Ordering::Relaxed);
        }
        y -= 40.0;

        // ---- Windows ----
        section_heading(cv, "Windows", NSRect { x: 20.0, y: y + 8.0, w: 360.0, h: 18.0 });
        y -= 26.0;
        place_check("Windows の音声を Mac で再生", c"sdAudio:", &PREFS_CHK_AUDIO, y);
        y -= 38.0;
        place_check("⌘キーを Alt に割当て(既定: Ctrl)", c"sdCmdMap:", &PREFS_CHK_CMD, y);
        y -= 38.0;
        place_check("接続中は Windows スピーカーをミュート", c"sdSpkMute:", &PREFS_CHK_SPK, y);
        y -= 38.0;

        // ---- クリップボード ----
        section_heading(cv, "クリップボード", NSRect { x: 20.0, y: y + 8.0, w: 360.0, h: 18.0 });
        y -= 26.0;
        place_check("クリップボードを共有(テキスト/画像)", c"sdClipShare:", &PREFS_CHK_CLIP, y);
        y -= 38.0;

        // ---- 操作ガイド(Mac の操作感の見える化) ----
        section_heading(cv, "操作ガイド(Windows 画面でも Mac と同じ操作)", NSRect { x: 20.0, y: y + 8.0, w: 360.0, h: 18.0 });
        y -= 24.0;
        let guide = [
            "⌘←→ 行頭/行末・⌘↑↓ 文書先頭/末尾・⌥←→ 単語移動",
            "⌘] / ⌘[ … タブの切替・⌘⇧4 切取り・⌘⇧5 録画",
            "⌘G 次を検索・⌘. キャンセル・⌘M 最小化・⌘Q 閉じる",
            "⌘⌥Esc タスクマネージャ・⌘Ctrl+Q ロック・fn+F11 デスクトップ",
            "Ctrl+クリック=右クリック・横スワイプ=戻る/進む",
            "⌘C/V/A などは Ctrl 系へ自動変換(⌘Tab=Alt+Tab)",
        ];
        let small: unsafe extern "C" fn(ID, SEL, f64) -> ID =
            std::mem::transmute(crate::objc_msgSend as usize);
        for line in guide {
            let l = label(
                objc_getClass(c"NSTextField".as_ptr()), sel(c"labelWithString:"),
                nsstring(line),
            );
            if !l.is_null() {
                set_frame(l, sel(c"setFrame:"), NSRect { x: 24.0, y: y - 16.0, w: 350.0, h: 16.0 });
                let f = crate::msg0(l, sel(c"font"));
                if !f.is_null() {
                    let sf = small(f, sel(c"fontWithSize:"), 11.0);
                    if !sf.is_null() {
                        msg1_void_id(l, sel(c"setFont:"), sf);
                    }
                }
                // 補足色へ
                let color_cls = objc_getClass(c"NSColor".as_ptr());
                if !color_cls.is_null() {
                    let get_color: unsafe extern "C" fn(ID, SEL) -> ID =
                        std::mem::transmute(crate::objc_msgSend as usize);
                    let color = get_color(color_cls, sel(c"secondaryLabelColor"));
                    if !color.is_null() {
                        msg1_void_id(l, sel(c"setTextColor:"), color);
                    }
                }
                msg1_void_id(cv, sel(c"addSubview:"), l);
            }
            y -= 20.0;
        }
        y -= 6.0;

        // ---- 操作ボタン(切替 + ファイル送信)。音量はキーボードの
        // F10/F11/F12(ミュート/▼/▲)で Windows 側を直接操作できる ----
        let toggle_btn = push_btn(
            btn_cls,
            sel(c"buttonWithTitle:target:action:"),
            nsstring("切替(Mac ⇄ Windows)"),
            target,
            sel(c"sdToggle:"),
        );
        if !toggle_btn.is_null() {
            set_frame(toggle_btn, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 4.0, w: 195.0, h: 32.0 });
            msg1_void_id(cv, sel(c"addSubview:"), toggle_btn);
        }
        let send = push_btn(
            btn_cls,
            sel(c"buttonWithTitle:target:action:"),
            nsstring("ファイルを送る…"),
            target,
            sel(c"sdSendFile:"),
        );
        if !send.is_null() {
            set_frame(send, sel(c"setFrame:"), NSRect { x: 225.0, y: y - 4.0, w: 145.0, h: 32.0 });
            msg1_void_id(cv, sel(c"addSubview:"), send);
        }
        y -= 46.0;

        // ---- Windows の音量操作(▲ / ▼ / ミュート。tag で判別) ----
        for (title, tag) in [("音量 ▲", 1isize), ("音量 ▼", 2), ("ミュート", 3)] {
            let b = push_btn(
                btn_cls,
                sel(c"buttonWithTitle:target:action:"),
                nsstring(title),
                target,
                sel(c"sdVol:"),
            );
            if !b.is_null() {
                set_frame(b, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 4.0, w: 130.0, h: 30.0 });
                msg1_void_id(cv, sel(c"addSubview:"), b);
                msg1_void_i64(b, sel(c"setTag:"), tag as i64);
            }
            y -= 36.0;
        }

        // バージョン/状態の情報行(選択不可ラベル)
        let info = label(
            objc_getClass(c"NSTextField".as_ptr()),
            sel(c"labelWithString:"),
            nsstring(&format!("Tsunagu {} ・ {}", crate::VERSION_STR, crate::BUILD_ID)),
        );
        if !info.is_null() {
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            if !color_cls.is_null() {
                let get_color: unsafe extern "C" fn(ID, SEL) -> ID =
                    std::mem::transmute(crate::objc_msgSend as usize);
                let color = get_color(color_cls, sel(c"secondaryLabelColor"));
                if !color.is_null() {
                    msg1_void_id(info, sel(c"setTextColor:"), color);
                }
            }
            set_frame(info, sel(c"setFrame:"), NSRect { x: 20.0, y: y - 4.0, w: 320.0, h: 22.0 });
            let f = msg0(info, sel(c"font"));
            if !f.is_null() {
                let small: unsafe extern "C" fn(ID, SEL, f64) -> ID =
                    std::mem::transmute(crate::objc_msgSend as usize);
                let sf = small(f, sel(c"fontWithSize:"), 11.0);
                if !sf.is_null() {
                    msg1_void_id(info, sel(c"setFont:"), sf);
                }
            }
            msg1_void_id(cv, sel(c"addSubview:"), info);
        }
        win
    }
}

unsafe extern "C" fn imp_quit(_s: ID, _c: SEL, _n: ID) {
    eprintln!("[gui] メニューから終了しました");
    std::process::exit(0);
}
unsafe extern "C" fn imp_update(_s: ID, _c: SEL, _n: ID) {
    // --show-prefs: NSApp.run 開始後のタイマーコンテキストで開く
    // (run 前のウィンドウ操作は NSException で abort するため遅延させる)
    if SHOW_AT_START.swap(false, Ordering::Relaxed) {
        show_prefs();
        // 検証用: TSUNAGU_SHOW_LAYOUT=1 で配置ウィンドウも同時オープン
        if crate::envutil::get("TSUNAGU_SHOW_LAYOUT").as_deref() == Some("1") {
            show_layout();
        }
    }
    refresh_status();
}

/// 開発環境のリポジトリを探す(<repo>/target/release/tsunagu-mac から3階層上)
fn restart_script() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut p = exe.clone();
    for _ in 0..3 {
        p.pop();
    }
    let s = p.join("scripts/restart-mac.sh");
    s.exists().then_some(s)
}

/// 状態表示の更新(メニューバーのボタンタイトル + メニュー内の動的項目)。
/// NSTimer から毎秒呼ばれる。メニュー開閉中も止まらないよう common modes で登録
fn refresh_status() {
    unsafe {
        let button = GUI_BUTTON.load(Ordering::Relaxed) as ID;
        if button.is_null() {
            return;
        }
        let connected = crate::CONNECTED.load(Ordering::Relaxed);
        let win = crate::WIN_MODE.load(Ordering::Relaxed);
        let title = if !connected {
            "SD·✕"
        } else if win {
            "SD·Win"
        } else {
            "SD·Mac"
        };
        msg1_void_id(button, sel(c"setTitle:"), nsstring(title));

        let state = GUI_STATE_ITEM.load(Ordering::Relaxed) as ID;
        if !state.is_null() {
            let conn = if connected { "接続済" } else { "切断(再接続待機中)" };
            let mode = if win { "Windows 操作中" } else { "Mac" };
            let rtt = crate::RTT_MS.load(Ordering::Relaxed);
            let rtt_s = if connected && rtt > 0 {
                format!("・遅延 {rtt}ms")
            } else {
                String::new()
            };
            let text = format!("状態: {conn} ・ {mode}{rtt_s} ・ {BUILD}", BUILD = crate::BUILD_ID);
            msg1_void_id(state, sel(c"setTitle:"), nsstring(&text));
        }
        let toggle = GUI_TOGGLE_ITEM.load(Ordering::Relaxed) as ID;
        if !toggle.is_null() {
            let t = if win { "Mac へ戻る" } else { "Windows へ切替" };
            msg1_void_id(toggle, sel(c"setTitle:"), nsstring(t));
        }
        let mode_item = GUI_MODE_ITEM.load(Ordering::Relaxed) as ID;
        if !mode_item.is_null() {
            let hotkey = crate::HOTKEY_ONLY.load(Ordering::Relaxed);
            let t = if hotkey { "切替方式: ホットキーロック" } else { "切替方式: 境界+ダブルタップ" };
            msg1_void_id(mode_item, sel(c"setTitle:"), nsstring(t));
        }
        let taps_item = GUI_TAPS_ITEM.load(Ordering::Relaxed) as ID;
        if !taps_item.is_null() {
            let t = if crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2 {
                "境界到達: ダブルタップ"
            } else {
                "境界到達: 1回"
            };
            msg1_void_id(taps_item, sel(c"setTitle:"), nsstring(t));
        }
        let side_item = GUI_SIDE_ITEM.load(Ordering::Relaxed) as ID;
        if !side_item.is_null() {
            msg1_void_id(
                side_item,
                sel(c"setTitle:"),
                nsstring(&format!("Windows の位置: {}", crate::side_name())),
            );
        }
        let audio_item = GUI_AUDIO_ITEM.load(Ordering::Relaxed) as ID;
        if !audio_item.is_null() {
            let t = if crate::audio::MUTED.load(Ordering::Relaxed) {
                "音声転送: OFF(ミュート)"
            } else {
                "音声転送: ON"
            };
            msg1_void_id(audio_item, sel(c"setTitle:"), nsstring(t));
        }
        let cmd_item = GUI_CMD_ITEM.load(Ordering::Relaxed) as ID;
        if !cmd_item.is_null() {
            let t = if crate::CMD_ALT.load(Ordering::Relaxed) {
                "⌘キー: Alt"
            } else {
                "⌘キー: Ctrl"
            };
            msg1_void_id(cmd_item, sel(c"setTitle:"), nsstring(t));
        }
        let scroll_item = GUI_SCROLL_ITEM.load(Ordering::Relaxed) as ID;
        if !scroll_item.is_null() {
            let t = if crate::SCROLL_FLIP.load(Ordering::Relaxed) {
                "スクロール方向: Windows 標準"
            } else {
                "スクロール方向: Mac に合わせる"
            };
            msg1_void_id(scroll_item, sel(c"setTitle:"), nsstring(t));
        }
        let spk_item = GUI_SPK_ITEM.load(Ordering::Relaxed) as ID;
        if !spk_item.is_null() {
            let t = if crate::SPK_MUTE.load(Ordering::Relaxed) {
                "Windowsスピーカー: 接続中ミュート(Macのみ発音)"
            } else {
                "Windowsスピーカー: 常時鳴らす"
            };
            msg1_void_id(spk_item, sel(c"setTitle:"), nsstring(t));
        }
        // 設定ウィンドウが開いていればチェック状態も保ち直す
        sync_prefs_state();
    }
}

/// メニューバー用テンプレートアイコン(アプリアイコンと同モチーフの白カーソル+残像)。
/// CoreGraphics で 44px ビットマップに描き NSImage(template) 化する。
/// template なのでメニューバーの明暗に自動追従する(色ではなくアルファで描画)
unsafe fn make_menu_icon() -> ID {
    const C: usize = 44;
    let mut data = vec![0u8; C * 4 * C];
    let space = CGColorSpaceCreateDeviceRGB();
    let ctx = CGBitmapContextCreate(
        data.as_mut_ptr(), C, C, 8, C * 4, space, 2 | (2 << 12), // BGRA
    );
    if ctx.is_null() {
        return std::ptr::null_mut();
    }
    // 黒(=テンプレート。実際の色はシステムが決める)
    CGContextSetRGBFillColor(ctx, 0.0, 0.0, 0.0, 1.0);
    CGContextSetRGBStrokeColor(ctx, 0.0, 0.0, 0.0, 1.0);
    CGContextSetLineCap(ctx, 1); // round

    // カーソルポインタ(設計座標、原点=左下)。スケール 1.7 で約 20x32px
    let arrow: [(f64, f64); 7] = [
        (0.0, 18.8), (0.0, 2.3), (4.2, 6.2), (6.8, 0.0), (9.3, 1.0), (6.7, 6.9), (11.9, 7.4),
    ];
    let (sc, ox, oy) = (1.7f64, 10.0, 5.5);
    // 残像(移動感の 2 ストローク)を先に描く
    for (alpha, w, x1, y1, x2, y2) in [
        (0.32f64, 3.2, 30.0, 12.0, 37.0, 5.0),
        (0.16, 3.2, 33.0, 19.0, 39.0, 13.0),
    ] {
        CGContextSetRGBStrokeColor(ctx, 0.0, 0.0, 0.0, alpha);
        CGContextSetLineWidth(ctx, w);
        CGContextBeginPath(ctx);
        CGContextMoveToPoint(ctx, x1, y1);
        CGContextAddLineToPoint(ctx, x2, y2);
        CGContextStrokePath(ctx);
    }
    // カーソル本体(少し傾ける)
    let theta = -14.0_f64.to_radians();
    let (t, cx, cy) = (theta, 22.0f64, 22.0f64);
    let (cos_t, sin_t) = (t.cos(), t.sin());
    // 回転を手動適用(中心(22,22)周り、CG は y 上向き)
    let rot = |x: f64, y: f64| -> (f64, f64) {
        let dx = x - cx;
        let dy = y - cy;
        (cx + cos_t * dx - sin_t * dy, cy + sin_t * dx + cos_t * dy)
    };
    CGContextSetRGBFillColor(ctx, 0.0, 0.0, 0.0, 1.0);
    CGContextBeginPath(ctx);
    let (x0, y0) = rot(ox + arrow[0].0 * sc, oy + arrow[0].1 * sc);
    CGContextMoveToPoint(ctx, x0, y0);
    for (x, y) in &arrow[1..] {
        let (rx, ry) = rot(ox + x * sc, oy + y * sc);
        CGContextAddLineToPoint(ctx, rx, ry);
    }
    CGContextClosePath(ctx);
    CGContextFillPath(ctx);

    let img = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    CFRelease(space);
    if img.is_null() {
        return std::ptr::null_mut();
    }
    // NSImage initWithCGImage:size: (NSSize は arm64 で d0/d1 レジスタ渡し)
    let f: unsafe extern "C" fn(ID, SEL, ID, f64, f64) -> ID =
        std::mem::transmute(crate::objc_msgSend as usize);
    let nsimg = f(
        msg0(objc_getClass(c"NSImage".as_ptr()), sel(c"alloc")),
        sel(c"initWithCGImage:size:"),
        img,
        18.0,
        18.0,
    );
    CGImageRelease(img);
    if nsimg.is_null() {
        return std::ptr::null_mut();
    }
    msg1_void_u8(nsimg, sel(c"setTemplate:"), 1);
    nsimg
}

unsafe fn make_target() -> ID {    let super_cls = objc_getClass(c"NSObject".as_ptr());
    if super_cls.is_null() {
        return std::ptr::null_mut();
    }
    let cls = objc_allocateClassPair(super_cls, c"SDMenuTarget".as_ptr(), 0);
    if cls.is_null() {
        return std::ptr::null_mut();
    }
    //IMP は (self, _cmd, sender) の v@:@ 型
    let types = c"v@:@".as_ptr();
    let methods: &[(&std::ffi::CStr, usize)] = &[
        (c"sdToggle:", imp_toggle as *const () as usize),
        (c"sdSwitchMode:", imp_switch_mode as *const () as usize),
        (c"sdEdgeTaps:", imp_edge_taps as *const () as usize),
        (c"sdOpenLog:", imp_open_log as *const () as usize),
        (c"sdRestart:", imp_restart as *const () as usize),
        (c"sdAudio:", imp_audio_toggle as *const () as usize),
        (c"sdCmdMap:", imp_cmd_map as *const () as usize),
        (c"sdScroll:", imp_scroll_flip as *const () as usize),
        (c"sdSpkMute:", imp_spk_mute as *const () as usize),
        (c"sdVol:", imp_vol as *const () as usize),
        (c"sdSendFile:", imp_send_file as *const () as usize),
        (c"sdShowPrefs:", imp_show_prefs as *const () as usize),
        (c"sdScrollGain:", imp_scroll_gain as *const () as usize),
        (c"sdSide:", imp_side as *const () as usize),
        (c"sdDelay:", imp_switch_delay as *const () as usize),
        (c"sdDblTap:", imp_dbl_tap as *const () as usize),
        (c"sdClipShare:", imp_clip_share as *const () as usize),
        (c"sdScrollCompat:", imp_scroll_compat as *const () as usize),
        (c"sdRotateSide:", imp_rotate_side as *const () as usize),
        (c"sdLayout:", imp_show_layout as *const () as usize),
        (c"sdMouseScale:", imp_mouse_scale as *const () as usize),
        (c"sdEdgePx:", imp_edge_px as *const () as usize),
        (c"sdQuit:", imp_quit as *const () as usize),
        (c"updateStatus:", imp_update as *const () as usize),
    ];
    for (name, imp) in methods {
        if class_addMethod(cls, sel(name), *imp, types) == 0 {
            return std::ptr::null_mut();
        }
    }
    objc_registerClassPair(cls);
    msg0(cls, sel(c"new"))
}

unsafe fn menu_item(title: &str, action: Option<&std::ffi::CStr>, key: &str) -> ID {
    let t = nsstring(title);
    let k = nsstring(key);
    let a = action.map(|x| sel(x)).unwrap_or(std::ptr::null_mut());
    // alloc + initWithTitle:action:keyEquivalent:(インスタンス初期化子)。
    // NSMenuItem には itemWithTitle:/menuItemWithTitle: クラスファクトリが無い
    // (未認識セレクタで NS例外→abort になるため使わない)
    let alloc = msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"alloc"));
    if alloc.is_null() {
        return std::ptr::null_mut();
    }
    msg3_id(alloc, sel(c"initWithTitle:action:keyEquivalent:"), t, a, k)
}

unsafe fn add_item(menu: ID, item: ID) {
    msg1_void_id(menu, sel(c"addItem:"), item);
}

/// メニューバー常駐を構築する。失敗(ssh 由来等 AppKit 不可)は false を返し、
/// 呼び出し側は CUI(CFRunLoop)へフォールバックする
pub fn start() -> bool {
    unsafe {
        // NSApplication は最初に取得しておく(AppKit の初期化順序の慣例)
        let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
        if app.is_null() {
            eprintln!("[gui] NSApplication を取得できません(CUI モードで継続)");
            return false;
        }
        let target = make_target();
        if target.is_null() {
            eprintln!("[gui] メニューターゲットクラスの登録に失敗(CUI モードで継続)");
            return false;
        }
        let _ = GUI_TARGET.store(target as usize, Ordering::Relaxed);

        let menu = msg0(objc_getClass(c"NSMenu".as_ptr()), sel(c"new"));
        if menu.is_null() {
            eprintln!("[gui] NSMenu 生成に失敗(CUI モードで継続)");
            return false;
        }
        // 自動有効化を切る(action 無しの状態行をクリック可能に見せないため)
        msg1_void_u8(menu, sel(c"setAutoenablesItems:"), 0);

        // 状態行(選択不可・refresh_status で毎秒更新)
        let state = menu_item("状態: …", None, "");
        if state.is_null() {
            return false;
        }
        msg1_void_u8(state, sel(c"setEnabled:"), 0);
        let _ = GUI_STATE_ITEM.store(state as usize, Ordering::Relaxed);
        add_item(menu, state);

        add_item(menu, msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")));

        let toggle = menu_item("Windows へ切替", Some(c"sdToggle:"), "");
        if toggle.is_null() {
            return false;
        }
        msg1_void_id(toggle, sel(c"setTarget:"), target);
        msg1_void_sel(toggle, sel(c"setAction:"), sel(c"sdToggle:"));
        let _ = GUI_TOGGLE_ITEM.store(toggle as usize, Ordering::Relaxed);
        add_item(menu, toggle);

        let mode_item = menu_item("切替方式: 境界+ダブルタップ", Some(c"sdSwitchMode:"), "");
        if mode_item.is_null() {
            return false;
        }
        msg1_void_id(mode_item, sel(c"setTarget:"), target);
        msg1_void_sel(mode_item, sel(c"setAction:"), sel(c"sdSwitchMode:"));
        let _ = GUI_MODE_ITEM.store(mode_item as usize, Ordering::Relaxed);
        add_item(menu, mode_item);

        let taps_item = menu_item("境界到達: ダブルタップ", Some(c"sdEdgeTaps:"), "");
        if taps_item.is_null() {
            return false;
        }
        msg1_void_id(taps_item, sel(c"setTarget:"), target);
        msg1_void_sel(taps_item, sel(c"setAction:"), sel(c"sdEdgeTaps:"));
        let _ = GUI_TAPS_ITEM.store(taps_item as usize, Ordering::Relaxed);
        add_item(menu, taps_item);

        let side_item = menu_item("Windows の位置: 右", Some(c"sdRotateSide:"), "");
        if side_item.is_null() {
            return false;
        }
        msg1_void_id(side_item, sel(c"setTarget:"), target);
        msg1_void_sel(side_item, sel(c"setAction:"), sel(c"sdRotateSide:"));
        let _ = GUI_SIDE_ITEM.store(side_item as usize, Ordering::Relaxed);
        add_item(menu, side_item);

        let audio_item = menu_item("音声転送: ON", Some(c"sdAudio:"), "");
        if audio_item.is_null() {
            return false;
        }
        msg1_void_id(audio_item, sel(c"setTarget:"), target);
        msg1_void_sel(audio_item, sel(c"setAction:"), sel(c"sdAudio:"));
        let _ = GUI_AUDIO_ITEM.store(audio_item as usize, Ordering::Relaxed);
        add_item(menu, audio_item);

        let cmd_item = menu_item("⌘キー: Ctrl", Some(c"sdCmdMap:"), "");
        if cmd_item.is_null() {
            return false;
        }
        msg1_void_id(cmd_item, sel(c"setTarget:"), target);
        msg1_void_sel(cmd_item, sel(c"setAction:"), sel(c"sdCmdMap:"));
        let _ = GUI_CMD_ITEM.store(cmd_item as usize, Ordering::Relaxed);
        add_item(menu, cmd_item);

        let scroll_item = menu_item("スクロール方向: 標準(Windows準拠)", Some(c"sdScroll:"), "");
        if scroll_item.is_null() {
            return false;
        }
        msg1_void_id(scroll_item, sel(c"setTarget:"), target);
        msg1_void_sel(scroll_item, sel(c"setAction:"), sel(c"sdScroll:"));
        let _ = GUI_SCROLL_ITEM.store(scroll_item as usize, Ordering::Relaxed);
        add_item(menu, scroll_item);

        let spk_item = menu_item("Windowsスピーカー: 接続中ミュート(Macのみ発音)", Some(c"sdSpkMute:"), "");
        if spk_item.is_null() {
            return false;
        }
        msg1_void_id(spk_item, sel(c"setTarget:"), target);
        msg1_void_sel(spk_item, sel(c"setAction:"), sel(c"sdSpkMute:"));
        let _ = GUI_SPK_ITEM.store(spk_item as usize, Ordering::Relaxed);
        add_item(menu, spk_item);

        add_item(menu, msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")));

        // Windows への操作(ファイル送信・音量)
        let send_item = menu_item("Windows へファイルを送る…", Some(c"sdSendFile:"), "");
        if send_item.is_null() {
            return false;
        }
        msg1_void_id(send_item, sel(c"setTarget:"), target);
        msg1_void_sel(send_item, sel(c"setAction:"), sel(c"sdSendFile:"));
        add_item(menu, send_item);

        // 音量3項目は tag で▲/▼/ミュートを判別(IMP は1つで受ける)
        for (title, tag) in [("Windows の音量 ▲", 1isize), ("Windows の音量 ▼", 2), ("Windows をミュート", 3)] {
            let item = menu_item(title, Some(c"sdVol:"), "");
            if item.is_null() {
                return false;
            }
            msg1_void_id(item, sel(c"setTarget:"), target);
            msg1_void_sel(item, sel(c"setAction:"), sel(c"sdVol:"));
            msg1_void_i64(item, sel(c"setTag:"), tag as i64);
            add_item(menu, item);
        }

        add_item(menu, msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")));

        let prefs = menu_item("設定…", Some(c"sdShowPrefs:"), ",");
        if prefs.is_null() {
            return false;
        }
        msg1_void_id(prefs, sel(c"setTarget:"), target);
        msg1_void_sel(prefs, sel(c"setAction:"), sel(c"sdShowPrefs:"));
        add_item(menu, prefs);

        let layout_item = menu_item("モニター配置…", Some(c"sdLayout:"), "");
        if !layout_item.is_null() {
            msg1_void_id(layout_item, sel(c"setTarget:"), target);
            msg1_void_sel(layout_item, sel(c"setAction:"), sel(c"sdLayout:"));
            add_item(menu, layout_item);
        }

        for (title, action) in [
            ("ログを開く…", c"sdOpenLog:"),
            ("再起動", c"sdRestart:"),
        ] {
            let item = menu_item(title, Some(action), "");
            if item.is_null() {
                return false;
            }
            msg1_void_id(item, sel(c"setTarget:"), target);
            msg1_void_sel(item, sel(c"setAction:"), sel(action));
            add_item(menu, item);
        }

        add_item(menu, msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")));
        let quit = menu_item("tsunagu を終了", Some(c"sdQuit:"), "q");
        if quit.is_null() {
            return false;
        }
        msg1_void_id(quit, sel(c"setTarget:"), target);
        msg1_void_sel(quit, sel(c"setAction:"), sel(c"sdQuit:"));
        add_item(menu, quit);

        // ステータスバー項目(可変幅)。button のタイトルで状態を常時表示する
        let sb = msg0(objc_getClass(c"NSStatusBar".as_ptr()), sel(c"systemStatusBar"));
        if sb.is_null() {
            return false;
        }
        let item = msg1_id_f64(sb, sel(c"statusItemWithLength:"), -1.0); // NSVariableStatusItemLength
        if item.is_null() {
            return false;
        }
        let button = msg0(item, sel(c"button"));
        if button.is_null() {
            return false;
        }
        let _ = GUI_BUTTON.store(button as usize, Ordering::Relaxed);
        // アイコン(テンプレート)+状態テキストの併記。失敗時はテキストのみで継続
        let icon = make_menu_icon();
        if !icon.is_null() {
            msg1_void_id(button, sel(c"setImage:"), icon);
        }
        msg1_void_id(item, sel(c"setMenu:"), menu);

        // 毎秒の状態反映。メニュー追跡中も止まらないよう common modes へ登録する
        let timer = msg5_timer(
            objc_getClass(c"NSTimer".as_ptr()),
            sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            1.0,
            target,
            sel(c"updateStatus:"),
            std::ptr::null_mut(),
            1,
        );
        if !timer.is_null() {
            let rl = msg0(objc_getClass(c"NSRunLoop".as_ptr()), sel(c"mainRunLoop"));
            // メニュー追跡(NSEventTrackingRunLoopMode)中も更新が続くよう common modes へ
            msg2_void_id_id(rl, sel(c"addTimer:forMode:"), timer, kCFRunLoopCommonModes as ID);
        }
        refresh_status();
        true
    }
}

/// アプリケーション実行(NSApp.run)。戻らない。終了はメニューの「終了」
pub unsafe fn run_app() {
    let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
    if app.is_null() {
        return;
    }
    // Accessory ポリシー = Dock アイコン非表示(メニューバー常駐型の標準)
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 1);
    msg0_void(app, sel(c"run"));
}
