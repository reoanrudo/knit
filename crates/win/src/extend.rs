#![allow(non_snake_case)]

use knit_common::video;
use std::mem::ManuallyDrop;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Foundation::VARIANT_BOOL;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
}

const WM_EXTEND_SHOW: u32 = WM_APP + 1;
struct Image {
    w: u32,
    h: u32,
    pixels: Vec<u8>,
}
struct ImageSlot {
    image: Option<Image>,
    dirty: bool,
}
static IMAGE: Mutex<ImageSlot> = Mutex::new(ImageSlot {
    image: None,
    dirty: false,
});
static ESC_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static VISIBLE: AtomicBool = AtomicBool::new(true);
static DROPPED: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    knit_common::envutil::get("KNIT_EXTEND").as_deref() == Some("1")
}
pub fn show() {
    VISIBLE.store(true, Ordering::Relaxed);
    let hwnd = WINDOW.load(Ordering::Relaxed) as HWND;
    if !hwnd.is_null() {
        unsafe {
            PostMessageW(hwnd, WM_EXTEND_SHOW, 0, 0);
        }
    }
}

pub fn start(token: String, control_port: u16) {
    if !enabled() {
        return;
    }
    let Some(port) = control_port.checked_add(video::PORT_OFFSET) else {
        eprintln!("[extend] 動画ポートが範囲外です");
        return;
    };
    std::thread::spawn(window_loop);
    std::thread::spawn(move || receiver(token, port));
}

