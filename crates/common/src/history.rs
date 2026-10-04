//! 全デバイスのクリップボードを統合して見る履歴(Universal Clipboard History)。
//! 各端末が「相手へ同期した・相手から受け取った」テキストとファイル参照を蓄積し、
//! メニューから選ぶとその端末のクリップボードへ復元できる。テキスト/URL に加え、
//! ファイルは「パスの改行区切り」で載せる(本体は送受信で各端末へ届いているため、
//! パスさえ残せば Finder/Explorer の ⌘V 相当で復元できる)。
//! 画像は履歴 JSON へは載せず、本体を端末ごとの images/ ディレクトリへ
//! ハッシュ名で保存し、履歴には「ファイル名\tバイト数」だけ残す
//! (Mac は BMP、Windows は DIB の生バイトで保存するため復元時に変換が要らない)。
//! 履歴は平文の JSON に保存されるため、機密らしき桁列は push_text の段階で
//! 加工する: 8 桁以上の連続数字は桁部分を ● へマスクして残し(長文が桁列ひとつで
//! 丸ごと消えると履歴として使えなくなるため)、Luhn チェックを通るカード番号を
//! 含む本文は保存から除外する(桁数・区切り位置ごと消す)。
//! 相手への送信可否は smartguard/秘匿印が担っており、この判定は保存のみに働く。

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

/// 保存用の借用ビュー。全件の clone を作らずに serde で書き出すためのもの
/// (50 件×1MB のクローンがコピーのたびに発生するのを避ける)
#[derive(Serialize)]
struct PersistedRef<'a> {
    entries: &'a [Entry],
}

/// 上限つき履歴。新しいものを末尾に置く
pub struct History {
    entries: Vec<Entry>,
    next_id: u64,
    cap: usize,
}

/// 載せる本文の上限(クリップボード同期と同じ 1MB。バイト数で見るため
/// 日本語の長文は約35万文字で引っかかる)
pub const MAX_CHARS: usize = 1024 * 1024;

/// 履歴へ残す本文の上限(同期の 1MB 上限とは別)。コピーのたびに全 50 件を
/// 丸ごと書き出す保存コストを 50 件×8KB≈400KB で頭打ちにする。超えた分は
/// 先頭から UTF-8 の文字境界で切って「…(履歴用に省略)」を添える
/// (共有・送信には影響せず、履歴からの復元だけ省略版になる)
pub const HISTORY_TEXT_BYTES: usize = 8 * 1024;

