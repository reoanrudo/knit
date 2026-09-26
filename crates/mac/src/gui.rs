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

mod preferences;
mod prefs;
pub static UI_PREVIEW: AtomicBool = AtomicBool::new(false);
pub fn restore_preferences() {
    preferences::restore();
}

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
    fn class_addMethod(cls: CLS, name: SEL, imp: usize, types: *const core::ffi::c_char) -> i32;
    fn objc_registerClassPair(cls: CLS);
    // メニューバーアイコンを CoreGraphics で描くための最小セット
    fn CGColorSpaceCreateDeviceRGB() -> *mut core::ffi::c_void;
    fn CGBitmapContextCreate(
        data: *mut u8,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *mut core::ffi::c_void,
        bitmap_info: u32,
    ) -> *mut core::ffi::c_void;
    fn CGBitmapContextCreateImage(ctx: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn CGContextRelease(ctx: *mut core::ffi::c_void);
    fn CGImageRelease(img: *mut core::ffi::c_void);
    fn CGContextSetRGBFillColor(ctx: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetRGBStrokeColor(ctx: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
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
static PREFS_DELAY_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_DELAY_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_DBL_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_DBL_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_MSCALE_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_MSCALE_LBL: AtomicUsize = AtomicUsize::new(0);
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
            let speed = if v <= 40.0 {
                "速い"
            } else if v >= 140.0 {
                "遅い"
            } else {
                "標準"
            };
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("スクロール速度: {speed}({v:.0})")),
            );
        }
    }
    preferences::save();
}

unsafe extern "C" fn imp_toggle(_s: ID, _c: SEL, _n: ID) {
    do_toggle("menu");
    refresh_status();
}
unsafe extern "C" fn imp_switch_mode(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::HOTKEY_ONLY.load(Ordering::Relaxed);
    crate::HOTKEY_ONLY.store(next, Ordering::Relaxed);
    eprintln!(
        "[mode] switch_mode -> {}",
        if next { "hotkey(ロック)" } else { "edge" }
    );
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_edge_taps(_s: ID, _c: SEL, _n: ID) {
    let next = if crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2 {
        1
    } else {
        2
    };
    crate::EDGE_TAPS.store(next, Ordering::Relaxed);
    eprintln!("[mode] edge_taps -> {next}");
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_open_log(_s: ID, _c: SEL, _n: ID) {
    let _ = std::process::Command::new("open")
        .args(["-a", "Console", "/tmp/tsunagu-mac.log"])
        .spawn();
}
unsafe extern "C" fn imp_restart(_s: ID, _c: SEL, _n: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
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
    preferences::save();
}
unsafe extern "C" fn imp_cmd_map(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CMD_ALT.load(Ordering::Relaxed);
    crate::CMD_ALT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] ⌘キー -> {}", if next { "Alt" } else { "Ctrl" });
    crate::send_cfg();
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_spk_mute(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SPK_MUTE.load(Ordering::Relaxed);
    crate::SPK_MUTE.store(next, Ordering::Relaxed);
    eprintln!(
        "[cfg] 接続中スピーカーミュート -> {}",
        if next { "ON" } else { "OFF" }
    );
    crate::send_cfg();
    refresh_status();
    preferences::save();
}
/// 「Windows の位置」ポップアップ(0=右/1=左/2=上/3=下)。Deskflow links 相当
unsafe extern "C" fn imp_side(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> isize =
            std::mem::transmute(crate::objc_msgSend as usize);
        let idx = get(sender, sel(c"indexOfSelectedItem"));
        crate::set_side(idx.clamp(0, 7) as u8);
        *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
    }
    preferences::save();
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
            let t = if v < 1.0 {
                "無効(即時/ダブルタップ)"
            } else {
                &format!("{v:.0}ms 滞って切替")
            };
            msg1_void_id(lbl, sel(c"setStringValue:"), nsstring(t));
        }
    }
    preferences::save();
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
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("{v:.0}ms 以内の2回")),
            );
        }
    }
    preferences::save();
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
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("速度 x{v:.1}")),
            );
        }
    }
    preferences::save();
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
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("敏感さ {v:.0}px")),
            );
        }
    }
    preferences::save();
}

