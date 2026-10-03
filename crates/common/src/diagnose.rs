//! 接続の「繋がらない」を原因まで絞り込む診断の純ロジック。
//! ネットワークの実測(IO)は各 OS 側が担い、ここは実測結果(Facts)を
//! 受けて判定・原因の絞り込み・対処の提示を組み立てる部分だけを持つ。
//! 動機は Windows 側の実ログ(knit-win.log)に「Mac を発見できず無限リトライ」
//! が続いたこと。状態をログでしか確認できない問題を解く。

use std::net::SocketAddr;

/// チェック 1 行の状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Info,
}

/// 診断レポートの 1 行
#[derive(Debug, Clone)]
pub struct Check {
    pub label: &'static str,
    pub status: Status,
    pub detail: String,
}

/// 診断への入力。各 OS 側の実測が詰める
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub connected: bool,
    pub rtt_ms: Option<u64>,
    pub listening: bool,
    pub has_peer: bool,
    pub peer: Option<SocketAddr>,
    pub peer_reachable: Option<bool>,
    pub recent: Option<Recent>,
}

/// conn-metric の直近集計
#[derive(Debug, Clone, Default)]
pub struct Recent {
    pub window_hours: u64,
    pub drops: u32,
    pub reconnects: u32,
    pub max_gap_ms: u64,
    pub total_gap_ms: u64,
}

/// 診断が絞り込んだ原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Connected,
    NotListening,
    NoPeer,
    PeerUnreachable,
    PeerProbeOkButNoSession,
    PeerNotFound,
}

/// 実測から原因を 1 つに絞る
pub fn judge(f: &Facts) -> Cause {
    if f.connected {
        return Cause::Connected;
    }
    if !f.listening {
        return Cause::NotListening;
    }
    if !f.has_peer {
        return Cause::NoPeer;
    }
    if f.peer_reachable == Some(false) {
        return Cause::PeerUnreachable;
    }
    if f.peer_reachable == Some(true) {
        return Cause::PeerProbeOkButNoSession;
    }
    Cause::PeerNotFound
}

/// ミリ秒の整形(1分以上は「分」)
pub fn fmt_ms(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{:.1}分", ms as f64 / 60_000.0)
    } else {
        format!("{:.0}秒", ms as f64 / 1_000.0)
    }
}

/// 対処文案の出し分け先。登録メニューの名前やファイアウォールの手順が OS で
/// 異なるため、実行環境で選んで report へ渡す(テストは両 OS を直接渡して検証)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
}

/// ビルド先の OS(診断を実行している側)。cfg! は定数畳み込みのため実行時分岐ではない
pub fn current_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else {
        Os::Mac
    }
}

/// 次の再接続試行までの表示(接続待ちの見える化)。Mac・Windows の状態表示で共用。
/// next_retry_at_ms は unix ms(0=待ちなし)
pub fn next_retry_line(next_retry_at_ms: u64, connected: bool, now_ms: u64) -> Option<String> {
    if next_retry_at_ms == 0 || connected {
        return None;
    }
    let remain = next_retry_at_ms.saturating_sub(now_ms) / 1000;
    Some(format!("再試行まで {remain}秒"))
}

/// 相手を見つけられない状態の継続表示(1 分を超えた断だけ出す)。
/// 「ログでしか確認できない無限リトライ」をステータス表示へ見える化する
pub fn not_found_line(disconnected_since_ms: u64, connected: bool, now_ms: u64) -> Option<String> {
    if disconnected_since_ms == 0 || connected {
        return None;
    }
    let secs = now_ms.saturating_sub(disconnected_since_ms) / 1000;
    (secs >= 60).then(|| format!("相手を {}分見つけられません", secs / 60))
}

impl Recent {
    /// 安定性チェックの表示(断なしは OK を出さず「断なし」だけ)
    pub fn line(&self) -> String {
        if self.drops == 0 && self.reconnects <= 1 {
            format!("直近{}時間: 断なし", self.window_hours)
        } else {
            format!(
                "直近{}時間: 断 {} 回(最長 {}・合計 {})",
                self.window_hours,
                self.drops,
                fmt_ms(self.max_gap_ms),
                fmt_ms(self.total_gap_ms)
            )
        }
    }
}

impl Check {
    fn mark(&self) -> &'static str {
        match self.status {
            Status::Ok => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Info => "・",
        }
    }

    /// メニュー・通知・窓でそのまま使える 1 行
    pub fn line(&self) -> String {
        if self.detail.is_empty() {
            format!("{} {}", self.mark(), self.label)
        } else {
            format!("{} {} {}", self.mark(), self.label, self.detail)
        }
    }
}

