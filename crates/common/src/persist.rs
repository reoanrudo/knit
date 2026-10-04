//! 状態ファイル(peer-sides.json・device-id.txt 等)の保存と退避の共通ヘルパ。
//! 保存は「一時ファイルへ書いて sync、rename で置き換える」アトミック方式
//! (gui/preferences.rs・spkstate.rs と同じ手法)を1箇所に共通化したもの。
//! 読み込みで壊れていた場合は、次の保存で上書きされて救出不能になる前に
//! `.corrupt` へリネームして退避する(削除はしない)。

/// アトミックな置き換え書き込み。一時ファイルへ書き出して sync した後、
/// rename で差し替えるため、書き込み途中の電源断・強制終了で既存ファイルが
/// 壊れた状態へ変わらない(rename の前後で内容が切り替わるだけ)
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("保存先がありません"))?;
    std::fs::create_dir_all(parent)?;
    // 一時ファイル名に pid を含める: 同一端末で複数プロセスが走っても
    // 互いの一時ファイルを潰さないため
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    {
        use std::io::Write;
        // 作成時点から所有者のみにする(既定 644 で作って chmod する間に、共有
        // 端末の別ユーザーへ読まれる窓を残さない)
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
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// 壊れた状態ファイルを上書きされる前に退避する。`path` を `path.corrupt` へ
/// リネームする(既に退避ファイルがある場合は `.corrupt.1` `.corrupt.2` …と
/// 番号を増やし、過去の退避を上書きしない。Unix の rename は宛先を黙って
/// 上書きするため、事前の存在確認で守る)。成功したら退避先のパスを返す
pub fn quarantine(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let first = {
        let mut p = path.as_os_str().to_os_string();
        p.push(".corrupt");
        std::path::PathBuf::from(p)
    };
    if !first.exists() && std::fs::rename(path, &first).is_ok() {
        return Some(first);
    }
    // 既に退避ファイルが有る場合は番号を増やして試す(上限 9 で諦める)
    for i in 1..=9u32 {
        let mut p = first.clone().into_os_string();
        p.push(format!(".{i}"));
        let candidate = std::path::PathBuf::from(p);
        if !candidate.exists() && std::fs::rename(path, &candidate).is_ok() {
            return Some(candidate);
        }
    }
    eprintln!(
        "[persist] {} の退避に失敗しました(破損ファイルは次の保存で上書きされます)",
        path.display()
    );
    None
}

/// JSON の状態ファイルを読む。パースに失敗(壊れている)した場合は退避して
/// None を返す=呼び出し側は既定値で起動する。ファイルが無い・読めない場合は
/// 壊れていると断定できないため退避せず None
pub fn read_json_or_quarantine(path: &std::path::Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            let saved_to = quarantine(path);
            eprintln!(
                "[persist] {} が壊れています({e})。{}既定値で起動します",
                path.display(),
                saved_to
                    .as_ref()
                    .map(|p| format!("{} へ退避しました。", p.display()))
                    .unwrap_or_default()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("knit-persist-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// アトミック書き込み: 内容が正しく書かれ、一時ファイルが残らない
    #[test]
    fn write_atomic_replaces_content_and_leaves_no_temp() {
        let dir = temp_dir("atomic");
        let path = dir.join("state.json");
        write_atomic(&path, b"{\"a\":1}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":1}");
        // 既存ファイルの置き換えも 1 回の rename で完了する
        write_atomic(&path, b"{\"a\":2}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":2}");
        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["state.json".to_string()], "一時ファイルは残らない");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 保存した状態ファイルは所有者のみに読める権限で作られる(unix)
    #[cfg(unix)]
    #[test]
    fn write_atomic_creates_owner_only_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("mode");
        let path = dir.join("state.json");
        write_atomic(&path, b"{}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 壊れた JSON の読み込み: 退避ファイルが生成され None が返る。
    /// 元のバイトは退避先から読み戻せる(救出材料として残る)
    #[test]
    fn broken_json_is_quarantined_and_returns_none() {
        let dir = temp_dir("quarantine");
        let path = dir.join("peer-sides.json");
        std::fs::write(&path, b"{ broken").unwrap();
        assert!(read_json_or_quarantine(&path).is_none());
        assert!(!path.exists(), "元の位置からは消える");
        let backup = dir.join("peer-sides.json.corrupt");
        assert_eq!(std::fs::read(&backup).unwrap(), b"{ broken", "元の内容が退避先へ残る");
        // 正常な JSON はそのまま読める
        std::fs::write(&path, br#"{"a":1}"#).unwrap();
        assert_eq!(
            read_json_or_quarantine(&path).and_then(|v| v["a"].as_i64()),
            Some(1)
        );
        // 存在しないファイルは退避を作らず None
        let missing = dir.join("missing.json");
        assert!(read_json_or_quarantine(&missing).is_none());
        assert!(!dir.join("missing.json.corrupt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 退避の連続: 2 回目以降は番号を増やし、過去の退避を上書きしない
    #[test]
    fn quarantine_numbers_backups_instead_of_overwriting() {
        let dir = temp_dir("numbering");
        let path = dir.join("s.json");
        std::fs::write(&path, b"first").unwrap();
        let b1 = quarantine(&path).unwrap();
        std::fs::write(&path, b"second").unwrap();
        let b2 = quarantine(&path).unwrap();
        assert_ne!(b1, b2, "退避先は毎回別のファイル");
        assert_eq!(std::fs::read(&b1).unwrap(), b"first");
        assert_eq!(std::fs::read(&b2).unwrap(), b"second");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
