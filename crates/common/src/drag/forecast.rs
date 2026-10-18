//! 「今離すとどうなるか」の予報(I2)。**表示はこの値の写像にすぎない**ので、
//! 文言や判断を UI 側に書かない。入力は観測できる事実だけに限り、分からない
//! ことは `Unknown` にして成功語を出さない(調査 M2・M3 / 摩擦 F2・F3)
//!
//! 申し送り(次の増分で扱うこと):
//! 1. `peer_lazy` は相手の**能力**であって「今回の遅延提供が受理されたか」ではない。
//!    受理の事実を扱う増分(I8)で `ForecastInput` に受理状態を足すこと。
//! 2. 残り時間(`eta_ms`)と相手の残り我慢(`deadline_ms`)の比較は**表示側の仕事**。
//!    ETA が期限より長いときに「あと◯秒」と言うと嘘になる(I4 で比較する)。
//! 3. `speed_samples` は呼び出し側の自己申告。実測を渡す責任は呼び出し側にある。
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
        /// 相手が待つのをやめるまでの残り(None = 期限が不明。Some(0) は期限ちょうど)
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
    /// 相手が遅延提供(drag.lazy)に**対応しているか**(能力。版で決まる)
    pub peer_lazy: bool,
    /// 相手が**この掴みを受理したか**(事実。受理の行が届いたときだけ true)。
    /// **能力と混同しない**: 対応していても受理していなければ待つ必要がある
    pub accepted: bool,
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

