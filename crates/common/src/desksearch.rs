//! Search My Desk(ビジョン§14)の検索ロジック。
//! アプリ・デスクのファイル/フォルダ・コマンド・クリップボード履歴・URL を
//! 横断して候補に並べる。OS や UI に依存しない純粋な順位付けとして、両側でテストできる。

/// 候補の種別。App は起動、File は既定アプリで開く、Cmd は内蔵コマンド実行、
/// History はこの端末のクリップボードへ復元、Url は相手 PC の既定ブラウザで開く
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    App,
    File,
    Cmd,
    History,
    Url,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub kind: Kind,
    /// メニュー等への表示(種別の接頭辞つき)
    pub title: String,
    /// 実行に使う本文(アプリのフルパス / ファイルのパス / コマンド ID /
    /// 履歴の本文 / URL)
    pub text: String,
}

/// 本文全体が 1 つの http(s) URL のときだけ「相手で開く」候補になる
pub fn is_single_url(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty()
        && !t.contains([' ', '\n', '\r', '\t'])
        && (t.starts_with("http://") || t.starts_with("https://"))
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn starts_with_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().starts_with(&needle.to_lowercase())
}

/// (表示名, 実行本文)のリストをクエリで評価し、前方一致→部分一致の順に返す
fn ranked<'a>(items: &'a [(String, String)], query: &str) -> Vec<&'a (String, String)> {
    let mut forward = Vec::new();
    let mut partial = Vec::new();
    for it in items {
        if starts_with_ignore_case(&it.0, query) {
            forward.push(it);
        } else if contains_ignore_case(&it.0, query) {
            partial.push(it);
        }
    }
    forward.into_iter().chain(partial).collect()
}

/// クエリから候補を最大 max 件返す。
/// - 入力全体が URL のときは URL 候補 1 件を先頭に置く(相手 PC で開く)
/// - アプリ→ファイル/フォルダ→コマンド→履歴の順で、各カテゴリ内は
///   名前の前方一致を優先し、同点は入力順
pub fn search(
    query: &str,
    apps: &[(String, String)],
    files: &[(String, String)],
    commands: &[(String, String)],
    history_entries: &[(u64, String)],
    max: usize,
) -> Vec<Hit> {
    let query = query.trim();
    let mut hits: Vec<Hit> = Vec::new();
    if is_single_url(query) {
        hits.push(Hit {
            kind: Kind::Url,
            title: format!("URL・{} で開く(Windows)", query),
            text: query.to_string(),
        });
    }
    if !query.is_empty() {
        for (name, path) in ranked(apps, query) {
            hits.push(Hit {
                kind: Kind::App,
                title: format!("アプリ・{name}"),
                text: path.clone(),
            });
        }
        for (name, path) in ranked(files, query) {
            hits.push(Hit {
                kind: Kind::File,
                title: format!("ファイル・{name}"),
                text: path.clone(),
            });
        }
        for (name, id) in ranked(commands, query) {
            hits.push(Hit {
                kind: Kind::Cmd,
                title: format!("コマンド・{name}"),
                text: id.clone(),
            });
        }
        for (_, text) in history_entries {
            if contains_ignore_case(text, query) {
                hits.push(Hit {
                    kind: Kind::History,
                    title: format!("履歴・{}", history_preview(text, 40)),
                    text: text.clone(),
                });
            }
        }
    } else {
        // 空クエリ: アプリ上位と履歴の新しい順を混ぜて提案する
        let mut i = 0usize;
        for (name, path) in apps.iter().take(max / 2 + 1) {
            hits.push(Hit {
                kind: Kind::App,
                title: format!("アプリ・{name}"),
                text: path.clone(),
            });
            i += 1;
            if i >= max {
                break;
            }
        }
        for (_, text) in history_entries.iter().take(max.saturating_sub(hits.len())) {
            hits.push(Hit {
                kind: Kind::History,
                title: format!("履歴・{}", history_preview(text, 40)),
                text: text.clone(),
            });
        }
    }
    hits.truncate(max);
    hits
}

/// 履歴候補の見た目。画像エントリ(ファイル名\tサイズ)は本体を文字列で
/// 見せても意味がないため、大きさだけを示す
fn history_preview(text: &str, max: usize) -> String {
    match crate::history::parse_image_entry(text) {
        Some((_, size)) => format!("画像・{}KB", (size / 1024).max(1)),
        None => clip(text, max),
    }
}

/// 表示用に丸める(改行を潰し max 文字で切る)
fn clip(text: &str, max: usize) -> String {
    let flat = text
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect::<String>();
    let flat = flat.trim();
    if flat.chars().count() <= max {
        flat.to_string()
    } else {
        let head: String = flat.chars().take(max).collect();
        format!("{head}…")
    }
}

