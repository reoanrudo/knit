use crate::audio;
use crate::dragdrop;
use crate::tray;
use crate::input::list_monitors;
use crate::registration_authenticated;
use crate::session::session;
use crate::state::{
    log_safe, now_ms, CONNECTED, MAIN_SHUTDOWN, METRIC_CONNECTS, METRIC_DROPS, METRIC_MAX_GAP_MS,
    METRIC_TOTAL_GAP_MS, RTT_MS, SPK_MUTE_MODE, WAKE, WTX,
};
use crate::xfer::{BULK_LINK, RX_BYTES, RX_DRAG};
use knit_common::proto::{compatible, decode, encode, safe_peer_name, Msg, VERSION};
use knit_common::secure;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::exit;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// 最終接続時刻と断の検知時刻(unix ms)、次の再試行時刻(0=待ちなし)
static LAST_CONNECTED_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DISCONNECTED_SINCE_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static NEXT_RETRY_AT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 接続が確立した時の共通処理(時刻の記録と、長い断の後の再接続通知)
fn mark_connected() {
    CONNECTED.store(true, Ordering::Relaxed);
    NEXT_RETRY_AT_MS.store(0, Ordering::Relaxed);
    let now = now_ms();
    let since = DISCONNECTED_SINCE_MS.swap(0, Ordering::Relaxed);
    LAST_CONNECTED_MS.store(now, Ordering::Relaxed);
    // 接続安定性の実測(scripts/stability-report.sh の集計源)。since=0 は断なしの接続
    let gap = if since == 0 { 0 } else { now.saturating_sub(since) };
    println!("[conn-metric] connected unix_ms={now} gap_ms={gap}");
    // 稼働計測(診断の安定性表示の源)。断=再接続が成功した回数のみ数える
    if gap > 0 {
        METRIC_DROPS.fetch_add(1, Ordering::Relaxed);
        METRIC_TOTAL_GAP_MS.fetch_add(gap, Ordering::Relaxed);
        METRIC_MAX_GAP_MS.fetch_max(gap, Ordering::Relaxed);
    }
    METRIC_CONNECTS.fetch_add(1, Ordering::Relaxed);
    if since != 0 && now.saturating_sub(since) > 60_000 {
        conn_notify(true, "Mac と再接続しました");
    }
}

/// 切断を記録する(次の再接続までの表示と、長い断の判定に使う)
fn note_disconnected() {
    CONNECTED.store(false, Ordering::Relaxed);
    println!("[conn-metric] lost unix_ms={}", now_ms());
    DISCONNECTED_SINCE_MS
        .compare_exchange(0, now_ms(), Ordering::Relaxed, Ordering::Relaxed)
        .ok();
}

/// 最終接続の表示(ステータス窓のフッター用)
pub fn last_connected_line() -> Option<String> {
    let ms = LAST_CONNECTED_MS.load(Ordering::Relaxed);
    if ms == 0 {
        return None;
    }
    let ago = now_ms().saturating_sub(ms) / 1000;
    let text = match ago {
        0..=4 => "今しがた".to_string(),
        5..=59 => format!("{ago}秒前"),
        60..=3599 => format!("{}分前", ago / 60),
        _ => format!("{}時間前", ago / 3600),
    };
    Some(format!("最終接続 {text}"))
}

/// 次の再接続試行までの表示(接続待ちの見える化)。
/// 文言の組み立ては common::diagnose で Mac 側と共用
pub fn next_retry_line() -> Option<String> {
    knit_common::diagnose::next_retry_line(
        NEXT_RETRY_AT_MS.load(Ordering::Relaxed),
        CONNECTED.load(Ordering::Relaxed),
        now_ms(),
    )
}

/// 相手を見つけられない状態の継続表示(1 分を超えた断だけ出す)。
/// knit-win.log に「Mac を発見できず」の無限リトライが何時間も続いた
/// 実ログへの対処: 状態をログでしか確認できない問題をステータス窓の
/// 常時表示で解く。文言の組み立ては common::diagnose で Mac 側と共用
pub fn not_found_line() -> Option<String> {
    knit_common::diagnose::not_found_line(
        DISCONNECTED_SINCE_MS.load(Ordering::Relaxed),
        CONNECTED.load(Ordering::Relaxed),
        now_ms(),
    )
}

/// 接続・切断通知の間引き(断続的な切替で連打しない。種別が変われば都度出す)
static CONN_NOTIFY: std::sync::Mutex<Option<(bool, Instant)>> = std::sync::Mutex::new(None);

