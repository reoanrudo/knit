//! 非公開の仮想ディスプレイ API。オブジェクトの所有期間が画面の寿命になる。
use std::ffi::{c_char, c_void};

type Id = *mut c_void;
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}
#[link(name = "AppKit", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Id;
    fn objc_msgSend(receiver: Id, selector: Id, ...) -> Id;
    fn objc_autoreleasePoolPush() -> Id;
    fn objc_autoreleasePoolPop(pool: Id);
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> Id;
}
unsafe fn sel(name: &'static std::ffi::CStr) -> Id {
    sel_registerName(name.as_ptr())
}
unsafe fn send0(o: Id, s: Id) -> Id {
    let f: unsafe extern "C" fn(Id, Id) -> Id = std::mem::transmute(objc_msgSend as *const ());
    f(o, s)
}
unsafe fn send1(o: Id, s: Id, a: Id) -> Id {
    let f: unsafe extern "C" fn(Id, Id, Id) -> Id = std::mem::transmute(objc_msgSend as *const ());
    f(o, s, a)
}
unsafe fn set_u32(o: Id, s: Id, a: u32) {
    let f: unsafe extern "C" fn(Id, Id, u32) = std::mem::transmute(objc_msgSend as *const ());
    f(o, s, a)
}
unsafe fn class(name: &'static std::ffi::CStr) -> Result<Id, String> {
    let c = objc_getClass(name.as_ptr());
    if c.is_null() {
        Err(format!(
            "仮想ディスプレイのクラスがありません: {}",
            name.to_string_lossy()
        ))
    } else {
        Ok(c)
    }
}
unsafe fn release(o: Id) {
    if !o.is_null() {
        send0(o, sel(c"release"));
    }
}

pub struct VirtualDisplay {
    object: Id,
    pub display_id: u32,
}
impl VirtualDisplay {
    pub fn create(width: u32, height: u32, fps: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || fps == 0 {
            return Err("解像度またはフレームレートが不正です".into());
        }
        unsafe {
            let pool = objc_autoreleasePoolPush();
            let result = Self::create_inner(width, height, fps);
            objc_autoreleasePoolPop(pool);
            result
        }
    }
    unsafe fn create_inner(width: u32, height: u32, fps: u32) -> Result<Self, String> {
        let descriptor_class = class(c"CGVirtualDisplayDescriptor")?;
        let mode_class = class(c"CGVirtualDisplayMode")?;
        let settings_class = class(c"CGVirtualDisplaySettings")?;
        let display_class = class(c"CGVirtualDisplay")?;
        let desc = send0(send0(descriptor_class, sel(c"alloc")), sel(c"init"));
        if desc.is_null() {
            return Err("ディスプレイ記述子を作成できません".into());
        }
        let string_class = class(c"NSString")?;
        let string_fn: unsafe extern "C" fn(Id, Id, *const c_char) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let name = string_fn(
            string_class,
            sel(c"stringWithUTF8String:"),
            c"Knit 拡張ディスプレイ".as_ptr(),
        );
        send1(desc, sel(c"setName:"), name);
        send1(desc, sel(c"setQueue:"), dispatch_get_global_queue(0, 0));
        let set_size: unsafe extern "C" fn(Id, Id, Size) =
            std::mem::transmute(objc_msgSend as *const ());
        set_size(
            desc,
            sel(c"setSizeInMillimeters:"),
            Size {
                width: width as f64 * 25.4 / 96.0,
                height: height as f64 * 25.4 / 96.0,
            },
        );
        set_u32(desc, sel(c"setMaxPixelsWide:"), width);
        set_u32(desc, sel(c"setMaxPixelsHigh:"), height);
        set_u32(desc, sel(c"setVendorID:"), 0x4b4e);
        set_u32(desc, sel(c"setProductID:"), 1);
        set_u32(desc, sel(c"setSerialNum:"), 1);
        let display = send1(
            send0(display_class, sel(c"alloc")),
            sel(c"initWithDescriptor:"),
            desc,
        );
        release(desc);
        if display.is_null() {
            return Err("仮想ディスプレイを作成できません".into());
        }
        let mode_fn: unsafe extern "C" fn(Id, Id, usize, usize, f64) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let mode = mode_fn(
            send0(mode_class, sel(c"alloc")),
            sel(c"initWithWidth:height:refreshRate:"),
            width as usize,
            height as usize,
            fps as f64,
        );
        let settings = send0(send0(settings_class, sel(c"alloc")), sel(c"init"));
        if mode.is_null() || settings.is_null() {
            release(mode);
            release(settings);
            release(display);
            return Err("仮想ディスプレイの設定を作成できません".into());
        }
        let array = send1(class(c"NSArray")?, sel(c"arrayWithObject:"), mode);
        send1(settings, sel(c"setModes:"), array);
        set_u32(settings, sel(c"setHiDPI:"), 0);
        let apply: unsafe extern "C" fn(Id, Id, Id) -> i8 =
            std::mem::transmute(objc_msgSend as *const ());
        let ok = apply(display, sel(c"applySettings:"), settings) != 0;
        release(mode);
        release(settings);
        if !ok {
            release(display);
            return Err("仮想ディスプレイの設定に失敗しました".into());
        }
        let get_id: unsafe extern "C" fn(Id, Id) -> u32 =
            std::mem::transmute(objc_msgSend as *const ());
        let display_id = get_id(display, sel(c"displayID"));
        if display_id == 0 {
            release(display);
            return Err("ディスプレイ ID を取得できません".into());
        }
        Ok(Self {
            object: display,
            display_id,
        })
    }
}
impl Drop for VirtualDisplay {
    fn drop(&mut self) {
        unsafe { release(self.object) }
    }
}
