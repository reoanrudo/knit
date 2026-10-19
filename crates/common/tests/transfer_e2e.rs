//! 実ソケット + Noise での大容量経路の結合試験。フレーム列の単体試験では
//! 通らない `spawn_reader`(受信スレッド)側の保持・破棄・公開を確かめる。
//! 127.0.0.1 の一時ポートを使い、外部に接続しない。
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use knit_common::bulk;
use knit_common::secure;

// bulk.rs の LABEL と同じ値(モジュール外へは出していないため、ここで固定する)
const LABEL: &[u8] = b"knit-bulk";
const TOKEN: &str = "e2e-token-0123456789abcdef0123456789abcdef";

static EVENTS: Mutex<Vec<bulk::Event>> = Mutex::new(Vec::new());
/// サーバの停止フラグはプロセス共通なので、テスト同士を直列化する
static E2E_LOCK: Mutex<()> = Mutex::new(());
static STOP: AtomicBool = AtomicBool::new(false);

fn on_event(e: bulk::Event) {
    EVENTS.lock().unwrap_or_else(|e| e.into_inner()).push(e);
}
fn no_log(_: &str) {}
fn no_bytes(_: u64) {}
fn no_begin() {}
fn no_bind_error(_: &str) {}

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "knit-e2e-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(p.join("received")).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 空きポートを1つ取る(取り合いは稀なので、取れた番号をそのまま使う)
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn endpoint(dir: &std::path::Path) -> &'static bulk::Endpoint {
    Box::leak(Box::new(bulk::Endpoint {
        link: Box::leak(Box::new(bulk::Link::new())),
        credential: bulk::EndpointCred::Single(TOKEN.to_string()),
        dir: dir.to_path_buf(),
        on_event,
        log: no_log,
        on_rx_bytes: no_bytes,
        on_batch_begin: no_begin,
        on_bind_error: no_bind_error,
    }))
}

fn start_server(ep: &'static bulk::Endpoint, port: u16) {
    STOP.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        bulk::serve(ep, "127.0.0.1", port, |_| true, || STOP.load(Ordering::Relaxed))
    });
}

/// 待受が立ち上がるまで接続を試す(handshake まで通った組を返す)
fn connect(port: u16) -> (secure::Reader, secure::Writer) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(10))).ok();
            s.set_write_timeout(Some(Duration::from_secs(10))).ok();
            if let Ok(pair) = secure::connect(s, TOKEN, LABEL) {
                return pair;
            }
        }
        assert!(
            Instant::now() < deadline,
            "待受が立ち上がりません(port={port})"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_until(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !f() {
        assert!(Instant::now() < deadline, "待ち時間切れ: {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn partial_count(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir.join(knit_common::files::RESUME_DIR))
        .map(|rd| rd.count())
        .unwrap_or(0)
}

/// 切断で部分を保持し、再接続で受信済みを送り直さずに完了する
#[test]
fn disconnect_retains_partial_and_reconnect_resumes_over_real_secure_link() {
    let _serial = E2E_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp = Temp::new("resume");
    let dst = temp.0.join("received");
    let ep = endpoint(&dst);
    let port = free_port();
    start_server(ep, port);

    let src = temp.0.join("big.bin");
    // 2MiB(チャンク 256KiB × 8)。途中で確実に切る
    let data: Vec<u8> = (0..2 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&src, &data).unwrap();
    let entries = bulk::collect(std::slice::from_ref(&src), true, true, false).unwrap();

    // 1回目: 2チャンク送って中止し、接続を切る(中止フレームは送らない)
    {
        let (_r, mut w) = connect(port);
        let mut chunks = 0u32;
        let _ = bulk::send_entries(
            &mut w,
            &entries,
            false,
            None,
            &mut || {
                chunks += 1;
                chunks > 2
            },
            &mut |_, _, _| {},
        );
    }
    wait_until("部分ファイルが保持される", || partial_count(&dst) == 1);
    assert!(!dst.join("big.bin").exists(), "未完の実体は公開しない");

    // 受信側と同じ手順で位置を求め、残りだけを送る
    let items = bulk::resume_items(&entries);
    let positions = bulk::resume_positions(&dst, &items);
    let offsets = bulk::resume_offsets_of(&entries, &positions);
    assert!(offsets[0] > 0, "受信済みの位置が返る");
    {
        let (_r, mut w) = connect(port);
        bulk::send_entries_from(&mut w, &entries, &offsets, false, None, &mut || false, &mut |_, _, _| {})
            .unwrap();
    }
    wait_until("公開される", || {
        std::fs::metadata(dst.join("big.bin"))
            .map(|m| m.len() == data.len() as u64)
            .unwrap_or(false)
    });
    assert_eq!(std::fs::read(dst.join("big.bin")).unwrap(), data, "内容が一致");
    assert_eq!(partial_count(&dst), 0, "完了で部分が消える");
    STOP.store(true, Ordering::Relaxed);
}

/// 明示中止は部分を残さない(切断との区別)
#[test]
fn explicit_cancel_discards_the_partial_over_real_secure_link() {
    let _serial = E2E_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp = Temp::new("cancel");
    let dst = temp.0.join("received");
    let ep = endpoint(&dst);
    let port = free_port();
    start_server(ep, port);

    let src = temp.0.join("big.bin");
    std::fs::write(&src, vec![5u8; 600 * 1024]).unwrap();
    let entries = bulk::collect(std::slice::from_ref(&src), true, true, false).unwrap();

    {
        let (_r, mut w) = connect(port);
        let mut chunks = 0u32;
        // send_entries_from は中止時に RESUME_DROP を送る
        let _ = bulk::send_entries_from(
            &mut w,
            &entries,
            &[0],
            false,
            None,
            &mut || {
                chunks += 1;
                chunks > 1
            },
            &mut |_, _, _| {},
        );
    }
    // 中止フレームが届いて片付くまで待つ
    wait_until("部分が残らない", || partial_count(&dst) == 0);
    assert!(!dst.join("big.bin").exists(), "中止した実体は公開しない");
    STOP.store(true, Ordering::Relaxed);
}
