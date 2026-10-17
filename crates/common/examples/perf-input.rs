// 性能改善ループ3(全23回): 入力レイテンシ(Mac→Windows 送信経路)の計測。
// proto::encode / proto::decode の p50 と合計を計測する。
// 実行: cargo run --release --example perf-input --manifest-path crates/common/Cargo.toml
use std::hint::black_box;
use std::time::Instant;
use knit_common::proto::{decode, encode, Msg};

/// 時間ベクトルの p50 を ns で返す(ソートして中央値)
fn p50(mut times: Vec<u128>) -> u128 {
    times.sort();
    times[times.len() / 2]
}

fn main() {
    let n = 100_000u32;
    let msg = Msg::MouseAbs { nx: 0.5, ny: 0.75 };
    let line = encode(&msg);

    let mut enc = Vec::with_capacity(n as usize);
    for i in 0..n {
        let t = Instant::now();
        let s = encode(black_box(&Msg::MouseAbs {
            nx: (i % 1000) as f64 / 1000.0,
            ny: ((i * 7) % 1000) as f64 / 1000.0,
        }));
        black_box(&s);
        enc.push(t.elapsed().as_nanos());
    }
    println!("encode: p50={}ns total={}ms", p50(enc.clone()), enc.iter().sum::<u128>() / 1_000_000);

    let mut dec = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let t = Instant::now();
        let m = decode(black_box(&line));
        black_box(&m);
        dec.push(t.elapsed().as_nanos());
    }
    println!("decode: p50={}ns total={}ms", p50(dec.clone()), dec.iter().sum::<u128>() / 1_000_000);
}