/// 相手 PC のアプリ候補に「{label}・」前置を付ける(両 PC を 1 つのリストへ混ぜる)。
/// どちらの PC の候補か実行前に分かることが越境検索の最低条件
pub fn mark_remote(hits: Vec<Hit>, label: &str) -> Vec<Hit> {
    hits.into_iter()
        .map(|h| Hit {
            kind: h.kind,
            title: format!("{label}・{}", h.title),
            text: h.text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps() -> Vec<(String, String)> {
        vec![
            ("Safari.app".into(), "/Applications/Safari.app".into()),
            (
                "Visual Studio Code.app".into(),
                "/Applications/Visual Studio Code.app".into(),
            ),
            ("Terminal.app".into(), "/Applications/Terminal.app".into()),
        ]
    }

    fn history() -> Vec<(u64, String)> {
        vec![
            (3, "https://example.com/docs".into()),
            (2, "meeting notes 2026-09".into()),
            (1, "cargo test --locked".into()),
        ]
    }

    fn files() -> Vec<(String, String)> {
        vec![
            (
                "JapaneseHouse.blend".into(),
                "/Users/me/Blender/JapaneseHouse.blend".into(),
            ),
            (
                "JapaneseHouse_reference.pdf".into(),
                "/Users/me/Desktop/JapaneseHouse_reference.pdf".into(),
            ),
        ]
    }

    fn commands() -> Vec<(String, String)> {
        vec![
            ("Windows をロック".into(), "win_lock".into()),
            ("画面を暗くする".into(), "display_sleep".into()),
        ]
    }

    #[test]
    fn full_url_query_becomes_the_first_hit() {
        let hits = search(
            "https://example.com",
            &apps(),
            &files(),
            &commands(),
            &history(),
            8,
        );
        assert_eq!(
            hits[0].kind,
            Kind::Url,
            "URL 全文入力は先頭に「相手で開く」"
        );
        assert_eq!(hits[0].text, "https://example.com");
    }

    #[test]
    fn app_name_forward_match_beats_partial_match() {
        let hits = search("s", &apps(), &files(), &commands(), &history(), 8);
        assert_eq!(hits[0].kind, Kind::App);
        assert_eq!(hits[0].text, "/Applications/Safari.app", "前方一致が先");
        assert!(
            hits.iter()
                .any(|h| h.kind == Kind::App && h.text.contains("Visual")),
            "部分一致も出る(大文字小文字を区別しない)"
        );
    }

    #[test]
    fn history_matches_by_substring_in_newest_order() {
        let hits = search("example", &apps(), &files(), &commands(), &history(), 8);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, Kind::History);
        assert_eq!(hits[0].text, "https://example.com/docs");
    }

    #[test]
    fn empty_query_proposes_apps_then_history() {
        let hits = search("", &apps(), &files(), &commands(), &history(), 4);
        assert!(hits.len() <= 4);
        assert!(
            hits.iter().any(|h| h.kind == Kind::App),
            "空ならアプリを提案"
        );
        assert!(
            hits.iter().any(|h| h.kind == Kind::History),
            "空なら履歴を提案"
        );
    }

    #[test]
    fn results_are_bounded() {
        let hits = search("", &apps(), &files(), &commands(), &history(), 2);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn multiline_history_is_flattened_in_title() {
        let h = vec![(1u64, "line1\nline2".to_string())];
        let hits = search("line2", &apps(), &files(), &commands(), &h, 8);
        assert!(!hits[0].title.contains('\n'), "候補タイトルは 1 行");
    }

    #[test]
    fn remote_apps_are_labelled_for_cross_pc_results() {
        let hits = search("term", &apps(), &files(), &commands(), &history(), 8);
        let marked = mark_remote(hits, "Windows");
        assert_eq!(marked[0].kind, Kind::App);
        assert!(
            marked[0].title.starts_with("Windows・アプリ・"),
            "前置きが付く"
        );
        assert_eq!(
            marked[0].text, "/Applications/Terminal.app",
            "本文は起動用のまま"
        );
    }

    #[test]
    fn file_hits_rank_after_apps_and_carry_their_path() {
        let hits = search(
            "japanesehouse",
            &apps(),
            &files(),
            &commands(),
            &history(),
            8,
        );
        assert!(
            hits.iter().all(|h| h.kind != Kind::App),
            "アプリ名が一部重ならない前提"
        );
        let f = hits.iter().find(|h| h.kind == Kind::File).unwrap();
        assert_eq!(
            f.text, "/Users/me/Blender/JapaneseHouse.blend",
            "前方一致のファイルを先頭に"
        );
        assert!(f.title.starts_with("ファイル・"), "種別をタイトルへ出す");
        assert!(hits.iter().filter(|h| h.kind == Kind::File).count() >= 2);
    }

    #[test]
    fn command_hits_execute_by_id() {
        let hits = search("ロック", &apps(), &files(), &commands(), &history(), 8);
        let c = hits.iter().find(|h| h.kind == Kind::Cmd).unwrap();
        assert_eq!(c.text, "win_lock", "実行はコマンド ID で識別する");
        assert!(c.title.contains("Windows をロック"));
    }

    #[test]
    fn all_categories_share_the_max_budget() {
        let hits = search("e", &apps(), &files(), &commands(), &history(), 3);
        assert_eq!(hits.len(), 3, "カテゴリを増やしても上限は守られる");
    }
}