/// 接続・切断の通知(種別が同じものは 60 秒に 1 回に間引く)。
/// 初回や状態が変わったときは必ず出る
fn conn_notify(connected: bool, text: &str) {
    let now = Instant::now();
    let due = {
        let mut g = CONN_NOTIFY.lock().unwrap_or_else(|e| e.into_inner());
        let ok = match *g {
            Some((same_kind, at)) => {
                same_kind != connected || now.duration_since(at) >= Duration::from_secs(60)
            }
            None => true,
        };
        if ok {
            *g = Some((connected, now));
        }
        ok
    };
    if due {
        tray::notify("Knit", text);
    }
}

/// 接続モード(既定): 相手(Mac)へ接続し続ける。切断は指数バックオフで再接続
/// 本線がいま繋がっている Mac のアドレス(大容量経路・音声はここへ追従する)
pub(crate) static PEER: std::sync::Mutex<Option<std::net::IpAddr>> = std::sync::Mutex::new(None);
/// 接続相手の名前(hello で受け取る)。通知・ログへ出す
pub static PEER_NAME: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// 相手(Mac)に表示するこの端末の名前。登録(pairing)で名乗る COMPUTERNAME と
/// 同じ値を hello/hello_ok にも載せる(複数台接続の区別のため)
fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Windows".into())
}

pub(crate) fn peer_ip() -> Option<std::net::IpAddr> {
    *PEER.lock().unwrap_or_else(|e| e.into_inner())
}

/// 接続モード(既定): 候補(KNIT_HOST のカンマ区切り)へ同時に接続を試み、
/// 最初に繋がった経路を使う。切断は指数バックオフ+フルジッターで再接続し、
/// 電源イベント(WAKE)で待機を飛ばしてすぐ再試行する
pub(crate) fn client_loop(hosts: Option<String>, port: u16, token: &str, w: i32, h: i32) {
    let mut backoff = knit_common::retry::Backoff::new(
        Duration::from_millis(500),
        Duration::from_secs(10),
    );
    loop {
        let addrs = knit_common::connect::resolve(hosts.as_deref(), port, token);
        if addrs.is_empty() {
            // 候補が空なら first_reachable を呼ばない(スレッド 0 のまま 3.5 秒待つだけの
            // 無駄。発見 600ms と合算して再接続が遅れる)
            println!(
                "[conn] 接続先が見つかりません(LAN の Mac を発見できず KNIT_HOST の候補も空です)"
            );
            let delay = backoff.next_delay();
            NEXT_RETRY_AT_MS.store(now_ms() + delay.as_millis() as u64, Ordering::Relaxed);
            WAKE.sleep(delay);
            continue;
        }
        let t0 = Instant::now();
        match knit_common::connect::first_reachable(&addrs, Duration::from_secs(3)) {
            Some((s, a)) => {
                println!("[conn] connected ({a}) in {}ms", t0.elapsed().as_millis());
                *PEER.lock().unwrap_or_else(|e| e.into_inner()) = Some(a.ip());
                *crate::tray::HOST_NOW
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = a.ip().to_string();
                // 起動一発目は LAN 発見が間に合わず Tailscale へ落ちることがある
                //(発見は 600ms で諦めるため)。Tailscale 接続の間は 30 秒毎に LAN を
                // 探し直し、見つかれば本線を張り直して次の再接続で LAN 直へ昇格する
                if knit_common::net::is_tailscale(a.ip()) {
                    let tk = token.to_string();
                    // 起動一発目は LAN 発見が間に合わず Tailscale へ落ちることがある。
                    // Tailscale 接続の間は 30 秒毎に LAN を探し直し、見つかれば本線を
                    // 張り直して次の再接続で LAN 直へ昇格する。
                    // この時点の LAST_CONNECTED_MS は前回セッションの値のため、開始時に
                    // 読んで比較すると 30 秒後の判定が常に「別セッション」となり監視が
                    // 自滅していた。今回の確立(mark_connected による更新)を待ってから
                    // 監視に入る
                    let prev_connected_ms = LAST_CONNECTED_MS.load(Ordering::Relaxed);
                    std::thread::spawn(move || {
                        let mut waited_ms = 0u64;
                        let established = loop {
                            std::thread::sleep(Duration::from_millis(500));
                            let cur = LAST_CONNECTED_MS.load(Ordering::Relaxed);
                            if cur != prev_connected_ms && CONNECTED.load(Ordering::Relaxed) {
                                break cur;
                            }
                            waited_ms += 500;
                            if waited_ms >= 15_000 {
                                return; // この接続は確立しなかった(監視しない)
                            }
                        };
                        loop {
                            std::thread::sleep(Duration::from_secs(30));
                            if !CONNECTED.load(Ordering::Relaxed)
                                || LAST_CONNECTED_MS.load(Ordering::Relaxed) != established
                            {
                                return; // セッション終了済み/別セッションに切り替わった
                            }
                            match peer_ip() {
                                Some(p) if !knit_common::net::is_tailscale(p) => return, // 昇格済み
                                None => return,
                                _ => {}
                            }
                            if let Some(ip) = knit_common::discover::seek_first_lan(port, &tk) {
                                println!("[conn] LAN 直の相手を発見({ip})。経路昇格のため張り直します");
                                if let Some(tx) = WTX.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                                {
                                    let _ = tx.send(MAIN_SHUTDOWN.to_string());
                                }
                                return;
                            }
                        }
                    });
                }
                // TCP だけ繋がる相手(LAN 発見で拾った旧版・別トークンの応答者)はハンドシェイクで
                // 失敗する。セッション失敗まで backoff をリセットすると高頻度の無限再試行に
                // なるため、リセットはセッションが最後まで成功した時のみ
                match client_session(s, token, w, h) {
                    Ok(()) => backoff.reset(),
                    Err(e) => println!("[disc] {e}"),
                }
            }
            None => println!("[conn] failed: どの接続先にも繋がりません"),
        }
        let delay = backoff.next_delay();
        NEXT_RETRY_AT_MS.store(now_ms() + delay.as_millis() as u64, Ordering::Relaxed);
        WAKE.sleep(delay);
    }
}