fn receiver(token: String, port: u16) {
    unsafe {
        if let Err(e) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            eprintln!("[extend] COM 初期化失敗: {e}");
            return;
        }
        if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
            eprintln!("[extend] Media Foundation 初期化失敗: {e}");
            CoUninitialize();
            return;
        }
    }
    loop {
        if !crate::CONNECTED.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        let epoch = crate::conn::CONNECTION_EPOCH.load(Ordering::Acquire);
        let Some(ip) = crate::peer_ip() else {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        let addr = SocketAddr::new(ip, port);
        let stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[extend] 接続失敗({addr}): {e}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
        let guard = match stream.try_clone() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[extend] ソケット複製失敗: {e}");
                continue;
            }
        };
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let watch_done = done.clone();
        let watcher = std::thread::spawn(move || {
            while !watch_done.load(Ordering::Acquire) {
                if !crate::CONNECTED.load(Ordering::Acquire)
                    || crate::peer_ip() != Some(ip)
                    || crate::conn::CONNECTION_EPOCH.load(Ordering::Acquire) != epoch
                {
                    ACTIVE.store(false, Ordering::Release);
                    let _ = guard.shutdown(Shutdown::Both);
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let result = receive_stream(stream, &token, epoch);
        ACTIVE.store(false, Ordering::Release);
        {
            let mut slot = IMAGE.lock().unwrap_or_else(|e| e.into_inner());
            slot.image = None;
            slot.dirty = false;
        }
        done.store(true, Ordering::Release);
        let _ = watcher.join();
        if let Err(e) = result {
            eprintln!("[extend] 動画接続終了: {e}");
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn receive_stream(stream: TcpStream, token: &str, epoch: u64) -> Result<(), String> {
    let (mut reader, _writer) =
        knit_common::secure::connect(stream, token, b"knit-video").map_err(|e| e.to_string())?;
    let first = video::read_frame(&mut reader).map_err(|e| e.to_string())?;
    if first.kind != video::CONFIG {
        return Err("最初の動画フレームが設定ではありません".into());
    }
    let config: video::Config = serde_json::from_slice(&first.body).map_err(|e| e.to_string())?;
    if config.codec != "h264"
        || config.w == 0
        || config.h == 0
        || config.w > 8192
        || config.h > 8192
        || config.w % 2 != 0
        || config.h % 2 != 0
        || config.fps == 0
    {
        return Err("対応できない動画設定です".into());
    }
    let mut decoder = unsafe { Decoder::new(&config).map_err(|e| e.to_string())? };
    if !crate::CONNECTED.load(Ordering::Acquire)
        || crate::conn::CONNECTION_EPOCH.load(Ordering::Acquire) != epoch
    {
        return Err("制御接続が切れました".into());
    }
    VISIBLE.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Release);
    let mut waiting = true;
    let mut received = 0u64;
    let mut failures = 0u32;
    let mut decoded = 0u64;
    let mut elapsed = Duration::ZERO;
    let mut last = Instant::now();
    DROPPED.store(0, Ordering::Relaxed);
    loop {
        let frame = video::read_frame(&mut reader).map_err(|e| e.to_string())?;
        if frame.kind != video::KEYFRAME && frame.kind != video::DELTA {
            return Err("予期しない動画フレームです".into());
        }
        received += 1;
        if waiting && frame.kind != video::KEYFRAME {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        } else {
            let now = Instant::now();
            match unsafe { decoder.decode(&frame.body, frame.timestamp_us) } {
                Ok(images) => {
                    failures = 0;
                    waiting = false;
                    elapsed += now.elapsed();
                    decoded += 1;
                    for image in images {
                        let mut slot = IMAGE.lock().unwrap_or_else(|e| e.into_inner());
                        if slot.dirty {
                            DROPPED.fetch_add(1, Ordering::Relaxed);
                        }
                        slot.image = Some(image);
                        slot.dirty = true;
                        drop(slot);
                        let hwnd = WINDOW.load(Ordering::Relaxed) as HWND;
                        if !hwnd.is_null() {
                            unsafe {
                                InvalidateRect(hwnd, std::ptr::null(), 0);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[extend] 復号失敗: {e}");
                    DROPPED.fetch_add(1, Ordering::Relaxed);
                    failures += 1;
                    if failures >= 3 {
                        waiting = true;
                        failures = 0;
                        unsafe {
                            decoder = Decoder::new(&config).map_err(|e| e.to_string())?;
                        }
                    }
                }
            }
        }
        if last.elapsed() >= Duration::from_secs(10) {
            let avg = if decoded == 0 {
                0.0
            } else {
                elapsed.as_secs_f64() * 1000.0 / decoded as f64
            };
            println!(
                "[extend] 受信 {received} / 破棄 {} / 復号 {avg:.2} ms",
                DROPPED.swap(0, Ordering::Relaxed)
            );
            received = 0;
            decoded = 0;
            elapsed = Duration::ZERO;
            last = Instant::now();
        }
    }
}

struct Decoder {
    mft: IMFTransform,
    w: u32,
    h: u32,
    stride: usize,
}
impl Decoder {
    unsafe fn new(config: &video::Config) -> windows::core::Result<Self> {
        let mft: IMFTransform = CoCreateInstance(&CMSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)?;
        let low_latency = mft.cast::<ICodecAPI>().and_then(|api| {
            let mut value = VARIANT::default();
            (*value.Anonymous.Anonymous).vt = VT_BOOL;
            (*value.Anonymous.Anonymous).Anonymous.boolVal = VARIANT_BOOL(-1);
            api.SetValue(&CODECAPI_AVLowLatencyMode, &value)
        });
        if let Err(e) = low_latency {
            eprintln!("[extend] 低遅延モードを設定できません: {e}");
        }
        let input = MFCreateMediaType()?;
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        input.SetUINT64(
            &MF_MT_FRAME_SIZE,
            ((config.w as u64) << 32) | config.h as u64,
        )?;
        input.SetUINT64(&MF_MT_FRAME_RATE, ((config.fps as u64) << 32) | 1)?;
        mft.SetInputType(0, &input, 0)?;
        let mut decoder = Self {
            mft,
            w: config.w,
            h: config.h,
            stride: config.w as usize,
        };
        decoder.configure_output()?;
        decoder
            .mft
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
        decoder
            .mft
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        Ok(decoder)
    }
    unsafe fn configure_output(&mut self) -> windows::core::Result<()> {
        let mut index = 0;
        loop {
            let ty = self.mft.GetOutputAvailableType(0, index)?;
            if ty.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                self.mft.SetOutputType(0, &ty, 0)?;
                let size = ty
                    .GetUINT64(&MF_MT_FRAME_SIZE)
                    .unwrap_or(((self.w as u64) << 32) | self.h as u64);
                self.w = (size >> 32) as u32;
                self.h = size as u32;
                self.stride = ty.GetUINT32(&MF_MT_DEFAULT_STRIDE).unwrap_or(self.w) as usize;
                return Ok(());
            }
            index += 1;
        }
    }
    unsafe fn decode(&mut self, bytes: &[u8], timestamp: u64) -> windows::core::Result<Vec<Image>> {
        let sample = MFCreateSample()?;
        let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
        let mut data = std::ptr::null_mut();
        buffer.Lock(&mut data, None, None)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp.saturating_mul(10).min(i64::MAX as u64) as i64)?;
        self.mft.ProcessInput(0, &sample, 0)?;
        let mut images = Vec::new();
        loop {
            let info = self.mft.GetOutputStreamInfo(0)?;
            let provided = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            let sample = if provided {
                None
            } else {
                let out = MFCreateSample()?;
                let buffer = if info.cbAlignment > 1 {
                    MFCreateAlignedMemoryBuffer(info.cbSize, info.cbAlignment - 1)?
                } else {
                    MFCreateMemoryBuffer(info.cbSize)?
                };
                out.AddBuffer(&buffer)?;
                Some(out)
            };
            let mut output = [MFT_OUTPUT_DATA_BUFFER {
                pSample: ManuallyDrop::new(sample),
                ..Default::default()
            }];
            let mut status = 0;
            let result = self.mft.ProcessOutput(0, &mut output, &mut status);
            let output_sample = ManuallyDrop::take(&mut output[0].pSample);
            let events = ManuallyDrop::take(&mut output[0].pEvents);
            drop(events);
            match result {
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.configure_output()?;
                    continue;
                }
                Err(e) => return Err(e),
                Ok(()) => {}
            }
            if let Some(sample) = output_sample {
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut data = std::ptr::null_mut();
                let mut len = 0;
                buffer.Lock(&mut data, None, Some(&mut len))?;
                let image = self.image_from_nv12(std::slice::from_raw_parts(data, len as usize));
                buffer.Unlock()?;
                if let Some(image) = image {
                    images.push(image);
                } else {
                    return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                        0x8007000du32 as i32,
                    )));
                }
            }
        }
        Ok(images)
    }
    fn image_from_nv12(&self, data: &[u8]) -> Option<Image> {
        let w = self.w as usize;
        let h = self.h as usize;
        let stride = self.stride;
        if w == 0
            || h == 0
            || w > 8192
            || h > 8192
            || w % 2 != 0
            || h % 2 != 0
            || stride < w
            || stride > 32768
        {
            return None;
        }
        let needed = stride.checked_mul(h.checked_add(h / 2)?)?;
        if data.len() < needed {
            return None;
        }
        let mut tight = Vec::with_capacity(w * (h + h / 2));
        for row in 0..h + h / 2 {
            tight.extend_from_slice(&data[row * stride..row * stride + w]);
        }
        Some(Image {
            w: self.w,
            h: self.h,
            pixels: crate::extend_math::nv12_to_bgra(&tight, self.w, self.h)?,
        })
    }
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    param: LPARAM,
) -> i32 {
    let monitors = &mut *(param as *mut Vec<(RECT, bool)>);
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) != 0 {
        monitors.push((info.rcMonitor, info.dwFlags & 1 != 0));
    }
    1
}

