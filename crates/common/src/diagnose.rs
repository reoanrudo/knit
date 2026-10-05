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
    /// ファイル・画像経路(bulk)の待受が動いているか。本線と別ポートのため
    /// 独立して死にうる。未測定(クライアント役など待受自体が無い)は None
    pub bulk_listening: Option<bool>,
    /// 音声経路(24901)の待受が動いているか。本線と別ポートのため独立して死にうる。
    /// 未測定(音声無効・クライアント役など待受自体が無い)は None
    pub audio_listening: Option<bool>,
    /// 自動発見(UDP 24903)の応答待受が動いているか。発見できないと初回接続が
    /// 繋がらない。未測定(クライアント役など応答側でない)は None
    pub discover_listening: Option<bool>,
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
/// next_retry_at_ms と now_ms は同じ単調時計の ms(0=待ちなし)。壁時計(NTP 補正で
/// 飛ぶ)を渡すと残り時間が狂うため、各 OS の `now_ms()` で揃える
pub fn next_retry_line(next_retry_at_ms: u64, connected: bool, now_ms: u64) -> Option<String> {
    if next_retry_at_ms == 0 || connected {
        return None;
    }
    let remain = next_retry_at_ms.saturating_sub(now_ms) / 1000;
    Some(format!("再試行まで {remain}秒"))
}

/// 相手を見つけられない状態の継続表示(1 分を超えた断だけ出す)。
/// 「ログでしか確認できない無限リトライ」をステータス表示へ見える化する。
/// disconnected_since_ms と now_ms は同じ単調時計の ms(継続時間の測定のため)
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
    // 2b. ファイル・画像の待受(bulk 経路。本線と別ポートのため独立して死ぬ)
    if let Some(up) = f.bulk_listening {
        checks.push(Check {
            label: "ファイル・画像の待受",
            status: if up { Status::Ok } else { Status::Fail },
            detail: format!("ポート {}", crate::proto::PORT + crate::bulk::PORT_OFFSET),
        });
    }
    // 2c. 音声の待受(本線と別ポートのため独立して死ぬ)
    if let Some(up) = f.audio_listening {
        checks.push(Check {
            label: "音声の待受",
            status: if up { Status::Ok } else { Status::Fail },
            detail: format!("ポート {}", crate::proto::PORT + 1),
        });
    }
    // 2d. 自動発見の待受(UDP。届かないと初回接続の相手探しができない)
    if let Some(up) = f.discover_listening {
        checks.push(Check {
            label: "自動発見の待受",
            status: if up { Status::Ok } else { Status::Fail },
            detail: format!("ポート {}(UDP)", crate::proto::PORT + crate::discover::PORT_OFFSET),
        });
    }
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
    let mut advice = advice_for(cause, f, current_os());
    // 本線は生きていて各経路だけ死んでいる状態の対処。原因の絞り込み(Cause)とは
    // 独立した追記事項として扱う(bulk と同じ形式)
    if f.bulk_listening == Some(false) {
        let port = crate::proto::PORT + crate::bulk::PORT_OFFSET;
        advice.push_str(&format!(
            "\n\nファイル・画像の待受(ポート {port})が取れていません。他のアプリが同じ\
             ポートを使っていると転送だけが失敗し続けます。再試行中のため、占有が\
             解ければ自動で回復します。"
        ));
    }
    if f.audio_listening == Some(false) {
        let port = crate::proto::PORT + 1;
        advice.push_str(&format!(
            "\n\n音声の待受(ポート {port})が取れていません。他のアプリが同じポートを\
             使っていると入力はできても音だけが流れません。再起動で直ることが\
             あります。"
        ));
    }
    if f.discover_listening == Some(false) {
        let port = crate::proto::PORT + crate::discover::PORT_OFFSET;
        advice.push_str(&format!(
            "\n\n自動発見の待受(ポート {port}/UDP)が取れていません。待受が無いと\
             相手からの初回接続の探し合わせができません。再起動で直ることがあります。"
        ));
    }
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
            "Knitの常駐が止まっています。Knitを起動すると自動で待ち受けます。".into()
        }
        Cause::NoPeer => match os {
            Os::Mac => "まだ端末登録がありません。Knit設定の「端末を登録…」から登録します。".into(),
            Os::Windows => "まだ端末登録がありません。トレイメニューの「登録情報」から登録します。".into(),
        },
        Cause::PeerUnreachable => {
            let peer = f.peer.map(|p| p.to_string()).unwrap_or_default();
            let fw = match os {
                Os::Mac => "システム設定 > ネットワーク > ファイアウォール",
                Os::Windows => "Windowsセキュリティ > ファイアウォールとネットワーク保護",
            };
            format!(
                "{peer} に届きません。両PCが同じWi-Fi/LANにいるか、相手が起動しているか確認してください。\
                 同じネットワークのはずなら、ファイアウォールの設定({fw})が \
                 Knitの受信をブロックしていないか確認します。"
            )
        }
        Cause::PeerProbeOkButNoSession => {
            let peer = f.peer.map(|p| p.to_string()).unwrap_or_default();
            let re_register = match os {
                Os::Mac => "設定の「端末を登録…」",
                Os::Windows => "トレイメニューの「登録情報」",
            };
            format!(
                "{peer} のポートには届くのに接続が成立していません。両PCでKnitの版が同じか確認し、\
                 改善しない場合は{re_register}で登録のやり直しを試してください。"
            )
        }
        Cause::PeerNotFound => {
            let peer = f.peer.map(|p| p.to_string()).unwrap_or_default();
            // 自動発見(UDP 24903)は AP 分離・ゲスト用 Wi-Fi では届かない。
            // その環境の正解は相手 IP の直接指定(KNIT_HOST)
            let host_howto = match os {
                Os::Mac => "Mac側の ~/.config/knit/env へ KNIT_HOST=(WindowsのIP)を書く\
                    (IPはWindowsの「設定 > ネットワークとインターネット」で確認)",
                Os::Windows => {
                    "Windows側の %USERPROFILE%\\knit\\.env へ KNIT_HOST=(MacのIP)を書く\
                    (IPはMacの「システム設定 > Wi-Fi > 詳細」で確認)"
                }
            };
            format!(
                "{peer} に届くか確認できませんでした。相手がスリープ・電源オフ・別ネットワークの可能性があります。\
                 相手側のKnitが起動しているか確認してください。自動発見が届かないWi-Fi\
                 (AP 分離・ゲスト用ネットワーク)ではIPの直接指定が正解です: {host_howto}"
            )
        }
    }
}

