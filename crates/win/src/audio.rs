// 音声取得・送信(Windows→Mac)。WASAPI ループバックキャプチャでシステム音を取り、
// f32/stereo PCM として独立ポート 24901 で Mac へ送り続ける。
// 本線(24900)と分ける理由: バルク転送が入力の head-of-line blocking を
// 起こさないようにするため。無音フレームは送信を省略する(帯域節約)
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::io::{BufRead, Write};
use std::net::ToSocketAddrs;
use std::sync::atomic::Ordering;

use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL};
// windows-sys は COM インターフェース構造体を提供しないため、WASAPI の
// vtbl を自前定義する(ABI は固定。NOTIFYICONDATAW と同じ手法)
type HRESULT = i32;
type VtblPtr<T> = *const T;
#[repr(C)]
struct IUnknownVtbl {
    QueryInterface: unsafe extern "system" fn(*mut core::ffi::c_void, *const windows_sys::core::GUID, *mut *mut core::ffi::c_void) -> HRESULT,
    AddRef: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    Release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
}
#[repr(C)]
struct ObjVt<T> {
    lpVtbl: VtblPtr<T>,
}
#[repr(C)]
struct IMMDeviceEnumeratorVtbl {
    base: IUnknownVtbl,
    EnumAudioEndpoints: usize,
    GetDefaultAudioEndpoint: unsafe extern "system" fn(*mut core::ffi::c_void, i32, i32, *mut *mut core::ffi::c_void) -> HRESULT,
    GetDevice: usize,
    RegisterEndpointNotificationCallback: usize,
    UnregisterEndpointNotificationCallback: usize,
}
#[repr(C)]
struct IMMDeviceVtbl {
    base: IUnknownVtbl,
    Activate: unsafe extern "system" fn(*mut core::ffi::c_void, *const windows_sys::core::GUID, u32, *mut core::ffi::c_void, *mut *mut core::ffi::c_void) -> HRESULT,
    OpenPropertyStore: usize,
    GetId: usize,
    GetState: usize,
}
#[repr(C)]
struct IAudioClientVtbl {
    base: IUnknownVtbl,
    Initialize: unsafe extern "system" fn(*mut core::ffi::c_void, i32, u32, i64, i64, *const WfxHead, *const windows_sys::core::GUID) -> HRESULT,
    GetBufferSize: usize,
    GetStreamLatency: usize,
    GetCurrentPadding: usize,
    IsFormatSupported: usize,
    GetMixFormat: unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut WfxHead) -> HRESULT,
    GetDevicePeriod: usize,
    Start: unsafe extern "system" fn(*mut core::ffi::c_void) -> HRESULT,
    Stop: unsafe extern "system" fn(*mut core::ffi::c_void) -> HRESULT,
    Reset: usize,
    SetEventHandle: usize,
    GetService: unsafe extern "system" fn(*mut core::ffi::c_void, *const windows_sys::core::GUID, *mut *mut core::ffi::c_void) -> HRESULT,
}
#[repr(C)]
struct IAudioCaptureClientVtbl {
    base: IUnknownVtbl,
    GetBuffer: unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut u8, *mut u32, *mut u32, *mut u64, *mut u64) -> HRESULT,
    ReleaseBuffer: unsafe extern "system" fn(*mut core::ffi::c_void, u32) -> HRESULT,
    GetNextPacketSize: usize,
}
const CLSID_MMDEVICE_ENUMERATOR: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0xBCDE0395_E52F_467C_8E3D_C4579291692E);
const IID_IMMDEVICE_ENUMERATOR: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0xA95664D2_9614_4F35_A746_DE8DB63617E6);
const IID_IAUDIO_CLIENT: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0x1CB9AD4C_DBFA_4C32_B178_C2F568A703B2);
const IID_IAUDIO_CAPTURE: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0xC8ADBD64_E71E_48A0_A4DE_185C395CD317);

/// トレイ/設定から ON/OFF できる(既定 ON)
pub static AUDIO_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

const AUDCLNT_STREAMFLAGS_LOOPBACK: u32 = 0x0002_0000;
const AUDCLNT_SHAREMODE_SHARED: i32 = 0;
const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;

/// ミックスフォーマットの先頭(WAVEFORMATEX とレイアウト互換)
#[repr(C)]
#[derive(Clone, Copy)]
struct WfxHead {
    tag: u16,
    channels: u16,
    rate: u32,
    avg_bytes: u32,
    block_align: u16,
    bits: u16,
    cb_size: u16,
}

