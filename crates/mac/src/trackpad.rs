//! Session tapでジェスチャーを受け、NSEventの公開アクセサでピンチを読む。
use crate::*;
use std::sync::atomic::AtomicBool;
use std::sync::OnceLock;
mod contacts;
mod navigation;
pub static NAV_ENABLED: AtomicBool = AtomicBool::new(true);

fn remote_active() -> bool {
    CONNECTED.load(Ordering::Relaxed)
        && WIN_MODE.load(Ordering::Relaxed)
        && active_peer_is_android()
}
fn navigation_active() -> bool {
    NAV_ENABLED.load(Ordering::Relaxed) && AVAILABLE.load(Ordering::Relaxed) && remote_active()
}
pub fn navigation_available() -> bool {
    contacts::AVAILABLE.load(Ordering::Relaxed) && AVAILABLE.load(Ordering::Relaxed)
}
pub fn reset_session() {
    contacts::reset();
    // 捕捉中のまま切替・切断が起きた場合、合成2指を上げずに捨てると Android 側に
    // 押し下げられた指が残留する(set_enabled と同じ終端を送る)
    if CAPTURED.swap(false, Ordering::Relaxed) {
        send_msg(&Msg::Pinch {
            delta: 0.0,
            phase: 3,
        });
    }
}
pub fn set_navigation(enabled: bool) {
    NAV_ENABLED.store(enabled, Ordering::Relaxed);
    contacts::reset();
}
pub fn suppress_scroll(dx: f64, dy: f64) -> bool {
    contacts::suppress(dx.abs() > dy.abs() * 1.7)
}
#[cfg(debug_assertions)]
pub unsafe fn probe() {
    contacts::probe();
}

fn send_action(generation: u64, action: knit_common::proto::TabletAction) {
    if !NAV_ENABLED.load(Ordering::Relaxed)
        || !WIN_MODE.load(Ordering::Relaxed)
        || !CONNECTED.load(Ordering::Relaxed)
        || generation != OUTBOUND_GENERATION.load(Ordering::SeqCst)
    {
        return;
    }
    // 判定開始時の接続世代を明示し、切替直前のジェスチャーを別端末へ送らない。
    if let Some(tx) = TX.get() {
        let _ = tx.send(outgoing::Queued {
            generation,
            line: encode(&Msg::TabletGesture { action }),
        });
    }
}

pub static ENABLED: AtomicBool = AtomicBool::new(true);
pub static AVAILABLE: AtomicBool = AtomicBool::new(false);
static CAPTURED: AtomicBool = AtomicBool::new(false);
static PORT: OnceLock<usize> = OnceLock::new();
const GESTURE: u32 = 29;

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled && CAPTURED.swap(false, Ordering::Relaxed) {
        send_msg(&Msg::Pinch {
            delta: 0.0,
            phase: 3,
        });
    }
}

pub unsafe fn start() {
    let tap = CGEventTapCreate(1, 0, 0, 1u64 << GESTURE, callback, std::ptr::null_mut());
    if tap.is_null() {
        eprintln!("[trackpad] ピンチの監視を開始できません");
        return;
    }
    let _ = PORT.set(tap as usize);
    let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
    CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
    CGEventTapEnable(tap, true);
    AVAILABLE.store(true, Ordering::Relaxed);
    contacts::start();
}

unsafe extern "C" fn callback(
    _proxy: *mut core::ffi::c_void,
    ty: u32,
    event: CGEventRef,
    _info: *mut core::ffi::c_void,
) -> CGEventRef {
    if ty == 0xFFFFFFFE || ty == 0xFFFFFFFD {
        contacts::reset();
        if let Some(port) = PORT.get() {
            CGEventTapEnable(*port as CFMachPortRef, true);
        }
        if CAPTURED.swap(false, Ordering::Relaxed) {
            send_msg(&Msg::Pinch {
                delta: 0.0,
                phase: 3,
            });
        }
        return event;
    }
    if ty != GESTURE || !remote_active() {
        // 切替・復帰で遠隔操作が途切れた場合も、捕捉中の合成2指を上げてから抜ける
        if remote_active() && CAPTURED.swap(false, Ordering::Relaxed) {
            send_msg(&Msg::Pinch {
                delta: 0.0,
                phase: 3,
            });
        }
        return event;
    }
    let ns = msg1_id(
        objc_getClass(c"NSEvent".as_ptr()),
        sel_registerName(c"eventWithCGEvent:".as_ptr()),
        event,
    );
    if ns.is_null() {
        return event;
    }
    let kind = msg0_isize(ns, sel_registerName(c"type".as_ptr()));
    if kind == 20 && CAPTURED.swap(false, Ordering::Relaxed) {
        send_msg(&Msg::Pinch {
            delta: 0.0,
            phase: 2,
        });
        return std::ptr::null_mut();
    }
    if matches!(kind, 19 | 20 | 31) && contacts::suppress(true) {
        return std::ptr::null_mut();
    }
    if kind != 30 || !ENABLED.load(Ordering::Relaxed) {
        return event;
    }
    let phase = msg0_isize(ns, sel_registerName(c"phase".as_ptr()));
    let get: unsafe extern "C" fn(ID, SEL) -> f64 = std::mem::transmute(objc_msgSend as *const ());
    let delta = get(ns, sel_registerName(c"magnification".as_ptr()));
    if !delta.is_finite() {
        return event;
    }
    let wire_phase = if phase & 16 != 0 {
        3
    } else if phase & 8 != 0 {
        2
    } else if phase & 1 != 0 {
        0
    } else {
        1
    };
    CAPTURED.store(wire_phase < 2, Ordering::Relaxed);
    send_msg(&Msg::Pinch {
        delta: delta.clamp(-0.5, 0.5),
        phase: wire_phase,
    });
    std::ptr::null_mut()
}