fn window_loop() {
    unsafe {
        let mut monitors: Vec<(RECT, bool)> = Vec::new();
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_monitor),
            &mut monitors as *mut _ as isize,
        );
        let requested = match knit_common::envutil::get("KNIT_EXTEND_MONITOR") {
            Some(s) => match s.parse::<usize>() {
                Ok(n) => Some(n),
                Err(_) => {
                    eprintln!("[extend] モニター番号が不正です: {s}");
                    return;
                }
            },
            None => None,
        };
        let flags: Vec<bool> = monitors.iter().map(|m| m.1).collect();
        let Some(index) = crate::extend_math::select_monitor(&flags, requested) else {
            eprintln!("[extend] 指定されたモニターがありません");
            return;
        };
        let rect = monitors[index].0;
        let class: Vec<u16> = "KnitExtendWindow\0".encode_utf16().collect();
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            hInstance: instance,
            lpfnWndProc: Some(window_proc),
            lpszClassName: class.as_ptr(),
            hbrBackground: GetStockObject(BLACK_BRUSH) as _,
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            eprintln!("[extend] ウィンドウクラスの登録に失敗");
            return;
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            eprintln!("[extend] ウィンドウの作成に失敗");
            return;
        }
        WINDOW.store(hwnd as isize, Ordering::Release);
        SetTimer(hwnd, 1, 100, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        WINDOW.store(0, Ordering::Release);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            FillRect(dc, &rect, GetStockObject(BLACK_BRUSH) as _);
            let mut slot = IMAGE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(image) = &slot.image {
                if let Some((x, y, w, h)) = crate::extend_math::aspect_fit(
                    image.w,
                    image.h,
                    (rect.right - rect.left).max(0) as u32,
                    (rect.bottom - rect.top).max(0) as u32,
                ) {
                    let mut info: BITMAPINFO = std::mem::zeroed();
                    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                    info.bmiHeader.biWidth = image.w as i32;
                    info.bmiHeader.biHeight = -(image.h as i32);
                    info.bmiHeader.biPlanes = 1;
                    info.bmiHeader.biBitCount = 32;
                    info.bmiHeader.biCompression = BI_RGB;
                    StretchDIBits(
                        dc,
                        x as i32,
                        y as i32,
                        w as i32,
                        h as i32,
                        0,
                        0,
                        image.w as i32,
                        image.h as i32,
                        image.pixels.as_ptr() as _,
                        &info,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                }
                slot.dirty = false;
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_TIMER | WM_EXTEND_SHOW => {
            let active = ACTIVE.load(Ordering::Acquire) && crate::CONNECTED.load(Ordering::Acquire);
            let visible = VISIBLE.load(Ordering::Relaxed);
            if active && visible {
                if IsWindowVisible(hwnd) == 0 {
                    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                let mut since = ESC_SINCE.lock().unwrap_or_else(|e| e.into_inner());
                if GetAsyncKeyState(VK_ESCAPE as i32) < 0 {
                    let start = since.get_or_insert_with(Instant::now);
                    if start.elapsed() >= Duration::from_secs(1) {
                        VISIBLE.store(false, Ordering::Relaxed);
                        ShowWindow(hwnd, SW_HIDE);
                        *since = None;
                    }
                } else {
                    *since = None;
                }
            } else {
                if IsWindowVisible(hwnd) != 0 {
                    ShowWindow(hwnd, SW_HIDE);
                }
                *ESC_SINCE.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            0
        }
        WM_CLOSE => {
            VISIBLE.store(false, Ordering::Relaxed);
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
