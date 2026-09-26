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
/// 溢れたら古い方を捨てる。実効的な滞留は aq_callback の 2 段階クリップで
/// 「目標 ≈83ms・上限 333ms」に保つ(下記定数参照)
static RING: Mutex<std::collections::VecDeque<u8>> =
    Mutex::new(std::collections::VecDeque::new());
const RING_CAP: usize = 192 * 1024;
/// プリロール量(48kHz f32/stereo で ≈83ms)。ストリーム開始/ミュート明けに
/// これだけ溜まるまで再生を始めない。ネットワークの到着むら(バースト)を
/// このクッションで吸収し、RING 空による断続音(モールス音)を防ぐ
/// = 全ストリーミング再生の定番構成(遅延はこの分だけ一定に乗る)
const RING_PRE_ROLL: usize = 32 * 1024;
/// プリロール完了状態(開始/ミュート明けごとにやり直す)
static PRIMED: AtomicBool = AtomicBool::new(false);
/// ソフト追い込みの目標滞留(≈83ms=プリロール量と同値)。これを超えたら
/// 毎回「超過分の 1/32」だけ捨て、指数的に目標へ戻す。
/// 1 回あたりのドロップが数サンプル〜数ms 程度に収まるため、
/// まとめ捨て(位相ジャンプ=プツプツ音)にならない
const RING_SOFT_TARGET: usize = 32 * 1024;
/// 緊急クリップの上限(≈333ms)。大バースト(再接続直後等)で一気に溜まった
/// 場合だけ目標値まで一括で捨てる(恒常的には発動しない)
const RING_HARD_CLIP: usize = 128 * 1024;

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
    fn AudioQueueStop(in_aq: AudioQueueRef, immediate: u8) -> OSStatus;
    fn AudioQueueDispose(in_aq: AudioQueueRef, immediate: u8) -> OSStatus;
}

/// 再生中の AudioQueue と、そのサンプリングレート。レートが変わった接続では作り直す
/// (旧実装は最初の接続のレートで作ったきりで、Windows 側のデバイス切替で
/// 44.1kHz⇄48kHz が変わると音程がずれたまま再生された)
static AQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static AQ_RATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn ensure_playback(rate: u32) {
    if AQ.load(Ordering::Relaxed) != 0 && AQ_RATE.load(Ordering::Relaxed) == rate {
        return;
    }
    let old = AQ.swap(0, Ordering::Relaxed);
    if old != 0 {
        unsafe {
            AudioQueueStop(old as AudioQueueRef, 1);
            AudioQueueDispose(old as AudioQueueRef, 1);
        }
        eprintln!("[audio] サンプリングレート変更 {} → {rate}Hz。再生キューを作り直します", AQ_RATE.load(Ordering::Relaxed));
    }
    if let Some(aq) = start_playback(rate) {
        AQ.store(aq as usize, Ordering::Relaxed);
        AQ_RATE.store(rate, Ordering::Relaxed);
    }
}

/// リサンプル位相: 前回コールバックの消費端数(フレーム単位、0..1)。
/// コールバック間で持ち越すことで、読み出し位置が波形上で完全に連続になり
/// フレーム欠け/重複が構造的に起きない(端数の切り捨てが毎回ノイズになるのを防ぐ)
static PHASE_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn phase_load() -> f64 {
    f64::from_bits(PHASE_FRAMES.load(Ordering::Relaxed))
}
fn phase_store(v: f64) {
    PHASE_FRAMES.store(v.to_bits(), Ordering::Relaxed);
}