/// 初期化済みループバックキャプチャ(1接続分の寿命)
struct Capture {
    client: *mut ObjVt<IAudioClientVtbl>,
    capture: *mut ObjVt<IAudioCaptureClientVtbl>,
    channels: usize,
    sample_rate: u32,
    float_fmt: bool,
    bytes_per_frame: usize,
}

unsafe fn capture_open() -> Result<Capture, String> {
    unsafe {
        let mut enumerator: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_MMDEVICE_ENUMERATOR,
            std::ptr::null_mut(),
            CLSCTX_ALL,
            &IID_IMMDEVICE_ENUMERATOR,
            &mut enumerator,
        );
        if hr < 0 || enumerator.is_null() {
            return Err("MMDeviceEnumerator 作成失敗".into());
        }
        let evt = &*(*(enumerator as *mut ObjVt<IMMDeviceEnumeratorVtbl>)).lpVtbl;
        let mut device: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = (evt.GetDefaultAudioEndpoint)(enumerator, 0 /*eRender*/, 0 /*eConsole*/, &mut device);
        if hr < 0 || device.is_null() {
            return Err("既定オーディオデバイス取得失敗".into());
        }
        let dvt = &*(*(device as *mut ObjVt<IMMDeviceVtbl>)).lpVtbl;
        let mut client: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = (dvt.Activate)(
            device,
            &IID_IAUDIO_CLIENT,
            CLSCTX_ALL,
            std::ptr::null_mut(),
            &mut client,
        );
        if hr < 0 || client.is_null() {
            return Err("IAudioClient 取得失敗".into());
        }
        let client = client as *mut ObjVt<IAudioClientVtbl>;
        let cvt = &*(*client).lpVtbl;
        // ミックスフォーマット(共有モードでは変更不可)
        let mut fmt: *mut WfxHead = std::ptr::null_mut();
        let hr = (cvt.GetMixFormat)(client as *mut _, &mut fmt);
        if hr < 0 || fmt.is_null() {
            return Err("GetMixFormat 失敗".into());
        }
        let channels = (*fmt).channels as usize;
        let rate = (*fmt).rate;
        let bits = (*fmt).bits as usize;
        // float(3=WAVE_FORMAT_IEEE_FLOAT / 0xFFFE=Extensible+32bit) か s16 か
        let float_fmt = (*fmt).tag == 3 || ((*fmt).tag == 0xFFFE && bits == 32);
        let bytes_per_frame = channels * (bits / 8);
        // ループバック(再生音を取り込む)で 200ms バッファを初期化
        let hr = (cvt.Initialize)(
            client as *mut _,
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            2_000_000, // hnsBufferDuration(100ns単位)
            0,
            fmt,
            std::ptr::null(),
        );
        CoTaskMemFree(fmt as *mut core::ffi::c_void);
        if hr < 0 {
            return Err(format!("IAudioClient::Initialize 失敗 hr={hr:08x}"));
        }
        let mut capture: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = (cvt.GetService)(client as *mut _, &IID_IAUDIO_CAPTURE, &mut capture);
        if hr < 0 || capture.is_null() {
            return Err("IAudioCaptureClient 取得失敗".into());
        }
        let capture = capture as *mut ObjVt<IAudioCaptureClientVtbl>;
        (cvt.Start)(client as *mut _);
        Ok(Capture { client, capture, channels, sample_rate: rate, float_fmt, bytes_per_frame })
    }
}