/// クリップボード本文が上限(1MB)を超えたときの通知文。実サイズを文字数と
/// バイト数の両方で伝える(上限がバイト数ベースのため、日本語の長文は約35万文字
/// で引っかかる。「まだ 1MB 未満のはず」との誤解を防ぐための案内も添える)
pub fn too_large_clip_message(text: &str) -> String {
    format!(
        "クリップボードが大きすぎるため同期しません({}文字・{}KB。上限は1MB(日本語の長文では約35万文字)です。履歴にも載りません)",
        text.chars().count(),
        text.len() / 1024
    )
}

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
    /// 空・上限超過・Luhn 成立のカード番号を含む本文は載せない。
    /// 8 桁以上の連続数字を含む本文は、その部分を ● へマスクして載せる。
    /// 戻り値は採用した項目の id(重複更新を含む)
    pub fn push_text(&mut self, text: &str, device: &str, now_ms: u64) -> Option<u64> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        // 履歴は平文の JSON に残るため、カード番号(Luhn 成立)はマスクでも残さない。
        // 除外されるのは履歴だけで、送信・貼り付け・復元には影響しない
        if contains_luhn_card(trimmed) {
            return None;
        }
        // 8 桁以上の連続数字(認証コード・日付(YYYYMMDD)・ISBN 等)は桁部分を
        // ● へマスクして保存する。履歴からの復元・再コピーはマスク後の文字列に
        // なる(元の数字は戻さない)
        let text = if has_long_digit_run(trimmed) {
            mask_digit_runs(trimmed)
        } else {
            trimmed.to_string()
        };
        // 履歴へ残す本文は HISTORY_TEXT_BYTES(8KB)までの先頭部分。長文コピーの
        // 履歵載せを断る代わりに省略版で残す(1MB 上限で履歴からこぼれていた
        // ローカルの長文コピーも履歴から辿れるようにする)
        let text = truncate_for_history(&text, HISTORY_TEXT_BYTES);
        // 本文全体が桁列(マスクと空白以外に残るものが無い)は、マスクで中身が
        // 残らないため載せない(ワンタイムコード単体のコピー等)
        if !text.chars().any(|c| c != '●' && !c.is_whitespace()) {
            return None;
        }
        if text != trimmed {
            eprintln!("[history] 8桁以上の連続する数字を●へマスクして履歴へ保存します");
        }
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
    /// パスとして扱う)。改行を含むパス(Mac では合法なファイル名)は行割れして
    /// 復元不能になるため、履歴への載せから除外する(転送・通知には影響しない)。
    /// ファイル同期は機密判定の対象外のためここでも検査しない
    pub fn push_files(
        &mut self,
        paths: &[std::path::PathBuf],
        device: &str,
        now_ms: u64,
    ) -> Option<u64> {
        if paths.is_empty() {
            return None;
        }
        let usable: Vec<std::borrow::Cow<'_, str>> = paths
            .iter()
            .map(|p| p.as_os_str().to_string_lossy())
            .filter(|s| !s.contains('\n') && !s.contains('\r'))
            .collect();
        if usable.is_empty() {
            return None;
        }
        let text = usable
            .into_iter()
            .map(|s| s.into_owned())
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
        // entries は借り参照のまま書き出す(clone は保存の都度全件複製になる)
        let data = PersistedRef {
            entries: &self.entries,
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
    /// ファイルが無い場合は空のまま起動する。壊れている場合は
    /// (1) 退避して(.corrupt)から (2) 末尾欠けの回復的パースで完全な要素だけ
    /// 復元する(全損の場合は空)。退避を先にするのは、最初の保存で上書きされて
    /// 救出材料ごと消えるのを防ぐため
    pub fn load_from(&mut self, path: &std::path::Path) {
        let Ok(s) = std::fs::read_to_string(path) else {
            return;
        };
        // 旧版が umask 既定(644)で作ったファイルを読めたら 600 へ是正する
        restrict(path);
        let p = match serde_json::from_str::<Persisted>(&s) {
            Ok(p) => p,
            Err(e) => {
                let recovered = recover_partial(&s);
                let backup = crate::persist::quarantine(path);
                eprintln!(
                    "[history] 履歴ファイルが壊れています({e})。{}{}件を復元しました",
                    backup
                        .as_ref()
                        .map(|p| format!("{} へ退避しました。", p.display()))
                        .unwrap_or_default(),
                    recovered.len()
                );
                Persisted { entries: recovered }
            }
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

/// 末尾欠けした history.json(保存中の電源断などで途中までしか書かれていない
/// テキスト)から、完全な形で残っている要素だけを復元する純関数。
/// 「{"entries":[e1,e2,…」の形を前提に、要素(オブジェクト)の閉じ括号の直後
/// 位置を文字列リテラルを飛ばしながら列挙し、最後の完全な要素まで切り詰めて
/// `]}` を補ってパースする。パースは serde が検証するため、本文中の `}` 等の
/// 誤認は復元失敗(空)として現れるだけで誤った復元にはならない
fn recover_partial(text: &str) -> Vec<Entry> {
    // 保存形式は常に {"entries":[…]}(to_string の compact 形式)。先頭のキーが
    // entries でなければ復元の見当が付かないため諦める
    let t = text.trim_start();
    if !t.starts_with('{') {
        return Vec::new();
    }
    let after_obj = t[1..].trim_start();
    let Some(rest) = after_obj.strip_prefix("\"entries\"") else {
        return Vec::new();
    };
    let Some(colon) = rest.find(':') else {
        return Vec::new();
    };
    let after_colon = rest[colon + 1..].trim_start();
    let Some(open) = after_colon.strip_prefix('[') else {
        return Vec::new();
    };
    let array_start = open.as_ptr() as usize - text.as_ptr() as usize;
    // 要素ごとの終了位置(閉じ '}' の直後)を列挙する
    let ends = element_ends(text, array_start);
    // 後ろ(新しい項目)から順に「切り詰めて ]} を補う」パースを試み、最初に
    // 通ったもので復元する。末尾欠けの直前まで完全な要素ほど新しい履歴のため
    for &end in ends.iter().rev() {
        let mut candidate = String::with_capacity(end + 2);
        candidate.push_str(&text[..end]);
        candidate.push_str("]}");
        if let Ok(p) = serde_json::from_str::<Persisted>(&candidate) {
            return p.entries;
        }
    }
    Vec::new()
}

/// text[array_start..] を JSON として走査し、配列の各要素(トップレベルの
/// オブジェクト)が閉じた位置(インデックス)を列挙する。文字列リテラル内の
/// 記号は無視する(履歴本文は任意のテキストを含むため)。バイト単位の走査でも
/// マルチバイト文字のバイトは ASCII と衝突しないため安全
fn element_ends(text: &str, array_start: usize) -> Vec<usize> {
    let b = text.as_bytes();
    let mut ends = Vec::new();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut i = array_start;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 1; // エスケープ: 次の 1 文字を読み飛ばす
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' | b'[' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        ends.push(i + 1);
                    }
                }
                b']' if depth == 0 => break, // 配列の閉じ: これ以降は外側
                b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        i += 1;
    }
    ends
}

/// 履歴本文と画像実体を、所有者だけが読める権限で書き込む。
/// ~/.config 下は既定 umask(022)だと他ユーザーに読める(644)ため、
/// トークン設定(~/.config/knit/env=600)と同じ土俵へ揃える。
/// Windows はプロファイル直下の既定 ACL に任せる(何もしない)
pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    // 作成時点から所有者のみにする(既定 644 で作ってから chmod する間に、
    // 共有 Mac の別ユーザーへ本文が読まれる窓を残さない)
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
            .write_all(bytes)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)?;
    }
    let ok = restrict(path);
    if !ok {
        eprintln!("[history] 権限の限定(0600)に失敗しました: {}", path.display());
    }
    Ok(())
}

