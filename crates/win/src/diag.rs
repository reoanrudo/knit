//! 接続診断の実測(Windows 側)。判定は common::diagnose(単体テスト済みの共通ロジック)。
//! Mac と同じ構成で、待ち受けはホスト役のときだけ要件のため、役割で listening を分ける
//! (クライアント役は相手が待ち受け、自分は待ち受け不要で常に要件を満たす)。
//! 安定性の実測はプロセス内カウンタ(state.rs)で、ログの場所に依存しない

use crate::state::{
    BOOT_WALL_MS, CONNECTED, METRIC_CONNECTS, METRIC_DROPS, METRIC_MAX_GAP_MS,
    METRIC_TOTAL_GAP_MS, RTT_MS,
};
use knit_common::credentials;
use knit_common::diagnose::{self, Facts, Recent};
use knit_common::proto;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// 診断を実行してレポート文字列を返す。IO を伴う(peer への確認で最大 3 秒)
pub fn run() -> String {
    let connected = CONNECTED.load(Ordering::Relaxed);
    let rtt_ms = {
        let r = RTT_MS.load(Ordering::Relaxed);
        (connected && r > 0).then_some(r)
    };
    let peer = credentials::load_peer();
    let has_peer = peer.is_some();
    // 待ち受けはホスト役のときだけ要件。クライアント役は相手が待ち受けるため常に要件を満たす
    let listening = !is_host_role() || probe_listening();
    // ファイル・画像経路(bulk)の待受もホスト役だけ。bulk::serve 自体の状態を
    // 見る(bind 再試行中かを含む)
    let bulk_listening = is_host_role().then(knit_common::bulk::serving);
    // 音声(24901)の待受: Windows は音声を Mac へ送る側のため待受を持たない
    //(常に未測定=行を出さない)。自動発見(UDP 24903)の応答待受はホスト役だけ
    let audio_listening = None;
    let discover_listening = is_host_role().then(probe_discover_listening);
    let peer_reachable = peer.map(|mut p| {
        p.set_port(proto::PORT);
        probe_peer(p)
    });
    let recent = read_recent_metrics();
    let facts = Facts {
        connected,
        rtt_ms,
        listening,
        bulk_listening,
        audio_listening,
        discover_listening,
        has_peer,
        peer,
        peer_reachable,
        recent: Some(recent),
    };
    // 実測と判定・ドクターの記録に加え、直近ログの抜粋も末尾へ連結する
    //(Mac 側 diag と対称。ログは exe と同じフォルダの knit-win.log で、
    // tray の「ログを開く」と同じ場所。起動直後でまだ無いときは空になり落ちない)
    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("knit-win.log")))
        .unwrap_or_default();
    format!(
        "{}\n\n{}{}",
        diagnose::report(&facts).1,
        knit_common::doctor::journal_text(),
        diagnose::log_tail_section(&log_path)
    )
}

/// ホスト役(待ち受け側)か。設定の「ホストとして待ち受ける」と KNIT_ROLE から判定
fn is_host_role() -> bool {
    knit_common::envutil::get("KNIT_ROLE").as_deref() == Some("server") || crate::tray::host_mode_pref()
}

/// 待ち受けの有無: 同じポートでの bind を試みる。既に待ち受けがあれば
/// AddressInUse で失敗するため、それを「待ち受けあり」と判定する
fn probe_listening() -> bool {
    TcpListener::bind(("0.0.0.0", proto::PORT)).is_err()
}

/// 自動発見(UDP 24903)の応答待受の有無: 同じポートでの UDP bind を試みる
///(Mac 側の probe_discover_listening と同じ方式)
fn probe_discover_listening() -> bool {
    std::net::UdpSocket::bind((
        "0.0.0.0",
        proto::PORT + knit_common::discover::PORT_OFFSET,
    ))
    .is_err()
}

/// peer の proto::PORT への到達性(3 秒のタイムアウト)
fn probe_peer(mut addr: SocketAddr) -> bool {
    addr.set_port(proto::PORT);
    TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()
}

/// 稼働計測カウンタ(state.rs)からの直近集計(Mac と同じ方式)
fn read_recent_metrics() -> Recent {
    let boot = BOOT_WALL_MS.load(Ordering::Relaxed);
    let window_hours = if boot == 0 {
        1
    } else {
        // 起動時刻(BOOT_WALL_MS)は壁時計で記録しているため、窓も壁時計で測る
        (crate::state::wall_ms().saturating_sub(boot) / 3_600_000).max(1)
    };
    Recent {
        window_hours,
        drops: METRIC_DROPS.load(Ordering::Relaxed),
        reconnects: METRIC_CONNECTS.load(Ordering::Relaxed),
        max_gap_ms: METRIC_MAX_GAP_MS.load(Ordering::Relaxed),
        total_gap_ms: METRIC_TOTAL_GAP_MS.load(Ordering::Relaxed),
    }
}
