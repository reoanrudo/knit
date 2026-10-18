//! 転送結果の通知を**1箇所で**作る(I5)。摩擦 F5(「置いたのに置かれていない」が
//! 成功通知と同じ見た目になる)を構造で防ぐのが目的:
//!
//! - 結果を「置けた / 置けず保存のみ / 渡らなかった」の3状態に言い切る
//! - `SavedOnly` の本文に**成功語を入れない**(「置きました」「送信しました」を使わない)
//! - 必ず (a) 実体がどこにあるか (b) なぜ置けなかったか (c) 次に何をすればよいか
//!   を含める(調査 M3「成功を装わない」・H5「回復動線を示す」)
//!
//! 通知の抑制設定は既存の文化を保つ: 成功は `Suppressible`、失敗・保存のみは `Always`
//! (`crates/mac/src/files.rs` の「注意があるときは必ず出す」を型で表す)

/// 通知を出してよいかの区分。**抑制してよいのは成功だけ**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// 必ず出す(失敗・保存のみ・取りこぼし)
    Always,
    /// ユーザーの設定(転送完了の通知)で抑制してよい
    Suppressible,
}

/// 置けなかった理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveWhy {
    /// 渡し終わる前に離した(相手はネイティブのドラッグを始められない)
    ReleasedEarly,
}

/// 転送の結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryResult {
    /// 相手の画面に置けた(ドロップとして成立した)
    Placed,
    /// 置けず、相手側の受信フォルダに入った
    SavedOnly { why: SaveWhy },
    /// 渡らなかった
    Failed {
        /// 利用者に見せる理由(`bulk::send_error_label` の文言など)
        reason: String,
        /// もう一度試す価値があるか(false なら再試行を促さない)
        retryable: bool,
        /// 相手側に保存できた件数(0 なら言及しない)
        saved_count: usize,
    },
}

/// 1回の受け渡しの結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryOutcome {
    /// 相手の呼び名(例: "Windows PC")
    pub peer: String,
    /// 対象の件数
    pub count: usize,
    pub result: DeliveryResult,
}

/// 通知(タイトル・本文・出し方)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub kind: NoticeKind,
}

/// 通知のタイトルは既存と同じ
pub const TITLE: &str = "Knit";

/// もう一度試す価値があるエラーか。**再試行を促してよい場合だけ true**
/// (権限・共有設定・上限超過・中止は、同じ操作を繰り返しても直らない)
pub fn retryable_error(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    !matches!(
        e.kind(),
        PermissionDenied | InvalidInput | Interrupted | UnexpectedEof
    )
}

/// 「相手の画面に置けた」と**言い切る**語。`SavedOnly` には絶対に入れない
/// (テストは「SavedOnly がこのどれも含まない」を assert する)
pub const PLACEMENT_CLAIMS: &[&str] = &["置けました", "置きました", "置かれました"];

/// 転送が終わったときの結果を、**mac 側で観測できる事実だけ**から決める。
/// 観測できるのは「自分の送信が終わった」と「離した順序」の2つだけで、
/// **相手が実際に画面へ置けたかは相手側の結果(W4: DragDone の逆向き)が要る**。
/// そのため `Placed` の本文でも「置けました」とは言い切らない(摩擦 F5 の嘘を
/// 別の形で作らないため)。W4 が入ったらここで `PlacedToScreen` のような状態を足す
pub fn placed_or_saved(released_early: bool) -> DeliveryResult {
    if released_early {
        DeliveryResult::SavedOnly {
            why: SaveWhy::ReleasedEarly,
        }
    } else {
        DeliveryResult::Placed
    }
}

/// 転送が失敗したときの結果。再試行の可否は**エラー種別から決める**
/// (呼び出し側で反転させない: 判断はここに1つだけ置く)
pub fn failed_result(e: &std::io::Error, saved_count: usize) -> DeliveryResult {
    DeliveryResult::Failed {
        reason: crate::bulk::send_error_label(e),
        retryable: retryable_error(e),
        saved_count,
    }
}

