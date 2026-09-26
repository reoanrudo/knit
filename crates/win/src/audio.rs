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
    GetDevice: unsafe extern "system" fn(*mut core::ffi::c_void, *const u16, *mut *mut core::ffi::c_void) -> HRESULT,
    RegisterEndpointNotificationCallback: usize,
    UnregisterEndpointNotificationCallback: usize,
}
#[repr(C)]
struct IMMDeviceVtbl {
    base: IUnknownVtbl,
    Activate: unsafe extern "system" fn(*mut core::ffi::c_void, *const windows_sys::core::GUID, u32, *mut core::ffi::c_void, *mut *mut core::ffi::c_void) -> HRESULT,
    OpenPropertyStore: usize,
    GetId: unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut u16) -> HRESULT,
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
const IID_IAUDIO_ENDPOINT_VOLUME: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0x5CDF2C82_841E_4546_9722_0CF74078229A);

/// IAudioEndpointVolume(エンドポイントのマスター音量/ミュート操作)。
/// base(IUnknown) 3つの後、13個のメソッドが続いて SetMute=14 / GetMute=15
#[repr(C)]
struct IAudioEndpointVolumeVtbl {
    base: IUnknownVtbl,
    RegisterControlChangeNotify: usize,
    UnregisterControlChangeCallback: usize,
    GetChannelCount: usize,
    SetMasterVolumeLevel: usize,
    SetMasterVolumeLevelScalar: usize,
    GetMasterVolumeLevel: usize,
    GetMasterVolumeLevelScalar: usize,
    SetChannelVolumeLevel: usize,
    SetChannelVolumeLevelScalar: usize,
    GetChannelVolumeLevel: usize,
    GetChannelVolumeLevelScalar: usize,
    SetMute: unsafe extern "system" fn(*mut core::ffi::c_void, i32, *const windows_sys::core::GUID) -> HRESULT,
    GetMute: unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> HRESULT,
    GetVolumeStepInfo: usize,
    VolumeStepUp: usize,
    VolumeStepDown: usize,
    QueryHardwareSupport: usize,
    GetVolumeRange: usize,
}

/// トレイ/設定から ON/OFF できる(既定 ON)
pub static AUDIO_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[link(name = "winmm")]
unsafe extern "system" {
    /// スリープのタイマー分解能を 1ms へ(既定は約 15.6ms に量子化され、
    /// 8ms ポーリングの実効間隔が伸びて音声の追加滞留になる)
    fn timeBeginPeriod(ms: u32) -> u32;
}

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

/// 初期化済みループバックキャプチャ(1接続分の寿命)。
/// fmt_kind: 0=f32 / 1=s16 / 2=i32(ミックス形式の実型)
struct Capture {
    client: *mut ObjVt<IAudioClientVtbl>,
    capture: *mut ObjVt<IAudioCaptureClientVtbl>,
    channels: usize,
    sample_rate: u32,
    fmt_kind: u8,
    bytes_per_frame: usize,
}

