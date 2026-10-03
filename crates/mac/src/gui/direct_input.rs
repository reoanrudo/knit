//! Captured keyboard events are interpreted by the native IME on the main run loop.
use knit_common::proto::Msg;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    id: String,
    generation: u64,
    focus: u64,
}
struct Captured {
    target: Target,
    event: usize,
}
static EVENTS: Mutex<VecDeque<Captured>> = Mutex::new(VecDeque::new());
static TARGET: Mutex<Option<(u64, Target)>> = Mutex::new(None);
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
static FOCUS: AtomicU64 = AtomicU64::new(1);
pub(crate) fn new_session() {
    FOCUS.fetch_add(1, Ordering::SeqCst);
}

unsafe extern "C" {
    fn CGEventCreateCopy(event: crate::CGEventRef) -> crate::CGEventRef;
    fn knit_direct_input_start(
        epoch: u64,
        commit: extern "C" fn(u64, *const u8, usize),
        key: extern "C" fn(u64, u16, u64),
    );
    fn knit_direct_input_stop();
    fn knit_direct_input_event(event: crate::CGEventRef);
    fn knit_direct_input_ready() -> bool;
    fn knit_direct_input_composing() -> bool;
    fn knit_direct_input_finish();
    #[cfg(debug_assertions)]
    fn knit_direct_input_probe() -> bool;
}

fn desired_target() -> Option<Target> {
    if super::UI_PREVIEW.load(Ordering::Relaxed)
        || !crate::WIN_MODE.load(Ordering::Relaxed)
        || !crate::CONNECTED.load(Ordering::Relaxed)
    {
        return None;
    }
    let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let active = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    let p = peers.get(active)?;
    (p.id.starts_with("android-app-")
        && !crate::android::app::japanese(&p.id)
        && crate::android::app::get(&p.id).is_some_and(|(_, keyboard)| keyboard))
    .then(|| Target {
        id: p.id.clone(),
        generation: p.gen,
        focus: FOCUS.load(Ordering::SeqCst),
    })
}

/// Only copies an event; never calls AppKit inside the event tap.
pub(crate) unsafe fn capture(event: crate::CGEventRef) -> Option<bool> {
    let Some(target) = desired_target() else {
        return None;
    };
    // The tap runs on the main CFRunLoop. Allow original hardware events to
    // reach the native key window: input methods may reject synthetic events.
    let matching = TARGET
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|(_, t)| *t == target);
    if matching && knit_direct_input_ready() {
        return Some(true);
    }
    let mut queue = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    // Bound memory if a modal dialog pauses the main run loop. Drop, never type locally.
    if queue.len() < 256 {
        let copy = CGEventCreateCopy(event);
        if !copy.is_null() {
            queue.push_back(Captured {
                target,
                event: copy as usize,
            });
        }
    }
    Some(false)
}

/// Allows the IME's Ctrl-J/K/L conversions while a composition is active.
pub(crate) unsafe fn composing() -> bool {
    knit_direct_input_composing()
}
pub(crate) unsafe fn finish() {
    knit_direct_input_finish();
}

fn accepts(session: Option<&(u64, Target)>, epoch: u64, current: Option<&Target>) -> bool {
    session.is_some_and(|(e, target)| *e == epoch && Some(target) == current)
}

fn send(epoch: u64, msg: &Msg) -> bool {
    let _change = crate::PEER_CHANGES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let current = desired_target();
    let target = TARGET.lock().unwrap_or_else(|e| e.into_inner());
    if accepts(target.as_ref(), epoch, current.as_ref()) {
        crate::send_msg_reported(msg)
    } else {
        false
    }
}

extern "C" fn committed(epoch: u64, bytes: *const u8, len: usize) {
    if bytes.is_null() || len == 0 || len > 1024 * 1024 {
        return;
    }
    if let Ok(text) = std::str::from_utf8(unsafe { std::slice::from_raw_parts(bytes, len) }) {
        if send(epoch, &Msg::Text { text: text.into() }) {
            eprintln!("[direct-ime] committed UTF-8 ({len} bytes)");
        }
    }
}
extern "C" fn editing_key(epoch: u64, kc: u16, flags: u64) {
    for down in [true, false] {
        send(
            epoch,
            &Msg::Key {
                kc,
                down,
                ctrl: flags & crate::FLAG_CTRL != 0,
                opt: flags & crate::FLAG_OPT != 0,
                cmd: flags & crate::FLAG_CMD != 0,
                shift: flags & crate::FLAG_SHIFT != 0,
                tr: false,
                rcmd: false,
            },
        );
    }
}

/// Main thread only: called by the GUI's 16 ms timer.
pub(super) unsafe extern "C" fn poll(s: super::ID, c: super::SEL, sender: super::ID) {
    let desired = desired_target();
    let changed = {
        let mut session = TARGET.lock().unwrap_or_else(|e| e.into_inner());
        if session.as_ref().map(|(_, t)| t) == desired.as_ref() {
            false
        } else {
            *session = desired
                .clone()
                .map(|t| (NEXT_EPOCH.fetch_add(1, Ordering::Relaxed), t));
            true
        }
    };
    if changed {
        let epoch = TARGET
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(e, _)| *e);
        if let Some(epoch) = epoch {
            knit_direct_input_start(epoch, committed, editing_key);
            eprintln!("[direct-ime] native input session opened");
        } else {
            knit_direct_input_stop();
            eprintln!("[direct-ime] native input session closed");
        }
    }
    let events: Vec<_> = EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
        .collect();
    for item in events {
        if desired.as_ref() == Some(&item.target) && desired_target().as_ref() == Some(&item.target)
        {
            knit_direct_input_event(item.event as crate::CGEventRef);
        }
        crate::CFRelease(item.event as *mut core::ffi::c_void);
    }
    crate::incoming_drag::poll(s, c, sender);
}

#[cfg(debug_assertions)]
pub(crate) unsafe fn probe() {
    assert!(
        knit_direct_input_probe(),
        "native IME composition/commit regression"
    );
    eprintln!(
        "[direct-ime] native composition, Unicode commits, cancellation and editing keys passed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composition_cannot_cross_device_reconnect_or_leave_and_reenter() {
        let old = Target {
            id: "android-app-a".into(),
            generation: 1,
            focus: 1,
        };
        let session = (7, old.clone());
        assert!(accepts(Some(&session), 7, Some(&old)));
        assert!(!accepts(Some(&session), 7, None));
        assert!(!accepts(Some(&session), 6, Some(&old)));
        assert!(!accepts(
            Some(&session),
            7,
            Some(&Target {
                generation: 2,
                ..old.clone()
            })
        ));
        assert!(!accepts(
            Some(&session),
            7,
            Some(&Target {
                focus: 2,
                ..old.clone()
            })
        ));
        assert!(!accepts(
            Some(&session),
            7,
            Some(&Target {
                id: "android-app-b".into(),
                ..old
            })
        ));
    }
}