/// 受信1行の上限(hello と本線で同じ値。巨大1行の無制限メモリ確保を防ぐ)
pub(crate) const MAX_LINE: u64 = 8 * 1024 * 1024;
/// 進行中のハンドシェイク数(黙る相手の接続で待ちが積み上がらないよう上限を設ける)
static HANDSHAKE_PENDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// ハンドシェイク+hello 検証+hello_ok 送信までを1接続ぶんだけ行う。
/// accept ループは別スレッド+絶対期限(15秒)でこの関数を待つため、
/// 接続だけして黙る相手(slowloris・クラッシュループ)が待ち受けを塞がない
fn handshake_and_hello(
    stream: TcpStream,
    token: &str,
    w: i32,
    h: i32,
) -> Result<(std::io::BufReader<secure::Reader>, secure::Writer), String> {
    use std::io::{BufRead, Read};
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(9))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let (r, mut wr) = secure::accept(stream, token, b"knit-main").map_err(|e| {
        format!("暗号化ハンドシェイク失敗: {e}(トークン不一致の可能性)")
    })?;
    let mut pre = std::io::BufReader::new(r);
    let mut line = String::new();
    match (&mut pre).take(MAX_LINE + 1).read_line(&mut line) {
        Ok(0) | Err(_) => return Err("closed before hello".into()),
        Ok(_) => {}
    }
    if line.len() as u64 > MAX_LINE {
        return Err("hello too large. dropped".into());
    }
    let ok = match decode(&line) {
        Some(Msg::Hello {
            ver,
            name,
            monitors,
            ..
        }) if compatible(ver) => {
            dragdrop::edge::PEER_VERSION.store(ver, Ordering::Relaxed);
            // 表示名は制御文字・Bidi オーバーライドを除去してから載せる
            *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = {
                let n = safe_peer_name(name.trim());
                if n.is_empty() {
                    "Mac".into()
                } else {
                    n
                }
            };
            println!(
                "[hello] from {} モニター: {}",
                log_safe(&name),
                knit_common::proto::Monitor::summary(&monitors)
            );
            true
        }
        _ => false,
    };
    if !ok {
        return Err("invalid hello".into());
    }
    // hello_ok(相手=Mac が画面サイズを得られるよう自画面 w/h と全モニターを含れる)。
    // name には登録時と同じ端末名(COMPUTERNAME)を名乗る: 複数台を Mac に繋いだ
    // ときにピア選択・通知で区別できるようにする(固定文字列だと全台同名になる)
    if wr
        .write_all(
            encode(&Msg::HelloOk {
                name: device_name(),
                w,
                h,
                ver: VERSION,
                id: knit_common::proto::device_id(),
                monitors: list_monitors(),
            })
            .as_bytes(),
        )
        .and_then(|_| wr.flush())
        .is_err()
    {
        return Err("hello_ok 送信に失敗しました".into());
    }
    Ok((pre, wr))
}

