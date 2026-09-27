//! 全デバイスのクリップボードを統合して見る履歴(Universal Clipboard History)。
//! 各端末が「相手へ同期した・相手から受け取った」テキストとファイル参照を蓄積し、
//! メニューから選ぶとその端末のクリップボードへ復元できる。テキスト/URL に加え、
//! ファイルは「パスの改行区切り」で載せる(本体は送受信で各端末へ届いているため、
//! パスさえ残せば Finder/Explorer の ⌘V 相当で復元できる)。
//! 画像は履歴 JSON へは載せず、本体を端末ごとの images/ ディレクトリへ
//! ハッシュ名で保存し、履歴には「ファイル名\tバイト数」だけ残す
//! (Mac は BMP、Windows は DIB の生バイトで保存するため復元時に変換が要らない)。
//! 本文は相手へ送る前に機密判定を通ったものだけなので、ここには検査を置かない。

use serde::{Deserialize, Serialize};

/// 履歴の種別。URL は本文全体が 1 つの http(s) URL のとき、File は
/// push_files で載せたパス群のとき、Image は画像の本体を保存したときだけ使う
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Url,
    File,
    Image,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub kind: Kind,
    pub text: String,
    /// エポックミリ秒
    pub ts: u64,
    /// 発生した端末の名前("Mac"/"Windows")
    pub device: String,
}

/// 永続化形式(保存と読み込みで同一。将来の項目追加は Option で足す)
#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    entries: Vec<Entry>,
}

/// 上限つき履歴。新しいものを末尾に置く
pub struct History {
    entries: Vec<Entry>,
    next_id: u64,
    cap: usize,
}

/// 載せる本文の上限(クリップボード同期と同じ 1MB)
pub const MAX_CHARS: usize = 1024 * 1024;

/// メニューへ出す最大件数
pub const MENU_ITEMS: usize = 10;