/// 既存ファイルの権限を所有者のみへ是正する(起動時の読み込み後にも呼ぶ)
pub fn restrict(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        true
    }
}

/// write_private は truncate+write_all のため、失敗時には部分書き込みの実体が
/// 残りうる。画像実体の保存のように呼び出し側が「実体があれば書き直さない」
/// 判定(exists チェック)を使うと、その残骸が恒久的に壊れた実体として固定化
/// される。失敗時は実体を消して「無い」状態へ戻す
pub fn write_private_or_remove(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    write_or_remove_with(path, bytes, write_private)
}

fn write_or_remove_with(
    path: &std::path::Path,
    bytes: &[u8],
    write: impl Fn(&std::path::Path, &[u8]) -> std::io::Result<()>,
) -> std::io::Result<()> {
    match write(path, bytes) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(path);
            Err(e)
        }
    }
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

/// 履歴用に本文を先頭 max_bytes バイトまでに切る。UTF-8 のマルチバイト文字の
/// 途中で切らないよう、切る位置を文字境界まで戻す。切ったときだけ末尾に断りを添える
/// (断り書きぶん上限を数バイト超えるが、断り自体を切ると省略が分からなくなる)
fn truncate_for_history(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…(履歴用に省略)", &text[..cut])
}

/// 本文全体が 1 つの http(s) URL とみなせるか(表示種別としてのみ使う)
fn is_single_url(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty()
        && !t.contains([' ', '\n', '\r', '\t'])
        && (t.starts_with("http://") || t.starts_with("https://"))
}

/// 「機密らしき桁列」の連続桁数の閾値。ワンタイムコード(6〜8桁)・口座番号・
/// マイナンバー(12桁)などを 8 桁で拾う。注文番号・日付(YYYYMMDD)・ISBN も
/// 含まれるが、履歴では桁部分がマスクされるだけで送信は妨げないため、安全側に倒す
const LONG_DIGIT_RUN: usize = 8;

/// 履歴保存前のマスク(平文で残す履歴専用。送信の可否には関与しない)。
/// 8 桁以上の連続する数字を桁数分の ● へ置き換える。電話番号の区切り
/// (03-1234-5678 等)など 8 桁未満のまとまりはそのまま残る
fn mask_digit_runs(text: &str) -> String {
    /// それまでに溜めた数字のまとまりを確定させる(長ければマスク)
    fn flush(out: &mut String, run: &mut String) {
        if run.len() >= LONG_DIGIT_RUN {
            for _ in 0..run.len() {
                out.push('●');
            }
        } else {
            out.push_str(run);
        }
        run.clear();
    }
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            flush(&mut out, &mut run);
            out.push(c);
        }
    }
    flush(&mut out, &mut run);
    out
}