/// 待受モード(KNIT_ROLE=server): 相手(Mac=クライアント)からの接続を受け入れる。
/// hello のトークン検証後に hello_ok(自画面 w/h 付き)を返す
pub(crate) fn server_loop(token: &str, port: u16, w: i32, h: i32) {
    let bind_ip = knit_common::envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[fatal] listen {bind_ip}:{port} failed: {e}");
            exit(1);
        }
    };
    println!("[info] listening on {bind_ip}:{port}");
    // 認証に失敗し続ける接続の連打を鈍らせる(正規の接続が成功すれば即回復)
    let mut throttle = secure::FailThrottle::new();
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(x) => {
                knit_common::net::tune_tcp(&x.0);
                x
            }
            Err(e) => {
                println!("[conn] accept error: {e}");
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        println!("[conn] accepted from {peer}");
        // 接続元の制限(既定は LAN・有線直結のみ。Tailscale は KNIT_ALLOW_TS=1、
        // インターネット側は KNIT_ALLOW_ANY=1 で許可)。認証は暗号化ハンドシェイクで行う
        if !knit_common::net::is_allowed(peer.ip()) {
            println!(
                "[conn] rejected: {peer} は許可範囲外です(Tailscale は KNIT_ALLOW_TS=1、他ネットワークは KNIT_ALLOW_ANY=1 で許可)"
            );
            std::thread::sleep(throttle.fail());
            continue;
        }
        // ハンドシェイク+hello に絶対期限を切る(接続だけして黙る相手が待ち受けを塞ぐと、
        // 正規の再接続まで受け付けられなくなるため)。同時に待つハンドシェイクも少量に絞る
        if HANDSHAKE_PENDING.load(Ordering::Relaxed) >= 4 {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        HANDSHAKE_PENDING.fetch_add(1, Ordering::Relaxed);
        let (hs_tx, hs_rx) = std::sync::mpsc::channel();
        {
            let token = token.to_string();
            std::thread::spawn(move || {
                let out = handshake_and_hello(stream, &token, w, h);
                HANDSHAKE_PENDING.fetch_sub(1, Ordering::Relaxed);
                let _ = hs_tx.send(out);
            });
        }
        let (pre, wr) = match hs_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => {
                println!("[conn] {e}");
                std::thread::sleep(throttle.fail());
                continue;
            }
            Err(_) => {
                println!("[conn] ハンドシェイクが15秒で完了しないため切断({peer})");
                std::thread::sleep(throttle.fail());
                continue;
            }
        };
        *PEER.lock().unwrap_or_else(|e| e.into_inner()) = Some(peer.ip());
        mark_connected();
        throttle.success();
        registration_authenticated(token);
        println!("[conn] established");
        conn_notify(
            true,
            &format!(
                "{} と接続しました",
                PEER_NAME.lock().map(|n| n.clone()).unwrap_or_default()
            ),
        );
        audio::speaker_connect_mute(SPK_MUTE_MODE.load(Ordering::Relaxed));
        if let Err(e) = session(pre, wr) {
            println!("[disc] {e}");
        }
        note_disconnected();
        // 切断で進行中の掴み越境ドラッグを残さない(以降はタイマー保険も引き継ぐ)
        dragdrop::relay_cancel();
        dragdrop::edge::reset();
        // 相手の版も戻す: 未接続=版 0 にしないと、切断中の送信判定が前回接続の
        // 版のままで実態と乖離する(Mac 側 on_disconnect と同じ不変条件)
        dragdrop::edge::PEER_VERSION.store(0, Ordering::Relaxed);
        RTT_MS.store(0, Ordering::Relaxed);
        *WTX.lock().unwrap_or_else(|e| e.into_inner()) = None;
        BULK_LINK.clear();
        // サーバー側の切断でも受信表示を解く(client_session 側と同一の後始末)
        *RX_DRAG.lock().unwrap_or_else(|e| e.into_inner()) = None;
        RX_BYTES.store(0, Ordering::Relaxed);
        speaker_disconnect_if_audio_down();
        println!("[conn] lost. waiting for reconnect...");
        conn_notify(false, "切断しました(自動で再接続します)");
    }
}

