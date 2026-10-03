//! 指の位置は公開NSEventではグローバルに取得できないため、任意ロードの端末層に閉じ込める。
use super::{
    navigation::{Point, Recognizer},
    *,
};
use std::sync::atomic::AtomicBool;
use std::{collections::BTreeMap, ffi::c_void};

pub static AVAILABLE: AtomicBool = AtomicBool::new(false);
static DEVICES: Mutex<BTreeMap<usize, State>> = Mutex::new(BTreeMap::new());
struct State {
    recognizer: Recognizer,
    generation: u64,
    ready: bool,
    last_ms: u64,
    capture_until: u64,
    horizontal_only: bool,
    last_count: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            recognizer: Recognizer::default(),
            generation: u64::MAX,
            ready: false,
            last_ms: 0,
            capture_until: 0,
            horizontal_only: false,
            last_count: 0,
        }
    }
}
impl State {
    fn reset(&mut self) {
        self.recognizer.reset();
        self.capture_until = 0;
        self.ready = self.last_count == 0;
    }
    fn update(
        &mut self,
        active: bool,
        generation: u64,
        points: &[Point],
        now: u64,
    ) -> Option<knit_common::proto::TabletAction> {
        if !active {
            self.last_count = points.len();
            self.reset();
            self.generation = generation;
            return None;
        }
        if generation != self.generation {
            self.reset();
            self.generation = generation;
        }
        self.last_count = points.len();
        if !self.ready {
            self.ready = points.is_empty();
            return None;
        }
        self.last_ms = now;
        let action = self.recognizer.frame(points, now);
        if self.recognizer.suppress_scroll(true) {
            self.capture_until = now + 300;
            self.horizontal_only = !self.recognizer.suppress_scroll(false);
        }
        action
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Vector {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}
#[repr(C)]
struct Contact {
    frame: i32,
    time: f64,
    path: i32,
    state: i32,
    id: i32,
    hand: i32,
    normalized: Vector,
    total: f32,
    field9: i32,
    angle: f32,
    major: f32,
    minor: f32,
    absolute: Vector,
    field14: i32,
    field15: i32,
    density: f32,
}
type Device = *mut c_void;
type Callback = unsafe extern "C" fn(Device, *const Contact, i32, f64, i32) -> i32;
type Create = unsafe extern "C" fn() -> *mut c_void;
type Register = unsafe extern "C" fn(Device, Callback);
type Start = unsafe extern "C" fn(Device, i32);
type Dimensions = unsafe extern "C" fn(Device, *mut i32, *mut i32) -> i32;
unsafe extern "C" {
    fn dlopen(path: *const i8, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const i8) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> i32;
    fn CFArrayGetCount(array: *mut c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *mut c_void, index: isize) -> Device;
}
struct Runtime {
    handle: *mut c_void,
    list: *mut c_void,
    devices: Vec<Device>,
    register: Register,
    start: Start,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.list);
            dlclose(self.handle);
        }
    }
}
unsafe fn load() -> Option<Runtime> {
    let handle = dlopen(
        c"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport".as_ptr(),
        2,
    );
    if handle.is_null() {
        return None;
    }
    let symbols = [
        c"MTDeviceCreateList",
        c"MTRegisterContactFrameCallback",
        c"MTDeviceStart",
        c"MTDeviceGetSensorDimensions",
    ]
    .map(|s| dlsym(handle, s.as_ptr()));
    if symbols.iter().any(|s| s.is_null()) {
        dlclose(handle);
        return None;
    }
    let create: Create = std::mem::transmute(symbols[0]);
    let dimensions: Dimensions = std::mem::transmute(symbols[3]);
    let list = create();
    if list.is_null() {
        dlclose(handle);
        return None;
    }
    let n = CFArrayGetCount(list);
    if !(0..=32).contains(&n) {
        CFRelease(list);
        dlclose(handle);
        return None;
    }
    let devices = (0..n)
        .map(|i| CFArrayGetValueAtIndex(list, i))
        .filter(|d| {
            if d.is_null() {
                return false;
            }
            let (mut rows, mut cols) = (0, 0);
            dimensions(*d, &mut rows, &mut cols) == 0
                && (10..=128).contains(&rows)
                && (10..=128).contains(&cols)
        })
        .collect();
    Some(Runtime {
        handle,
        list,
        devices,
        register: std::mem::transmute(symbols[1]),
        start: std::mem::transmute(symbols[2]),
    })
}
pub unsafe fn start() {
    let Some(runtime) = load() else {
        eprintln!("[trackpad] 指の位置を取得できません。ピンチのみ利用できます");
        return;
    };
    for device in &runtime.devices {
        (runtime.register)(*device, frame);
        (runtime.start)(*device, 0);
    }
    AVAILABLE.store(!runtime.devices.is_empty(), Ordering::Relaxed);
    eprintln!(
        "[trackpad] ジェスチャー監視: トラックパッド {} 台",
        runtime.devices.len()
    );
    // フレーム通知が続く間、デバイスとコールバックのライブラリを保持する。
    std::mem::forget(runtime);
    if AVAILABLE.load(Ordering::Relaxed) {
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_millis(30));
            poll();
        });
    }
}
#[cfg(debug_assertions)]
pub unsafe fn probe() {
    let Some(runtime) = load() else {
        println!("contacts unavailable");
        return;
    };
    println!(
        "contacts available: {} trackpad(s), contact size {}",
        runtime.devices.len(),
        std::mem::size_of::<Contact>()
    );
}
fn poll() {
    if !navigation_active() {
        return;
    }
    let now = now_ms();
    let generation = OUTBOUND_GENERATION.load(Ordering::SeqCst);
    let mut devices = DEVICES.lock().unwrap_or_else(|e| e.into_inner());
    for state in devices
        .values_mut()
        .filter(|s| s.ready && s.generation == generation)
    {
        if let Some(action) = state.recognizer.tick(now) {
            send_action(generation, action);
        }
        if state.recognizer.suppress_scroll(true) {
            state.capture_until = now + 150;
            state.horizontal_only = !state.recognizer.suppress_scroll(false);
        }
    }
}
pub fn reset() {
    for state in DEVICES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values_mut()
    {
        state.reset();
    }
}
pub fn suppress(horizontal: bool) -> bool {
    if !navigation_active() {
        return false;
    }
    let now = now_ms();
    let generation = OUTBOUND_GENERATION.load(Ordering::SeqCst);
    DEVICES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .any(|s| {
            s.generation == generation
                && (s.recognizer.suppress_scroll(horizontal) && now.saturating_sub(s.last_ms) < 200
                    || now < s.capture_until && (horizontal || !s.horizontal_only))
        })
}
unsafe extern "C" fn frame(
    device: Device,
    data: *const Contact,
    count: i32,
    _time: f64,
    _frame: i32,
) -> i32 {
    if !(0..=16).contains(&count) || count > 0 && data.is_null() {
        return 0;
    }
    let points: Vec<Point> = if count == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(data, count as usize)
            .iter()
            .filter(|p| p.state == 3 || p.state == 4)
            .map(|p| Point {
                id: p.path,
                x: p.normalized.x as f64,
                y: p.normalized.y as f64,
            })
            .collect()
    };
    let now = now_ms();
    let generation = OUTBOUND_GENERATION.load(Ordering::SeqCst);
    let active = navigation_active();
    {
        let mut devices = DEVICES.lock().unwrap_or_else(|e| e.into_inner());
        let state = devices.entry(device as usize).or_default();
        let action = state.update(active, generation, &points, now);
        // 復帰時のreset_sessionより前に送信キューへ入れる。Leaveより後へ持ち越さない。
        if let Some(action) = action {
            send_action(generation, action);
        }
    }
    0
}
#[cfg(test)]
mod tests {
    use super::*;
    fn points(y: f64) -> Vec<Point> {
        (0..3)
            .map(|id| Point {
                id,
                x: 0.4 + id as f64 * 0.03,
                y,
            })
            .collect()
    }
    #[test]
    fn fresh_simultaneous_contacts_work_after_mode_reset() {
        let mut state = State::default();
        state.update(false, 1, &[], 0);
        state.reset();
        state.update(true, 1, &points(0.3), 20);
        // 0.56 まで動かす: Home は「下向きスワイプ(navigation::SWIPE 以上)の
        // 離し」なので、閾値未満の移動では発火しない
        state.update(true, 1, &points(0.56), 60);
        assert_eq!(
            state.update(true, 1, &[], 100),
            Some(knit_common::proto::TabletAction::Home)
        );
    }
    #[test]
    fn held_contacts_are_cancelled_on_peer_or_mode_switch() {
        for change_peer in [false, true] {
            let mut state = State::default();
            state.update(true, 1, &points(0.3), 0);
            state.update(true, 1, &points(0.56), 50);
            if !change_peer {
                state.reset();
            }
            let generation = if change_peer { 2 } else { 1 };
            assert_eq!(state.update(true, generation, &points(0.55), 80), None);
            assert_eq!(state.update(true, generation, &[], 100), None);
            state.update(true, generation, &points(0.3), 200);
            state.update(true, generation, &points(0.56), 240);
            assert_eq!(
                state.update(true, generation, &[], 280),
                Some(knit_common::proto::TabletAction::Home)
            );
        }
    }
    #[test]
    fn contact_layout_matches_the_runtime_abi() {
        assert_eq!(std::mem::size_of::<Contact>(), 96);
        assert_eq!(std::mem::offset_of!(Contact, state), 20);
        assert_eq!(std::mem::offset_of!(Contact, normalized), 32);
        assert_eq!(std::mem::offset_of!(Contact, absolute), 68);
    }
}
