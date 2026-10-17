// 性能改善ループ4(全23回): 音声(Mac側ミックス再生)の計測。
// (a) VecDeque<u8> のサンプル毎インデックス / (b) Vec<u8> drain 後の chunks_exact(4)
// 実行: cargo run --release --example perf-audio-mix --manifest-path crates/common/Cargo.toml
use std::collections::VecDeque;
use std::hint::black_box;
use std::time::Instant;

/// 時間ベクトルの p50 を ns で返す(ソートして中央値)
fn p50(mut times: Vec<u128>) -> u128 {
    times.sort();
    times[times.len() / 2]
}

const SOFT_B: usize = 2048 + 2048; // RING_PRE_ROLL(2048)+2048, audio.rs と同条件
const RING_PRE_ROLL: usize = 2048;

fn main() {
    let blocks = 2000u32;
    let bytes = 4096usize;

    // (a) VecDeque インデックス方式(audio.rs 1124-1130 と同一)
    let mut a_times = Vec::with_capacity(blocks as usize);
    for _ in 0..blocks {
        let mut rb: VecDeque<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
        if rb.len() > SOFT_B {
            let excess = ((rb.len() - RING_PRE_ROLL) / 32) & !7usize;
            let n = excess.min(rb.len());
            rb.drain(..n);
        }
        let take = rb.len().min(bytes) & !3usize;
        let mut dst = vec![0.0f32; take / 4];
        let t = Instant::now();
        for i in 0..take / 4 {
            let s = f32::from_le_bytes([
                rb[i * 4],
                rb[i * 4 + 1],
                rb[i * 4 + 2],
                rb[i * 4 + 3],
            ]);
            let cur = dst[i];
            let sum = cur + s;
            let out = if sum > 0.7 {
                0.7 + (sum - 0.7) / (1.0 + (sum - 0.7) * 4.0) * 0.3
            } else if sum < -0.7 {
                -0.7 - (-sum - 0.7) / (1.0 + (-sum - 0.7) * 4.0) * 0.3
            } else {
                sum
            };
            dst[i] = out;
        }
        black_box(dst);
        a_times.push(t.elapsed().as_nanos() as u128);
    }

    // (b) Vec drain + chunks_exact(4) 方式
    let mut b_times = Vec::with_capacity(blocks as usize);
    for _ in 0..blocks {
        let mut rb: Vec<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
        let mut tmp = Vec::with_capacity(rb.len());
        tmp.extend(rb.drain(..rb.len()));
        let take = tmp.len().min(bytes) & !3usize;
        let mut dst = vec![0.0f32; take / 4];
        let t = Instant::now();
        for (i, chunk) in tmp[..take].chunks_exact(4).enumerate() {
            let s = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let cur = dst[i];
            let sum = cur + s;
            let out = if sum > 0.7 {
                0.7 + (sum - 0.7) / (1.0 + (sum - 0.7) * 4.0) * 0.3
            } else if sum < -0.7 {
                -0.7 - (-sum - 0.7) / (1.0 + (-sum - 0.7) * 4.0) * 0.3
            } else {
                sum
            };
            dst[i] = out;
        }
        black_box(dst);
        b_times.push(t.elapsed().as_nanos() as u128);
    }

    let (ap, at) = (p50(a_times.clone()), a_times.iter().sum::<u128>());
    let (bp, bt) = (p50(b_times.clone()), b_times.iter().sum::<u128>());
    println!("a VecDeque idx : p50={ap} ns total={at} ns blocks={blocks} bytes/block={bytes}");
    println!("b Vec chunks   : p50={bp} ns total={bt} ns blocks={blocks} bytes/block={bytes}");
}