/// スクロール互換モードのトグル(120 未満を無視する古いアプリ向け)
unsafe extern "C" fn imp_scroll_compat(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_COMPAT.load(Ordering::Relaxed);
    crate::SCROLL_COMPAT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] スクロール互換モード -> {next}");
    preferences::save();
}

/// clipboardSharing トグル(Deskflow 標準オプション)
unsafe extern "C" fn imp_clip_share(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CLIP_SHARE.load(Ordering::Relaxed);
    crate::CLIP_SHARE.store(next, Ordering::Relaxed);
    eprintln!("[cfg] クリップボード共有 -> {next}");
    crate::send_cfg();
    preferences::save();
}

/// メニュー「Windows の位置」: 右→左→上→下→右 のローテート
unsafe extern "C" fn imp_rotate_side(_s: ID, _c: SEL, _n: ID) {
    let next = (crate::SIDE.load(Ordering::Relaxed) + 1) % 4;
    crate::set_side(next);
    refresh_status();
    preferences::save();
}

/// 配置エディタ(独立ウィンドウ)を開く
/// 配置エディタ(独立ウィンドウ)を開く(メニュー IMP と起動直後の両方から呼ぶ)
pub fn show_layout() {
    show_prefs();
    unsafe {
        prefs::select_page(1);
    }
}

unsafe extern "C" fn imp_show_layout(_s: ID, _c: SEL, _n: ID) {
    show_layout();
}

unsafe extern "C" fn imp_scroll_flip(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_FLIP.load(Ordering::Relaxed);
    crate::SCROLL_FLIP.store(next, Ordering::Relaxed);
    eprintln!(
        "[cfg] スクロール方向 -> {}",
        if next {
            "反転(Mac準拠)"
        } else {
            "標準(Windows準拠)"
        }
    );
    refresh_status();
    preferences::save();
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
    let app = msg0(
        crate::objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let panel = msg0(
        crate::objc_getClass(c"NSOpenPanel".as_ptr()),
        sel(c"openPanel"),
    );
    if panel.is_null() {
        eprintln!("[gui] NSOpenPanel を生成できません");
        return;
    }
    msg1_void_u8(panel, sel(c"setCanChooseFiles:"), 1);
    msg1_void_u8(panel, sel(c"setCanChooseDirectories:"), 0);
    msg1_void_u8(panel, sel(c"setAllowsMultipleSelection:"), 1);
    msg1_void_id(
        panel,
        sel(c"setMessage:"),
        crate::nsstring("Windows へ送信します(合計 200MB まで)"),
    );
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
        let s = std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned();
        if !s.is_empty() {
            paths.push(s);
        }
    }
    if !paths.is_empty() {
        let pb: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
        eprintln!("[gui] ファイル送信: {} 件", pb.len());
        crate::send_files_to_win(pb, false);
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
            if name.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(name).to_str().ok()
            },
            if reason.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(reason).to_str().ok()
            },
        );
    }
}