/// 完了までの目安。**総量が分かり、速度の実測が2件以上あるときだけ**出す。
/// `total <= sent` は現在の呼び出し元(未完の待ち状態)からは到達しないが、
/// 将来ここを別の状態から呼んでも 0 や負を作らないための防御として残す
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
    // 遅延提供で**受理済み**: 離す前にドロップが成立しているので押下を待たずに置ける。
    // 能力(`peer_lazy`)だけでは「置ける」と言わない(受理していなければ相手は
    // まだドラッグを始めておらず、離すと保存のみになる)
    if i.accepted {
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
            accepted: false,
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
            accepted: true,
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
            accepted: true,
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

    /// **能力と受理を混同しない**: 版 17 の相手でも、受理が届いていなければ
    /// 「待つ」ままにする(相手が遅延提供を無効にしている場合が実際にある)
    #[test]
    fn capability_without_acceptance_still_waits() {
        let capable = ForecastInput {
            peer_lazy: true,
            accepted: false,
            ..base()
        };
        assert!(
            matches!(forecast_for(&capable), Forecast::WaitingForPeer { .. }),
            "受理が来るまでは待つ"
        );
        // 受理が届いたら、同じ入力で「置ける」に反転する(能力だけでは反転しない)
        let accepted = ForecastInput {
            peer_lazy: true,
            accepted: true,
            ..base()
        };
        assert_eq!(forecast_for(&accepted), Forecast::WillPlace);
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

    /// 実際の流れ: 50% で離す → Mac は送信を続ける → 届き終える。ここで「離した順序」
    /// より「完了」を先に見ると **置けていないのに「置けた」と言う**(F5 の嘘の再発)。
    /// 到達可能な入力で順序を固定する(変異 m8 が生き残っていた穴)
    #[test]
    fn releasing_early_stays_save_only_even_after_the_bytes_arrive() {
        let finished_after_release = ForecastInput {
            phase: CarryPhase::ReleasedEarly,
            sent: 10_000,
            total: 10_000,
            ..base()
        };
        assert_eq!(
            forecast_for(&finished_after_release),
            Forecast::WillSaveOnly { dir: None },
            "完了前に離したら、後から届き終えても置けてはいない"
        );
        // 差が「離した順序」の1点だけで決まることを固定する
        let released_after_complete = ForecastInput {
            phase: CarryPhase::ReleasedAfterComplete,
            ..finished_after_release.clone()
        };
        assert_eq!(forecast_for(&released_after_complete), Forecast::WillPlace);
        // 保存先が分からない場合も「保存のみ」であることは言い切れる
        assert!(matches!(
            forecast_for(&finished_after_release),
            Forecast::WillSaveOnly { dir: None }
        ));
    }

    /// 相手の期限が不明(None)のときは、残り我慢を数字で出さない(嘘を作らない)
    #[test]
    fn an_unknown_deadline_is_not_shown_as_a_number() {
        let no_deadline = ForecastInput {
            peer_deadline_ms: 0,
            ..base()
        };
        let Forecast::WaitingForPeer {
            deadline_ms,
            percent,
            eta_ms,
        } = forecast_for(&no_deadline)
        else {
            panic!("未完の押下中は待ち");
        };
        assert_eq!(deadline_ms, None, "期限が不明なら数字を出さない");
        assert_eq!(percent, Some(10), "% は総量から出せる");
        assert_eq!(eta_ms, Some(9_000), "残り時間も総量と速度から出せる");
    }

    /// **AC2: 「今離すとどうなるか」が常に1つに定まる**(曖昧なまま離させない)。
    /// 代表ケースではなく**入力の全組合せ**(3×4×5×4×3×2×2 = 2880 通り)を回して、
    /// 返る値が必ず1つであることと、`Unknown` を返してよい条件を固定する
    #[test]
    fn every_input_combination_yields_exactly_one_forecast() {
        let phases = [
            CarryPhase::Held,
            CarryPhase::ReleasedEarly,
            CarryPhase::ReleasedAfterComplete,
        ];
        let refusals = [
            None,
            Some(Refusal::TooMany),
            Some(Refusal::LinkDown),
            Some(Refusal::Busy),
        ];
        let pairs = [(0u64, 0u64), (0, 100), (50, 100), (100, 100), (150, 100)];
        let samples = [0u32, 1, 2, 5];
        let deadlines = [0u64, 1_000, 30_000];
        let mut kinds: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        let mut cases = 0usize;
        for &phase in &phases {
            for refusal in &refusals {
                for &(sent, total) in &pairs {
                    for &speed_samples in &samples {
                        for &peer_deadline_ms in &deadlines {
                            for &accepted in &[false, true] {
                                for &peer_lazy in &[false, true] {
                                    let i = ForecastInput {
                                        phase,
                                        peer_lazy,
                                        accepted,
                                        sent,
                                        total,
                                        elapsed_ms: 500,
                                        speed_samples,
                                        peer_deadline_ms,
                                        refusal: refusal.clone(),
                                        save_dir: None,
                                    };
                                    let f = forecast_for(&i);
                                    cases += 1;
                                    let kind = match &f {
                                        Forecast::WillPlace => "place",
                                        Forecast::WillSaveOnly { .. } => "save",
                                        Forecast::WaitingForPeer { .. } => "wait",
                                        Forecast::WillFail { .. } => "fail",
                                        Forecast::Unknown { .. } => "unknown",
                                    };
                                    *kinds.entry(kind).or_insert(0) += 1;

                                    // 1) 材料不足のときだけ Unknown(理由も説明できる)
                                    if let Forecast::Unknown { why } = &f {
                                        assert!(
                                            refusal.is_none(),
                                            "拒否が確定していれば Unknown にしない: {i:?}"
                                        );
                                        assert_eq!(sent == 0, total == 0, "材料不足の条件: {i:?}");
                                        let want = if sent == 0 {
                                            Why::NoTransferStarted
                                        } else {
                                            Why::TotalUnknown
                                        };
                                        assert_eq!(why, &want, "理由が説明できる: {i:?}");
                                    }
                                    // 2) 拒否は何よりも優先される(「置けます」と言わない)
                                    if refusal.is_some() {
                                        assert!(
                                            matches!(
                                                f,
                                                Forecast::WillFail {
                                                    reason: FailReason::Refused(_)
                                                }
                                            ),
                                            "拒否が最優先: {i:?} → {f:?}"
                                        );
                                    }
                                    // 3) 「置ける」と言えるのは、受理済みか全部届いたときだけ
                                    if matches!(f, Forecast::WillPlace) {
                                        assert!(
                                            accepted || sent >= total,
                                            "置ける根拠が無い: {i:?} → {f:?}"
                                        );
                                    }
                                    // 4) 完了前に離したら保存のみ(受理済みの場合を除く)
                                    if phase == CarryPhase::ReleasedEarly
                                        && !accepted
                                        && refusal.is_none()
                                        && !(sent == 0 && total == 0)
                                        && total > 0
                                    {
                                        assert_eq!(
                                            f,
                                            Forecast::WillSaveOnly { dir: None },
                                            "完了前の離しは保存のみ: {i:?}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 2880, "全組合せを回している(空回りしていない)");
        assert!(kinds.len() >= 4, "複数の結果が出る組み合わせ: {kinds:?}");
        println!("[forecast] 組合せ {cases} 通り: {kinds:?}");
    }
}
