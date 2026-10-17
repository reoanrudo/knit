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

/// `SavedOnly` の本文に混ざってはいけない成功語(テストで否定 assert する)
pub const SUCCESS_WORDS: &[&str] = &["置きました", "送信しました", "渡しました", "完了しました"];

/// 結果から通知を組み立てる(純関数)。文言はここにしか無い
pub fn compose_notice(o: &DeliveryOutcome) -> Notice {
    let peer = o.peer.as_str();
    let count = o.count;
    match &o.result {
        DeliveryResult::Placed => Notice {
            title: TITLE.to_string(),
            body: format!("{count} 件を {peer} へ渡しました(相手の画面に置けました)"),
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
    fn placed_is_a_success_that_may_be_suppressed() {
        let n = compose_notice(&outcome(DeliveryResult::Placed));
        assert_eq!(n.kind, NoticeKind::Suppressible, "成功だけ抑制してよい");
        assert!(n.body.contains("3 件"), "件数: {}", n.body);
        assert!(n.body.contains("Windows PC"), "相手: {}", n.body);
        assert!(n.body.contains("置けました"), "置けたと言い切る: {}", n.body);
    }

    #[test]
    fn saved_only_never_uses_success_words() {
        for why in [SaveWhy::ReleasedEarly] {
            let n = compose_notice(&outcome(DeliveryResult::SavedOnly { why }));
            assert_eq!(n.kind, NoticeKind::Always, "置けなかったことは必ず出す");
            for w in SUCCESS_WORDS {
                assert!(
                    !n.body.contains(w),
                    "保存のみの本文に成功語「{w}」を入れない: {}",
                    n.body
                );
            }
            // (a) 実体の場所 (b) 理由 (c) 次の一手 を必ず含む
            assert!(n.body.contains("受信フォルダに保存されています"), "場所: {}", n.body);
            assert!(n.body.contains("離したため"), "理由: {}", n.body);
            assert!(n.body.contains("もう一度掴んで"), "次の一手: {}", n.body);
        }
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