pub fn show_prefs() {
    eprintln!("[prefs] enter");
    unsafe {
        let app = msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel(c"sharedApplication"),
        );
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
        prefs::sync();
        // Windows の位置ポップアップ(メニューのローテート反映。閉じた状態への
        // selectItemAtIndex はユーザー操作と競合しない)
        let pop = PREFS_SIDE_POP.load(Ordering::Relaxed) as ID;
        if !pop.is_null() {
            let select: unsafe extern "C" fn(ID, SEL, isize) =
                std::mem::transmute(crate::objc_msgSend as usize);
            select(
                pop,
                sel(c"selectItemAtIndex:"),
                crate::SIDE.load(Ordering::Relaxed) as isize,
            );
        }
        // 状態行(接続・操作中・遅延)
        let st = PREFS_STATE.load(Ordering::Relaxed) as ID;
        if !st.is_null() {
            let connected = crate::CONNECTED.load(Ordering::Relaxed);
            let win = crate::WIN_MODE.load(Ordering::Relaxed);
            let rtt = crate::RTT_MS.load(Ordering::Relaxed);
            let conn = if connected {
                if rtt > 0 {
                    format!("接続済(遅延 {rtt}ms)")
                } else {
                    "接続済".into()
                }
            } else {
                "切断(再接続待機中)".to_string()
            };
            let mode = if win {
                "Windows 操作中"
            } else {
                "Mac 操作中"
            };
            msg1_void_id(
                st,
                sel(c"setStringValue:"),
                nsstring(&format!("状態: {conn} ・ {mode}")),
            );
            // 切断時は赤で強調(接続時は標準ラベル色)
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            if !color_cls.is_null() {
                let get_color: unsafe extern "C" fn(ID, SEL) -> ID =
                    std::mem::transmute(crate::objc_msgSend as usize);
                let color = get_color(
                    color_cls,
                    sel(if connected {
                        c"labelColor"
                    } else {
                        c"systemRedColor"
                    }),
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
        set(
            &PREFS_CHK_TAPS,
            crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2,
        );
        set(
            &PREFS_CHK_AUDIO,
            !crate::audio::MUTED.load(Ordering::Relaxed),
        );
        set(&PREFS_CHK_CMD, crate::CMD_ALT.load(Ordering::Relaxed));
        set(
            &PREFS_CHK_SCROLL,
            !crate::SCROLL_FLIP.load(Ordering::Relaxed),
        );
        set(&PREFS_CHK_SPK, crate::SPK_MUTE.load(Ordering::Relaxed));
        set(&PREFS_CHK_CLIP, crate::CLIP_SHARE.load(Ordering::Relaxed));
        set(
            &PREFS_CHK_SCOMPAT,
            crate::SCROLL_COMPAT.load(Ordering::Relaxed),
        );
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
    let by_w = (LAY_VW - 80.0) / (mw + 2.0 * ww).max(1.0);
    let by_h = (LAY_VH - 40.0) / (mh + 2.0 * wh).max(1.0);
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
        let (cx, cy) = (m.x + m.w / 2.0, m.y + m.h / 2.0);
        return match crate::SIDE.load(Ordering::Relaxed) {
            1 => (m.x - 8.0 - ww / 2.0, cy),
            2 => (cx, m.y + m.h + 8.0 + wh / 2.0),
            3 => (cx, m.y - 8.0 - wh / 2.0),
            4 => (m.x + m.w + 8.0 + ww / 2.0, cy + m.h / 3.0),
            5 => (m.x + m.w + 8.0 + ww / 2.0, cy - m.h / 3.0),
            6 => (m.x - 8.0 - ww / 2.0, cy + m.h / 3.0),
            7 => (m.x - 8.0 - ww / 2.0, cy - m.h / 3.0),
            _ => (m.x + m.w + 8.0 + ww / 2.0, cy),
        };
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
        x: (LAY_VW - mw * sc) / 2.0,
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
        conv(
            _self,
            sel(c"convertPoint:fromView:"),
            p,
            std::ptr::null_mut(),
        )
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
            fn CGContextSetRGBStrokeColor(
                c: *mut core::ffi::c_void,
                r: f64,
                g: f64,
                b: f64,
                a: f64,
            );
            fn CGContextSetLineWidth(c: *mut core::ffi::c_void, w: f64);
            fn CGContextStrokeRect(c: *mut core::ffi::c_void, r: NSRect);
        }
        let ctx = port as *mut core::ffi::c_void;
        // 背景
        CGContextSetRGBFillColor(ctx, 0.94, 0.95, 0.98, 1.0);
        CGContextFillRect(
            ctx,
            NSRect {
                x: 0.0,
                y: 0.0,
                w: LAY_VW,
                h: LAY_VH,
            },
        );
        // Mac(灰+白枠: 青が重なっても輪郭が見える)
        let mr = lay_mac_rect();
        CGContextSetRGBFillColor(ctx, 0.52, 0.56, 0.65, 1.0);
        CGContextFillRect(ctx, mr);
        CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.9);
        CGContextSetLineWidth(ctx, 1.5);
        CGContextStrokeRect(ctx, mr);
        // Windows(青=標準アクセント)
        let wc = lay_win_center();
        let (ww, wh) = lay_win_size();
        CGContextSetRGBFillColor(ctx, 0.32, 0.38, 0.82, 1.0);
        CGContextFillRect(
            ctx,
            NSRect {
                x: wc.0 - ww / 2.0,
                y: wc.1 - wh / 2.0,
                w: ww,
                h: wh,
            },
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
            (
                if dx >= 0.0 { 0u8 } else { 1u8 },
                f0.clamp(0.0, 1.0),
                f1.clamp(0.0, 1.0),
            )
        } else {
            let f0 = ((wc0.0 - ww / 2.0) - m.x) / m.w;
            let f1 = ((wc0.0 + ww / 2.0) - m.x) / m.w;
            (
                if dy >= 0.0 { 2u8 } else { 3u8 },
                f0.clamp(0.0, 1.0),
                f1.clamp(0.0, 1.0),
            )
        };
        // 斜め(4-7)表現: 水平辺で接続範囲が半分未満なら上下の半分側へ
        let mut side = edge;
        if edge <= 1 && (f1 - f0) < 0.6 {
            side = match (edge, (f0 + f1) / 2.0 >= 0.5) {
                (0, true) => 4,
                (0, false) => 5,
                (1, true) => 6,
                _ => 7,
            };
        }
        crate::set_side(side);
        // 細かい範囲で上書き(set_side は半分単位で設定するため)
        let (start, end) = if edge <= 1 {
            (1.0 - f1, 1.0 - f0)
        } else {
            (f0, f1)
        };
        let start = start.clamp(0.0, 0.95);
        *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) =
            (start, end.max(start + 0.05).min(1.0));
        let snd: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(crate::objc_msgSend as usize);
        snd(_self, sel(c"setNeedsDisplay:"), 1);
        eprintln!(
            "[lay] 配置を更新: {}(範囲 {:.2}〜{:.2})",
            crate::side_name(),
            f0,
            f1
        );
    }
    preferences::save();
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
            let ok2 = class_addMethod(
                cls,
                sel(c"mouseDown:"),
                lay_down as *const () as usize,
                c"v@:@".as_ptr(),
            );
            let ok3 = class_addMethod(
                cls,
                sel(c"mouseDragged:"),
                lay_dragged as *const () as usize,
                c"v@:@".as_ptr(),
            );
            let ok4 = class_addMethod(
                cls,
                sel(c"mouseUp:"),
                lay_up as *const () as usize,
                c"v@:@".as_ptr(),
            );
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
            NSRect {
                x: 0.0,
                y: 0.0,
                w: LAY_VW,
                h: LAY_VH,
            },
        )
    }
}