/// 既定の再生デバイスの ID(ヘッドホン接続・出力先切替の検知に使う)
fn default_render_id() -> Option<String> {
    unsafe {
        let mut enumerator: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(&CLSID_MMDEVICE_ENUMERATOR, std::ptr::null_mut(), CLSCTX_ALL, &IID_IMMDEVICE_ENUMERATOR, &mut enumerator);
        if hr < 0 || enumerator.is_null() {
            return None;
        }
        let evt = &*(*(enumerator as *mut ObjVt<IMMDeviceEnumeratorVtbl>)).lpVtbl;
        let mut device: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = (evt.GetDefaultAudioEndpoint)(enumerator, 0, 0, &mut device);
        (evt.base.Release)(enumerator);
        if hr < 0 || device.is_null() {
            return None;
        }
        let dvt = &*(*(device as *mut ObjVt<IMMDeviceVtbl>)).lpVtbl;
        let mut pid: *mut u16 = std::ptr::null_mut();
        let hr = (dvt.GetId)(device, &mut pid);
        (dvt.base.Release)(device);
        if hr < 0 || pid.is_null() {
            return None;
        }
        let mut n = 0;
        while *pid.add(n) != 0 {
            n += 1;
        }
        let id = String::from_utf16_lossy(std::slice::from_raw_parts(pid, n));
        CoTaskMemFree(pid as *mut core::ffi::c_void);
        Some(id)
    }
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
            (evt.base.Release)(enumerator);
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
            (dvt.base.Release)(device);
            (evt.base.Release)(enumerator);
            return Err("IAudioClient 取得失敗".into());
        }
        let client = client as *mut ObjVt<IAudioClientVtbl>;
        let cvt = &*(*client).lpVtbl;
        // ミックスフォーマット(共有モードでは変更不可)
        let mut fmt: *mut WfxHead = std::ptr::null_mut();
        let hr = (cvt.GetMixFormat)(client as *mut _, &mut fmt);
        if hr < 0 || fmt.is_null() {
            (cvt.base.Release)(client as *mut _);
            (dvt.base.Release)(device);
            (evt.base.Release)(enumerator);
            return Err("GetMixFormat 失敗".into());
        }
        let channels = (*fmt).channels as usize;
        let rate = (*fmt).rate;
        let bits = (*fmt).bits as usize;
        let fmt_tag = (*fmt).tag;
        // サンプル実型の判別。WAVE_FORMAT_EXTENSIBLE(0xFFFE) は bits=32 だけでは
        // float か 32bit 整数か区別できず、SubFormat GUID の Data1 で判別する
        // (誤ると激しい割れ音になるため厳密に)。3=IEEE_FLOAT / 1=PCM
        let fmt_kind: u8 = if fmt_tag == 3 {
            0 // f32
        } else if fmt_tag == 0xFFFE && (*fmt).cb_size >= 22 {
            let p = fmt as *const u8;
            let sub1 = u32::from_le_bytes([
                *p.add(24), *p.add(25), *p.add(26), *p.add(27),
            ]);
            println!("[audio] extensible subformat data1=0x{sub1:x}");
            match (sub1, bits) {
                (3, 32) => 0,     // IEEE float
                (1, 16) => 1,     // 16bit PCM
                (1, 32) => 2,     // 32bit PCM(整数)
                _ => if bits == 16 { 1 } else { 2 },
            }
        } else if fmt_tag == 1 {
            if bits == 16 { 1 } else { 2 }
        } else {
            2 // 不明: 整数側とみなす
        };
        let bytes_per_frame = channels * (bits / 8);
        // ループバック(再生音を取り込む)で初期化。バッファ指定は共有モードの
        // エンジン周期に近い 50ms を指定(低遅延: 大きいと取得側の滞留が増える)
        let hr = (cvt.Initialize)(
            client as *mut _,
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            500_000, // hnsBufferDuration(100ns単位)=50ms
            0,
            fmt,
            std::ptr::null(),
        );
        CoTaskMemFree(fmt as *mut core::ffi::c_void);
        if hr < 0 {
            (cvt.base.Release)(client as *mut _);
            (dvt.base.Release)(device);
            (evt.base.Release)(enumerator);
            return Err(format!("IAudioClient::Initialize 失敗 hr={hr:08x}"));
        }
        let mut capture: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = (cvt.GetService)(client as *mut _, &IID_IAUDIO_CAPTURE, &mut capture);
        if hr < 0 || capture.is_null() {
            (cvt.base.Release)(client as *mut _);
            (dvt.base.Release)(device);
            (evt.base.Release)(enumerator);
            return Err("IAudioCaptureClient 取得失敗".into());
        }
        let capture = capture as *mut ObjVt<IAudioCaptureClientVtbl>;
        // ここから先 client と capture だけ使うため、直近の 2 参照を解放しておく
        (dvt.base.Release)(device);
        (evt.base.Release)(enumerator);
        (cvt.Start)(client as *mut _);
        // フォーマット診断(kind: 0=f32 / 1=s16 / 2=i32)
        println!(
            "[audio] mix format: tag={} ch={} rate={} bits={} kind={}",
            fmt_tag, channels, rate, bits, fmt_kind
        );
        Ok(Capture { client, capture, channels, sample_rate: rate, fmt_kind, bytes_per_frame })
    }
}