/// AudioQueue の出力コールバック: リングから必要分だけ取り出して埋める。
/// 不足分は無音で埋める。滞留が目標を超えているときは線形補間でわずかに
/// 速く読み(リサンプル追い込み)、波形を切らないまま遅延を調整する
static CB_FIRED: AtomicBool = AtomicBool::new(false);
/// 間引き(緊急クリップ)した累計バイト。音割れ調査の指標として diag へ出す
static DROP_BYTES: AtomicU64 = AtomicU64::new(0);
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
            // プリロール: RING_PRE_ROLL 分溜まるまで再生を始めない(ゼロ埋め)。
            // 溜まりきる前に鳴らし始めると供給の到着むらがそのまま音切れに
            // なる(断続音=モールス音の原因)
            if !PRIMED.load(Ordering::Relaxed) {
                if ring.len() < RING_PRE_ROLL {
                    for i in 0..cap {
                        dst.add(i).write(0);
                    }
                    (*buffer).mAudioDataByteSize = cap as u32;
                    PLAY_BYTES.fetch_add(cap as u64, Ordering::Relaxed);
                    AudioQueueEnqueueBuffer(aq, buffer, 0, std::ptr::null());
                    return;
                }
                PRIMED.store(true, Ordering::Relaxed);
                phase_store(0.0); // 在庫が新鮮なため位相もリセット
            }
            // 緊急クリップ(大バーストのみ): 8 バイト(f32×2ch=1フレーム)境界で
            // 一括間引き。超過は通常この経路を通らず、通った量は diag で見える
            if ring.len() > RING_HARD_CLIP {
                let excess = (ring.len() - RING_SOFT_TARGET) & !7usize;
                DROP_BYTES.fetch_add(excess as u64, Ordering::Relaxed);
                ring.drain(..excess);
            }
            // 滑らかな追い込み: 滞留が「目標+デッドバンド(≈5ms)」を超えている
            // 間だけ読み出し速度を最大 1% 速くする。フレーム欠けではなく波形を
            // 連続したまま遅延を目標へ戻す。位相はコールバック間で持ち越すため
            // ratio=1.0 のときは素通り=ドロップ・重複ゼロ。
            // 音質優先の設計: (a)超過が僅か(デッドバンド内)の間は補間しない
            // (b)補間は 3次 Catmull-Rom(高域までほぼフラット。線形補間は
            // 高域が減衰する)(c)速度変化の上限は 1%(知覚不可)
            const SOFT_DEAD: usize = 2048;
            let over = ring.len().saturating_sub(RING_SOFT_TARGET + SOFT_DEAD);
            let ratio = if over == 0 {
                1.0
            } else {
                1.0 + (over as f64 / RING_SOFT_TARGET as f64).min(1.0) * 0.01
            };
            // リアルタイムスレッドでメモリを確保しないよう、リングを直接読む
            let need = (cap as f64 * ratio) as usize + 16;
            let src_frames = need.min(ring.len()) / 8; // 1フレーム=8バイト(L+R)
            let out_frames = cap / 8;
            if src_frames >= 2 {
                let dst_f = dst as *mut f32;
                let sample = |i: usize| -> f32 {
                    f32::from_le_bytes([ring[i * 4], ring[i * 4 + 1], ring[i * 4 + 2], ring[i * 4 + 3]])
                };
                // sp はフレーム単位の読み出し位置(整数部=フレーム、端数=補間位相)。
                // L(偶数サンプル)は L 同士、R は R 同士で補間するため L/R は混ざらない。
                // 補間カーネルは Catmull-Rom(4点3次): 線形補間と違い高域まで
                // ほぼ劣化させない(音質優先)。frac=0 のときは厳密に素通り
                let mut sp = phase_load();
                let mut written_bytes = 0usize;
                let fidx = |k: i64| -> usize {
                    (k.max(0) as usize).min(src_frames.saturating_sub(1))
                };
                for o in 0..out_frames {
                    let i0 = sp as usize;
                    if i0 + 1 >= src_frames {
                        // 在庫の終端に到達: 最終フレームをホールドして埋める
                        let base = (src_frames - 1) * 2;
                        dst_f.add(o * 2).write(sample(base));
                        dst_f.add(o * 2 + 1).write(sample(base + 1));
                        written_bytes = (o + 1) * 8;
                        sp = (src_frames - 1) as f64;
                        break;
                    }
                    let t = (sp - i0 as f64) as f32;
                    let (l, r);
                    if i0 + 2 < src_frames {
                        let t2 = t * t;
                        let t3 = t2 * t;
                        let cm = |p0: f32, p1: f32, p2: f32, p3: f32| -> f32 {
                            0.5 * ((2.0 * p1)
                                + (-p0 + p2) * t
                                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
                        };
                        let m = i0 as i64;
                        l = cm(
                            sample(fidx(m - 1) * 2),
                            sample(i0 * 2),
                            sample((i0 + 1) * 2),
                            sample(fidx(m + 2) * 2),
                        );
                        r = cm(
                            sample(fidx(m - 1) * 2 + 1),
                            sample(i0 * 2 + 1),
                            sample((i0 + 1) * 2 + 1),
                            sample(fidx(m + 2) * 2 + 1),
                        );
                    } else {
                        // 在庫の終端1フレーム手前: 線形補間でフォールバック
                        l = sample(i0 * 2) + (sample(i0 * 2 + 2) - sample(i0 * 2)) * t;
                        r = sample(i0 * 2 + 1) + (sample(i0 * 2 + 3) - sample(i0 * 2 + 1)) * t;
                    }
                    dst_f.add(o * 2).write(l);
                    dst_f.add(o * 2 + 1).write(r);
                    written_bytes = (o + 1) * 8;
                    sp += ratio;
                }
                // 消費 = 読み出し位置の整数部(フレーム単位=自動的に 8 バイト境界)。
                // 端数は PHASE として次回へ繰り越すため、捨てても重複しても無い
                let consumed_frames = (sp as usize).min(src_frames);
                let consumed = consumed_frames * 8;
                let n = consumed.min(ring.len());
                ring.drain(..n);
                phase_store(sp - consumed_frames as f64);
                filled = written_bytes;
            } else if !ring.is_empty() {
                // 在庫が 1 フレーム未満(通常ない): そのまま取り出す
                let take = ring.len().min(cap);
                for i in 0..take {
                    dst.add(i).write(ring.pop_front().unwrap_or(0));
                }
                filled = take;
            }
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

/// AudioQueue を起動する(4バッファ x 15ms = 60ms の再生バッファ)。
/// 低遅延と音切れ防止のバランス点: 供給側の揺らぎ(TCP 到着間隔)を
/// この余裕で吸収し、滞留はクリップで「目標 ≈78ms」に保つ
fn start_playback(rate: u32) -> Option<AudioQueueRef> {
    unsafe {
        let bytes_per_frame: u32 = 8; // f32 x 2ch
        // 15ms 分。奇数サンプルレート環境で L/R が割れないよう 8 バイト境界へ丸める
        let frame_bytes = (rate as usize * bytes_per_frame as usize * 15 / 1000) & !7usize;
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
            return None;
        }
        for _ in 0..4 {
            let mut buf: AudioQueueBufferRef = std::ptr::null_mut();
            if AudioQueueAllocateBuffer(aq, frame_bytes, &mut buf) != 0 {
                eprintln!("[audio] AllocateBuffer 失敗");
                // 生成済みの AudioQueue(内部スレッド・バッファ)を解放せず返ると、
                // 接続のたびに ensure_playback が再試行して蓄積する
                AudioQueueDispose(aq, 1);
                return None;
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
            AudioQueueDispose(aq, 1);
            return None;
        }
        eprintln!("[audio] AudioQueueStart ok ({rate}Hz, frame_bytes={frame_bytes})");
        Some(aq)
    }
}

/// 音声受信サーバ(独立スレッド)。24901 で待ち受け、認証後に PCM を受け流す
pub fn start(token: String) {
    std::thread::spawn(move || {
        use std::io::{BufRead, Read, Write};
        let port: u16 = 24901;
        let bind_ip = crate::envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[audio] listen {bind_ip}:{port} 失敗: {e}(音声なしで継続)");
                return;
            }
        };
        eprintln!("[audio] listening on {bind_ip}:{port}");
        loop {
            let (stream, peer) = match listener.accept() {
                Ok(x) => x,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };
            // 本線と同じ接続元制限と暗号化(トークン不一致はハンドシェイクで弾かれる)
            if !tsunagu_common::net::is_allowed(peer.ip()) {
                eprintln!("[audio] rejected: {peer}");
                continue;
            }
            eprintln!("[audio] accepted from {peer}");
            stream.set_nodelay(true).ok();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
            let (r, mut w) = match tsunagu_common::secure::accept(stream, &token, b"tsunagu-audio") {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("[audio] 暗号化ハンドシェイク失敗: {e}");
                    continue;
                }
            };
            let mut reader = std::io::BufReader::new(r);
            // 形式の申告: "SDAUDIO3 <rate> s16\n" → "ok\n"(認証は暗号化で済んでいる)
            let mut line = String::new();
            if (&mut reader).take(512).read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            let s16 = parts.len() == 3 && parts[0] == "SDAUDIO3" && parts[2] == "s16";
            if !s16 {
                eprintln!("[audio] invalid handshake");
                let _ = w.write_all(b"ng\n");
                continue;
            }
            let Ok(rate) = parts[1].parse::<u32>() else { continue };
            if !(4000..=192_000).contains(&rate) {
                continue;
            }
            SAMPLE_RATE.store(rate, Ordering::Relaxed);
            if w.write_all(b"ok\n").and_then(|_| w.flush()).is_err() {
                continue;
            }
            ensure_playback(rate);
            RING.lock().unwrap_or_else(|e| e.into_inner()).clear();
            PRIMED.store(false, Ordering::Relaxed);
            phase_store(0.0);
            eprintln!("[audio] streaming started ({rate}Hz s16/stereo, 暗号化)");
            // PCM フレーム受信: [u32 LE 長][データ]。長さ上限は 128KB
            let mut len_buf = [0u8; 4];
            let mut last_diag = std::time::Instant::now();
            loop {
                if reader.read_exact(&mut len_buf).is_err() {
                    break;
                }
                let len = u32::from_le_bytes(len_buf) as usize;
                if len == 0 {
                    // キープアライブ(無音期間の死活監視用)。
                    // 無音期間はここがループの唯一の出口のため diag も出す
                    diag_log(&mut last_diag, rate);
                    continue;
                }
                if len > 128 * 1024 {
                    eprintln!("[audio] invalid frame len={len}. disconnect");
                    break;
                }
                let mut frame = vec![0u8; len];
                if reader.read_exact(&mut frame).is_err() {
                    break;
                }
                if s16 {
                    // 再生リングは f32 のまま(補間・追い込み処理を共通にする)
                    frame = frame
                        .chunks_exact(2)
                        .flat_map(|b| (i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).to_le_bytes())
                        .collect();
                }
                // フレーム整合の防御: 8 バイト(f32×2ch)境界に切り詰める。
                // 送信側はフレーム単位で送るはずだが、万一の途中欠けが
                // 混入しても破壊音として再生されないようにする
                frame.truncate(frame.len() & !7usize);
                if frame.is_empty() {
                    continue;
                }
                RX_BYTES.fetch_add(len as u64, Ordering::Relaxed);
                if !MUTED.load(Ordering::Relaxed) {
                    let mut ring = RING.lock().unwrap_or_else(|e| e.into_inner());
                    // ミュート明けに古い音を鳴らさない、かつ遅延を溜めない:
                    // 上限超過分は古い方から捨てる(8バイト境界で)
                    if ring.len() + frame.len() > RING_CAP {
                        let overflow = (ring.len() + frame.len() - RING_CAP) & !7usize;
                        let n = overflow.min(ring.len());
                        ring.drain(..n);
                    }
                    ring.extend(frame.iter().copied());
                } else {
                    // ミュート明けはプリロールからやり直す(クッション無しの
                    // 即再生で断続音が出るのを防ぐ)
                    PRIMED.store(false, Ordering::Relaxed);
                    phase_store(0.0);
                }
                if last_diag.elapsed() >= std::time::Duration::from_secs(10) {
                    last_diag = std::time::Instant::now();
                    let rx = RX_BYTES.load(Ordering::Relaxed) / 1024;
                    let play = PLAY_BYTES.load(Ordering::Relaxed) / 1024;
                    let queued = RING.lock().unwrap_or_else(|e| e.into_inner()).len();
                    // 滞留時間(=ここが実効的な追加遅延)。プリロールで ≈83ms に保たれる
                    let lag_ms = queued as u64 * 1000 / (rate as u64 * 8);
                    eprintln!(
                        "[audio] diag rx={rx}KB played={play}KB queued={}KB lag={lag_ms}ms",
                        queued / 1024
                    );
                }
            }
            eprintln!("[audio] stream ended");
        }
    });
}

/// 10 秒毎の診断ログ(受信量/再生量/滞留 lag/間引き drop)。
/// 無音期間はキープアライブ受信時に、鳴っている間はフレーム受信時に呼ばれる。
/// drop が増え続けていれば波形を切っている=音割れの原因として疑う
fn diag_log(last_diag: &mut std::time::Instant, rate: u32) {
    if last_diag.elapsed() < std::time::Duration::from_secs(10) {
        return;
    }
    *last_diag = std::time::Instant::now();
    let rx = RX_BYTES.load(Ordering::Relaxed) / 1024;
    let play = PLAY_BYTES.load(Ordering::Relaxed) / 1024;
    let queued = RING.lock().unwrap_or_else(|e| e.into_inner()).len();
    let lag_ms = queued as u64 * 1000 / (rate as u64 * 8);
    let drop = DROP_BYTES.swap(0, Ordering::Relaxed) / 1024;
    eprintln!(
        "[audio] diag rx={rx}KB played={play}KB queued={}KB lag={lag_ms}ms drop={drop}KB",
        queued / 1024
    );
}