/// ログの注意行([error]/[warn]/[fatal] で始まる行)か
fn is_attention_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("[error]") || t.starts_with("[warn]") || t.starts_with("[fatal]")
}

/// 診断レポート末尾に付ける「直近ログ」の行選択(純粋関数)。
/// 末尾 `tail` 行を基本とし、その範囲に含まれない直近の注意行([error]/[warn]/
/// [fatal])を `attention` 行まで遡って先頭側へ足す。障害直前のエラーが末尾の
/// 通常行に埋もれるのを防ぐためのもの。行の並びは元のログの順序を保つ
pub fn log_tail_lines<'a>(lines: &[&'a str], tail: usize, attention: usize) -> Vec<&'a str> {
    if lines.len() <= tail {
        return lines.to_vec();
    }
    let split = lines.len() - tail;
    let tail_part = &lines[split..];
    // 末尾範囲に既に注意行があるときは、その分だけ遡る枠を減らす
    let already = tail_part.iter().filter(|l| is_attention_line(l)).count();
    let want = attention.saturating_sub(already);
    let mut head: Vec<&str> = Vec::new();
    if want > 0 {
        for l in lines[..split].iter().rev() {
            if is_attention_line(l) {
                head.push(l);
                if head.len() >= want {
                    break;
                }
            }
        }
        head.reverse();
    }
    let mut out = head;
    out.extend_from_slice(tail_part);
    out
}

/// 診断レポートに連結するログの末尾行数(報告材料として十分な量)
const LOG_TAIL_LINES: usize = 30;
/// 末尾範囲の外から遡って拾う注意行([error]/[warn]/[fatal])の上限
const LOG_ATTENTION_LINES: usize = 10;