/// 結果から通知を組み立てる(純関数)。文言はここにしか無い
pub fn compose_notice(o: &DeliveryOutcome) -> Notice {
    let peer = o.peer.as_str();
    let count = o.count;
    match &o.result {
        DeliveryResult::Placed => Notice {
            title: TITLE.to_string(),
            // 観測できたのは「転送が終わった」ことまで。置けたかどうかは言い切らない
            body: format!("{count} 件を {peer} へ渡しました(転送完了。相手側で受け取れます)"),
            kind: NoticeKind::Suppressible,
        },
        DeliveryResult::SavedOnly { why } => {
            // 成功語を使わない。「どこに入るか」「なぜか」「次にどうするか」を必ず書く
            let why = match why {
                SaveWhy::ReleasedEarly => {
                    "渡し終わる前に離したため、相手はドラッグを始められませんでした"
                }
            };
            Notice {
                title: TITLE.to_string(),
                body: format!(
                    "{count} 件は {peer} の受信フォルダに保存されています({why})。もう一度掴んで、渡し終わるまで待ってから離すと相手の画面に置けます"
                ),
                kind: NoticeKind::Always,
            }
        }
        DeliveryResult::Failed {
            reason,
            retryable,
            saved_count,
        } => {
            let saved = if *saved_count > 0 {
                format!("(保存済み {saved_count} 件)")
            } else {
                String::new()
            };
            let body = if *retryable {
                format!(
                    "{peer} へファイルを渡せませんでした({reason}。接続を確認してもう一度掴んでください){saved}"
                )
            } else {
                // 直らない失敗で再試行を促さない(同じ操作を繰り返させない)
                format!("{peer} へファイルを渡せませんでした({reason}){saved}")
            };
            Notice {
                title: TITLE.to_string(),
                body,
                kind: NoticeKind::Always,
            }
        }
    }
}

/// 失敗のあとにメニューへ出す回復手段(I6)。通知バナーには
/// `osascript display notification` の制約でアクションボタンを付けられないため、
/// **押せる導線はメニューに置く**(実測: 通知の起動に 133〜255ms かかる点も理由)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// 同じファイルでもう一度送り直せる(再試行の価値がある)
    Resend,
    /// 渡せず相手側の受信フォルダに入った。実体の場所を案内する
    SavedOnPeer,
    /// メニューに出す導線は無い(理由の表示だけ)
    None,
}

/// 直前の結果をメニューに出すか。**成功は出さない**(メニューを混ませない)
pub fn needs_attention(result: &DeliveryResult) -> bool {
    !matches!(result, DeliveryResult::Placed)
}