/// レポートの組み立て。チェック行と対処を 1 つの文字列にする
pub fn report(f: &Facts) -> (Cause, String) {
    let mut checks: Vec<Check> = Vec::new();
    // 1. 接続
    checks.push(Check {
        label: "接続",
        status: if f.connected { Status::Ok } else { Status::Warn },
        detail: f.rtt_ms.map(|r| format!("RTT {}ms", r)).unwrap_or_default(),
    });
    // 2. 待ち受け
    checks.push(Check {
        label: "待ち受け",
        status: if f.listening { Status::Ok } else { Status::Fail },
        detail: format!("ポート {}", crate::proto::PORT),
    });
    // 3. 登録端末(接続中は到達性が自明。未登録でも発見で繋がる運用があるため失敗にしない)
    checks.push(Check {
        label: "登録端末",
        status: if f.has_peer {
            Status::Ok
        } else if f.connected {
            Status::Info
        } else {
            Status::Fail
        },
        detail: f.peer.map(|p| p.to_string()).unwrap_or_else(|| {
            if f.connected {
                "未登録(発見で接続中)".into()
            } else {
                String::new()
            }
        }),
    });
    // 4. 相手への到達(接続中は自明のため未確認を Info にする)
    checks.push(Check {
        label: "相手への到達",
        status: if f.connected && f.peer_reachable.is_none() {
            Status::Info
        } else {
            match f.peer_reachable {
                Some(true) => Status::Ok,
                Some(false) => Status::Fail,
                None => Status::Info,
            }
        },
        detail: if f.connected && f.peer_reachable.is_none() {
            "接続中のため確認不要".into()
        } else {
            f.peer
                .map(|p| format!("{} の {} 番へ確認", p.ip(), crate::proto::PORT))
                .unwrap_or_default()
        },
    });
    // 5. 安定性(conn-metric の直近集計)
    checks.push(Check {
        label: "安定性",
        status: match &f.recent {
            Some(r) => if r.drops == 0 { Status::Ok } else { Status::Info },
            None => Status::Info,
        },
        detail: f.recent.as_ref().map(|r| r.line()).unwrap_or_default(),
    });
    let cause = judge(f);
    let advice = advice_for(cause, f, current_os());
    let text = checks.iter().map(|c| c.line()).collect::<Vec<_>>().join("\n");
    (cause, format!("{text}\n\n{advice}"))
}