/// GetBuffer で溜まっている分を s16/stereo へ変換して返す(無音なら Ok(None))。
/// Err はデバイスの無効化等(負の HRESULT)で、キャプチャを開き直す必要がある。
/// 旧実装は失敗と「空」(AUDCLNT_S_BUFFER_EMPTY=正の成功コード)を区別せず、
/// ヘッドホン接続などで既定デバイスが変わると無音のまま復帰しなかった
unsafe fn capture_read(cap: &mut Capture) -> Result<Option<Vec<u8>>, HRESULT> {
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
            if hr < 0 {
                return Err(hr);
            }
            if hr != 0 {
                break; // AUDCLNT_S_BUFFER_EMPTY: 今回分は取り切った
            }
            if frames > 0 && !data.is_null() && (flags & AUDCLNT_BUFFERFLAGS_SILENT) == 0 {
                let n = frames as usize * cap.bytes_per_frame;
                let src = std::slice::from_raw_parts(data, n);
                push_converted(&mut out, src, cap);
            }
            (vt.ReleaseBuffer)(cap.capture as *mut core::ffi::c_void, frames);
        }
        let silent = out.iter().all(|&b| b == 0);
        Ok((!silent).then_some(out))
    }
}

/// ミックス形式(任意ch/f32, s16, i32)→ s16/stereo への変換(ch>2 は先頭2chで代表)
fn push_converted(out: &mut Vec<u8>, src: &[u8], cap: &Capture) {
    let ch = cap.channels.max(1);
    let frames = src.len() / cap.bytes_per_frame;
    out.reserve(frames * 4);
    for f in 0..frames {
        let base = f * cap.bytes_per_frame;
        let (l, r): (f32, f32) = match cap.fmt_kind {
            0 => {
                // f32
                let s = |i: usize| -> f32 {
                    let o = base + i * 4;
                    f32::from_le_bytes([src[o], src[o + 1], src[o + 2], src[o + 3]])
                };
                (s(0), if ch >= 2 { s(1) } else { s(0) })
            }
            1 => {
                // s16
                let s = |i: usize| -> f32 {
                    let o = base + i * 2;
                    i16::from_le_bytes([src[o], src[o + 1]]) as f32 / 32768.0
                };
                (s(0), if ch >= 2 { s(1) } else { s(0) })
            }
            _ => {
                // 32bit 整数 PCM(WAVEFORMATEXTENSIBLE 環境向け)
                let s = |i: usize| -> f32 {
                    let o = base + i * 4;
                    i32::from_le_bytes([src[o], src[o + 1], src[o + 2], src[o + 3]]) as f32
                        / 2147483648.0
                };
                (s(0), if ch >= 2 { s(1) } else { s(0) })
            }
        };
        // 送信は 16bit 整数(f32 の半分の帯域。16bit のダイナミックレンジ 96dB は
        // ループバック音声の再生には十分)
        let q = |v: f32| ((v.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes();
        out.extend_from_slice(&q(l));
        out.extend_from_slice(&q(r));
    }
}

/// 音声送信スレッド本体: キャプチャ初期化→Mac:24901 へ接続→ストリーミング。
/// 切断/デバイス失効時は 2 秒後に全体をやり直す
/// host=None は本線の接続先(複数経路のうち繋がったもの)へ追従する
fn audio_run(fixed_host: Option<String>, token: String, port: u16) {
    unsafe {
        CoInitializeEx(std::ptr::null_mut(), 0 /*COINIT_MULTITHREADED*/);
        timeBeginPeriod(1);
    }
    println!("[audio] 開始(→{}:{port})", fixed_host.as_deref().unwrap_or("本線の接続先"));
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
        // 接続先: fixed_host 優先、無ければ本線(PEER)が選んだ Mac へ追従。
        // 接続に失敗するたび PEER を引き直す(本線の再接続で Mac の IP が変わった後、
        // 旧アドレスへ無言で無限リトライして音声だけ復帰しないのを防ぐ)
        let fixed_addr = match &fixed_host {
            Some(h) => match (h.as_str(), port).to_socket_addrs().ok().and_then(|mut it| it.next()) {
                Some(a) => Some(a),
                None => {
                    println!("[audio] ホスト解決失敗: {h}");
                    return;
                }
            },
            None => None,
        };
        let stream = loop {
            let addr = match fixed_addr {
                Some(a) => a,
                None => match crate::peer_ip() {
                    Some(ip) => std::net::SocketAddr::new(ip, port),
                    None => {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        continue;
                    }
                },
            };
            match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)) {
                Ok(s) => break s,
                Err(_) => {
                    println!("[audio] 接続失敗({addr})。1秒後に再試行");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        };
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
        let (r, mut w) = match tsunagu_common::secure::connect(stream, &token, b"tsunagu-audio") {
            Ok(x) => x,
            Err(e) => {
                println!("[audio] 暗号化ハンドシェイク失敗: {e}");
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
        };
        let hs = format!("SDAUDIO3 {} s16\n", cap.sample_rate);
        if w.write_all(hs.as_bytes()).and_then(|_| w.flush()).is_err() {
            continue;
        }
        let mut r = std::io::BufReader::new(r);
        let mut reply = String::new();
        if r.read_line(&mut reply).unwrap_or(0) == 0 || !reply.starts_with("ok") {
            println!("[audio] ハンドシェイク拒否: {}", reply.trim());
            std::thread::sleep(std::time::Duration::from_secs(2));
            continue;
        }
        println!("[audio] ストリーミング開始({}Hz s16/stereo)", cap.sample_rate);
        let dev_id = default_render_id();
        let mut last_dev_check = std::time::Instant::now();
        let mut sent_bytes: u64 = 0;
        let mut last_diag = std::time::Instant::now();
        let mut last_send = std::time::Instant::now();
        loop {
            // 無効中もループは回し続ける: keepalive(下の None 分岐)だけは送る。
            // ここで continue すると keepalive も止まり、Mac 側の受信タイムアウト
            // (12 秒)で接続が切れて、トグルを戻した時に再接続待ちが発生する
            let enabled = AUDIO_ENABLED.load(Ordering::Relaxed);
            // 8ms 間隔でポーリング(低遅延: WASAPI のエンジン周期 10ms に対し
            // 取得側の追加滞留を平均 4ms 程に抑える)
            std::thread::sleep(std::time::Duration::from_millis(8));
            // 既定デバイスの切替(ヘッドホン接続・出力先変更)を検知して開き直す
            if last_dev_check.elapsed() >= std::time::Duration::from_secs(2) {
                last_dev_check = std::time::Instant::now();
                if default_render_id() != dev_id {
                    println!("[audio] 既定の再生デバイスが変わりました。キャプチャを開き直します");
                    break;
                }
            }
            let read = if enabled {
                unsafe { capture_read(&mut cap) }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(200));
                Ok(None)
            };
            let read = match read {
                Ok(r) => r,
                Err(hr) => {
                    println!("[audio] キャプチャ失敗 hr={hr:08x}(デバイス無効化)。開き直します");
                    break;
                }
            };
            let Some(frame) = read else {
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
            // 開き直すたびに漏れないよう解放する
            ((*(*cap.capture).lpVtbl).base.Release)(cap.capture as *mut _);
            ((*(*cap.client).lpVtbl).base.Release)(cap.client as *mut _);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// 音声送信を開始(別スレッド)
pub fn start(host: Option<String>, token: String, port: u16) {
    AUDIO_ACTIVE.store(true, Ordering::Relaxed);
    std::thread::spawn(move || audio_run(host, token, port));
}

// ---------- スピーカーミュート(音声出力の集中) ----------
// 接続中は Windows 側スピーカーをミュートし、Mac のみで鳴らす。
// エンドポイントミュートは WASAPI ループバックの取り出し点(ポストミックス)に
// は効かない環境が多く、ミュート中もキャプチャは継続する(環境依存の注意は docs 記載)

/// 音声転送が稼働している(=ミュート制御が意味を持つ)か
pub static AUDIO_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// ミュート適用前のユーザー設定と適用時のデバイス ID(切断時にその ID へ戻すため)。
/// None=まだ記録していない
static SPK_WAS_MUTED: std::sync::Mutex<Option<(bool, String)>> = std::sync::Mutex::new(None);

/// エンドポイントの IAudioEndpointVolume を Activate して返す。
/// want_id=Some ならそのデバイス ID へ(ミュート復元時の取り違え防止)、
/// None なら現在の既定デバイスへ
unsafe fn open_endpoint_volume(want_id: Option<&str>) -> Result<*mut ObjVt<IAudioEndpointVolumeVtbl>, String> {
    unsafe {
        // 呼び出し元スレッドで COM 未初期化の可能性がある(本体セッションのスレッド)
        CoInitializeEx(std::ptr::null_mut(), 0 /*COINIT_MULTITHREADED*/);
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
        match want_id {
            Some(id) => {
                let mut w: Vec<u16> = id.encode_utf16().collect();
                w.push(0);
                let hr = (evt.GetDevice)(enumerator, w.as_ptr(), &mut device);
                let _ = (evt.base.Release)(enumerator);
                if hr < 0 || device.is_null() {
                    return Err(format!("デバイス取得失敗(切替・抜去の可能性) hr={hr:08x}"));
                }
            }
            None => {
                let hr = (evt.GetDefaultAudioEndpoint)(enumerator, 0 /*eRender*/, 0 /*eConsole*/, &mut device);
                let _ = (evt.base.Release)(enumerator); // enumerator はもう要らない
                if hr < 0 || device.is_null() {
                    return Err("既定オーディオデバイス取得失敗".into());
                }
            }
        }
        let dvt = &*(*(device as *mut ObjVt<IMMDeviceVtbl>)).lpVtbl;
        let mut vol: *mut core::ffi::c_void = std::ptr::null_mut();
        let hr = (dvt.Activate)(
            device,
            &IID_IAUDIO_ENDPOINT_VOLUME,
            CLSCTX_ALL,
            std::ptr::null_mut(),
            &mut vol,
        );
        let _ = (dvt.base.Release)(device); // IMMDevice ももう要らない
        if hr < 0 || vol.is_null() {
            return Err(format!("IAudioEndpointVolume 取得失敗 hr={hr:08x}"));
        }
        Ok(vol as *mut ObjVt<IAudioEndpointVolumeVtbl>)
    }
}

/// 接続確立時: ミュートモードが ON なら現在のミュート状態を記録してからミュートする。
/// 音声転送が無効(AUDIO_ACTIVE=false)のときは何もしない(音がどこにも行かなくなるため)
pub fn speaker_connect_mute(mode_on: bool) {
    if !mode_on || !AUDIO_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let Ok(vol) = open_endpoint_volume(None) else {
            println!("[spk] エンドポイント取得失敗。ミュートせず継続します");
            return;
        };
        // 適用時のデバイス ID を記録する: 接続中に既定出力が変わっても、
        // 復元はミュートした同じデバイスへ向ける(別デバイスのミュートを触らない)
        let dev_id = default_render_id();
        let vt = &*(*vol).lpVtbl;
        let mut now: i32 = 0;
        let got = (vt.GetMute)(vol as *mut core::ffi::c_void, &mut now);
        if got >= 0 {
            let _ = SPK_WAS_MUTED.lock().map(|mut g| *g = Some((now != 0, dev_id.unwrap_or_default())));
            let hr = (vt.SetMute)(vol as *mut core::ffi::c_void, 1, std::ptr::null());
            println!("[spk] 接続中ミュートを適用 (was_muted={})", now != 0);
            if hr < 0 {
                println!("[spk] SetMute 失敗 hr={hr:08x}");
            }
        } else {
            println!("[spk] GetMute 失敗 hr={got:08x}");
        }
        (vt.base.Release)(vol as *mut core::ffi::c_void);
    }
}

/// 切断時: ミュートを適用した際の元状態へ戻す(元がミュートでなければ鳴らす)
pub fn speaker_disconnect() {
    let was = SPK_WAS_MUTED.lock().ok().and_then(|g| g.clone());
    let Some((was_muted, dev_id)) = was else { return }; // ミュートを適用していない
    let _ = SPK_WAS_MUTED.lock().map(|mut g| *g = None);
    unsafe {
        // まずミュートしたデバイスそのものへ戻す。デバイスが消えていれば既定へ
        // フォールバックする(その場合の復元先は変わるが、触らないより良い)
        let vol = match open_endpoint_volume(if dev_id.is_empty() { None } else { Some(&dev_id) }) {
            Ok(v) => v,
            Err(e) => {
                println!("[spk] 復元先デバイスが取れません({e})。既定デバイスで試みます");
                match open_endpoint_volume(None) {
                    Ok(v) => v,
                    Err(e2) => {
                        println!("[spk] 復元時のエンドポイント取得失敗: {e2}");
                        return;
                    }
                }
            }
        };
        let vt = &*(*vol).lpVtbl;
        let hr = (vt.SetMute)(vol as *mut core::ffi::c_void, was_muted as i32, std::ptr::null());
        println!("[spk] 切断。ミュートを元へ戻しました (muted={was_muted})");
        if hr < 0 {
            println!("[spk] SetMute(復元) 失敗 hr={hr:08x}");
        }
        (vt.base.Release)(vol as *mut core::ffi::c_void);
    }
}

/// 接続中のモード切替(メニューから): ON=記録してミュート / OFF=元へ戻す
pub fn speaker_set_mode(on: bool, connected: bool) {
    if !connected {
        return; // 未接続なら確立時に適用される
    }
    if on {
        speaker_connect_mute(true);
    } else {
        speaker_disconnect();
    }
}
