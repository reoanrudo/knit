// メニューバー常駐 GUI(NSStatusItem + NSMenu)。
// objc_msgSend 固定シグネチャ方式(実績パターン#1。依存追加ゼロ)。
// objc2-app-kit への移行はモジュール分割(Wave3 Step6)時に検討する。
// 呼び出し規約: このモジュールの全関数はメインスレッドから呼ぶこと
// (start() は main() の末尾、IMP は AppKit のイベント配信=メインRunLoop)。

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{do_toggle, msg0, nsstring, objc_getClass};

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

// ---------- メニュー項目のアクション(Objective-C クラスの IMP) ----------

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
        .args(["-a", "Console", "/tmp/sd-mac-run.log"])
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
            crate::notify("seamless-desk", "再起動スクリプトが見つかりません");
        }
    }
}
unsafe extern "C" fn imp_quit(_s: ID, _c: SEL, _n: ID) {
    eprintln!("[gui] メニューから終了しました");
    std::process::exit(0);
}
unsafe extern "C" fn imp_update(_s: ID, _c: SEL, _n: ID) {
    refresh_status();
}

/// 開発環境のリポジトリを探す(<repo>/target/release/sd-mac から3階層上)
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
            let text = format!("状態: {conn} ・ {mode} ・ {BUILD}", BUILD = crate::BUILD_ID);
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

        add_item(menu, msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")));

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
        let quit = menu_item("seamless-desk を終了", Some(c"sdQuit:"), "q");
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