/// 現在のエポックミリ秒(端末の内蔵時計と分けないため履歴はこれで統一する)
pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl History {
    pub const fn new(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            cap,
        }
    }

    /// テキストを履歴へ追加する。直前と同じ内容は時刻だけ更新する。
    /// 空・上限超過は載せない。戻り値は採用した項目の id(重複更新を含む)
    pub fn push_text(&mut self, text: &str, device: &str, now_ms: u64) -> Option<u64> {
        let text = text.trim();
        if text.is_empty() || text.len() > MAX_CHARS {
            return None;
        }
        let text = text.to_string();
        if let Some(last) = self.entries.last_mut() {
            if last.text == text {
                last.ts = now_ms;
                last.device = device.to_string();
                return Some(last.id);
            }
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        let kind = if is_single_url(&text) {
            Kind::Url
        } else {
            Kind::Text
        };
        let id = self.next_id;
        self.entries.push(Entry {
            id,
            kind,
            text,
            ts: now_ms,
            device: device.to_string(),
        });
        self.next_id += 1;
        Some(id)
    }

    /// ファイル群を履歴へ追加する。本文はパスの改行区切り(復元側は行ごとに
    /// パスとして扱う)。ファイル同期は機密判定の対象外のためここでも検査しない
    pub fn push_files(
        &mut self,
        paths: &[std::path::PathBuf],
        device: &str,
        now_ms: u64,
    ) -> Option<u64> {
        if paths.is_empty() {
            return None;
        }
        let text = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() || text.len() > MAX_CHARS {
            return None;
        }
        if let Some(last) = self.entries.last_mut() {
            if last.kind == Kind::File && last.text == text {
                last.ts = now_ms;
                last.device = device.to_string();
                return Some(last.id);
            }
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        let id = self.next_id;
        self.entries.push(Entry {
            id,
            kind: Kind::File,
            text,
            ts: now_ms,
            device: device.to_string(),
        });
        self.next_id += 1;
        Some(id)
    }

    /// 画像を履歴へ追加する。本体は呼び出し側が images/ ディレクトリへ
    /// image_file_name() の名前で保存しており、本文は「ファイル名\tバイト数」。
    /// 同じ画像(=同じファイル名)が続いたら時刻だけ更新する
    pub fn push_image(
        &mut self,
        name: &str,
        size_bytes: usize,
        device: &str,
        now_ms: u64,
    ) -> Option<u64> {
        if !is_image_store_name(name) {
            return None;
        }
        let text = format!("{name}\t{size_bytes}");
        if let Some(last) = self.entries.last_mut() {
            if last.kind == Kind::Image && last.text == text {
                last.ts = now_ms;
                last.device = device.to_string();
                return Some(last.id);
            }
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        let id = self.next_id;
        self.entries.push(Entry {
            id,
            kind: Kind::Image,
            text,
            ts: now_ms,
            device: device.to_string(),
        });
        self.next_id += 1;
        Some(id)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// 新しい順で n 件(メニュー等の表示用)
    pub fn recent(&self, n: usize) -> Vec<&Entry> {
        self.entries.iter().rev().take(n).collect()
    }

    /// id で 1 件(復元用)
    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 変化検知用の世代(最後の項目 id。空なら 0)
    pub fn last_id(&self) -> u64 {
        self.entries.last().map(|e| e.id).unwrap_or(0)
    }

    /// 履歴を JSON ファイルへ保存する。書けない場合も静かに失敗する
    /// (履歴が消えるだけで操作は止めない)
    pub fn save_to(&self, path: &std::path::Path) {
        let data = Persisted {
            entries: self.entries.clone(),
        };
        if let Ok(json) = serde_json::to_string(&data) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
                restrict_dir(parent);
            }
            // まず一時ファイルへ書いて差し替える(書きかけの破損を避ける)
            let tmp = path.with_extension("json.tmp");
            if write_private(&tmp, json.as_bytes()).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    /// JSON ファイルから履歴を復元する。上限を超えた分は古い方から落とす。
    /// ファイルが無い・壊れている場合は空のまま起動する
    pub fn load_from(&mut self, path: &std::path::Path) {
        let Ok(s) = std::fs::read_to_string(path) else {
            return;
        };
        // 旧版が umask 既定(644)で作ったファイルを読めたら 600 へ是正する
        restrict(path);
        let Ok(p) = serde_json::from_str::<Persisted>(&s) else {
            return;
        };
        let mut loaded = p.entries;
        while loaded.len() > self.cap {
            loaded.remove(0);
        }
        self.entries = loaded;
        self.next_id = self.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    }

    /// 全件削除。保存ファイルも呼び出し側で削除する
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// 履歴本文と画像実体を、所有者だけが読める権限で書き込む。
/// ~/.config 下は既定 umask(022)だと他ユーザーに読める(644)ため、
/// トークン設定(~/.config/tsunagu/env=600)と同じ土俵へ揃える。
/// Windows はプロファイル直下の既定 ACL に任せる(何もしない)
pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    restrict(path);
    Ok(())
}

/// 既存ファイルの権限を所有者のみへ是正する(起動時の読み込み後にも呼ぶ)
pub fn restrict(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// 履歴・画像ディレクトリを所有者のみへ限定する
pub fn restrict_dir(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// 本文全体が 1 つの http(s) URL とみなせるか(表示種別としてのみ使う)
fn is_single_url(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty()
        && !t.contains([' ', '\n', '\r', '\t'])
        && (t.starts_with("http://") || t.starts_with("https://"))
}

/// 履歴本文がファイル参照の並びか(復元側の分岐用)。kind を保存していない旧
/// 履歴との互換のため、kind==File でなくても「全行が絶対パスらしき文字列」なら
/// true として扱う。パスの実在確認は呼び出し側(I/O)で行う
pub fn looks_like_file_paths(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty()
        && t.lines().all(|l| {
            let l = l.trim();
            !l.is_empty()
                && (l.starts_with('/')
                    || l.starts_with("\\\\")
                    // Windows のドライブレター(C:\...)。Unix 側では has_root が
                    // false になるため文字で見る
                    || l.as_bytes().get(1) == Some(&b':')
                    || std::path::Path::new(l).has_root())
        })
}

/// 画像保存名のパターン: "img-" + 16 桁の 16 進 + ".bmp"/".dib"。
/// 生成側と判定側で同じ形を保証するため、ここだけが知識の源
fn is_image_store_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 4 + 16 + 4
        && &b[..4] == b"img-"
        && b[4..20].iter().all(|c| c.is_ascii_hexdigit())
        && (&b[20..] == b".bmp" || &b[20..] == b".dib")
}

/// 画像バイトから保存ファイル名を作る。同名=同一内容なので、同じ画像を
/// 何度履歴へ載せても実体は 1 つ(上書き)になる
pub fn image_file_name(hash: u64, ext: &str) -> String {
    format!("img-{hash:016x}.{ext}")
}

/// FNV-1a(64bit)。画像の内容ハッシュ用(衝突耐性は不要、名前衝突の実害が
/// 別画像を間違って復元する程度のため簡易ハッシュで十分)
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 画像履歴エントリの本文を「(ファイル名, バイト数)」へ崩す。形式が少しでも
/// 違えば None(通常テキストを画像扱いしないための厳格チェック)
pub fn parse_image_entry(text: &str) -> Option<(String, u64)> {
    let (name, size) = text.split_once('\t')?;
    if size.is_empty() || !size.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !is_image_store_name(name) {
        return None;
    }
    let size: u64 = size.parse().ok()?;
    Some((name.to_string(), size))
}

/// 画像保存ディレクトリの刈り込み。新しい mtime 順に keep 件残して古い実体を
/// 削除する(履歴上限 50 に対し少し余裕を持たせた件数を呼び出し側が渡す)。
/// 履歴から追い出された画像の実体が居座り続けるのを防ぐ
pub fn prune_image_store(dir: &std::path::Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<(std::path::PathBuf, std::time::SystemTime)> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            (n.ends_with(".bmp") || n.ends_with(".dib")) && is_image_store_name(&n)
        })
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((e.path(), mtime))
        })
        .collect();
    if items.len() <= keep {
        return;
    }
    items.sort_by(|a, b| b.1.cmp(&a.1)); // 新しい順
    for (path, _) in items.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// メニュー/リスト表示用の 1 行プレビュー(改行を潰し max 文字で丸める)
pub fn preview(text: &str, max: usize) -> String {
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

/// メニュー用ラベル("URL・3分前・Windows・テキストの先頭…"。URL/ファイルは
/// 先頭に種別を添える。ファイルは 1 件目の名前と件数で示す)
pub fn label(entry: &Entry, now_ms: u64, max: usize) -> String {
    let mins = now_ms.saturating_sub(entry.ts) / 60_000;
    let ago = if mins < 1 {
        "たった今".to_string()
    } else if mins < 60 {
        format!("{mins}分前")
    } else if mins < 60 * 24 {
        format!("{}時間前", mins / 60)
    } else {
        format!("{}日前", mins / (60 * 24))
    };
    match entry.kind {
        Kind::Url => format!(
            "{ago}・{}・URL・{}",
            entry.device,
            preview(&entry.text, max)
        ),
        Kind::File => {
            let names: Vec<String> = entry
                .text
                .lines()
                .filter_map(|l| {
                    std::path::Path::new(l.trim())
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                })
                .collect();
            let shown = if names.len() > 1 {
                format!("{} ほか{}件", names[0], names.len() - 1)
            } else {
                names.first().cloned().unwrap_or_default()
            };
            format!(
                "{ago}・{}・ファイル・{}",
                entry.device,
                preview(&shown, max)
            )
        }
        Kind::Image => {
            // 画像は内容を示せないため、せめて大きさで見当をつけられるようにする
            let size = parse_image_entry(&entry.text)
                .map(|(_, s)| format!("・{}KB", (s / 1024).max(1)))
                .unwrap_or_default();
            format!("{ago}・{}・画像{size}", entry.device)
        }
        Kind::Text => format!("{ago}・{}・{}", entry.device, preview(&entry.text, max)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_recent_entries_bounded_and_merges_duplicates() {
        let mut h = History::new(3);
        for i in 1..=4u64 {
            h.push_text(&format!("text{i}"), "Mac", i * 1000);
        }
        assert_eq!(h.entries().len(), 3, "上限を超えたら最も古い項目を落とす");
        assert_eq!(h.entries()[0].text, "text2", "古いものから消える");
        // 直前と同じ内容は新規項目にせず時刻だけ更新する
        let before = h.last_id();
        h.push_text("text4", "Windows", 9_000);
        assert_eq!(h.entries().len(), 3);
        assert_eq!(h.last_id(), before, "重複は項目を増やさない");
        assert_eq!(
            h.entries().last().unwrap().device,
            "Windows",
            "重複更新は最新側を示す"
        );
    }

    #[test]
    fn classifies_a_single_url_and_rejects_empty() {
        let mut h = History::new(10);
        assert!(h.push_text("", "Mac", 1).is_none(), "空は載せない");
        assert!(
            h.push_text("   \n ", "Mac", 1).is_none(),
            "空白だけも載せない"
        );
        h.push_text("https://example.com/a?b=1", "Mac", 2);
        assert_eq!(
            h.entries().last().unwrap().kind,
            Kind::Url,
            "1行のhttp(s)はURL"
        );
        h.push_text("https://example.com\n2行目", "Mac", 3);
        assert_eq!(
            h.entries().last().unwrap().kind,
            Kind::Text,
            "複数行はテキスト"
        );
    }

    #[test]
    fn recent_and_get_are_stable_across_bound() {
        let mut h = History::new(2);
        let first = h.push_text("a", "Mac", 1).unwrap();
        h.push_text("b", "Mac", 2);
        h.push_text("c", "Windows", 3);
        assert_eq!(h.recent(10).len(), 2);
        assert_eq!(h.recent(10)[0].text, "c", "recent は新しい順");
        assert!(
            h.get(first).is_none(),
            "上限で追い出された項目は参照できない"
        );
        assert_eq!(h.last_id(), 3);
    }

    #[test]
    fn label_flattens_newlines_and_truncates() {
        let mut h = History::new(10);
        let long = "あいうえおかきくけこさしすせそ\nたちつてとなにぬねのはひふへほ";
        let pushed = h.push_text(long, "Windows", 9_999).unwrap();
        let l = label(h.get(pushed).unwrap(), 10_000, 20);
        assert!(!l.contains('\n'), "ラベルは 1 行");
        assert!(
            l.starts_with("たった今・Windows・"),
            "経過時間と端末名を先頭へ"
        );
        assert!(l.ends_with('…'), "長い本文は丸める");
    }

    #[test]
    fn label_marks_a_url_entry() {
        let mut h = History::new(10);
        h.push_text("https://example.com", "Mac", 1);
        let l = label(h.entries().last().unwrap(), 2_000, 30);
        assert!(l.contains("URL・"), "URL 種別をラベルへ出す");
    }

    #[test]
    fn file_entries_carry_paths_and_merge_duplicates() {
        let mut h = History::new(10);
        let paths = vec![
            std::path::PathBuf::from("/Users/me/Desktop/資料.pdf"),
            std::path::PathBuf::from("/Users/me/Desktop/画像.png"),
        ];
        let id = h.push_files(&paths, "Mac", 1_000).unwrap();
        let e = h.get(id).unwrap();
        assert_eq!(e.kind, Kind::File);
        assert_eq!(e.text.lines().count(), 2, "パスを改行区切りで保持");

        let before = h.last_id();
        h.push_files(&paths, "Windows", 2_000);
        assert_eq!(h.last_id(), before, "同一ファイル群は項目を増やさない");
        assert_eq!(h.get(id).unwrap().device, "Windows");
        assert!(h.push_files(&[], "Mac", 3_000).is_none(), "0 件は載せない");

        let l = label(h.get(id).unwrap(), 3_000, 40);
        assert!(l.contains("ファイル・"), "ファイル種別をラベルへ出す");
        assert!(l.contains("資料.pdf ほか1件"), "1 件目の名前と件数で示す");
    }

    #[test]
    fn file_kind_survives_disk_round_trip() {
        let dir = std::env::temp_dir().join(format!("tsunagu-history-file-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = History::new(5);
        h.push_files(&[std::path::PathBuf::from("/tmp/a.txt")], "Mac", 1);
        h.save_to(&path);
        let mut loaded = History::new(5);
        loaded.load_from(&path);
        assert_eq!(
            loaded.entries()[0].kind,
            Kind::File,
            "ファイル種別は保存・復元を通る"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_path_detection_is_line_wise_and_absolute_only() {
        assert!(
            looks_like_file_paths("/Users/me/a b.txt\n/tmp/x"),
            "絶対パスの並びはファイル"
        );
        assert!(
            looks_like_file_paths("C:\\Users\\me\\a.txt"),
            "Windows 絶対パスもファイル"
        );
        assert!(
            !looks_like_file_paths("メモ\n/単独パスだけ混ざったテキスト"),
            "相対行が混ざるとテキスト"
        );
        assert!(!looks_like_file_paths("ただのテキスト"), "通常文はテキスト");
        assert!(!looks_like_file_paths(""), "空はテキスト扱いにしない");
    }

    #[test]
    fn round_trips_through_disk_and_drops_overflow() {
        let dir = std::env::temp_dir().join(format!("tsunagu-history-test-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = History::new(2);
        h.push_text("a", "Mac", 1);
        h.push_text("b", "Mac", 2);
        h.push_text("https://example.com", "Windows", 3);
        h.save_to(&path);
        let mut loaded = History::new(2);
        loaded.load_from(&path);
        assert_eq!(loaded.entries().len(), 2, "上限超過は古い方から落とす");
        assert_eq!(loaded.entries()[0].text, "b");
        assert_eq!(loaded.entries()[1].kind, Kind::Url);
        assert_eq!(loaded.last_id(), 3, "読み込み後も id が重複しない");
        loaded.push_text("c", "Mac", 4);
        assert_eq!(loaded.last_id(), 4, "復元後も次の id が続く");
        loaded.clear();
        loaded.save_to(&path);
        let mut empty = History::new(2);
        empty.load_from(&path);
        assert!(
            empty.entries().is_empty(),
            "クリアは保存され、次回起動でも消えている"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerates_a_broken_file() {
        let dir = std::env::temp_dir().join(format!("tsunagu-history-bad-{}", std::process::id()));
        let path = dir.join("history.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "not json").unwrap();
        let mut h = History::new(3);
        h.load_from(&path);
        assert!(h.entries().is_empty(), "壊れたファイルは空として起動する");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn image_entries_carry_name_and_size_and_merge_duplicates() {
        let mut h = History::new(10);
        let name = image_file_name(fnv1a64(b"pixels"), "bmp");
        let id = h.push_image(&name, 12_345, "Mac", 1_000).unwrap();
        let e = h.get(id).unwrap();
        assert_eq!(e.kind, Kind::Image);
        assert_eq!(parse_image_entry(&e.text), Some((name.clone(), 12_345)));

        let before = h.last_id();
        h.push_image(&name, 12_345, "Windows", 2_000);
        assert_eq!(h.last_id(), before, "同一画像は項目を増やさない");
        assert_eq!(h.get(id).unwrap().device, "Windows");

        let other = image_file_name(fnv1a64(b"different"), "bmp");
        h.push_image(&other, 99, "Mac", 3_000);
        assert_eq!(h.last_id(), before + 1, "別画像は別項目");

        // 形式の壊れた名前は履歴へ入れない(誤復元のもとを塞ぐ)
        assert!(h
            .push_image("スクリーンショット.png", 100, "Mac", 4_000)
            .is_none());
        assert!(h
            .push_image("img-zzzzzzzzzzzzzzzz.bmp", 100, "Mac", 5_000)
            .is_none());

        let l = label(h.get(id).unwrap(), 3_000, 40);
        assert!(
            l.contains("画像・12KB"),
            "ラベルは画像種別と大きさを出す: {l}"
        );
    }

    #[test]
    fn image_kind_survives_disk_round_trip() {
        let dir = std::env::temp_dir().join(format!("tsunagu-history-img-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = History::new(5);
        let name = image_file_name(0xdeadbeef, "dib");
        h.push_image(&name, 4_567, "Windows", 1);
        h.save_to(&path);
        let mut loaded = History::new(5);
        loaded.load_from(&path);
        assert_eq!(
            loaded.entries()[0].kind,
            Kind::Image,
            "画像種別は保存・復元を通る"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn image_entry_parsing_is_strict() {
        assert_eq!(
            parse_image_entry("img-0123456789abcdef.bmp\t2048"),
            Some(("img-0123456789abcdef.bmp".to_string(), 2048))
        );
        assert!(parse_image_entry("img-0123456789abcdef.dib\t2048").is_some());
        // 通常のテキストを画像扱いにしない(タブ無し・名前形式違い・数字以外)
        assert!(parse_image_entry("ただのテキスト").is_none());
        assert!(parse_image_entry("img-0123456789abcdef.bmp").is_none());
        assert!(parse_image_entry("img-0123456789abcdef.bmp\t12KB").is_none());
        assert!(
            parse_image_entry("img-0123456789abc.bmp\t100").is_none(),
            "桁数不足"
        );
        assert!(
            parse_image_entry("img-0123456789abcdef.png\t100").is_none(),
            "拡張子違い"
        );
        assert!(
            parse_image_entry("/Users/me/img-0123456789abcdef.bmp\t100").is_none(),
            "パスは不可"
        );
    }

    #[test]
    fn prunes_image_store_keeping_newest() {
        let dir =
            std::env::temp_dir().join(format!("tsunagu-history-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // mtime の分解能に頼らないよう、書き込み順=新しい順になるよう少し空けて作る
        let mut names = Vec::new();
        for i in 0..5u64 {
            let n = image_file_name(i, "bmp");
            std::fs::write(dir.join(&n), b"x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            names.push(n);
        }
        std::fs::write(dir.join("readme.txt"), b"x"); // 対象外は触らない
        prune_image_store(&dir, 2);
        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(left.contains(&names[4]), "最も新しい画像は残る");
        assert!(left.contains(&names[3]), "新しい方から keep 件残す");
        assert!(!left.contains(&names[0]), "古い画像は削除される");
        assert!(
            left.contains(&"readme.txt".to_string()),
            "画像以外のファイルは触らない"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 保存した履歴と画像実体は所有者以外に読めない権限で書かれる(Mac)。
    /// 旧版は umask 既定の 644 で本文が平文のまま残っていた
    #[cfg(unix)]
    #[test]
    fn history_and_images_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tsunagu-hist-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut h = History::new(3);
        h.push_text("clipboard text", "Mac", 1);
        h.save_to(&dir.join("history.json"));
        write_private(&dir.join("img-0000000000000000.bmp"), b"BMP").unwrap();
        for name in ["history.json", "img-0000000000000000.bmp"] {
            let mode = std::fs::metadata(dir.join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name} は 600 で保存される");
        }
        let dmode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(dmode & 0o777, 0o700, "履歴ディレクトリは 700 になる");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
