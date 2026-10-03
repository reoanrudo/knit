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
    let recent = read_recent_metrics();
    let facts = Facts {
        connected,
        rtt_ms,
        listening,
        has_peer,
        peer,
        peer_reachable,
        recent: Some(recent),
    };
    format!("{}\n\n{}", diagnose::report(&facts).1, knit_common::doctor::journal_text())
}

/// 待ち受けの有無: 同じポートでの bind を試みる。既に待ち受けがあれば
/// AddressInUse で失敗するため、それを「待ち受けあり」と判定する
fn probe_listening() -> bool {
    TcpListener::bind(("0.0.0.0", proto::PORT)).is_err()
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
