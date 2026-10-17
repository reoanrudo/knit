// 性能改善ループ9(全23回): クリップボード履歴保存I/Oの計測。
// History::new(50) に 8KB テキスト 50 件を push し、save_to / load_from /
// proto::encode(&Msg::Clip{text:8KB}) の p50 と合計を計測する。
// 実行: cargo run --release --example perf-history --manifest-path crates/common/Cargo.toml
use std::hint::black_box;
use std::time::Instant;

use knit_common::history::History;
use knit_common::proto::{encode, Msg};

/// 時間ベクトルの p50 を ns で返す(ソートして中央値)
fn p50(mut times: Vec<u128>) -> u128 {
    times.sort();
    times[times.len() / 2]
}

/// 8KB の本文(ASCII 半角で 8192 バイト)
fn text_8kb() -> String {
    let mut s = String::with_capacity(8192);
    for i in 0..8192usize {
        s.push(char::from(b'A' + (i % 26) as u8));
    }
    s
}

/// 50 件で埋まった履歴(8KB 本文、内容は 1 件ごとに異なる)。
/// 連番は必ず先頭に置く: 末尾に置くと HISTORY_TEXT_BYTES(8KB)の切り詰めで
/// 消えて全件が同一本文になり、1 件へ集約されてしまう(リード検証で判明)
fn full_history() -> History {
    let mut h = History::new(50);
    for i in 0..50 {
        let mut s = format!("-{i}-");
        s.push_str(&text_8kb());
        h.push_text(&s, "Mac", i + 1);
    }
    h
}

fn main() {
    let text = text_8kb();
    let dir = std::env::temp_dir().join("knit-perf-history");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("history.json");

    let h = full_history();
    println!("history entries: {} (each text ~{} bytes)", h.entries().len(), text.len());

    // save_to: 100 回(50 件×8KB の serde_json 直列化 + tmp 書き込み + rename)
    let mut sav = Vec::with_capacity(100);
    for i in 0..100 {
        let t = Instant::now();
        h.save_to(black_box(&path));
        sav.push(t.elapsed().as_nanos());
    }
    println!("save_to: p50={}ns total={}ms", p50(sav.clone()), sav.iter().sum::<u128>() / 1_000_000);

    // to_bytes: 100 回(直列化のみ。ループ9 以降 history_save が HISTORY ロックを
    // 保持する時間はこちら。save_to - to_bytes = ロック外へ出たファイル I/O 分)
    let mut tob = Vec::with_capacity(100);
    for _ in 0..100 {
        let t = Instant::now();
        let b = h.to_bytes();
        tob.push(t.elapsed().as_nanos());
        assert!(black_box(b).is_some());
    }
    println!("to_bytes(ロック保持分): p50={}ns total={}ms", p50(tob.clone()), tob.iter().sum::<u128>() / 1_000_000);
    let save_p50 = p50(sav.clone()) as i64;
    let to_bytes_p50 = p50(tob.clone()) as i64;
    println!(
        "ロック保持時間の短縮: {}ns -> {}ns ({}ns 分の I/O をロック外へ)",
        save_p50,
        to_bytes_p50,
        save_p50 - to_bytes_p50
    );

    // load_from: 100 回(一度だけ保存して、そのファイルを 100 回読み込む)
    h.save_to(&path);
    let mut lo = History::new(50);
    let mut load = Vec::with_capacity(100);
    for _ in 0..100 {
        let t = Instant::now();
        lo.load_from(black_box(&path));
        load.push(t.elapsed().as_nanos());
    }
    println!("load_from: p50={}ns total={}ms", p50(load.clone()), load.iter().sum::<u128>() / 1_000_000);

    // proto::encode(&Msg::Clip{text: 8KB}): 1 万回(同期メッセージの経路)
    let mut enc = Vec::with_capacity(10_000);
    for i in 0..10_000 {
        let t = Instant::now();
        let s = encode(black_box(&Msg::Clip {
            text: format!("{text}-enc-{i}"),
        }));
        black_box(&s);
        enc.push(t.elapsed().as_nanos());
    }
    println!("clip_encode: p50={}ns total={}ms", p50(enc.clone()), enc.iter().sum::<u128>() / 1_000_000);

    let _ = std::fs::remove_dir_all(&dir);
}