/// ログファイルの直近抜粋(報告材料)をレポート末尾へ付け足す形の文字列で返す。
/// 無い・読めない・空のときは空文字列(初回起動でも落ちない)。
/// 大きなログは末尾 256KB だけを読み(先頭の切れ端行は捨てる)、Mac・Windows の
/// 両方の diag から同じ形式で使う。壊れたバイトは置換文字へ落とす
pub fn log_tail_section(path: &std::path::Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    /// 抜粋のために読む末尾サイズ(ログ上限 5MB より十分小さく、行数は十分多い)
    const READ_TAIL: u64 = 256 * 1024;
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let Ok(meta) = f.metadata() else {
        return String::new();
    };
    let len = meta.len();
    if len == 0 {
        return String::new();
    }
    if len > READ_TAIL {
        // seek 失敗時は先頭から読む(速度が落ちるだけのため無視する)
        let _ = f.seek(SeekFrom::End(-(READ_TAIL as i64)));
    }
    let mut bytes = Vec::new();
    if f.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let skipped_head = len > READ_TAIL;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if skipped_head && lines.len() > 1 {
        // 途中から読み始めた先頭行は切れ端(と UTF-8 境界の置換文字)のため捨てる
        lines.remove(0);
    }
    let picked = log_tail_lines(&lines, LOG_TAIL_LINES, LOG_ATTENTION_LINES);
    if picked.is_empty() {
        return String::new();
    }
    let extra = if picked.len() > LOG_TAIL_LINES.min(lines.len()) {
        "・古い [error]/[warn] 行を含む"
    } else {
        ""
    };
    let mut out = format!(
        "\n\n直近のログ({}・末尾を中心に {}行{extra}):",
        path.display(),
        picked.len()
    );
    for l in picked {
        out.push('\n');
        out.push_str(l);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Facts {
        Facts {
            connected: false,
            rtt_ms: None,
            listening: true,
            bulk_listening: None,
            audio_listening: None,
            discover_listening: None,
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
        assert!(win.contains("Windowsセキュリティ > ファイアウォールとネットワーク保護"));
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

    /// ファイル・画像の待受(bulk)は本線と別ポートのため独立して死ぬ。
    /// 未測定(None)では行を出さず、測定結果次第で OK/失敗と対処案内を出す
    #[test]
    fn bulk_listening_line_shows_port_and_advice_only_when_measured() {
        // 未測定: 行も対処案内も出ない(クライアント役など待受自体が無い)
        let (_, text) = report(&base());
        assert!(!text.contains("ファイル・画像の待受"));
        // 測定して稼働中: OK 行だけ
        let mut f = base();
        f.bulk_listening = Some(true);
        let (_, text) = report(&f);
        assert!(text.contains("✓ ファイル・画像の待受 ポート 24902"));
        assert!(!text.contains("ポート 24902)が取れていません"));
        // 測定して死んでいる: 失敗行と対処案内を出す
        let mut f = base();
        f.bulk_listening = Some(false);
        let (_, text) = report(&f);
        assert!(text.contains("✗ ファイル・画像の待受 ポート 24902"));
        assert!(text.contains("ポート 24902)が取れていません"));
    }

    /// 音声(24901)と自動発見(24903/UDP)の待受も bulk と同じ出し分けにする。
    /// 未測定(音声無効・クライアント役など)では行を出さない
    #[test]
    fn audio_and_discover_listening_lines_follow_measurement() {
        // 未測定: 行も対処案内も出ない
        let (_, text) = report(&base());
        assert!(!text.contains("音声の待受"));
        assert!(!text.contains("自動発見の待受"));
        // 測定して稼働中: OK 行だけ(発見は UDP 表記も付く)
        let mut f = base();
        f.audio_listening = Some(true);
        f.discover_listening = Some(true);
        let (_, text) = report(&f);
        assert!(text.contains("✓ 音声の待受 ポート 24901"));
        assert!(text.contains("✓ 自動発見の待受 ポート 24903(UDP)"));
        assert!(!text.contains("ポート 24901)が取れていません"));
        assert!(!text.contains("ポート 24903/UDP)が取れていません"));
        // 測定して死んでいる: 失敗行と対処案内を出す
        let mut f = base();
        f.audio_listening = Some(false);
        f.discover_listening = Some(false);
        let (_, text) = report(&f);
        assert!(text.contains("✗ 音声の待受 ポート 24901"));
        assert!(text.contains("ポート 24901)が取れていません"));
        assert!(text.contains("✗ 自動発見の待受 ポート 24903(UDP)"));
        assert!(text.contains("ポート 24903/UDP)が取れていません"));
    }

    /// ログ末尾の抜粋(報告材料): 短いログはそのまま全部、長いログは末尾 tail 行
    #[test]
    fn log_tail_lines_returns_all_when_short_and_tail_when_long() {
        let short: Vec<&str> = vec!["a", "[error] x", "b"];
        assert_eq!(log_tail_lines(&short, 30, 10), short, "30行未満は全部");

        let long: Vec<String> = (0..50).map(|i| format!("line{i}")).collect();
        let long: Vec<&str> = long.iter().map(|s| s.as_str()).collect();
        let picked = log_tail_lines(&long, 30, 10);
        assert_eq!(picked.len(), 30, "末尾 30 行だけ");
        assert_eq!(picked.first().copied(), Some("line20"), "先頭は切れ目の次");
        assert_eq!(picked.last().copied(), Some("line49"), "最後は末尾行");
        assert_eq!(picked, long[20..], "並びは元の順序を保つ");
    }

    /// 末尾範囲の外にある直近の [error]/[warn]/[fatal] 行を遡って先頭側へ足す。
    /// 上限(attention)と、末尾に既に注意行があるときの枠の減りも確認する
    #[test]
    fn log_tail_lines_prepends_recent_attention_lines() {
        // 末尾30行の外に古い注意行が 2 行: 両方を遡って先頭へ足す(順序維持)
        let mut lines: Vec<String> = (0..40).map(|i| format!("line{i}")).collect();
        lines[1] = "[error] old1".into();
        lines[2] = "[warn] old2".into();
        let lines: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let picked = log_tail_lines(&lines, 30, 10);
        assert_eq!(picked.len(), 32, "注意行 2 行を先頭へ足す");
        assert_eq!(&picked[..2], &["[error] old1", "[warn] old2"], "古い順を保つ");
        assert_eq!(picked[2], "line10", "通常の末尾行が続く");

        // 上限: 末尾範囲の外に注意行が 15 行あるときは新しい 10 行まで
        let many: Vec<String> = (0..50)
            .map(|i| {
                if i < 15 {
                    format!("[error] e{i}")
                } else {
                    "info".to_string()
                }
            })
            .collect();
        let many: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
        let picked = log_tail_lines(&many, 30, 10);
        assert_eq!(picked.len(), 40, "注意行は上限 10 行まで");
        assert_eq!(&picked[..10], &many[5..15], "新しい方(e5..e14)を拾う");

        // 末尾範囲に既に 3 行の注意行があるときは 7 行だけ遡る
        let mut mixed: Vec<&str> = (0..50).map(|_| "info").collect();
        for i in 0..7 {
            mixed[i] = "[warn] w{i}";
        }
        mixed[47] = "[error] in-tail-1";
        mixed[48] = "[error] in-tail-2";
        mixed[49] = "[error] in-tail-3";
        let picked = log_tail_lines(&mixed, 30, 10);
        assert_eq!(picked.len(), 37, "末尾30 + 遡り 7 行");
        assert_eq!(&picked[..7], &mixed[..7], "末尾側の注意行 3 行分だけ枠が減る");
        assert!(picked.contains(&"[error] in-tail-1"), "末尾側の注意行も当然含む");
    }

    /// ログファイルが無い・空のときは空文字列(初回起動で落ちない)。あるときは
    /// 見出しと末尾行を含む
    #[test]
    fn log_tail_section_handles_missing_and_real_files() {
        let missing = std::env::temp_dir().join("knit-diag-log-nonexistent.log");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(log_tail_section(&missing), "", "無いファイルは空文字列");

        let dir = std::env::temp_dir().join(format!(
            "knit-diag-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.log");
        std::fs::write(&path, "").unwrap();
        assert_eq!(log_tail_section(&path), "", "空のファイルは空文字列");

        let body: String = (0..40)
            .map(|i| {
                if i == 3 {
                    "[error] boom\n".to_string()
                } else {
                    format!("line{i}\n")
                }
            })
            .collect();
        std::fs::write(&path, body).unwrap();
        let section = log_tail_section(&path);
        assert!(section.starts_with("\n\n直近のログ("), "見出しで始まる: {section}");
        assert!(section.contains("古い [error]/[warn] 行を含む"), "注意行を拾った旨");
        assert!(section.contains("[error] boom"), "末尾範囲外のエラー行を含む");
        assert!(section.contains("line39"), "末尾行を含む");
        assert!(!section.contains("line5\n"), "古い通常行は含めない");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