/// 接続モード(既定)のセッション: hello 送信 → hello_ok 受信 → 本体セッション
fn client_session(stream: TcpStream, token: &str, w: i32, h: i32) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    // 暗号化ハンドシェイクと hello_ok 待ちに期限を切る(Mac が accept 後に応答しない場合に
    // 再接続ループへ戻れるように)。本体の受信タイムアウトは session で設定し直す
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let (r, mut writer) = secure::connect(stream, token, b"knit-main").map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("暗号化ハンドシェイク失敗: {e}(トークン不一致の可能性)"),
        )
    })?;
    let hello = encode(&Msg::Hello {
        ver: VERSION,
        name: device_name(),
        token: String::new(),
        w,
        h,
        id: knit_common::proto::device_id(),
        monitors: list_monitors(),
    });
    writer
        .write_all(hello.as_bytes())
        .and_then(|_| writer.flush())?;
    // hello_ok と後続(Cfg 等)を同じ受信器で読む。旧実装は hello_ok 用と本体用で
    // BufReader を別々に作り、直後に届いた Cfg を前者のバッファに取り残して失っていた。
    // 読み取りは待受側(handshake_and_hello)と同じ行長上限付きにする
    let mut pre = BufReader::new(r);
    let mut line = String::new();
    {
        use std::io::Read;
        (&mut pre).take(MAX_LINE + 1).read_line(&mut line)?;
    }
    if line.len() as u64 > MAX_LINE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "hello_ok too large",
        ));
    }
    match decode(line.trim()) {
        Some(Msg::HelloOk {
            name,
            w: mw,
            h: mh,
            ver,
            monitors,
            ..
        }) => {
            dragdrop::edge::PEER_VERSION.store(ver, Ordering::Relaxed);
            *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = {
                let n = safe_peer_name(name.trim());
                if n.is_empty() {
                    "Mac".into()
                } else {
                    n
                }
            };
            println!(
                "[hello] ok from {} (mac screen {mw}x{mh}) モニター: {}",
                log_safe(&name),
                knit_common::proto::Monitor::summary(&monitors)
            );
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid hello_ok",
            ))
        }
    }
    mark_connected();
    registration_authenticated(token);
    conn_notify(
        true,
        &format!(
            "{} と接続しました",
            PEER_NAME.lock().map(|n| n.clone()).unwrap_or_default()
        ),
    );
    audio::speaker_connect_mute(SPK_MUTE_MODE.load(Ordering::Relaxed));
    let r = session(pre, writer);
    note_disconnected();
    // 切断で進行中の掴み越境ドラッグを残さない(以降はタイマー保険も引き継ぐ)
    dragdrop::relay_cancel();
    dragdrop::edge::reset();
    // 相手の版も戻す(未接続=版 0。Mac 側 on_disconnect と同じ不変条件)
    dragdrop::edge::PEER_VERSION.store(0, Ordering::Relaxed);
    // 進行中表示も止める(送信はエラーで、受信は切断でそれぞれ終わる)
    *RX_DRAG.lock().unwrap_or_else(|e| e.into_inner()) = None;
    RX_BYTES.store(0, Ordering::Relaxed);
    // 旧セッションの RTT が再接続直後に「前の接続の値」として表示されるのを防ぐ
    RTT_MS.store(0, Ordering::Relaxed);
    // 旧セッションの送信チャネルを外す(切断中にトレイ等が旧ソケットへ書き込むのを防ぐ)
    *WTX.lock().unwrap_or_else(|e| e.into_inner()) = None;
    BULK_LINK.clear();
    speaker_disconnect_if_audio_down();
    conn_notify(false, "切断しました(自動で再接続します)");
    r
}

/// 本線切断時のスピーカーミュート解除。音声ストリーム(独立スレッド・無限再接続)
/// がまだ確立している間は解除しない: 本線だけの瞬断でここを解除すると、
/// Mac から鳴っている音と Windows スピーカーの音が二重に発音する。
/// 音声線も切れていれば(Mac 側の終了・ネットワーク断)通常どおり復元する。
/// ミュートを維持したままでも、音声線が死んだ後の本線再接続で改めて
/// 適用し直されるため、復元機会は失われない(spkstate の退避記録にも残る)
fn speaker_disconnect_if_audio_down() {
    if audio::AUDIO_LINK_UP.load(Ordering::Relaxed) {
        println!("[spk] 本線切断。音声ストリームが生存のためミュートを維持します");
    } else {
        audio::speaker_disconnect();
    }
}
