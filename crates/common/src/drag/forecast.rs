//! 「今離すとどうなるか」の予報(I2)。**表示はこの値の写像にすぎない**ので、
//! 文言や判断を UI 側に書かない。入力は観測できる事実だけに限り、分からない
//! ことは `Unknown` にして成功語を出さない(調査 M2・M3 / 摩擦 F2・F3)
use super::Refusal;

/// 押下と転送完了の前後関係。相手(Windows)は「押下が続いている間に届いた
/// 転送」だけを OLE ドラッグにするため、**離した順序**が結果を決める
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryPhase {
    /// まだ押している
    Held,
    /// 転送の完了前に離した(相手はドラッグを始めず、保存のみになる)
    ReleasedEarly,
    /// 完了してから離した(相手はドラッグを開始できた)
    ReleasedAfterComplete,
}

/// 予報が「渡らない」と言うときの理由
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    /// そもそも越えられない(理由つき)
    Refused(Refusal),
    /// 相手が待つのをやめた(時間切れ)
    PeerTimeout,
}

/// 判断材料が足りない理由。**成功語を出してはいけない**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// 転送がまだ始まっていない
    NoTransferStarted,
    /// 総量が分からない(% も残り時間も出せない)
    TotalUnknown,
}

/// 「今離すとどうなるか」
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forecast {
    /// 今離せば相手の画面に置ける(ドロップとして成立する)
    WillPlace,
    /// 今離すと相手側の保存フォルダに入る(置けない)。dir は分かれば入る
    WillSaveOnly { dir: Option<String> },
    /// 押したまま待つ必要がある
    WaitingForPeer {
        /// 進捗(%)。総量が分からないときは None
        percent: Option<u8>,
        /// 完了までの目安。総量が分かり、速度の実測が2件以上あるときだけ
        eta_ms: Option<u64>,
        /// 相手が待つのをやめるまでの残り(0 = 期限が不明)
        deadline_ms: Option<u64>,
    },
    /// 今離しても渡らない
    WillFail { reason: FailReason },
    /// 判断できない(材料不足)
    Unknown { why: Why },
}

/// 予報の入力(観測できる事実だけ。推定や表示を入れない)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastInput {
    pub phase: CarryPhase,
    /// 相手が遅延提供(drag.lazy)に対応しているか
    pub peer_lazy: bool,
    pub sent: u64,
    pub total: u64,
    pub elapsed_ms: u64,
    /// 速度の実測サンプル数(2未満では残り時間を出さない)
    pub speed_samples: u32,
    /// 相手が待つ上限(0 = 不明)
    pub peer_deadline_ms: u64,
    /// 既に確定している拒否(あれば最優先)
    pub refusal: Option<Refusal>,
    /// 保存のみになるときの保存先(分からなければ None)
    pub save_dir: Option<String>,
}

/// 完了までの目安。**総量が分かり、速度の実測が2件以上あるときだけ**出す
fn eta_ms(sent: u64, total: u64, elapsed_ms: u64, speed_samples: u32) -> Option<u64> {
    if speed_samples < 2 || sent == 0 || elapsed_ms == 0 || total <= sent {
        return None;
    }
    Some((total - sent).saturating_mul(elapsed_ms) / sent)
}

/// 進捗(%)。総量が分からないときは None(0% と偽らない)
fn percent(sent: u64, total: u64) -> Option<u8> {
    if total == 0 {
        return None;
    }
    Some(sent.saturating_mul(100).saturating_div(total).min(100) as u8)
}

