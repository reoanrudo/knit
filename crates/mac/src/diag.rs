//! 接続診断の実測(macOS 側)。common::diagnose の Facts を実測して詰める。
//! 表示は GUI(メニュー「接続を診断…」)が担当する。
//! 判定ロジックの本体は common::diagnose(単体テスト済み)。

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
    let listening = probe_listening();
    let peer_reachable = peer.map(|mut p| {
        p.set_port(proto::PORT);
        probe_peer(p)
    });
    // ファイル・画像経路(bulk)の待受。クライアント役は待受自体が無いため
    // 未測定(None)として行を出さない
    let bulk_listening = (!is_client_role()).then(knit_common::bulk::serving);
    // 音声(24901)の待受もサーバ役+音声有効のときだけ(main の起動条件と同じ)
    let audio_listening = audio_server_role().then(probe_audio_listening);
    // 自動発見(UDP 24903)の応答待受もサーバ役だけ
    let discover_listening = (!is_client_role()).then(probe_discover_listening);
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
    // 実測と判定・ドクターの記録に加え、直近ログの抜粋も末尾へ連結する。
    // 「繋がらない時に報告してもらう材料」をこのレポート1つで賄うため
    //(ログは /tmp/knit-mac.log。起動直後でまだ無いときは空文字列になり落ちない)
    format!(
        "{}\n\n{}{}",
        diagnose::report(&facts).1,
        knit_common::doctor::journal_text(),
        diagnose::log_tail_section(std::path::Path::new("/tmp/knit-mac.log"))
    )
}

/// 待ち受けの有無: 同じポートでの bind を試みる。既に待ち受けがあれば
/// AddressInUse で失敗するため、それを「待ち受けあり」と判定する
fn probe_listening() -> bool {
    TcpListener::bind(("0.0.0.0", proto::PORT)).is_err()
}

/// 音声受信(24901)の待受を持つか。main の起動条件と同じ
///(サーバ役 + KNIT_AUDIO≠0 + 共有範囲で音声が許可されている)
fn audio_server_role() -> bool {
    !is_client_role()
        && knit_common::envutil::get("KNIT_AUDIO").as_deref() != Some("0")
        && knit_common::share::allow_audio()
}

/// 音声経路(24901)の待受の有無: 本線と同じく同じポートでの bind を試みる
fn probe_audio_listening() -> bool {
    TcpListener::bind(("0.0.0.0", proto::PORT + 1)).is_err()
}

/// 自動発見(UDP 24903)の応答待受の有無: 同じポートでの UDP bind を試みる
fn probe_discover_listening() -> bool {
    std::net::UdpSocket::bind((
        "0.0.0.0",
        proto::PORT + knit_common::discover::PORT_OFFSET,
    ))
    .is_err()
}

/// クライアント役(待受しない)か。main の接続方向判定と同じ条件
///(effective_client_role に一元化。環境変数 KNIT_ROLE が GUI 設定に優先する)。
/// bulk(ファイル・画像)の待受はサーバ役だけが持つため、診断の bulk 行も
/// 役割に応じて出し分ける
fn is_client_role() -> bool {
    crate::effective_client_role()
}

/// peer の proto::PORT への到達性(3 秒のタイムアウト)
fn probe_peer(mut addr: SocketAddr) -> bool {
    addr.set_port(proto::PORT);
    TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()
}

/// 稼働計測カウンタ(state.rs)からの直近集計。ログの場所・保持に依存せず、
/// Windows 側(exe 自身がログの場所を知らない)と共通の方式にする
fn read_recent_metrics() -> Recent {
    let boot = BOOT_WALL_MS.load(Ordering::Relaxed);
    let window_hours = if boot == 0 {
        1
    } else {
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
