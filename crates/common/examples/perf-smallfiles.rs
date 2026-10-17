// 性能改善ループ7(全23回): 小ファイル多数(4KB×2000)転送のスループット計測。
// secure::connect/accept 経由で bulk::send_files/Receiver を流し、files/s と MB/s を出す。
use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::Instant;

use knit_common::bulk::{read_frame, DATA, FILE_BEGIN, FILE_END, BATCH_END, DROP_BEGIN, DROP_END};
use knit_common::secure::{accept, connect};

const COUNT: usize = 500; // MAX_BATCH_FILES=512 以下(小ファイル多数の体感性能)
const SIZE: usize = 4 * 1024; // 4KB
const TOKEN: &str = "bench-token";
const LABEL: &[u8] = b"knit-bulk";

fn main() {
    let dir = std::env::temp_dir().join("knit-perf-smallfiles");
    std::fs::create_dir_all(&dir).unwrap();
    let mut paths = Vec::new();
    for i in 0..COUNT {
        let p = dir.join(format!("f_{i:04}.bin"));
        std::fs::write(&p, vec![0u8; SIZE]).unwrap();
        paths.push(p);
    }
    let _total_bytes = (COUNT as u64) * SIZE as u64;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    // 受信側: secure::accept → read_frame でファイル完了を数える
    let server = thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let (mut r, _w) = accept(s, TOKEN, LABEL).unwrap();
        let mut buf = Vec::new();
        let mut files = 0u64;
        let mut bytes = 0u64;
        loop {
            let k = match read_frame(&mut r, &mut buf) {
                Ok(k) => k,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(_) => break,
            };
            match k {
                DATA => bytes += buf.len() as u64,
                FILE_END => files += 1,
                BATCH_END => break,
                DROP_BEGIN | DROP_END | FILE_BEGIN => {}
                _ => {}
            }
        }
        (files, bytes)
    });

    let start = Instant::now();
    let s = TcpStream::connect(addr).unwrap();
    let (mut _r, mut w) = connect(s, TOKEN, LABEL).unwrap();
    knit_common::bulk::send_files(&mut w, &paths, false).unwrap();
    w.flush().unwrap();
    w.shutdown();
    let elapsed = start.elapsed();
    let (files, bytes) = server.join().unwrap();

    let secs = elapsed.as_secs_f64();
    let files_per_s = files as f64 / secs;
    let mb_per_s = (bytes as f64 / 1024.0 / 1024.0) / secs;
    println!("files={files} bytes={bytes} elapsed={:.3}s files/s={:.0} MB/s={:.2}", secs, files_per_s, mb_per_s);
}
