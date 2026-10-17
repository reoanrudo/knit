// 性能改善ループ5(全23回): ファイル転送(bulk)のスループット計測。
// secure::connect/accept 経由で bulk::write_frame/read_frame を流し、MB/s を出す。
// 計測は 127.0.0.1 の上限値(実ネットワークでは回線が支配的になる)。
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Instant;

use knit_common::bulk::{read_frame, write_frame, MAX_FRAME};
use knit_common::secure::{accept, connect};

const TOTAL: u64 = 64 * 1024 * 1024; // 64MB
const TOKEN: &str = "bench-token";
const LABEL: &[u8] = b"knit-bulk";

fn run_once(size: usize, label: &str) -> f64 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    // 受信側スレッド: accept -> secure::accept -> read_frame を数える
    let server = thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let (mut r, _w) = accept(s, TOKEN, LABEL).unwrap();
        let mut buf = Vec::new();
        let mut total: u64 = 0;
        let mut frames = 0u64;
        loop {
            match read_frame(&mut r, &mut buf) {
                Ok(_k) => {
                    total += buf.len() as u64;
                    frames += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(_) => break,
            }
        }
        (total, frames)
    });

    // 送信側: connect -> secure::connect -> write_frame で TOTAL を送出
    let start = Instant::now();
    let s = TcpStream::connect(addr).unwrap();
    let (mut _r, mut w) = connect(s, TOKEN, LABEL).unwrap();
    let chunk = vec![7u8; size];
    let mut sent = 0u64;
    while sent + (size as u64) < TOTAL {
        write_frame(&mut w, 0, &chunk).unwrap();
        sent += size as u64;
    }
    let rest = vec![9u8; (TOTAL - sent) as usize];
    write_frame(&mut w, 0, &rest).unwrap();
    w.flush().unwrap();
    w.shutdown();
    let elapsed = start.elapsed();

    let (received, frames) = server.join().unwrap();
    let mb = received as f64 / (1024.0 * 1024.0);
    let mbps = mb / elapsed.as_secs_f64();
    println!("[{label}] body={size}B frames={frames} received={received}B elapsed={:.3}s => {:.1} MB/s", elapsed.as_secs_f64(), mbps);
    mbps
}

fn main() {
    println!("bulkスループット計測(localhost上限値・合成データ)");
    // CHUNK 定数に依存せず、明示サイズで真の A/B を取る(定数を書き換えると両ケースが
    // 同じサイズになり比較が成立しないため)。ループ5 のリード検証で修正。
    let _a = run_once(256 * 1024, "256KB");
    let _b = run_once(MAX_FRAME, "1MB");
}