/// OSネイティブの4分類設定画面を構築する。
unsafe fn make_prefs_window(target: ID) -> ID {
    prefs::build(target)
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
        if !UI_PREVIEW.load(Ordering::Relaxed)
            && crate::envutil::get("TSUNAGU_SHOW_LAYOUT").as_deref() == Some("1")
        {
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
            "未接続"
        } else if win {
            "Windows"
        } else {
            "Mac"
        };
        msg1_void_id(button, sel(c"setTitle:"), nsstring(title));

        let state = GUI_STATE_ITEM.load(Ordering::Relaxed) as ID;
        if !state.is_null() {
            let conn = if connected {
                "接続済"
            } else {
                "切断(再接続待機中)"
            };
            let mode = if win { "Windows 操作中" } else { "Mac" };
            let rtt = crate::RTT_MS.load(Ordering::Relaxed);
            let rtt_s = if connected && rtt > 0 {
                format!("・遅延 {rtt}ms")
            } else {
                String::new()
            };
            let text = format!("{conn} ・ {mode}{rtt_s}");
            msg1_void_id(state, sel(c"setTitle:"), nsstring(&text));
        }
        let toggle = GUI_TOGGLE_ITEM.load(Ordering::Relaxed) as ID;
        if !toggle.is_null() {
            let t = if win {
                "Mac へ戻る"
            } else {
                "Windows へ切替"
            };
            msg1_void_id(toggle, sel(c"setTitle:"), nsstring(t));
        }
        let mode_item = GUI_MODE_ITEM.load(Ordering::Relaxed) as ID;
        if !mode_item.is_null() {
            let hotkey = crate::HOTKEY_ONLY.load(Ordering::Relaxed);
            let t = if hotkey {
                "切替方式: ホットキーロック"
            } else {
                "切替方式: 境界+ダブルタップ"
            };
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

/// メニューバー用テンプレートアイコン(アプリアイコンと同じリボン)。
/// CoreGraphics で 44px ビットマップに描き NSImage(template) 化する。
/// template なのでメニューバーの明暗に自動追従する(色ではなくアルファで描画)
unsafe fn make_menu_icon() -> ID {
    const C: usize = 44;
    let mut data = vec![0u8; C * 4 * C];
    let space = CGColorSpaceCreateDeviceRGB();
    let ctx = CGBitmapContextCreate(
        data.as_mut_ptr(),
        C,
        C,
        8,
        C * 4,
        space,
        2 | (2 << 12), // BGRA
    );
    if ctx.is_null() {
        return std::ptr::null_mut();
    }
    extern "C" {
        fn CGContextAddCurveToPoint(c: ID, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64);
    }
    CGContextSetRGBStrokeColor(ctx, 0.0, 0.0, 0.0, 1.0);
    CGContextSetLineWidth(ctx, 4.0);
    CGContextSetLineCap(ctx, 1);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 20.0, 19.0);
    CGContextAddCurveToPoint(ctx, 11.0, 8.0, 4.0, 13.0, 8.0, 21.0);
    CGContextAddCurveToPoint(ctx, 10.0, 25.0, 14.0, 28.0, 19.0, 29.0);
    CGContextStrokePath(ctx);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 16.0, 14.0);
    CGContextAddCurveToPoint(ctx, 23.0, 22.0, 27.0, 36.0, 34.0, 29.0);
    CGContextAddCurveToPoint(ctx, 40.0, 22.0, 29.0, 16.0, 24.0, 15.0);
    CGContextStrokePath(ctx);

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

unsafe fn make_target() -> ID {
    let super_cls = objc_getClass(c"NSObject".as_ptr());
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
        (c"sdHotkey:", prefs::hotkey as *const () as usize),
        (c"sdNavigate:", prefs::navigate as *const () as usize),
        (
            c"sdSwitchMethod:",
            prefs::switch_method as *const () as usize,
        ),
        (c"sdReturnMac:", prefs::return_mac as *const () as usize),
        (c"sdScrollSpeed:", prefs::scroll_speed as *const () as usize),
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
        let app = msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel(c"sharedApplication"),
        );
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

        add_item(
            menu,
            msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")),
        );

        let toggle = menu_item("Windows へ切替", Some(c"sdToggle:"), "");
        if toggle.is_null() {
            return false;
        }
        msg1_void_id(toggle, sel(c"setTarget:"), target);
        msg1_void_sel(toggle, sel(c"setAction:"), sel(c"sdToggle:"));
        let _ = GUI_TOGGLE_ITEM.store(toggle as usize, Ordering::Relaxed);
        add_item(menu, toggle);

        let prefs = menu_item("設定…", Some(c"sdShowPrefs:"), ",");
        if prefs.is_null() {
            return false;
        }
        msg1_void_id(prefs, sel(c"setTarget:"), target);
        msg1_void_sel(prefs, sel(c"setAction:"), sel(c"sdShowPrefs:"));
        add_item(menu, prefs);

        add_item(
            menu,
            msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")),
        );
        let quit = menu_item("tsunagu を終了", Some(c"sdQuit:"), "q");
        if quit.is_null() {
            return false;
        }
        msg1_void_id(quit, sel(c"setTarget:"), target);
        msg1_void_sel(quit, sel(c"setAction:"), sel(c"sdQuit:"));
        add_item(menu, quit);

        // ステータスバー項目(可変幅)。button のタイトルで状態を常時表示する
        let sb = msg0(
            objc_getClass(c"NSStatusBar".as_ptr()),
            sel(c"systemStatusBar"),
        );
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
            msg2_void_id_id(
                rl,
                sel(c"addTimer:forMode:"),
                timer,
                kCFRunLoopCommonModes as ID,
            );
        }
        refresh_status();
        true
    }
}

/// アプリケーション実行(NSApp.run)。戻らない。終了はメニューの「終了」
pub unsafe fn run_app() {
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    if app.is_null() {
        return;
    }
    // Accessory ポリシー = Dock アイコン非表示(メニューバー常駐型の標準)
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 1);
    msg0_void(app, sel(c"run"));
}