fn has_long_digit_run(text: &str) -> bool {
    let mut run = 0usize;
    for &b in text.as_bytes() {
        if b.is_ascii_digit() {
            run += 1;
            if run >= LONG_DIGIT_RUN {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// 桁区切り(空白・ハイフン1文字)を挟んだ数字のまとまりのうち、クレジット
/// カードの形式(13〜19 桁・Luhn 成立)のものがあれば true。連続桁のルールを
/// 拾えない "4111 1111 1111 1111" 形式をカバーする
fn contains_luhn_card(text: &str) -> bool {
    let mut digits: Vec<u8> = Vec::new();
    let mut after_separator = false;
    // まとまりの終わりで判定して空に戻す。以降の桁は新しいまとまりとして扱う
    // (長い番号の後ろに続いたカード番号も拾えるように、19 桁を超えたら切る)
    fn finish(digits: &mut Vec<u8>, after_separator: &mut bool) -> bool {
        let hit = is_card_number(digits);
        digits.clear();
        *after_separator = false;
        hit
    }
    for &b in text.as_bytes() {
        if b.is_ascii_digit() {
            if digits.len() == 19 && finish(&mut digits, &mut after_separator) {
                return true;
            }
            digits.push(b - b'0');
            after_separator = false;
        } else if !digits.is_empty() && (b == b' ' || b == b'-') && !after_separator {
            // 桁と桁の間の区切り1文字。2文字続くか他の文字でまとまりは終わり
            after_separator = true;
        } else if finish(&mut digits, &mut after_separator) {
            return true;
        }
    }
    is_card_number(&digits)
}

fn is_card_number(digits: &[u8]) -> bool {
    (13..=19).contains(&digits.len()) && luhn_ok(digits)
}

fn luhn_ok(digits: &[u8]) -> bool {
    let mut sum = 0u32;
    for (i, d) in digits.iter().rev().enumerate() {
        let mut v = u32::from(*d);
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum % 10 == 0
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

/// 既存の画像実体の mtime を現在へ更新する(内容は書き換えない)。
/// 同じ画像の再コピーでは実体が既に存在するため保存を省くが、prune_image_store
/// は mtime 順に残すため、更新しないと「履歴の最新」として参照中の実体が
/// 古い方から削除される。64MiB を超える実体の書き直しを避けるため、時刻の
/// 設定だけを行う(std::fs::File::set_times。設定しなかった時刻は変わらない)
pub fn touch_mtime(path: &std::path::Path) {
    let now = std::time::SystemTime::now();
    let times = std::fs::FileTimes::new().set_modified(now);
    match std::fs::File::options().write(true).open(path) {
        Ok(f) => {
            if let Err(e) = f.set_times(times) {
                eprintln!("[history] 画像の mtime 更新に失敗: {e}({})", path.display());
            }
        }
        Err(e) => {
            eprintln!("[history] 画像の mtime 更新のため開けません: {e}({})", path.display());
        }
    }
}

/// 画像保存ディレクトリの総バイト上限。1 枠が bulk.rs の MAX_IMAGE(64MiB)
/// まで入り得るため、件数上限(60 件)だけだと最悪 3.75GiB まで膨らむ。
/// 件数とは独立にバイトでも頭打ちにする(新しいものから残すため、溢れた分は
/// 古い画像から消える)
pub const MAX_IMAGE_STORE_BYTES: u64 = 512 * 1024 * 1024;

/// 画像保存ディレクトリの刈り込み。新しい mtime 順に「keep 件」かつ「総量
/// max_bytes バイト」に収まる分を残して古い実体から削除する(履歴上限 50 に
/// 対し少し余裕を持たせた件数を呼び出し側が渡す)。履歴から追い出された画像の
/// 実体が居座り続けるのを防ぐ。最新の 1 つはサイズに関わらず必ず残す
/// (今保存した画像をこの刈り込み自身が消さないようにするため)
pub fn prune_image_store(dir: &std::path::Path, keep: usize, max_bytes: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<(std::path::PathBuf, std::time::SystemTime, u64)> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            (n.ends_with(".bmp") || n.ends_with(".dib")) && is_image_store_name(&n)
        })
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let mtime = meta.modified().ok()?;
            Some((e.path(), mtime, meta.len()))
        })
        .collect();
    items.sort_by_key(|a| std::cmp::Reverse(a.1)); // 新しい順
    let mut total = 0u64;
    for (i, (path, _, size)) in items.iter().enumerate() {
        if i > 0 && (i >= keep || total.saturating_add(*size) > max_bytes) {
            let _ = std::fs::remove_file(path);
        } else {
            total = total.saturating_add(*size);
        }
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
    fn digit_runs_are_masked_and_card_numbers_are_dropped() {
        let mut h = History::new(10);
        // 8 桁以上の連続数字(認証コード・口座番号・日付(YYYYMMDD)等)は桁部分を
        // ● へマスクして履歴に残る(長文が桁列ひとつで丸ごと消えないようにするため)
        let id = h.push_text("認証コードは 12345678 です", "Mac", 1).unwrap();
        assert_eq!(h.get(id).unwrap().text, "認証コードは ●●●●●●●● です");
        let id = h.push_text("予定日 20261004", "Mac", 5).unwrap();
        assert_eq!(h.get(id).unwrap().text, "予定日 ●●●●●●●●");
        // 本文全体が桁列(ワンタイムコード単体等)はマスクで中身が残らないため載せない
        assert!(h.push_text("0123456789012345", "Windows", 2).is_none());
        assert!(h.push_text("12345678 90123456", "Windows", 3).is_none());
        // 桁区切りのクレジットカード番号(Luhn が成立する 13〜19 桁)を含む本文は
        // マスクでも残さない(桁数・区切り位置から探索空間を絞れるため)
        assert!(h.push_text("カード 4111 1111 1111 1111", "Mac", 3).is_none());
        assert!(h.push_text("4111-1111-1111-1111", "Mac", 4).is_none());
        // 通常の文・電話番号(区切りの桁が短い)・西暦はそのまま残る
        assert!(h.push_text("電話 03-1234-5678 まで", "Mac", 6).is_some());
        assert!(h.push_text("2026年10月4日の予定", "Windows", 7).is_some());
        assert!(h.push_text("https://example.com/a?b=1", "Mac", 8).is_some());
        // 16 桁でも Luhn に落ちる並びは「カード番号らしくない」ため残る
        assert!(h.push_text("4111 1111 1111 1112", "Mac", 9).is_some());
        // 区切りの長い番号の続きにカード番号が来ても、まとまりの切り替えで拾う
        assert!(
            h.push_text(
                "伝票 012 3456 7890 1234 5678 9012 3456、カード 4242 4242 4242 4242",
                "Mac",
                10
            )
            .is_none()
        );
        // 記録されたのはマスク2件+通常4件(電話・西暦・URL・Luhn落ち16桁)
        assert_eq!(h.entries().len(), 6);
    }

    /// 想定ユーザー(日本語の長文をコピーする人)の主要ケース: 日付・電話番号・
    /// ISBN を含む長文が、桁部分だけマスクされて履歴に残る
    #[test]
    fn long_text_with_dates_phone_and_isbn_is_kept_masked() {
        let mut h = History::new(10);
        let text = "締切は 20261031、ISBN 9784865940658 で申請してください。問い合わせは 09012345678 まで";
        let id = h.push_text(text, "Mac", 1).unwrap();
        let e = h.get(id).unwrap();
        assert_eq!(
            e.text,
            "締切は ●●●●●●●●、ISBN ●●●●●●●●●●●●● で申請してください。問い合わせは ●●●●●●●●●●● まで"
        );
        assert!(!e.text.contains("20261031"), "元の日付は残さない");
        assert!(!e.text.contains("9784865940658"), "元の ISBN は残さない");
        assert!(!e.text.contains("09012345678"), "元の電話番号は残さない");
        // マスク済みの本文をそのまま push し直しても idempotent(● は数字ではない)
        let masked = e.text.clone();
        let again = h.push_text(&masked, "Windows", 2).unwrap();
        assert_eq!(h.get(again).unwrap().text, masked);
    }

    #[test]
    fn too_large_message_carries_actual_size_and_limit() {
        // 日本語 40 万文字 = 約 1.14MB(UTF-8 で 3 バイト/文字)で上限超過
        let text = "あ".repeat(400_000);
        let m = too_large_clip_message(&text);
        assert!(m.contains("400000文字"), "実際の文字数を含む: {m}");
        assert!(m.contains("1171KB"), "実際のサイズを含む: {m}");
        assert!(m.contains("1MB"), "上限を含む: {m}");
        assert!(m.contains("35万"), "日本語長文の目安を含む: {m}");
    }

    /// 履歴用の 8KB truncate: バイト境界で切るとき UTF-8 の文字境界を守る。
    /// 「あ」(3 バイト)×上限+1 バイトの位置に置くと、切る位置が 1 バイト
    /// 手前に戻って文字の途中で切らない
    #[test]
    fn history_truncation_respects_utf8_char_boundaries() {
        // 3 バイト文字の並びで境界が割り切れないケース: cut=8 は「あ」の途中
        // (6 バイト=2 文字目の後。8 バイト地点は 3 文字目の途中)のため
        // 6 バイト(2 文字)へ戻る
        let text = "あいうえお";
        let cut = truncate_for_history(text, 8);
        assert_eq!(cut, "あい…(履歴用に省略)");

        // 境界がちょうど文字区切りに当たるケース(6 バイト=2 文字)
        let cut = truncate_for_history(text, 6);
        assert_eq!(cut, "あい…(履歴用に省略)");

        // 上限内はそのまま(断りも付けない)
        let cut = truncate_for_history(text, 15);
        assert_eq!(cut, "あいうえお");

        // 絵文字(4 バイト)の並びでも同様に文字境界を守る
        let emoji = "😀😃😄";
        let cut = truncate_for_history(emoji, 6);
        assert_eq!(cut, "😀…(履歴用に省略)");
    }

    /// push_text 経由の省略: 8KB 超の長文が断り付きの省略版で履歴に載り、
    /// 同じ長文の再コピーは省略版同士でマージされる
    #[test]
    fn push_text_truncates_long_bodies_for_history() {
        let mut h = History::new(10);
        let long = format!("{}{}", "本文".repeat(4_000), "末尾に一意の数字 12345678");
        let id = h.push_text(&long, "Mac", 1).unwrap();
        let saved = h.get(id).unwrap().text.clone();
        assert!(
            saved.len() <= HISTORY_TEXT_BYTES + 64,
            "省略後は 8KB+断り書き程度: {}",
            saved.len()
        );
        assert!(saved.ends_with("…(履歴用に省略)"));
        assert!(saved.starts_with("本文本文"), "先頭部分はそのまま残る");
        assert!(!saved.contains("末尾に一意"), "上限を超えた末尾は載らない");
        // 同じ長文を再度コピーしても省略版が一致するため重複扱い(marge)になる
        let again = h.push_text(&long, "Mac", 2).unwrap();
        assert_eq!(again, id, "省略版が同じなら同じ項目にマージ");
        assert_eq!(h.entries().len(), 1);
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

    /// 改行入りファイル名(Mac では合法)は履歴本文の行区切りを壊すため、
    /// 履歴への載せから除外する(転送・通知には影響しない。復元不能な項目を
    /// 作らないための防御)
    #[test]
    fn file_entries_drop_paths_containing_newlines() {
        let mut h = History::new(10);
        let paths = vec![
            std::path::PathBuf::from("/Users/me/Desktop/資料.pdf"),
            std::path::PathBuf::from("/Users/me/Desktop/改行\n入り.txt"),
            std::path::PathBuf::from("/Users/me/Desktop/復帰\r入り.txt"),
        ];
        let id = h.push_files(&paths, "Mac", 1_000).unwrap();
        let saved = h.get(id).unwrap();
        assert_eq!(saved.text, "/Users/me/Desktop/資料.pdf", "改行・復帰を含むパスは載せない");
        // 全て改行入りなら履歴項目自体を作らない
        assert!(
            h.push_files(&[std::path::PathBuf::from("/tmp/a\nb.txt")], "Mac", 2_000).is_none(),
            "載せられるパスが無ければ None"
        );
    }

    #[test]
    fn file_kind_survives_disk_round_trip() {
        let dir = std::env::temp_dir().join(format!("knit-history-file-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("knit-history-test-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("knit-history-bad-{}", std::process::id()));
        let path = dir.join("history.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "not json").unwrap();
        let mut h = History::new(3);
        h.load_from(&path);
        assert!(h.entries().is_empty(), "壊れたファイルは空として起動する");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 保存中の電源断などで末尾が欠けた history.json: 完全な要素だけ復元し、
    /// 壊れた実ファイルは上書きされる前に .corrupt へ退避する
    #[test]
    fn load_recovers_partial_entries_and_quarantines() {
        let dir = std::env::temp_dir().join(format!("knit-history-part-{}", std::process::id()));
        let path = dir.join("history.json");
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = History::new(10);
        h.push_text("1st", "Mac", 1);
        h.push_text("2nd", "Mac", 2);
        h.push_text("3rd", "Mac", 3);
        h.save_to(&path);
        let mut full = std::fs::read_to_string(&path).unwrap();
        // 末尾欠け: 最後の要素の途中で切れた状態を作る
        let cut = full.len() - 12;
        full.truncate(cut);
        std::fs::write(&path, &full).unwrap();
        let mut loaded = History::new(10);
        loaded.load_from(&path);
        assert_eq!(loaded.entries().len(), 2, "完全な 2 件だけ復元される");
        assert_eq!(loaded.entries()[0].text, "1st");
        assert_eq!(loaded.entries()[1].text, "2nd");
        // 退避: 元の位置からは消え、.corrupt に欠けた実ファイルが残る
        assert!(!path.exists(), "壊れた実ファイルは退避されて元の位置に無い");
        let backup = dir.join("history.json.corrupt");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            full,
            "退避先に欠けた実ファイルが残る"
        );
        // 復元後も id が継続する(復元した最後の id の次から採番)
        assert_eq!(loaded.last_id(), 2);
        loaded.push_text("4th", "Mac", 4);
        assert_eq!(loaded.last_id(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 完全に不正な履歴ファイル: 復復 0 件で空起動し、退避だけ残る
    #[test]
    fn completely_broken_history_is_quarantined_and_starts_empty() {
        let dir = std::env::temp_dir().join(format!("knit-history-junk-{}", std::process::id()));
        let path = dir.join("history.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "<<<not json at all>>>").unwrap();
        let mut h = History::new(3);
        h.load_from(&path);
        assert!(h.entries().is_empty());
        assert!(dir.join("history.json.corrupt").exists(), "救出材料として退避が残る");
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// recover_partial の純関数テスト: 本文に '}'・']'・エスケープを含む要素が
    /// あっても要素境界を正しく数え、文字列内の記号を要素の閉じと誤認しない
    #[test]
    fn recover_partial_handles_braces_and_escapes_inside_text() {
        let mut h = History::new(10);
        h.push_text("a\"}b", "Mac", 1);
        h.push_text("]} \\\" x", "Mac", 2);
        h.push_text("last", "Mac", 3);
        let full = serde_json::to_string(&PersistedRef { entries: h.entries() }).unwrap();
        // 末尾欠け(最後の要素の途中): 2 件復元
        let mut cut = full.clone();
        cut.truncate(full.len() - 10);
        let got = recover_partial(&cut);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].text, "a\"}b", "本文の '}}' で要素が切れない");
        assert_eq!(got[1].text, "]} \\\" x", "本文の ']'・エスケープも同様");
        // 配列の途中で丸ごと切れた場合: そこまでの完全な要素だけ
        let head_end = full.find("{\"id\":3").unwrap();
        let got = recover_partial(&full[..head_end + 5]);
        assert_eq!(got.len(), 2);
        // 冒頭から壊れている場合は空(誤った復元をしない)
        assert!(recover_partial("not json").is_empty());
        assert!(recover_partial("{\"other\":[1,2").is_empty());
        // 完全なファイルもそのまま通る(末尾の ]} が二重にならない)
        assert_eq!(recover_partial(&full).len(), 3);
    }

    /// 同じ画像の再コピーで実体の mtime を更新すると、刈り込みが「履歴の最新」
    /// の実体を新しい側として残す(mtime 更新が無いと再コピー済みの実体が
    /// 古い方から削除され、履歴から復元できなくなる)
    #[test]
    fn touching_an_existing_image_updates_its_mtime_and_protects_it_from_prune() {
        let dir = std::env::temp_dir().join(format!("knit-history-touch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let older = image_file_name(1, "bmp");
        let newer = image_file_name(2, "bmp");
        std::fs::write(dir.join(&older), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join(&newer), b"new").unwrap();
        // 古い方(=履歴で使い回している実体)を再コピー相当で touch する
        std::thread::sleep(std::time::Duration::from_millis(30));
        touch_mtime(&dir.join(&older));
        // keep=1: touch した方が新しいので残り、touch しなかった方が消える
        prune_image_store(&dir, 1, MAX_IMAGE_STORE_BYTES);
        assert!(
            std::fs::metadata(dir.join(&older)).is_ok(),
            "再コピー(touch)した実体は残る"
        );
        assert!(
            std::fs::metadata(dir.join(&newer)).is_err(),
            "最後に使われていない実体の方が削除される"
        );
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
        let dir = std::env::temp_dir().join(format!("knit-history-img-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("knit-history-prune-{}", std::process::id()));
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
        let _ = std::fs::write(dir.join("readme.txt"), b"x"); // 対象外は触らない
        prune_image_store(&dir, 2, MAX_IMAGE_STORE_BYTES);
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

    /// バイト総量上限: 件数の余裕があっても、新しいものからの累積が max_bytes を
    /// 超えた分は古い実体から削除される(64MiB 枠×60 件の無限膨張を防ぐ仕組み)
    #[test]
    fn prunes_image_store_by_total_bytes() {
        let dir = std::env::temp_dir().join(format!("knit-history-prune-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut names = Vec::new();
        for i in 0..4u64 {
            let n = image_file_name(i, "bmp");
            std::fs::write(dir.join(&n), vec![b'x'; 100]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            names.push(n);
        }
        // keep=10(件数では余裕)・max=250 バイト: 新しい 2 つ(200B)までは入り、
        // 3 つ目で 300B>250B のため古い 2 つが削除される
        prune_image_store(&dir, 10, 250);
        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(left.contains(&names[3]), "最も新しい画像は残る");
        assert!(left.contains(&names[2]), "累積 250B 内の新しい方から残す");
        assert!(!left.contains(&names[1]), "総量超過の古い画像は削除");
        assert!(!left.contains(&names[0]), "総量超過の最古画像は削除");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 件数上限とバイト上限の併用: keep(件数)に余裕があっても総量側が触れた
    /// 分は削除され、両方を満たす新しいものだけが残る
    #[test]
    fn prunes_image_store_by_count_and_bytes_together() {
        let dir = std::env::temp_dir().join(format!("knit-history-prune-c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut names = Vec::new();
        for i in 0..5u64 {
            let n = image_file_name(i, "bmp");
            std::fs::write(dir.join(&n), vec![b'x'; 60]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            names.push(n);
        }
        // keep=4(件数では余裕)・max=150: 新い 2 つ(120B)までは入り、3 つ目で
        // 180B>150B のため 3〜5 番目が削除される(バイト側が効る併用)
        prune_image_store(&dir, 4, 150);
        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(left.contains(&names[4]) && left.contains(&names[3]));
        for old in &names[0..3] {
            assert!(!left.contains(old), "件数の余裕があっても総量超過分は削除");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 最新の 1 つは必ず残す: 1 枚だけでも max_bytes を超える巨大画像を保存した
    /// 直後の初期込みで、自分自身を消して履歴と実体が矛盾しないようにする
    #[test]
    fn prune_keeps_the_newest_even_if_it_exceeds_the_byte_cap() {
        let dir = std::env::temp_dir().join(format!("knit-history-prune-d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = image_file_name(1, "bmp");
        std::fs::write(dir.join(&old), b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newest = image_file_name(2, "bmp");
        std::fs::write(dir.join(&newest), vec![b'x'; 1_000]).unwrap();
        prune_image_store(&dir, 60, 100);
        assert!(
            std::fs::metadata(dir.join(&newest)).is_ok(),
            "上限を1枚で超えても最新は残す"
        );
        assert!(
            std::fs::metadata(dir.join(&old)).is_err(),
            "古い分は総量に収まるよう削除"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 保存した履歴と画像実体は所有者以外に読めない権限で書かれる(Mac)。
    /// 旧版は umask 既定の 644 で本文が平文のまま残っていた
    #[cfg(unix)]
    #[test]
    fn history_and_images_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("knit-hist-perm-{}", std::process::id()));
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

    /// 画像実体の保存に失敗した時、部分書き込みの残骸を消して「実体が無い」
    /// 状態へ戻す。exists() チェックで再保存を省く呼び出し側では、残骸が残ると
    /// 恒久的に壊れた実体として固定化されるため
    #[test]
    fn write_private_or_remove_leaves_no_partial_file_on_failure() {
        let dir = std::env::temp_dir().join(format!("knit-hist-rem-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("img.bin");
        // 成功時: 内容がそのまま書かれる
        write_private_or_remove(&path, b"ok").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"ok");
        // write_all が途中で失敗した状況の再現(truncate と部分書き込みが済んだ
        // 状態で Err を返す)。失敗時は実体が消える
        let broken = |p: &std::path::Path, _: &[u8]| {
            std::fs::write(p, b"partial").unwrap();
            Err(std::io::Error::other("disk full"))
        };
        assert!(write_or_remove_with(&path, b"bytes", broken).is_err());
        assert!(!path.exists(), "失敗時は部分書き込みの実体を残さない");
        // 消した後は再保存できる(次回の exists チェックが正しく働く)
        write_private_or_remove(&path, b"retry").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"retry");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
