//! macOS の電源通知を接続制御に反映し、一時的な復帰中の再接続を抑える。
//! 仕様: docs/cleanroom/2026-10-29-mac-sleep-gate.md

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum State {
    Awake,
    Asleep,
    DarkWake,
}
#[derive(Clone, Copy)]
enum Event {
    Sleep,
    PoweredOn { display_asleep: bool },
    ScreenWake,
    Timeout,
}

// OS の呼び出しとは独立して状態遷移を決める。
fn next(state: State, event: Event) -> State {
    match event {
        Event::Sleep => State::Asleep,
        Event::PoweredOn {
            display_asleep: true,
        } => State::DarkWake,
        Event::PoweredOn {
            display_asleep: false,
        } => State::Awake,
        Event::ScreenWake | Event::Timeout if state == State::DarkWake => State::Awake,
        _ => state,
    }
}
fn connect_allowed(state: State, disabled: bool) -> bool {
    disabled || state == State::Awake
}
fn dark_timed_out(elapsed: Duration) -> bool {
    elapsed >= Duration::from_secs(600)
}
static STATE: AtomicU8 = AtomicU8::new(State::Awake as u8);
static ROOT: AtomicU32 = AtomicU32::new(0);
static SLEEP_MS: AtomicU64 = AtomicU64::new(0);
static LAST_WAKE_MS: AtomicU64 = AtomicU64::new(0);
static REJECT_LOGGED: AtomicBool = AtomicBool::new(false);
fn state() -> State {
    match STATE.load(Ordering::SeqCst) {
        1 => State::Asleep,
        2 => State::DarkWake,
        _ => State::Awake,
    }
}
fn gate_disabled(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim() == "1")
}
fn disabled() -> bool {
    gate_disabled(knit_common::envutil::get("KNIT_NO_SLEEP_GATE").as_deref())
}
pub(crate) fn should_connect() -> bool {
    connect_allowed(state(), disabled())
}
pub(crate) fn log_rejected() {
    if !REJECT_LOGGED.swap(true, Ordering::SeqCst) {
        eprintln!("[power] 画面が消えている間は接続を受け付けません");
    }
}
pub(crate) fn clock_wake_needs_reconnect() -> bool {
    let last = LAST_WAKE_MS.load(Ordering::SeqCst);
    (disabled() || state() == State::Awake)
        && (last == 0 || crate::state::wall_ms().saturating_sub(last) > 10_000)
}
// 複数スレッドからの遷移を原子的に反映し、復帰時に待機中の接続を起こす。
fn transition(event: Event) {
    loop {
        let old = state();
        let new = next(old, event);
        if STATE
            .compare_exchange(old as u8, new as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            continue;
        }
        if old != State::Awake && new == State::Awake {
            let now = crate::state::wall_ms();
            LAST_WAKE_MS.store(now, Ordering::SeqCst);
            REJECT_LOGGED.store(false, Ordering::SeqCst);
            let slept = now.saturating_sub(SLEEP_MS.load(Ordering::SeqCst)) / 1000;
            eprintln!("[conn-metric] wake unix_ms={now} slept_s={slept} source=iokit");
            crate::WAKE.notify();
        }
        break;
    }
}