/// 「今離すとどうなるか」を決める(純関数)。順序:
/// 確定した拒否 → 材料不足 → 遅延提供 → 完了前に離した → 完了 → 相手の期限 → 待ち
///
/// 拒否を最優先にするのは、遅延提供に対応していても**越えられない**掴みを
/// 「置けます」と言わないため(X8)。期限切れより完了を先に見るのは、届き終えた
/// 転送を「相手が待つのをやめた」と誤って断らないため
pub fn forecast_for(i: &ForecastInput) -> Forecast {
    if let Some(r) = &i.refusal {
        return Forecast::WillFail {
            reason: FailReason::Refused(r.clone()),
        };
    }
    if i.sent == 0 && i.total == 0 {
        return Forecast::Unknown {
            why: Why::NoTransferStarted,
        };
    }
    if i.total == 0 {
        return Forecast::Unknown {
            why: Why::TotalUnknown,
        };
    }
    // 遅延提供: 離す前に相手がドロップを受理できるので、押下を待たずに置ける
    if i.peer_lazy {
        return Forecast::WillPlace;
    }
    if i.phase == CarryPhase::ReleasedEarly {
        return Forecast::WillSaveOnly {
            dir: i.save_dir.clone(),
        };
    }
    if i.sent >= i.total {
        return Forecast::WillPlace;
    }
    if i.peer_deadline_ms > 0 && i.elapsed_ms > i.peer_deadline_ms {
        return Forecast::WillFail {
            reason: FailReason::PeerTimeout,
        };
    }
    Forecast::WaitingForPeer {
        percent: percent(i.sent, i.total),
        eta_ms: eta_ms(i.sent, i.total, i.elapsed_ms, i.speed_samples),
        deadline_ms: (i.peer_deadline_ms > 0)
            .then(|| i.peer_deadline_ms.saturating_sub(i.elapsed_ms)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ForecastInput {
        ForecastInput {
            phase: CarryPhase::Held,
            peer_lazy: false,
            sent: 1_000,
            total: 10_000,
            elapsed_ms: 1_000,
            speed_samples: 2,
            peer_deadline_ms: 30_000,
            refusal: None,
            save_dir: None,
        }
    }

    #[test]
    fn a_refusal_beats_even_lazy_delivery() {
        // 遅延提供でも「越えられない」掴みを「置けます」と言わない(X8)
        let i = ForecastInput {
            peer_lazy: true,
            refusal: Some(Refusal::LinkDown),
            ..base()
        };
        assert_eq!(
            forecast_for(&i),
            Forecast::WillFail {
                reason: FailReason::Refused(Refusal::LinkDown)
            }
        );
    }

    #[test]
    fn no_eta_without_two_speed_samples_or_a_known_total() {
        let one = ForecastInput {
            speed_samples: 1,
            ..base()
        };
        let Forecast::WaitingForPeer { eta_ms: None, .. } = forecast_for(&one) else {
            panic!("1サンプルでは残り時間を出さない");
        };
        let with_two = base();
        let Forecast::WaitingForPeer {
            eta_ms: Some(eta), ..
        } = forecast_for(&with_two)
        else {
            panic!("2サンプルなら残り時間を出す");
        };
        assert_eq!(eta, 9_000, "残り9000B ÷ (1000B/1000ms) = 9000ms");
        // 総量が不明なら % も残り時間も出さない
        let unknown_total = ForecastInput {
            total: 0,
            ..base()
        };
        assert_eq!(
            forecast_for(&unknown_total),
            Forecast::Unknown {
                why: Why::TotalUnknown
            }
        );
    }

    #[test]
    fn a_deadline_that_has_passed_means_the_peer_gives_up() {
        let late = ForecastInput {
            elapsed_ms: 30_001,
            ..base()
        };
        assert_eq!(
            forecast_for(&late),
            Forecast::WillFail {
                reason: FailReason::PeerTimeout
            }
        );
        // 期限ちょうどはまだ待てる(現行の「超えたら」に合わせる)
        let on_time = ForecastInput {
            elapsed_ms: 30_000,
            ..base()
        };
        assert!(matches!(
            forecast_for(&on_time),
            Forecast::WaitingForPeer { .. }
        ));
    }

    #[test]
    fn completing_first_makes_the_deadline_irrelevant() {
        // 届き終えた転送を「相手が待つのをやめた」と誤って断らない
        let done_late = ForecastInput {
            sent: 10_000,
            elapsed_ms: 99_999,
            ..base()
        };
        assert_eq!(forecast_for(&done_late), Forecast::WillPlace);
    }

    #[test]
    fn releasing_before_completion_saves_instead_of_placing() {
        let early = ForecastInput {
            phase: CarryPhase::ReleasedEarly,
            save_dir: Some("Downloads/Knit".to_string()),
            ..base()
        };
        assert_eq!(
            forecast_for(&early),
            Forecast::WillSaveOnly {
                dir: Some("Downloads/Knit".to_string())
            }
        );
        // 遅延提供なら、完了前に離しても置けた(受理は離す前)
        let early_lazy = ForecastInput {
            peer_lazy: true,
            ..early
        };
        assert_eq!(forecast_for(&early_lazy), Forecast::WillPlace);
        // 完了後に離した場合は置けている
        let late = ForecastInput {
            phase: CarryPhase::ReleasedAfterComplete,
            sent: 10_000,
            ..base()
        };
        assert_eq!(forecast_for(&late), Forecast::WillPlace);
    }

    #[test]
    fn missing_input_never_promises_a_place() {
        let nothing = ForecastInput {
            sent: 0,
            total: 0,
            ..base()
        };
        assert_eq!(
            forecast_for(&nothing),
            Forecast::Unknown {
                why: Why::NoTransferStarted
            }
        );
    }

    #[test]
    fn waiting_reports_progress_and_the_peers_patience() {
        let i = base();
        let Forecast::WaitingForPeer {
            percent,
            deadline_ms,
            ..
        } = forecast_for(&i)
        else {
            panic!("未完の押下中は待ち");
        };
        assert_eq!(percent, Some(10));
        assert_eq!(deadline_ms, Some(29_000));
        // 総量より多く送っていても 100% を超えない
        let over = ForecastInput {
            sent: 99_999,
            ..base()
        };
        assert_eq!(forecast_for(&over), Forecast::WillPlace);
    }
}