/// GetBuffer で溜まっている分を f32/stereo へ変換して返す(無音なら None)
unsafe fn capture_read(cap: &mut Capture) -> Option<Vec<u8>> {
    unsafe {
        let mut out: Vec<u8> = Vec::new();
        let vt = &*(*cap.capture).lpVtbl;
        loop {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames: u32 = 0;
            let mut flags: u32 = 0;
            let hr = (vt.GetBuffer)(
                cap.capture as *mut core::ffi::c_void,
                &mut data,
                &mut frames,
                &mut flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if hr != 0 {
                break; // バッファ空または一時エラー(次ポーリングで再試行)
            }
            if frames > 0 && !data.is_null() && (flags & AUDCLNT_BUFFERFLAGS_SILENT) == 0 {
                let n = frames as usize * cap.bytes_per_frame;
                let src = std::slice::from_raw_parts(data, n);
                push_converted(&mut out, src, cap);
            }
            (vt.ReleaseBuffer)(cap.capture as *mut core::ffi::c_void, frames);
        }
        let silent = out.iter().all(|&b| b == 0);
        (!silent).then_some(out)
    }
}

/// ミックス形式(任意ch/f32 or s16)→ f32/stereo への変換(ch>2 は先頭2chで代表)
fn push_converted(out: &mut Vec<u8>, src: &[u8], cap: &Capture) {
    let ch = cap.channels.max(1);
    let frames = src.len() / cap.bytes_per_frame;
    out.reserve(frames * 8);
    for f in 0..frames {
        let base = f * cap.bytes_per_frame;
        let (l, r): (f32, f32) = if cap.float_fmt {
            let s = |i: usize| -> f32 {
                let o = base + i * 4;
                f32::from_le_bytes([src[o], src[o + 1], src[o + 2], src[o + 3]])
            };
            (s(0), if ch >= 2 { s(1) } else { s(0) })
        } else {
            let s = |i: usize| -> f32 {
                let o = base + i * 2;
                i16::from_le_bytes([src[o], src[o + 1]]) as f32 / 32768.0
            };
            (s(0), if ch >= 2 { s(1) } else { s(0) })
        };
        out.extend_from_slice(&l.clamp(-1.0, 1.0).to_le_bytes());
        out.extend_from_slice(&r.clamp(-1.0, 1.0).to_le_bytes());
    }
}

/// 音声送信スレッド本体: キャプチャ初期化→Mac:24901 へ接続→ストリーミング。
/// 切断/デバイス失効時は 2 秒後に全体をやり直す
fn audio_run(host: String, token: String) {
    unsafe {
        CoInitializeEx(std::ptr::null_mut(), 0 /*COINIT_MULTITHREADED*/);
    }
    println!("[audio] 開始(→{host}:24901)");
    loop {
        let mut cap = unsafe {
            match capture_open() {
                Ok(c) => c,
                Err(e) => {
                    println!("[audio] {e}(2秒後に再試行)");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }
            }
        };
        let addr = match (host.as_str(), 24901u16).to_socket_addrs().ok().and_then(|mut it| it.next()) {
            Some(a) => a,
            None => {
                println!("[audio] ホスト解決失敗: {host}");
                return;
            }
        };
        let stream = loop {
            match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)) {
                Ok(s) => break s,
                Err(_) => std::thread::sleep(std::time::Duration::from_secs(1)),
            }
        };
        stream.set_nodelay(true).ok();
        let Ok(mut w) = stream.try_clone() else { continue };
        let hs = format!("SDAUDIO1 {token} {}\n", cap.sample_rate);
        if w.write_all(hs.as_bytes()).and_then(|_| w.flush()).is_err() {
            continue;
        }
        let Ok(sr) = stream.try_clone() else { continue };
        let mut r = std::io::BufReader::new(sr);
        let mut reply = String::new();
        if r.read_line(&mut reply).unwrap_or(0) == 0 || !reply.starts_with("ok") {
            println!("[audio] ハンドシェイク拒否: {}", reply.trim());
            std::thread::sleep(std::time::Duration::from_secs(2));
            continue;
        }
        println!("[audio] ストリーミング開始({}Hz f32/stereo)", cap.sample_rate);
        let mut sent_bytes: u64 = 0;
        let mut last_diag = std::time::Instant::now();
        let mut last_send = std::time::Instant::now();
        loop {
            if !AUDIO_ENABLED.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            let Some(frame) = (unsafe { capture_read(&mut cap) }) else {
                // 無音が続いても1秒毎に長さ0のキープアライブを送る:
                // 相手の再起動等で死んだ接続を無音期間中に検知するため
                if last_send.elapsed() >= std::time::Duration::from_secs(1) {
                    if w.write_all(&0u32.to_le_bytes()).is_err() || w.flush().is_err() {
                        println!("[audio] 送信切断(keepalive)。再接続します");
                        break;
                    }
                    last_send = std::time::Instant::now();
                }
                continue;
            };
            let len = (frame.len() as u32).to_le_bytes();
            if w.write_all(&len).is_err() || w.write_all(&frame).is_err() || w.flush().is_err() {
                println!("[audio] 送信切断。再接続します");
                break;
            }
            last_send = std::time::Instant::now();
            sent_bytes += frame.len() as u64;
            if last_diag.elapsed() >= std::time::Duration::from_secs(10) {
                last_diag = std::time::Instant::now();
                println!("[audio] sent={}KB", sent_bytes / 1024);
            }
        }
        unsafe {
            ((*(*cap.client).lpVtbl).Stop)(cap.client as *mut _);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// 音声送信を開始(別スレッド)
pub fn start(host: String, token: String) {
    std::thread::spawn(move || audio_run(host, token));
}
