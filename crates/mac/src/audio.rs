// 音声受信・再生(Windows→Mac)。
// 独立した TCP:24901 で PCM(f32/stereo) を受け取り AudioQueue で再生する。
// 本線(24900)と分ける理由: 画像クリップの head-of-line blocking で入力が
// 止まる問題を音声で再現しないため(レビュー Wave1 H3 と同じ設計判断)
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// 既定のサンプリングレート(ハンドシェイクで上書きされる)
static SAMPLE_RATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(48000);
/// メニューからのミュート(受信は続くが再生しない)
pub static MUTED: AtomicBool = AtomicBool::new(false);
/// 診断カウンタ(受信/再生バイト数)。10秒毎にログへ出す
static RX_BYTES: AtomicU64 = AtomicU64::new(0);
static PLAY_BYTES: AtomicU64 = AtomicU64::new(0);
/// 再生リングバッファ(音声スレッド→AudioQueue コールバック)。
/// 上限 1.5MB(≈4秒)。溢れたら古い方を捨てる(遅延を優先)
static RING: Mutex<std::collections::VecDeque<u8>> =
    Mutex::new(std::collections::VecDeque::new());
const RING_CAP: usize = 1_536 * 1024;

type AudioQueueRef = *mut core::ffi::c_void;
type AudioQueueBufferRef = *mut AudioQueueBuffer;
type OSStatus = i32;

// AudioToolbox/AudioQueue.h の現行レイアウト(タイムスタンプ等は無い。
// 旧レイアウトを仮定すると byteSize の書き込み位置がずれ BufferEmpty になる)
#[repr(C)]
struct AudioQueueBuffer {
    mAudioDataBytesCapacity: u32,
    _pad0: u32,
    mAudioData: *mut u8,
    mAudioDataByteSize: u32,
    _pad1: u32,
    mUserData: *mut core::ffi::c_void,
    mPacketDescriptionCapacity: u32,
    _pad2: u32,
    mPacketDescriptions: *mut core::ffi::c_void,
    mPacketDescriptionCount: u32,
    _pad3: u32,
}

/// LinearPCM f32 / interleaved / stereo
#[repr(C)]
#[derive(Clone, Copy)]
struct AudioStreamBasicDescription {
    mSampleRate: f64,
    mFormatID: u32,
    mFormatFlags: u32,
    mBytesPerPacket: u32,
    mFramesPerPacket: u32,
    mBytesPerFrame: u32,
    mChannelsPerFrame: u32,
    mBitsPerChannel: u32,
    mReserved: u32,
}
const KAUDIO_FORMAT_LINEAR_PCM: u32 = 0x6C_70_63_6D; // 'lpcm'
const KLINEAR_FLAGS_FLOAT_PACKED: u32 = 0x1 | (1 << 3); // IsFloat | IsPacked

#[link(name = "AudioToolbox", kind = "framework")]
#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioQueueNewOutput(
        in_desc: *const AudioStreamBasicDescription,
        in_callback_proc: unsafe extern "C" fn(
            in_user_data: *mut core::ffi::c_void,
            in_aq: AudioQueueRef,
            in_buffer: AudioQueueBufferRef,
        ),
        in_user_data: *mut core::ffi::c_void,
        in_callback_run_loop: *mut core::ffi::c_void,
        in_callback_run_loop_mode: *mut core::ffi::c_void,
        in_flags: u32,
        out_aq: *mut AudioQueueRef,
    ) -> OSStatus;
    fn AudioQueueAllocateBuffer(
        in_aq: AudioQueueRef,
        in_buffer_byte_size: usize,
        out_buffer: *mut AudioQueueBufferRef,
    ) -> OSStatus;
    fn AudioQueueEnqueueBuffer(
        in_aq: AudioQueueRef,
        in_buffer: AudioQueueBufferRef,
        in_num_packet_descs: u32,
        in_packet_descs: *const core::ffi::c_void,
    ) -> OSStatus;
    fn AudioQueueStart(in_aq: AudioQueueRef, in_time: *const core::ffi::c_void) -> OSStatus;
    fn AudioQueueFlush(in_aq: AudioQueueRef) -> OSStatus;
}

/// AudioQueue の出力コールバック: リングから必要分だけ取り出して埋める。
/// 不足分は無音で埋め(遅延がまだ追いついていない起動直後など)
static CB_FIRED: AtomicBool = AtomicBool::new(false);
unsafe extern "C" fn aq_callback(
    _user: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
) {
    unsafe {
        if !CB_FIRED.swap(true, Ordering::Relaxed) {
            eprintln!("[audio] callback alive (cap={})", (*buffer).mAudioDataBytesCapacity);
        }
        let cap = (*buffer).mAudioDataBytesCapacity as usize;
        let dst = (*buffer).mAudioData;
        let mut filled = 0usize;
        if !MUTED.load(Ordering::Relaxed) {
            let mut ring = RING.lock().unwrap_or_else(|e| e.into_inner());
            let take = ring.len().min(cap);
            for i in 0..take {
                dst.add(i).write(ring.pop_front().unwrap_or(0));
            }
            filled = take;
        }
        // 残りは無音のまま(バッファは前回の内容が残るため明示的にゼロクリア)
        for i in filled..cap {
            dst.add(i).write(0);
        }
        (*buffer).mAudioDataByteSize = cap as u32;
        PLAY_BYTES.fetch_add(cap as u64, Ordering::Relaxed);
        AudioQueueEnqueueBuffer(aq, buffer, 0, std::ptr::null());
    }
}

