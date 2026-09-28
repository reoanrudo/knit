//! 10GiB を実際に暗号化転送する検証。通常の単体テストからは除外する。
//! cargo test --locked --release -p knit-common --test bulk_large -- --ignored --nocapture
use blake2::{Blake2s256, Digest};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use knit_common::{bulk, secure};

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn digest(path: &Path) -> Vec<u8> {
    let mut f = std::fs::File::open(path).unwrap();
    let mut h = Blake2s256::new();
    let mut buf = vec![0; bulk::CHUNK];
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            return h.finalize().to_vec();
        }
        h.update(&buf[..n]);
    }
}

#[test]
#[ignore = "10GiB の転送・照合を実行し、受信先に10GiBの空き容量を使う"]
fn encrypted_transfer_at_limit_then_another_batch() {
    let base = TempDir(std::env::temp_dir().join(format!(
            "knit-large-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        )));
    std::fs::create_dir_all(&base.0).unwrap();
    let source = base.0.join("large.bin");
    let small = base.0.join("after.txt");
    std::fs::write(&small, b"after large transfer").unwrap();
    // スパースファイルで送信元のディスク消費を抑え、4GiB 境界の前後にも目印を置く。
    let size = 10u64 * 1024 * 1024 * 1024;
    let mut file = std::fs::File::create(&source).unwrap();
    file.set_len(size).unwrap();
    for offset in [0, (1u64 << 32) - 8, size - 32] {
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(b"knit-large-file-marker").unwrap();
    }
    drop(file);
    eprintln!("送信元10GiBのハッシュを計算");
    let expected = digest(&source);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let destination = base.0.join("received");
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        let (reader, mut writer) =
            secure::accept(stream, "large-transfer-test", b"knit-bulk").unwrap();
        let mut reader = std::io::BufReader::with_capacity(bulk::CHUNK + 16, reader);
        let mut receiver = bulk::Receiver::new(&destination);
        let mut frame = Vec::new();
        let mut completed = Vec::new();
        while completed.len() < 2 {
            let kind = bulk::read_frame(&mut reader, &mut frame).unwrap();
            if let Some(bulk::Event::Files { paths, .. }) = receiver.feed(kind, &frame) {
                assert_eq!(paths.len(), 1);
                completed.push(paths[0].clone());
            }
        }
        assert_eq!(std::fs::metadata(&completed[0]).unwrap().len(), size);
        eprintln!("受信した10GiBのハッシュを照合");
        assert_eq!(digest(&completed[0]), expected);
        assert_eq!(
            std::fs::read(&completed[1]).unwrap(),
            b"after large transfer"
        );
        // 検証ハーネス内の完了通知。製品プロトコルの受領確認ではない。
        writer.write_all(b"ok").unwrap();
        writer.flush().unwrap();
    });
    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let (mut reader, mut writer) =
        secure::connect(stream, "large-transfer-test", b"knit-bulk").unwrap();
    let mut last_step = 0;
    assert_eq!(
        bulk::send_files_with_progress(&mut writer, &[source], false, |sent, total| {
            assert_eq!(total, size);
            let step = sent * 10 / total;
            if step > last_step {
                eprintln!("10GiB 転送: {}%", step * 10);
                last_step = step;
            }
        })
        .unwrap(),
        1
    );
    assert_eq!(last_step, 10);
    assert_eq!(bulk::send_files(&mut writer, &[small], false).unwrap(), 1);
    let mut ack = [0; 2];
    reader.read_exact(&mut ack).unwrap();
    assert_eq!(&ack, b"ok");
    server.join().unwrap();
}
