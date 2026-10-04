//! 接続中スピーカーミュートの退避記録。Windows 側(knit-win)が接続中に
//! エンドポイントをミュートする際、適用前のユーザー設定をディスクへ記録して
//! から適用する。knit-win が異常終了(強制終了・電源断・パニック)しても、
//! 次回起動時にこの記録が残っていれば残留ミュートを検知して復元できる。
//! 正常切断では復元後に記録を消すため、通常運用ではこのファイルは存在しない。
//! ロジックを純関数として独立させ、mac 上でも単体テストで検証できるようにする
//! (knit-win は mac ではビルドできないため、実機は Windows での確認になる)。

use serde::{Deserialize, Serialize};

/// 記録ファイルの読み込み上限(バイト)。異常に大きいファイルは壊れていると扱う
const MAX_FILE_BYTES: u64 = 8 * 1024;
/// デバイス ID の長さ上限(文字)。WASAPI のエンドポイント ID は通常 100 文字前後
const MAX_DEV_ID_CHARS: usize = 1024;

/// 退避記録の中身
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// ミュート適用前のユーザーのミュート設定(true = 元々ミュートだった)
    pub was_muted: bool,
    /// ミュートを適用したデバイスの ID(復元先の取り違え防止。空なら既定デバイス)
    pub dev_id: String,
    /// 記録時刻(エポックミリ秒。診断用)
    pub ts: u64,
}

/// 記録ファイルのパス(端末固有データ置き場の中。Windows は %LOCALAPPDATA%\Knit)
pub fn path() -> Option<std::path::PathBuf> {
    crate::envutil::data_dir().map(|d| d.join("speaker-mute.json"))
}

/// ミュート適用をディスクへ記録する(以後の復元はこの内容で行う)
pub fn save(was_muted: bool, dev_id: &str) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("データ置き場が取れません"))?;
    let rec = Record {
        was_muted,
        dev_id: dev_id.chars().take(MAX_DEV_ID_CHARS).collect(),
        ts: now_ms(),
    };
    write_record(&path, &rec)
}

/// 残っている記録を読む(無い・読めない・壊れている場合は None)
pub fn load() -> Option<Record> {
    load_from(&path()?)
}

/// 復元後に記録を消す(失敗しても呼び出し側の処理は止めない)
pub fn clear() {
    if let Some(p) = path() {
        if let Err(e) = std::fs::remove_file(&p) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[spk] 退避記録の削除に失敗: {e}");
            }
        }
    }
}

/// 接続中ミュートの適用を維持するか取りやめるかの決定。
/// 退避記録をディスクへ書けないままミュートを適用すると、強制終了時に
/// ミュートが残留し復元手段(退避記録)を失う(スピーカーが恒久ミュート)。
/// そのため書き込み失敗時は適用を取りやめて元の状態へ戻す
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyDecision {
    /// ミュートを維持する(退避記録の書き込みまで済んでいる)
    Keep,
    /// ミュートの適用自体に失敗: ユーザーの設定は変わっていないため何もしない
    NotApplied,
    /// 退避記録を書けない: 強制終了時に復元手段が無くなるため、ミュートを
    /// 元の状態へ戻して適用を取りやめる
    Rollback,
}

/// applied: ミュート適用(SetMute)に成功したか。
/// persisted: 退避記録(spkstate::save)の書き込みに成功したか
pub fn apply_decision(applied: bool, persisted: bool) -> ApplyDecision {
    match (applied, persisted) {
        (false, _) => ApplyDecision::NotApplied,
        (true, true) => ApplyDecision::Keep,
        (true, false) => ApplyDecision::Rollback,
    }
}

/// テスト用: パスを指定して記録を読む。整合性検査もここで行う
fn load_from(path: &std::path::Path) -> Option<Record> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    parse_record(&bytes)
}

/// テスト用: パスを指定して記録を書く(アトミックな置き換え)
fn write_record(path: &std::path::Path, rec: &Record) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("保存先がありません"))?;
    std::fs::create_dir_all(parent)?;
    crate::history::restrict_dir(parent);
    let body = serde_json::to_vec(rec)?;
    // 一時ファイルへ書いてから置き換える(書き込み途中の電源断で壊れた状態を
    // 読まないため。preferences.json と同じ手法)
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    {
        use std::io::Write;
        #[cfg(unix)]
        let mut f = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?
        };
        #[cfg(not(unix))]
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// バイト列を記録へパースする(上限・形式の検査つき。壊れていたら None)
fn parse_record(bytes: &[u8]) -> Option<Record> {
    let rec: Record = serde_json::from_slice(bytes).ok()?;
    if rec.dev_id.chars().count() > MAX_DEV_ID_CHARS {
        return None;
    }
    Some(rec)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "knit-spkstate-{}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn record_round_trips_through_disk() {
        let p = temp_path("roundtrip.json");
        write_record(&p, &Record { was_muted: false, dev_id: "{0.0.0.00000000}.{abc}".into(), ts: 5 })
            .unwrap();
        assert_eq!(
            load_from(&p),
            Some(Record { was_muted: false, dev_id: "{0.0.0.00000000}.{abc}".into(), ts: 5 })
        );
        // true(元々ミュート)も往復する: これを忘れると復元で勝手に鳴り出す
        write_record(&p, &Record { was_muted: true, dev_id: String::new(), ts: 6 }).unwrap();
        assert_eq!(
            load_from(&p),
            Some(Record { was_muted: true, dev_id: String::new(), ts: 6 })
        );
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn corrupt_or_oversized_input_is_rejected() {
        assert!(parse_record(b"").is_none());
        assert!(parse_record(b"not json").is_none());
        // JSON だがフィールドが足りない
        assert!(parse_record(br#"{"was_muted":true}"#).is_none());
        // 過大なデバイス ID は壊れていると扱う(読み込んで WASAPI へ渡さない)
        let long = "x".repeat(MAX_DEV_ID_CHARS + 1);
        let body = format!(r#"{{"was_muted":false,"dev_id":"{long}","ts":1}}"#);
        assert!(parse_record(body.as_bytes()).is_none());
        // ファイルサイズ上限: 上限を超える内容は読まない
        let p = temp_path("toobig.json");
        std::fs::write(&p, vec![b' '; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert!(load_from(&p).is_none());
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn missing_file_is_none_and_clear_is_idempotent() {
        let p = temp_path("missing.json");
        assert!(load_from(&p).is_none());
        // clear は存在しなくてもエラーにしない(正常切断の後始末で毎回呼ぶ)
        let _ = std::fs::remove_file(&p);
    }

    /// 退避記録を書けないままミュートを適用すると、強制終了時にミュートが
    /// 残留し復元手段を失う。書き込み失敗時は適用を取りやめる決定になる
    #[test]
    fn apply_is_rolled_back_when_persist_fails() {
        use ApplyDecision::*;
        // 適用も退避も成功: ミュートを維持する
        assert_eq!(apply_decision(true, true), Keep);
        // 適用は成功したが退避記録が書けない: 元の状態へ戻す(恒久ミュート防止)
        assert_eq!(apply_decision(true, false), Rollback);
        // 適用自体が失敗: ユーザーの設定は変わっていないため何もしない
        assert_eq!(apply_decision(false, true), NotApplied);
        assert_eq!(apply_decision(false, false), NotApplied);
    }
}