/// AudioQueue を起動する(4バッファ x 20ms ≒ 80ms の再生遅延)
fn start_playback(rate: u32) -> bool {
    unsafe {
        let bytes_per_frame: u32 = 8; // f32 x 2ch
        let frame_bytes = rate as usize * bytes_per_frame as usize / 50; // 20ms
        let desc = AudioStreamBasicDescription {
            mSampleRate: rate as f64,
            mFormatID: KAUDIO_FORMAT_LINEAR_PCM,
            mFormatFlags: KLINEAR_FLAGS_FLOAT_PACKED,
            mBytesPerPacket: bytes_per_frame,
            mFramesPerPacket: 1,
            mBytesPerFrame: bytes_per_frame,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        let mut aq: AudioQueueRef = std::ptr::null_mut();
        if AudioQueueNewOutput(
            &desc,
            aq_callback,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut aq,
        ) != 0
        {
            eprintln!("[audio] AudioQueueNewOutput 失敗");
            return false;
        }
        for _ in 0..4 {
            let mut buf: AudioQueueBufferRef = std::ptr::null_mut();
            if AudioQueueAllocateBuffer(aq, frame_bytes, &mut buf) != 0 {
                eprintln!("[audio] AllocateBuffer 失敗");
                return false;
            }
            (*buf).mAudioDataByteSize = frame_bytes as u32;
            for i in 0..frame_bytes {
                (*buf).mAudioData.add(i).write(0);
            }
            let st = AudioQueueEnqueueBuffer(aq, buf, 0, std::ptr::null());
            if st != 0 {
                eprintln!("[audio] EnqueueBuffer 失敗 st={st}");
            }
        }
        let st = AudioQueueStart(aq, std::ptr::null());
        if st != 0 {
            eprintln!("[audio] AudioQueueStart 失敗 st={st}");
            return false;
        }
        eprintln!("[audio] AudioQueueStart ok (frame_bytes={frame_bytes})");
        // aq は停止しない(プロセス終了まで)。ref は意図的に保持しない
        std::mem::forget(aq);
        true
    }
}

/// 音声受信サーバ(独立スレッド)。24901 で待ち受け、認証後に PCM を受け流す
pub fn start(token: String) {
    std::thread::spawn(move || {
        use std::io::{BufRead, Read, Write};
        let port: u16 = 24901;
        let bind_ip = crate::envutil::get("SEAMLESS_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[audio] listen {bind_ip}:{port} 失敗: {e}(音声なしで継続)");
                return;
            }
        };
        eprintln!("[audio] listening on {bind_ip}:{port}");
        let mut playback_started = false;
        loop {
            let (stream, peer) = match listener.accept() {
                Ok(x) => x,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };
            // 本線と同じピア制限(Tailscale CGNAT 範囲外は拒否)
            if let std::net::IpAddr::V4(v4) = peer.ip() {
                let o = v4.octets();
                if !(o[0] == 100 && (64..=127).contains(&o[1])) {
                    eprintln!("[audio] rejected: {peer}");
                    continue;
                }
            } else {
                continue;
            }
            eprintln!("[audio] accepted from {peer}");
            stream.set_nodelay(true).ok();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
            let Ok(sr) = stream.try_clone() else { continue };
            let mut reader = std::io::BufReader::new(sr);
            let mut w = stream;
            // ハンドシェイク: "SDAUDIO1 <token> <rate>\n" → "ok\n"
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            if parts.len() != 3 || parts[0] != "SDAUDIO1" || parts[1] != token {
                eprintln!("[audio] invalid handshake");
                let _ = w.write_all(b"ng\n");
                continue;
            }
            let Ok(rate) = parts[2].parse::<u32>() else { continue };
            if !(4000..=192_000).contains(&rate) {
                continue;
            }
            SAMPLE_RATE.store(rate, Ordering::Relaxed);
            if w.write_all(b"ok\n").and_then(|_| w.flush()).is_err() {
                continue;
            }
            if !playback_started {
                playback_started = start_playback(rate);
            }
            RING.lock().unwrap_or_else(|e| e.into_inner()).clear();
            eprintln!("[audio] streaming started ({rate}Hz f32/stereo)");
            // PCM フレーム受信: [u32 LE 長][データ]。長さ上限は 128KB
            let mut len_buf = [0u8; 4];
            let mut last_diag = std::time::Instant::now();
            loop {
                if reader.read_exact(&mut len_buf).is_err() {
                    break;
                }
                let len = u32::from_le_bytes(len_buf) as usize;
                if len == 0 {
                    continue; // キープアライブ(無音期間の死活監視用)
                }
                if len > 128 * 1024 {
                    eprintln!("[audio] invalid frame len={len}. disconnect");
                    break;
                }
                let mut frame = vec![0u8; len];
                if reader.read_exact(&mut frame).is_err() {
                    break;
                }
                RX_BYTES.fetch_add(len as u64, Ordering::Relaxed);
                if !MUTED.load(Ordering::Relaxed) {
                    let mut ring = RING.lock().unwrap_or_else(|e| e.into_inner());
                    // ミュート明けに古い音を鳴らさない、かつ遅延を溜めない:
                    // 上限超過分は古い方から捨てる
                    if ring.len() + frame.len() > RING_CAP {
                        let overflow = ring.len() + frame.len() - RING_CAP;
                        for _ in 0..overflow.min(ring.len()) {
                            ring.pop_front();
                        }
                    }
                    ring.extend(frame.iter().copied());
                }
                if last_diag.elapsed() >= std::time::Duration::from_secs(10) {
                    last_diag = std::time::Instant::now();
                    let rx = RX_BYTES.load(Ordering::Relaxed) / 1024;
                    let play = PLAY_BYTES.load(Ordering::Relaxed) / 1024;
                    let queued = RING.lock().unwrap_or_else(|e| e.into_inner()).len() / 1024;
                    eprintln!("[audio] diag rx={rx}KB played={play}KB queued={queued}KB");
                }
            }
            eprintln!("[audio] stream ended");
        }
    });
}
