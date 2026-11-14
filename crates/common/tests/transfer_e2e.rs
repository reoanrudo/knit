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

/// 逆方向構成の本質的な経路: 待受側が受理した 1 つの bulk 接続を双方向に**同時に**
/// 使っても両方向の転送が完了する。接続側は 10 秒毎に keepalive を送るため、
/// 待受側の応答(reply_keepalive)が送信中のロック待ちで受信ループを止める旧回帰
/// (逆方向のデータが吸われなくなり相手の書き込みタイムアウトで転送が死ぬ)が
/// 再発するとこの試験の完了が止まる。実アプリと同じ connect_loop + link.send で
/// 駆動し、60 秒の完了期限で回帰を壁にする
#[test]
fn one_link_simultaneous_bidirectional_transfer_completes_both_ways() {
    let _serial = E2E_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_a = Temp::new("simul-a");
    let temp_b = Temp::new("simul-b");
    let dst_a = temp_a.0.join("received");
    let dst_b = temp_b.0.join("received");
    let ep_a = endpoint(&dst_a); // 接続しに行く側(逆方向の Mac=クライアント役相当)
    let ep_b = endpoint(&dst_b); // 待受側(逆方向の Windows=サーバ役相当)
    let port_b = free_port();
    start_server(ep_b, port_b);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port_b));
    std::thread::spawn(move || {
        bulk::connect_loop(ep_a, || Some(addr), || true, || STOP.load(Ordering::Relaxed))
    });
    wait_until(
        "受理した 1 接続で双方の link が確立",
        || ep_a.link.is_up() && ep_b.link.is_up(),
    );

    // 8MiB(32 チャンク)。進捗を 500ms/chunk で引き伸ばし計約 16 秒: 接続側の
    // keepalive(10 秒周期)が相手側の送信中に確実に発生する窓を作る
    let data_a: Vec<u8> = (0..8 * 1024 * 1024u32).map(|i| (i % 251) as u8 ^ 0x5a).collect();
    let data_b: Vec<u8> = (0..8 * 1024 * 1024u32).map(|i| (i % 249) as u8 ^ 0xa5).collect();
    let src_a = temp_a.0.join("a-to-b.bin");
    let src_b = temp_b.0.join("b-to-a.bin");
    std::fs::write(&src_a, &data_a).unwrap();
    std::fs::write(&src_b, &data_b).unwrap();

    let slow_send = |ep: &'static bulk::Endpoint, src: std::path::PathBuf| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(ep.link.send(|w| {
                bulk::send_files_with_progress(w, std::slice::from_ref(&src), false, |_, _| {
                    std::thread::sleep(Duration::from_millis(500));
                })
            }));
        });
        rx
    };
    let rx_ab = slow_send(ep_a, src_a.clone());
    let rx_ba = slow_send(ep_b, src_b.clone());

    let report_ab = rx_ab
        .recv_timeout(Duration::from_secs(60))
        .expect("A→B が 60 秒以内に完了しない(逆方向の停滞回帰の疑い)")
        .expect("A→B の送信が成功する");
    let report_ba = rx_ba
        .recv_timeout(Duration::from_secs(60))
        .expect("B→A が 60 秒以内に完了しない(逆方向の停滞回帰の疑い)")
        .expect("B→A の送信が成功する");
    assert_eq!(report_ab.sent, 1);
    assert_eq!(report_ba.sent, 1);

    wait_until(
        "A→B の実体が公開される",
        || {
            std::fs::metadata(dst_b.join("a-to-b.bin"))
                .map(|m| m.len() == data_a.len() as u64)
                .unwrap_or(false)
        },
    );
    wait_until(
        "B→A の実体が公開される",
        || {
            std::fs::metadata(dst_a.join("b-to-a.bin"))
                .map(|m| m.len() == data_b.len() as u64)
                .unwrap_or(false)
        },
    );
    assert_eq!(std::fs::read(dst_b.join("a-to-b.bin")).unwrap(), data_a, "A→B の内容");
    assert_eq!(std::fs::read(dst_a.join("b-to-a.bin")).unwrap(), data_b, "B→A の内容");
    assert_eq!(partial_count(&dst_a), 0, "A 側に部分ファイルが残らない");
    assert_eq!(partial_count(&dst_b), 0, "B 側に部分ファイルが残らない");
    assert!(
        ep_a.link.is_up() && ep_b.link.is_up(),
        "同時双方向転送の後も 1 接続の経路は生きている"
    );
    STOP.store(true, Ordering::Relaxed);
}