/// 失敗の分類から回復手段を決める(純関数)。**再試行を促してよい場合だけ Resend**
pub fn recovery_for(result: &DeliveryResult) -> Recovery {
    match result {
        DeliveryResult::Placed => Recovery::None,
        DeliveryResult::SavedOnly { .. } => Recovery::SavedOnPeer,
        DeliveryResult::Failed {
            retryable: true, ..
        } => Recovery::Resend,
        DeliveryResult::Failed {
            retryable: false, ..
        } => Recovery::None,
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    fn failed(retryable: bool) -> DeliveryResult {
        DeliveryResult::Failed {
            reason: "接続が切れました".to_string(),
            retryable,
            saved_count: 0,
        }
    }

    #[test]
    fn only_failures_ask_for_attention() {
        assert!(!needs_attention(&DeliveryResult::Placed), "成功はメニューに出さない");
        assert!(needs_attention(&DeliveryResult::SavedOnly {
            why: SaveWhy::ReleasedEarly
        }));
        assert!(needs_attention(&failed(true)));
        assert!(needs_attention(&failed(false)));
    }

    #[test]
    fn retryable_failure_offers_resend() {
        assert_eq!(recovery_for(&failed(true)), Recovery::Resend);
        // 権限・上限のような直らない失敗では再試行を促さない(理由の表示だけ)
        assert_eq!(recovery_for(&failed(false)), Recovery::None);
        assert_eq!(
            recovery_for(&DeliveryResult::SavedOnly {
                why: SaveWhy::ReleasedEarly
            }),
            Recovery::SavedOnPeer
        );
        assert_eq!(recovery_for(&DeliveryResult::Placed), Recovery::None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(result: DeliveryResult) -> DeliveryOutcome {
        DeliveryOutcome {
            peer: "Windows PC".to_string(),
            count: 3,
            result,
        }
    }

    #[test]
    fn placed_reports_only_what_the_sender_can_observe() {
        let n = compose_notice(&outcome(DeliveryResult::Placed));
        assert_eq!(n.kind, NoticeKind::Suppressible, "成功だけ抑制してよい");
        assert!(n.body.contains("3 件"), "件数: {}", n.body);
        assert!(n.body.contains("Windows PC"), "相手: {}", n.body);
        assert!(n.body.contains("転送完了"), "観測できた事実: {}", n.body);
        // 相手が実際に置けたかは相手側の結果(W4)が要るので、ここでは言い切らない
        for w in PLACEMENT_CLAIMS {
            assert!(
                !n.body.contains(w),
                "送信側だけでは「{w}」と言い切れない: {}",
                n.body
            );
        }
    }

    #[test]
    fn saved_only_never_uses_success_words() {
        for why in [SaveWhy::ReleasedEarly] {
            let n = compose_notice(&outcome(DeliveryResult::SavedOnly { why }));
            assert_eq!(n.kind, NoticeKind::Always, "置けなかったことは必ず出す");
            // 「置けた」と言い切る語(実装が使う語そのもの)を1つも含まない
            for w in PLACEMENT_CLAIMS {
                assert!(
                    !n.body.contains(w),
                    "保存のみの本文に「{w}」を入れない: {}",
                    n.body
                );
            }
            // (a) 実体の場所 (b) 理由 (c) 次の一手 を必ず含む
            assert!(n.body.contains("受信フォルダに保存されています"), "場所: {}", n.body);
            assert!(n.body.contains("離したため"), "理由: {}", n.body);
            assert!(n.body.contains("もう一度掴んで"), "次の一手: {}", n.body);
        }
    }

    /// 語の網羅性は実装と自動で一致させる: `Placed` が使う語を `PLACEMENT_CLAIMS` に
    /// 入れ忘れると、`SavedOnly` に紛れ込んでも検出できない(敵対的検証の変異 a3)
    #[test]
    fn placement_claims_cover_the_words_the_implementation_uses() {
        // 語彙そのものは「置けた」という**起きた事実**を主張する語の一覧として固定する
        assert_eq!(PLACEMENT_CLAIMS, &["置けました", "置きました", "置かれました"]);
        let saved = compose_notice(&outcome(DeliveryResult::SavedOnly {
            why: SaveWhy::ReleasedEarly,
        }));
        // 「置けます」(今後の案内)は正当。禁じるのは「置けました」(起きた事実の主張)
        assert!(
            saved.body.contains("置けます"),
            "次の一手としての案内は残す: {}",
            saved.body
        );
        assert!(
            !saved.body.contains("置けました"),
            "起きた事実としての「置けました」は入れない: {}",
            saved.body
        );
    }

    #[test]
    fn the_sender_side_decision_is_a_pure_function() {
        // mac 側の配線が1行になるよう、判断はここに集約する(変異 c1/c2 対策)
        assert_eq!(placed_or_saved(true), DeliveryResult::SavedOnly {
            why: SaveWhy::ReleasedEarly
        });
        assert_eq!(placed_or_saved(false), DeliveryResult::Placed);
        let retryable = failed_result(&std::io::Error::from(std::io::ErrorKind::NotConnected), 0);
        assert_eq!(
            retryable,
            DeliveryResult::Failed {
                reason: crate::bulk::send_error_label(&std::io::Error::from(
                    std::io::ErrorKind::NotConnected
                )),
                retryable: true,
                saved_count: 0,
            }
        );
        let hopeless = failed_result(&std::io::Error::from(std::io::ErrorKind::PermissionDenied), 2);
        let DeliveryResult::Failed { retryable, saved_count, .. } = hopeless else {
            panic!("失敗として分類される");
        };
        assert!(!retryable, "権限エラーは再試行を促さない");
        assert_eq!(saved_count, 2, "保存済み件数はそのまま通す");
    }

    #[test]
    fn failed_asks_for_a_retry_only_when_it_can_help() {
        let retryable = compose_notice(&outcome(DeliveryResult::Failed {
            reason: "接続が切れました".to_string(),
            retryable: true,
            saved_count: 0,
        }));
        assert_eq!(retryable.kind, NoticeKind::Always);
        assert!(retryable.body.contains("もう一度掴んでください"), "{}", retryable.body);
        assert!(!retryable.body.contains("保存済み"), "0 件なら言及しない: {}", retryable.body);

        let hopeless = compose_notice(&outcome(DeliveryResult::Failed {
            reason: "ファイルの共有が許可されていません".to_string(),
            retryable: false,
            saved_count: 2,
        }));
        assert!(
            !hopeless.body.contains("もう一度"),
            "直らない失敗で再試行を促さない: {}",
            hopeless.body
        );
        assert!(hopeless.body.contains("(保存済み 2 件)"), "{}", hopeless.body);
    }

    #[test]
    fn retryable_follows_the_error_kind() {
        use std::io::ErrorKind::*;
        for (kind, expected) in [
            (NotConnected, true),
            (TimedOut, true),
            (ConnectionReset, true),
            (PermissionDenied, false),
            (InvalidInput, false),
            (Interrupted, false),
            (UnexpectedEof, false),
        ] {
            assert_eq!(
                retryable_error(&std::io::Error::from(kind)),
                expected,
                "{kind:?}"
            );
        }
    }

    /// 表テスト: 既存文言から引き継ぐ要素が消えていないこと
    #[test]
    fn keeps_the_existing_phrases() {
        let failed = compose_notice(&outcome(DeliveryResult::Failed {
            reason: "接続が切れました".to_string(),
            retryable: true,
            saved_count: 0,
        }));
        assert!(
            failed.body.contains("へファイルを渡せませんでした("),
            "既存の言い回しを保つ: {}",
            failed.body
        );
        assert_eq!(failed.title, TITLE);
    }
}
