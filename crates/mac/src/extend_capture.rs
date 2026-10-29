use knit_common::video;
use std::ffi::c_void;
use std::sync::{mpsc, Mutex};

#[link(name = "VideoToolbox", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn knit_capture_start(
        id: u32,
        w: u32,
        h: u32,
        fps: u32,
        bitrate: u32,
        callback: unsafe extern "C" fn(
            *mut c_void,
            *const u8,
            usize,
            i32,
            *const u8,
            usize,
            u64,
            bool,
        ) -> bool,
        context: *mut c_void,
    ) -> *mut c_void;
    fn knit_capture_stop(ptr: *mut c_void);
    fn knit_capture_force_key(ptr: *mut c_void);
}

pub struct Frame {
    pub bytes: Vec<u8>,
    pub timestamp: u64,
    pub key: bool,
}
struct State {
    tx: mpsc::SyncSender<Frame>,
    waiting: bool,
    dropped: u64,
}
unsafe extern "C" fn frame(
    context: *mut c_void,
    bytes: *const u8,
    len: usize,
    length_size: i32,
    prefix: *const u8,
    prefix_len: usize,
    timestamp: u64,
    key: bool,
) -> bool {
    if context.is_null()
        || bytes.is_null()
        || len > video::MAX_FRAME
        || prefix_len > video::MAX_FRAME
        || (prefix_len > 0 && prefix.is_null())
    {
        return true;
    }
    let state = &*(context as *const Mutex<State>);
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if video::drop_decision(false, state.waiting, key).0 {
        state.dropped += 1;
        return true;
    }
    let avcc = std::slice::from_raw_parts(bytes, len);
    let Ok(nals) = video::avcc_to_annex_b(avcc, length_size as usize) else {
        state.waiting = true;
        return true;
    };
    if prefix_len.saturating_add(nals.len()) > video::MAX_FRAME {
        state.waiting = true;
        return true;
    }
    let mut data = Vec::with_capacity(prefix_len + nals.len());
    if prefix_len != 0 {
        data.extend_from_slice(std::slice::from_raw_parts(prefix, prefix_len));
    }
    data.extend_from_slice(&nals);
    let frame = Frame {
        bytes: data,
        timestamp,
        key,
    };
    match state.tx.try_send(frame) {
        Ok(()) => {
            state.waiting = video::drop_decision(false, state.waiting, key).1;
            false
        }
        Err(_) => {
            state.waiting = video::drop_decision(true, state.waiting, key).1;
            state.dropped += 1;
            if state.dropped == 1 || state.dropped % 120 == 0 {
                eprintln!("[extend] 混雑による破棄: {} フレーム", state.dropped);
            }
            true
        }
    }
}
pub struct Capture {
    ptr: *mut c_void,
    state: Box<Mutex<State>>,
}
impl Capture {
    pub fn start(
        id: u32,
        w: u32,
        h: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<(Self, mpsc::Receiver<Frame>), String> {
        if w == 0
            || h == 0
            || fps == 0
            || w > i32::MAX as u32
            || h > i32::MAX as u32
            || fps > i32::MAX as u32
            || bitrate == 0
        {
            return Err("動画設定が不正です".into());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let state = Box::new(Mutex::new(State {
            tx,
            waiting: true,
            dropped: 0,
        }));
        let ptr = unsafe {
            knit_capture_start(
                id,
                w,
                h,
                fps,
                bitrate,
                frame,
                &*state as *const _ as *mut c_void,
            )
        };
        if ptr.is_null() {
            return Err("画面の取り込みまたは H.264 符号化を開始できません".into());
        }
        unsafe {
            knit_capture_force_key(ptr);
        }
        Ok((Self { ptr, state }, rx))
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        unsafe {
            knit_capture_stop(self.ptr);
        }
        let dropped = self.state.lock().unwrap_or_else(|e| e.into_inner()).dropped;
        eprintln!("[extend] 破棄したフレーム: {dropped} 枚");
    }
}
