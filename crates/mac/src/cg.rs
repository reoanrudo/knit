use std::sync::atomic::Ordering;

// ---------- CoreGraphics C API 直宣言 ----------
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGSize {
    pub w: f64,
    pub h: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub(crate) type CGEventRef = *mut core::ffi::c_void;
pub(crate) type CFMachPortRef = *mut core::ffi::c_void;
pub(crate) type CFRunLoopRef = *mut core::ffi::c_void;
pub(crate) type CFRunLoopSourceRef = *mut core::ffi::c_void;
pub(crate) type CFAllocatorRef = *mut core::ffi::c_void;
pub(crate) type CFStringRef = *mut core::ffi::c_void;
pub(crate) type CGEventMask = u64;
pub(crate) type CGEventFlags = u64;

// EventType 定数(SDK ヘッダ実測値)
pub(crate) const EVT_LEFT_DOWN: u32 = 1;
pub(crate) const EVT_LEFT_UP: u32 = 2;
pub(crate) const EVT_RIGHT_DOWN: u32 = 3;
pub(crate) const EVT_RIGHT_UP: u32 = 4;
pub(crate) const EVT_MOUSE_MOVED: u32 = 5;
pub(crate) const EVT_LEFT_DRAGGED: u32 = 6;
pub(crate) const EVT_RIGHT_DRAGGED: u32 = 7;
pub(crate) const EVT_KEY_DOWN: u32 = 10;
pub(crate) const EVT_KEY_UP: u32 = 11;
pub(crate) const EVT_FLAGS_CHANGED: u32 = 12;
/// NSSystemDefined(F 行のメディアキー・輝度等)。key イベントとして届かない
/// ため、Windows モードではここから翻訳する(実績: F5 が届かなかった)
pub(crate) const EVT_SYSTEM_DEFINED: u32 = 14;
pub(crate) const EVT_SCROLL_WHEEL: u32 = 22;
pub(crate) const EVT_OTHER_DOWN: u32 = 25;
pub(crate) const EVT_OTHER_UP: u32 = 26;
pub(crate) const EVT_OTHER_DRAGGED: u32 = 27;

// field 定数(SDK ヘッダ実測値)
pub(crate) const FIELD_DELTA_X: i32 = 4;
pub(crate) const FIELD_DELTA_Y: i32 = 5;
pub(crate) const FIELD_KEYCODE: i32 = 9;
pub(crate) const FIELD_SCROLL_A1: i32 = 96; // PointDeltaAxis1(縦, ピクセル)
pub(crate) const FIELD_SCROLL_A2: i32 = 97; // PointDeltaAxis2(横, ピクセル)

// flags(IOLLEvent.h 実測値)
pub(crate) const FLAG_SHIFT: CGEventFlags = 0x0002_0000;
pub(crate) const FLAG_CTRL: CGEventFlags = 0x0004_0000;
pub(crate) const FLAG_OPT: CGEventFlags = 0x0008_0000;
pub(crate) const FLAG_CMD: CGEventFlags = 0x0010_0000;
pub(crate) const FLAG_FN: CGEventFlags = 0x8000_0000; // kCGEventFlagMaskSecondaryFn
/// Caps Lock(alphaShift)。越境時の Caps 状態同期(Msg::Caps)で使う
pub(crate) const FLAG_ALPHA_SHIFT: CGEventFlags = 0x0001_0000;
/// kCGEventSourceStateCombinedSessionState(全セッション合成の状態)
pub(crate) const EVENT_SOURCE_STATE_COMBINED: i32 = 0;

pub(crate) const KC_F13: i64 = 105;
/// 切替ホットキー(Mac keycode)。KNIT_HOTKEY_KC で変更可。
/// MacBook 内蔵キーボードには F13 が無いため、例えば右Cmd(54)等に変えられる
pub(crate) static HOTKEY_KC: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(KC_F13);

pub(crate) fn hotkey_kc() -> i64 {
    HOTKEY_KC.load(Ordering::Relaxed)
}

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn CGEventTapCreate(
        tap: i32,
        place: i32,
        options: u32,
        events_of_interest: CGEventMask,
        callback: unsafe extern "C" fn(
            proxy: *mut core::ffi::c_void,
            event_type: u32,
            event: CGEventRef,
            user_info: *mut core::ffi::c_void,
        ) -> CGEventRef,
        user_info: *mut core::ffi::c_void,
    ) -> CFMachPortRef;
    pub(crate) fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub(crate) fn CGEventGetIntegerValueField(event: CGEventRef, field: i32) -> i64;
    pub(crate) fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    /// 合成セッション状態の修飾フラグ(Caps Lock の ON/OFF 読み取り用)
    pub(crate) fn CGEventSourceFlagsState(state_id: i32) -> CGEventFlags;
    pub(crate) fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub(crate) fn CGMainDisplayID() -> u32;
    pub(crate) fn CGDisplayBounds(display: u32) -> CGRect;
    pub(crate) fn CGDisplayScreenSize(display: u32) -> CGSize;
    pub(crate) fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    pub(crate) fn CGDisplayRegisterReconfigurationCallback(
        callback: unsafe extern "C" fn(u32, u32, *mut core::ffi::c_void),
        user_info: *mut core::ffi::c_void,
    ) -> i32;
    pub(crate) fn CGWarpMouseCursorPosition(new: CGPoint) -> i32;
    pub(crate) fn CGAssociateMouseAndMouseCursorPosition(connect: bool) -> i32;
    pub(crate) fn CGDisplayHideCursor(display: u32) -> i32;
    pub(crate) fn CGDisplayShowCursor(display: u32) -> i32;
    pub(crate) fn CGSetLocalEventsSuppressionInterval(seconds: f64) -> i32;
    pub(crate) fn CGEventCreate(allocator: CFAllocatorRef) -> CGEventRef;
    pub(crate) fn CGEventCreateMouseEvent(
        source: CFAllocatorRef,
        mouse_type: u32,
        mouse_position: CGPoint,
        button: u64,
    ) -> CGEventRef;
    pub(crate) fn CGEventSetIntegerValueField(event: CGEventRef, field: i32, value: i64);
    pub(crate) fn CGEventPost(tap: i32, event: CGEventRef);
    pub(crate) fn CFRelease(cf: *mut core::ffi::c_void);
    pub(crate) fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const core::ffi::c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub(crate) static kCFBooleanTrue: *const core::ffi::c_void;
    // Deskflow hideCursor/showCursor が使う非公開 CGS API(カーソル非表示の安定化)
    pub(crate) fn _CGSDefaultConnection() -> i32;
    pub(crate) fn CGSSetConnectionProperty(
        cid: i32,
        target_cid: i32,
        key: CFStringRef,
        value: *const core::ffi::c_void,
    ) -> i32;
    pub(crate) fn CFMachPortCreateRunLoopSource(
        alloc: CFAllocatorRef,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    pub(crate) fn CFRunLoopGetMain() -> CFRunLoopRef;
    pub(crate) fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub(crate) fn CFRunLoopRun();
    pub(crate) static kCFRunLoopCommonModes: CFStringRef;
}