fn advice_for(cause: Cause, f: &Facts, os: Os) -> String {
    match cause {
        Cause::Connected => {
            let rtt = f
                .rtt_ms
                .map(|r| format!("(RTT {}ms)", r))
                .unwrap_or_default();
            format!("問題ありません{rtt}。")
        }
        Cause::NotListening => {
            "Knit の常駐が止まっています。Knit を起動すると自動で待ち受けます。".into()
        }
        Cause::NoPeer => match os {
            Os::Mac => "まだ端末登録がありません。Knit 設定の「端末を登録…」から登録します。".into(),
            Os::Windows => "まだ端末登録がありません。トレイメニューの「登録情報」から登録します。".into(),
        },
        Cause::PeerUnreachable => {
            let peer = f.peer.map(|p| p.to_string()).unwrap_or_default();
            let fw = match os {
                Os::Mac => "システム設定 > ネットワーク > ファイアウォール",
                Os::Windows => "Windows セキュリティ > ファイアウォールとネットワーク保護",
            };
            format!(
                "{peer} に届きません。両 PC が同じ Wi-Fi/LAN にいるか、相手が起動しているか確認してください。\
                 同じネットワークのはずなら、ファイアウォールの設定({fw})が \
                 Knit の受信をブロックしていないか確認します。"
            )
        }
        Cause::PeerProbeOkButNoSession => {
            let peer = f.peer.map(|p| p.to_string()).unwrap_or_default();
            let re_register = match os {
                Os::Mac => "設定の「端末を登録…」",
                Os::Windows => "トレイメニューの「登録情報」",
            };
            format!(
                "{peer} のポートには届くのに接続が成立していません。両 PC で Knit の版が同じか確認し、\
                 改善しない場合は{re_register}で登録のやり直しを試してください。"
            )
        }
        Cause::PeerNotFound => {
            let peer = f.peer.map(|p| p.to_string()).unwrap_or_default();
            // 自動発見(UDP 24903)は AP 分離・ゲスト用 Wi-Fi では届かない。
            // その環境の正解は相手 IP の直接指定(KNIT_HOST)
            let host_howto = match os {
                Os::Mac => "Mac 側の ~/.config/knit/env へ KNIT_HOST=(Windows の IP) を書く\
                    (IP は Windows の「設定 > ネットワークとインターネット」で確認)",
                Os::Windows => {
                    "Windows 側の %USERPROFILE%\\knit\\.env へ KNIT_HOST=(Mac の IP) を書く\
                    (IP は Mac の「システム設定 > Wi-Fi > 詳細」で確認)"
                }
            };
            format!(
                "{peer} に届くか確認できませんでした。相手がスリープ・電源オフ・別ネットワークの可能性があります。\
                 相手側の Knit が起動しているか確認してください。自動発見が届かない Wi-Fi\
                 (AP 分離・ゲスト用ネットワーク)では IP の直接指定が正解です: {host_howto}"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Facts {
        Facts {
            connected: false,
            rtt_ms: None,
            listening: true,
            has_peer: true,
            peer: Some("192.168.0.216:24900".parse().unwrap()),
            peer_reachable: Some(true),
            recent: None,
        }
    }

    #[test]
    fn judge_pins_single_cause() {
        let mut f = base();
        f.connected = true;
        assert!(matches!(judge(&f), Cause::Connected));

        f = base();
        f.listening = false;
        assert!(matches!(judge(&f), Cause::NotListening));

        f = base();
        f.has_peer = false;
        assert!(matches!(judge(&f), Cause::NoPeer));

        f = base();
        f.peer_reachable = Some(false);
        assert!(matches!(judge(&f), Cause::PeerUnreachable));

        f = base();
        f.peer_reachable = Some(true);
        assert!(matches!(judge(&f), Cause::PeerProbeOkButNoSession));

        f = base();
        f.peer_reachable = None;
        assert!(matches!(judge(&f), Cause::PeerNotFound));
    }

    #[test]
    fn recent_line_reports_drops() {
        let r = Recent {
            window_hours: 24,
            drops: 2,
            reconnects: 2,
            max_gap_ms: 90_000,
            total_gap_ms: 120_000,
        };
        let line = r.line();
        assert!(line.contains("断 2 回"));
        assert!(line.contains("最長 1.5分"));
        assert!(!line.contains("断なし"));

        let quiet = Recent {
            window_hours: 24,
            drops: 0,
            reconnects: 1,
            max_gap_ms: 0,
            total_gap_ms: 0,
        };
        assert!(quiet.line().contains("断なし"));
    }

    #[test]
    fn report_contains_advice_per_cause() {
        let mut f = base();
        f.connected = true;
        f.rtt_ms = Some(3);
        let (_, text) = report(&f);
        assert!(text.contains("問題ありません"));

        f = base();
        f.peer_reachable = Some(false);
        let (_, text) = report(&f);
        assert!(text.contains("ファイアウォール"));
        assert!(text.contains("Wi-Fi"));
    }

    #[test]
    fn check_line_formats_mark_and_detail() {
        let c = Check {
            label: "待ち受け",
            status: Status::Ok,
            detail: "ポート 24900".into(),
        };
        assert_eq!(c.line(), "✓ 待ち受け ポート 24900");
    }

    #[test]
    fn advice_differs_by_os_for_registration_and_firewall() {
        // 未登録: Mac は「端末を登録…」、Windows は「登録情報」
        let mut f = base();
        f.has_peer = false;
        f.peer = None;
        assert!(advice_for(judge(&f), &f, Os::Mac).contains("端末を登録"));
        assert!(advice_for(judge(&f), &f, Os::Windows).contains("登録情報"));

        // 届かない: ファイアウォール手順が OS のものを出す
        let mut f = base();
        f.peer_reachable = Some(false);
        let mac = advice_for(judge(&f), &f, Os::Mac);
        let win = advice_for(judge(&f), &f, Os::Windows);
        assert!(mac.contains("システム設定 > ネットワーク > ファイアウォール"));
        assert!(win.contains("Windows セキュリティ > ファイアウォールとネットワーク保護"));
        assert!(!win.contains("システム設定 > ネットワーク"));
    }

    #[test]
    fn advice_tells_knit_host_on_peer_not_found() {
        // 自動発見が届かない環境(AP 分離・ゲスト Wi-Fi)の正解=KNIT_HOST を案内する
        let mut f = base();
        f.peer_reachable = None;
        let mac = advice_for(judge(&f), &f, Os::Mac);
        let win = advice_for(judge(&f), &f, Os::Windows);
        assert!(mac.contains("KNIT_HOST"));
        assert!(mac.contains("~/.config/knit/env"));
        assert!(win.contains("KNIT_HOST"));
        assert!(win.contains(r"%USERPROFILE%\knit\.env"));
    }

    #[test]
    fn retry_and_not_found_lines_show_wait_and_duration() {
        // 接続中は出さない・待ちなし(0)は出さない
        assert_eq!(next_retry_line(0, false, 10_000), None);
        assert_eq!(next_retry_line(90_000, true, 10_000), None);
        assert_eq!(next_retry_line(90_000, false, 80_000).unwrap(), "再試行まで 10秒");
        // 過去の時刻(既に試行済み)は 0 秒で出す(次の表示周期で消える)
        assert_eq!(next_retry_line(50_000, false, 80_000).unwrap(), "再試行まで 0秒");

        assert_eq!(not_found_line(0, false, 10_000), None);
        // 断から 59 秒では出さない(60 秒の通知のしきい値と同じ)
        assert_eq!(not_found_line(0, false, 59_000), None);
        assert_eq!(not_found_line(71_000, false, 59_000 + 59_000), None);
        assert_eq!(
            not_found_line(10_000, false, 130_000).unwrap(),
            "相手を 2分見つけられません"
        );
        // 接続中は断の継続が残っていても出さない
        assert_eq!(not_found_line(10_000, true, 130_000), None);
    }
}
