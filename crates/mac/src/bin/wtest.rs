// NSWindow 生成の ABI 検証用スタンドアロン(設定ウィンドウの abort 調査)。
// 使い方: wtest hfa | wtest u64
#![allow(non_snake_case)]
use std::ffi::c_void;

type ID = *mut c_void;
type SEL = *const i8;

#[link(name = "objc", kind = "dylib")]
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    fn objc_getClass(name: *const i8) -> ID;
    fn sel_registerName(name: *const i8) -> SEL;
    fn objc_msgSend();
}

unsafe fn msg0(t: ID, c: SEL) -> ID {
    unsafe {
        let f: unsafe extern "C" fn(ID, SEL) -> ID = std::mem::transmute(objc_msgSend as usize);
        f(t, c)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "full" {
        unsafe { full_prefs_probe() };
        return;
    }
    unsafe {
        let nsapp = objc_getClass(c"NSApplication".as_ptr());
        let app = msg0(nsapp, sel_registerName(c"sharedApplication".as_ptr()));
        let set_policy: unsafe extern "C" fn(ID, SEL, i64) =
            std::mem::transmute(objc_msgSend as usize);
        set_policy(app, sel_registerName(c"setActivationPolicy:".as_ptr()), 1);
        let activate: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(objc_msgSend as usize);
        activate(app, sel_registerName(c"activateIgnoringOtherApps:".as_ptr()), 1);

        let alloc = msg0(
            msg0(objc_getClass(c"NSWindow".as_ptr()), sel_registerName(c"alloc".as_ptr())),
            sel_registerName(c"retain".as_ptr()), // noop保障(alloc 直は既にretain)
        );
        let _ = alloc;
        let alloc = msg0(objc_getClass(c"NSWindow".as_ptr()), sel_registerName(c"alloc".as_ptr()));
        let sel_init = sel_registerName(c"initWithContentRect:styleMask:backing:defer:".as_ptr());
        let rect = NSRect { x: 100.0, y: 100.0, w: 360.0, h: 240.0 };
        let mask: u64 = 1 | 2 | 8;
        let win = match mode.as_str() {
            "u64" => {
                println!("[wtest] u64 ビット列渡しで試行");
                let f: unsafe extern "C" fn(ID, SEL, u64, u64, u64, u64, u64, u64, u8) -> ID =
                    std::mem::transmute(objc_msgSend as usize);
                f(
                    alloc, sel_init,
                    rect.x.to_bits(), rect.y.to_bits(), rect.w.to_bits(), rect.h.to_bits(),
                    mask, 2, 0,
                )
            }
            _ => {
                println!("[wtest] HFA(NSRect by-value)渡しで試行");
                let f: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
                    std::mem::transmute(objc_msgSend as usize);
                f(alloc, sel_init, rect, mask, 2, 0)
            }
        };
        println!("[wtest] init 戻り値 is_null={}", win.is_null());
        if win.is_null() {
            return;
        }
        let make_key: unsafe extern "C" fn(ID, SEL, ID) =
            std::mem::transmute(objc_msgSend as usize);
        make_key(win, sel_registerName(c"makeKeyAndOrderFront:".as_ptr()), std::ptr::null_mut());
        println!("[wtest] 表示しました。3 秒待機…");
        std::thread::sleep(std::time::Duration::from_secs(3));
        println!("[wtest] 終了(abort しなければ ABI 正常)");
    }
}

/// 設定ウィンドウ相当の部品を段階的に試し、どこでabortするか特定する
unsafe fn full_prefs_probe() {
    unsafe {
        let nsapp = objc_getClass(c"NSApplication".as_ptr());
        let app = msg0(nsapp, sel_registerName(c"sharedApplication".as_ptr()));
        let act: unsafe extern "C" fn(ID, SEL, u8) = std::mem::transmute(objc_msgSend as usize);
        act(app, sel_registerName(c"activateIgnoringOtherApps:".as_ptr()), 1);

        let initf: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
            std::mem::transmute(objc_msgSend as usize);
        let alloc = msg0(objc_getClass(c"NSWindow".as_ptr()), sel_registerName(c"alloc".as_ptr()));
        let win = initf(alloc, sel_registerName(c"initWithContentRect:styleMask:backing:defer:".as_ptr()),
            NSRect { x: 50.0, y: 50.0, w: 400.0, h: 700.0 }, 1 | 2 | 8 | 0x8000, 2, 0);
        step(1, "NSWindow init", !win.is_null());

        let b1: unsafe extern "C" fn(ID, SEL, u8) = std::mem::transmute(objc_msgSend as usize);
        b1(win, sel_registerName(c"setTitlebarAppearsTransparent:".as_ptr()), 1);
        step(2, "titlebarAppearsTransparent", true);

        let minsz: unsafe extern "C" fn(ID, SEL, NSRect) = std::mem::transmute(objc_msgSend as usize);
        minsz(win, sel_registerName(c"setContentMinSize:".as_ptr()), NSRect { x: 400.0, y: 700.0, w: 400.0, h: 700.0 });
        step(3, "setContentMinSize", true);

        let cv = msg0(win, sel_registerName(c"contentView".as_ptr()));
        step(4, "contentView", !cv.is_null());

        let bounds: unsafe extern "C" fn(ID, SEL) -> NSRect = std::mem::transmute(objc_msgSend as usize);
        let b = bounds(cv, sel_registerName(c"bounds".as_ptr()));
        let ve_alloc = msg0(objc_getClass(c"NSVisualEffectView".as_ptr()), sel_registerName(c"alloc".as_ptr()));
        let ve = initf(ve_alloc, sel_registerName(c"initWithFrame:".as_ptr()), b, 0, 0, 0);
        step(5, "NSVisualEffectView init", !ve.is_null());
        if !ve.is_null() {
            let i: unsafe extern "C" fn(ID, SEL, i64) = std::mem::transmute(objc_msgSend as usize);
            i(ve, sel_registerName(c"setMaterial:".as_ptr()), 2);
            i(ve, sel_registerName(c"setBlendingMode:".as_ptr()), 0);
            i(ve, sel_registerName(c"setState:".as_ptr()), 1);
            i(ve, sel_registerName(c"setAutoresizingMask:".as_ptr()), 2 | 16);
            let add: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as usize);
            add(cv, sel_registerName(c"addSubview:".as_ptr()), ve);
            step(6, "effect view setup", true);
        }

        // labelWithString: が例外になる環境があるため伝統構成で作る
        // (alloc+init+setStringValue+編集不可/ bezel 無し)
        let lbl = msg0(objc_getClass(c"NSTextField".as_ptr()), sel_registerName(c"alloc".as_ptr()));
        let lbl = msg0(lbl, sel_registerName(c"initWithFrame:".as_ptr()));
        let initr: unsafe extern "C" fn(ID, SEL, NSRect) -> ID = std::mem::transmute(objc_msgSend as usize);
        let lbl = initr(lbl, sel_registerName(c"initWithFrame:".as_ptr()), NSRect { x: 0.0, y: 0.0, w: 300.0, h: 20.0 });
        let sets: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as usize);
        sets(lbl, sel_registerName(c"setStringValue:".as_ptr()), ns_str("見出しテスト"));
        let setb: unsafe extern "C" fn(ID, SEL, u8) = std::mem::transmute(objc_msgSend as usize);
        setb(lbl, sel_registerName(c"setBezeled:".as_ptr()), 0);
        setb(lbl, sel_registerName(c"setEditable:".as_ptr()), 0);
        setb(lbl, sel_registerName(c"setSelectable:".as_ptr()), 0);
        setb(lbl, sel_registerName(c"setDrawsBackground:".as_ptr()), 0);
        step(7, "NSTextField traditional", !lbl.is_null());
        let bold: unsafe extern "C" fn(ID, SEL, f64) -> ID = std::mem::transmute(objc_msgSend as usize);
        let bf = bold(objc_getClass(c"NSFont".as_ptr()), sel_registerName(c"boldSystemFontOfSize:".as_ptr()), 11.0);
        step(8, "NSFont boldSystemFontOfSize", !bf.is_null());

        let chkf: unsafe extern "C" fn(ID, SEL, ID, ID, SEL) -> ID = std::mem::transmute(objc_msgSend as usize);
        let chk = chkf(objc_getClass(c"NSButton".as_ptr()), sel_registerName(c"checkboxWithTitle:target:action:".as_ptr()),
            ns_str("チェック"), std::ptr::null_mut(), std::ptr::null());
        step(9, "checkWithTitle", !chk.is_null());

        let slf: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID = std::mem::transmute(objc_msgSend as usize);
        let sl = slf(objc_getClass(c"NSSlider".as_ptr()), sel_registerName(c"sliderWithValue:minValue:maxValue:target:action:".as_ptr()),
            60.0, 20.0, 240.0, std::ptr::null_mut(), std::ptr::null());
        step(10, "NSSlider", !sl.is_null());

        let frame: unsafe extern "C" fn(ID, SEL, NSRect) = std::mem::transmute(objc_msgSend as usize);
        frame(lbl, sel_registerName(c"setFrame:".as_ptr()), NSRect { x: 20.0, y: 640.0, w: 300.0, h: 20.0 });
        let add: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as usize);
        add(cv, sel_registerName(c"addSubview:".as_ptr()), lbl);
        let sets: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as usize);
        sets(lbl, sel_registerName(c"setStringValue:".as_ptr()), ns_str("状態: 接続済"));
        let colorf: unsafe extern "C" fn(ID, SEL) -> ID = std::mem::transmute(objc_msgSend as usize);
        let red = colorf(objc_getClass(c"NSColor".as_ptr()), sel_registerName(c"systemRedColor".as_ptr()));
        let setc: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as usize);
        setc(lbl, sel_registerName(c"setTextColor:".as_ptr()), red);
        step(11, "setFrame/setStringValue/setTextColor", true);

        let mk: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as usize);
        mk(win, sel_registerName(c"makeKeyAndOrderFront:".as_ptr()), std::ptr::null_mut());
        step(12, "makeKeyAndOrderFront", true);
        std::thread::sleep(std::time::Duration::from_secs(2));
        println!("[wtest] full probe 完了(全段階通過なら本体側の別要因)");
    }
}

fn step(n: u32, name: &str, ok: bool) {
    println!("[wtest] {:>2}. {} {}", n, name, if ok { "OK" } else { "NULL!" });
    std::thread::sleep(std::time::Duration::from_millis(150));
}

unsafe fn ns_str(s: &str) -> ID {
    unsafe {
        let f: unsafe extern "C" fn(ID, SEL, *const i8) -> ID =
            std::mem::transmute(objc_msgSend as usize);
        let c = std::ffi::CString::new(s).unwrap();
        f(objc_getClass(c"NSString".as_ptr()), sel_registerName(c"stringWithUTF8String:".as_ptr()), c.as_ptr())
    }
}
