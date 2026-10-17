// 性能改善ループ2(全23回): 計測ベースライン。
// 代表操作の所要時間を std::time のみで計測し、後続ループ(3-22)の改善前後比較の基準とする。
// 実行: cargo run --release --example perf-baseline --manifest-path crates/common/Cargo.toml
use std::hint::black_box;
use std::time::Instant;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Event {
    id: u64,
    kind: String,
    payload: f64,
}

/// 時間ベクトルの p50 を ns で返す(ソートして中央値)
fn p50(mut times: Vec<u128>) -> u128 {
    times.sort();
    times[times.len() / 2]
}

fn bench(name: &str, n: u32, mut f: impl FnMut(u32) -> u128) {
    let mut times: Vec<u128> = Vec::with_capacity(n as usize);
    for i in 0..n {
        times.push(f(i));
    }
    let total_ns: u128 = times.iter().copied().sum();
    println!("{} n={} p50={}ns total={}ms", name, n, p50(times), total_ns / 1_000_000);
}

fn main() {
    bench("serde_json roundtrip x10000", 10_000, |i| {
        let t = Instant::now();
        let e = Event { id: i as u64, kind: "keystroke".into(), payload: 3.14 };
        let s = serde_json::to_string(&e).unwrap();
        let _b: Event = serde_json::from_str(&s).unwrap();
        t.elapsed().as_nanos()
    });

    bench("clone 64KB Vec<u8> x1000", 1_000, |_| {
        let buf = vec![0u8; 64 * 1024];
        let t = Instant::now();
        let c = buf.clone();
        black_box(&c);
        t.elapsed().as_nanos()
    });

    bench("clone 1MB Vec<u8> x1000", 1_000, |_| {
        let buf = vec![0u8; 1024 * 1024];
        let t = Instant::now();
        let c = buf.clone();
        black_box(&c);
        t.elapsed().as_nanos()
    });

    bench("string format x10000", 10_000, |i| {
        let t = Instant::now();
        let s = format!("event#{} payload={:.2} bytes", i, 1.5 + (i % 1000) as f64);
        black_box(&s);
        t.elapsed().as_nanos()
    });
}