// IOMessage.h の iokit_common_msg は sys_iokit | sub_iokit_common | message。
// IOReturn.h と mach/error.h により、それぞれ (0x38 << 26) と (0 << 14)。
const IOKIT_COMMON: u32 = (0x38_u32 << 26) | (0_u32 << 14);
// IOMessage.h: kIOMessageCanSystemSleep
const CAN_SYSTEM_SLEEP: u32 = IOKIT_COMMON | 0x270;
// IOMessage.h: kIOMessageSystemWillSleep
const SYSTEM_WILL_SLEEP: u32 = IOKIT_COMMON | 0x280;
// IOMessage.h: kIOMessageSystemWillPowerOn
const SYSTEM_WILL_POWER_ON: u32 = IOKIT_COMMON | 0x320;
// IOMessage.h: kIOMessageSystemHasPoweredOn
const SYSTEM_HAS_POWERED_ON: u32 = IOKIT_COMMON | 0x300;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IORegisterForSystemPower(
        ctx: *mut c_void,
        port: *mut *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void),
        notifier: *mut u32,
    ) -> u32;
    fn IOAllowPowerChange(root: u32, id: isize) -> i32;
    fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> *mut c_void;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(runloop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRun();
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}
fn display_asleep() -> bool {
    // SAFETY: CoreGraphics が返すディスプレイ ID をそのまま照会する。
    unsafe { CGDisplayIsAsleep(CGMainDisplayID()) != 0 }
}
// 電源通知を状態遷移へ変換し、応答が必要な通知だけ OS に応答する。
// SAFETY: IOKit が登録時の署名に従って呼び出し、argument に通知 ID を渡す。
unsafe extern "C" fn callback(_: *mut c_void, _: u32, message: u32, argument: *mut c_void) {
    match message {
        CAN_SYSTEM_SLEEP => {
            IOAllowPowerChange(ROOT.load(Ordering::SeqCst), argument as isize);
        }
        SYSTEM_WILL_SLEEP => {
            SLEEP_MS.store(crate::state::wall_ms(), Ordering::SeqCst);
            transition(Event::Sleep);
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                crate::drop_stream("スリープに入るため");
                let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
                for peer in peers.iter() {
                    if let Some(socket) = &peer.shut {
                        let _ = socket.shutdown(std::net::Shutdown::Both);
                    }
                }
                eprintln!("[power] スリープに入ります(接続を閉じました)");
                let _ = done_tx.send(());
            });
            if done_rx.recv_timeout(Duration::from_secs(1)).is_err() {
                eprintln!("[power] 接続の切断を待ちきれなかったためスリープを許可します");
            }
            IOAllowPowerChange(ROOT.load(Ordering::SeqCst), argument as isize);
        }
        SYSTEM_WILL_POWER_ON => eprintln!("[power] 復帰を開始しました"),
        SYSTEM_HAS_POWERED_ON => transition(Event::PoweredOn {
            display_asleep: !disabled() && display_asleep(),
        }),
        _ => {}
    }
}
// 専用 run loop で電源通知を受け、別スレッドで画面の点灯を監視する。
pub(crate) fn start() {
    // SAFETY: 登録に必要な出力先はこのスレッド内で有効な値を渡し、
    // 成功した通知ポートと run loop source は run loop の実行中保持する。
    std::thread::spawn(|| unsafe {
        let mut port = std::ptr::null_mut();
        let mut notifier = 0;
        let root =
            IORegisterForSystemPower(std::ptr::null_mut(), &mut port, callback, &mut notifier);
        if root == 0 || port.is_null() {
            eprintln!("[power] 電源通知の登録に失敗");
            return;
        }
        ROOT.store(root, Ordering::SeqCst);
        let source = IONotificationPortGetRunLoopSource(port);
        if source.is_null() {
            eprintln!("[power] 電源通知の登録に失敗");
            return;
        }
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
        std::thread::spawn(|| {
            let mut dark_since: Option<Instant> = None;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                if state() != State::DarkWake {
                    dark_since = None;
                    continue;
                }
                let since = dark_since.get_or_insert_with(Instant::now);
                if disabled() || !display_asleep() {
                    transition(Event::ScreenWake);
                } else if dark_timed_out(since.elapsed()) {
                    eprintln!("[power] 一時的な目覚めが10分続いたため復帰扱いにします");
                    transition(Event::Timeout);
                }
            }
        });
        CFRunLoopRun();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sleep_dark_wake_and_screen_wake() {
        let s = next(State::Awake, Event::Sleep);
        let s = next(
            s,
            Event::PoweredOn {
                display_asleep: true,
            },
        );
        assert_eq!(s, State::DarkWake);
        assert!(!connect_allowed(s, false));
        assert_eq!(next(s, Event::ScreenWake), State::Awake);
    }
    #[test]
    fn ordinary_wake_and_timeout() {
        assert_eq!(
            next(
                State::Asleep,
                Event::PoweredOn {
                    display_asleep: false
                }
            ),
            State::Awake
        );
        assert_eq!(next(State::DarkWake, Event::Timeout), State::Awake);
        assert_eq!(next(State::Asleep, Event::Timeout), State::Asleep);
    }
    #[test]
    fn sleep_applies_from_every_state_and_timeout_has_a_boundary() {
        for state in [State::Awake, State::Asleep, State::DarkWake] {
            assert_eq!(next(state, Event::Sleep), State::Asleep);
            assert_eq!(
                next(
                    state,
                    Event::PoweredOn {
                        display_asleep: true
                    }
                ),
                State::DarkWake
            );
            assert_eq!(
                next(
                    state,
                    Event::PoweredOn {
                        display_asleep: false
                    }
                ),
                State::Awake
            );
        }
        assert_eq!(next(State::Asleep, Event::ScreenWake), State::Asleep);
        assert_eq!(next(State::Awake, Event::ScreenWake), State::Awake);
        assert!(!dark_timed_out(Duration::from_secs(599)));
        assert!(dark_timed_out(Duration::from_secs(600)));
    }
    #[test]
    fn system_power_message_values_match_sdk() {
        assert_eq!(CAN_SYSTEM_SLEEP, 0xe0000270);
        assert_eq!(SYSTEM_WILL_SLEEP, 0xe0000280);
        assert_eq!(SYSTEM_WILL_POWER_ON, 0xe0000320);
        assert_eq!(SYSTEM_HAS_POWERED_ON, 0xe0000300);
    }
    #[test]
    fn gate_disabled_only_for_trimmed_one() {
        assert!(gate_disabled(Some("1")));
        assert!(gate_disabled(Some(" \t1\n")));
        for value in [
            None,
            Some(""),
            Some(" "),
            Some("0"),
            Some("true"),
            Some("01"),
        ] {
            assert!(!gate_disabled(value));
        }
    }
    #[test]
    fn disabled_gate_allows_connections() {
        assert!(connect_allowed(State::Asleep, true));
        assert!(connect_allowed(State::DarkWake, true));
        assert!(!connect_allowed(State::Asleep, false));
    }
}
