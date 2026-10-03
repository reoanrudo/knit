    //! 設定値の参照: 環境変数 > 実行ファイル同階層の .env > ~/.config/knit/env。
    //! 配布形態(.app バンドル埋め込み / exe 同梱 .env / ホーム設定)のどれでも
    //! 同一コードで動かすための仕組み。KEY=VALUE 形式(1行1エントリ、# はコメント)。
    //! 旧名称(v0.25 までの Tsunagu、v0.7 以前の seamless-desk)の環境変数・設定パスも
    //! フォールバックで読むため、既存環境を書き換えずにそのまま移行できる。

    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    /// 旧名称へのキー変換(新しい順)。
    /// KNIT_X ← TSUNAGU_X ← SEAMLESS_X(トークンだけは SEAMLESS_DESK_TOKEN)
    fn legacy_keys(key: &str) -> Vec<String> {
        let Some(rest) = key.strip_prefix("KNIT_") else {
            return Vec::new();
        };
        let oldest = if rest == "TOKEN" {
            "SEAMLESS_DESK_TOKEN".to_string()
        } else {
            format!("SEAMLESS_{rest}")
        };
        vec![format!("TSUNAGU_{rest}"), oldest]
    }

    /// 旧名称の設定フォルダを、新しいフォルダが無い時だけ丸ごと複製する。
    /// 旧版へ戻しても設定が残るよう、移動ではなく複製にする
    pub fn migrate_dir(old: &Path, new: &Path) {
        fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
            std::fs::create_dir_all(to)?;
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                let target = to.join(entry.file_name());
                if kind.is_dir() {
                    copy(&entry.path(), &target)?;
                } else if kind.is_file() {
                    std::fs::copy(entry.path(), target)?;
                }
            }
            Ok(())
        }
        if new.exists() || !old.is_dir() {
            return;
        }
        if let Err(e) = copy(old, new) {
            // 部分コピーのまま残すと new.exists() で次回以降も補完されない
            //(トークンが転写されないまま起動し続ける)。消して次回に委ねる
            let _ = std::fs::remove_dir_all(new);
            eprintln!("[config] 旧設定 {} の移行に失敗: {e}(次回の起動でやり直します)", old.display());
        }
    }

    /// 設定フォルダ(~/.config/knit)。初回は旧名称のフォルダから移行する
    pub fn config_dir() -> Option<PathBuf> {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        let cfg = Path::new(&home).join(".config");
        let dir = cfg.join("knit");
        migrate_dir(&cfg.join("tsunagu"), &dir);
        Some(dir)
    }

    /// 端末固有データの置き場(Windows は %LOCALAPPDATA%\Knit、Mac は
    /// ~/Library/Application Support/Knit)。初回は旧名称のフォルダから移行する
    pub fn data_dir() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
        #[cfg(not(target_os = "windows"))]
        let root = PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support");
        let dir = root.join("Knit");
        migrate_dir(&root.join("Tsunagu"), &dir);
        Some(dir)
    }

    fn entries() -> &'static Vec<(String, String)> {
        static E: OnceLock<Vec<(String, String)>> = OnceLock::new();
        E.get_or_init(|| {
            let mut v = Vec::new();
            let mut paths = Vec::new();
            if let Ok(exe) = std::env::current_exe() {
                if let Some(d) = exe.parent() {
                    paths.push(d.join(".env"));
                    // .app バンドル配布用: Contents/Resources/.env。
                    // 注意: どちらの場所も署名済み .app へ後から足すとコード署名の
                    // 検証が壊れる(sealed resource)。.app 内へ置く場合は再署名が必要
                    if let Some(res) = d.parent().map(|p| p.join("Resources/.env")) {
                        paths.push(res);
                    }
                }
            }
            for key in ["HOME", "USERPROFILE"] {
                if let Some(home) = std::env::var_os(key) {
                    let cfg = Path::new(&home).join(".config");
                    paths.push(cfg.join("knit/env"));
                    // 旧名称時代の設定パス(移行措置)
                    paths.push(cfg.join("tsunagu/env"));
                    paths.push(cfg.join("seamless-desk/env"));
                }
            }
            for p in paths {
                let Ok(s) = std::fs::read_to_string(&p) else {
                    continue;
                };
                for line in s.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((k, val)) = line.split_once('=') {
                        v.push((
                            k.trim().to_string(),
                            val.trim().trim_matches('"').to_string(),
                        ));
                    }
                }
            }
            v
        })
    }

    fn env_var(key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    /// 環境変数を第一優先とし、未設定なら設定ファイル群から検索する。
    /// 旧名称のキー(TSUNAGU_*・SEAMLESS_*)も新しい順に確認する
    pub fn get(key: &str) -> Option<String> {
        if let Some(v) = env_var(key) {
            return Some(v);
        }
        let legacy = legacy_keys(key);
        let find = |k: &str| {
            entries()
                .iter()
                .find(|(name, _)| name == k)
                .map(|(_, v)| v.clone())
        };
        find(key)
            .or_else(|| legacy.iter().find_map(|k| find(k)))
            // 旧名称の環境変数も受け入れる(スクリプト側の書き換え漏れ保険)
            .or_else(|| legacy.iter().find_map(|k| env_var(k)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn new_keys_fall_back_to_both_older_names() {
            assert_eq!(
                legacy_keys("KNIT_TOKEN"),
                ["TSUNAGU_TOKEN", "SEAMLESS_DESK_TOKEN"]
            );
            assert_eq!(legacy_keys("KNIT_HOST"), ["TSUNAGU_HOST", "SEAMLESS_HOST"]);
            assert!(legacy_keys("OTHER").is_empty());
        }

        #[test]
        fn migrates_old_config_once_without_touching_it() {
            let base = std::env::temp_dir().join(format!("knit-migrate-{}", std::process::id()));
            let (old, new) = (base.join("tsunagu"), base.join("knit"));
            std::fs::create_dir_all(old.join("images")).unwrap();
            std::fs::write(old.join("env"), "TSUNAGU_TOKEN=x\n").unwrap();
            std::fs::write(old.join("images/a.png"), b"png").unwrap();
            migrate_dir(&old, &new);
            assert_eq!(std::fs::read(new.join("images/a.png")).unwrap(), b"png");
            assert!(old.join("env").exists(), "旧版へ戻せるよう元は残す");
            std::fs::write(new.join("env"), "KNIT_TOKEN=y\n").unwrap();
            migrate_dir(&old, &new);
            assert_eq!(
                std::fs::read_to_string(new.join("env")).unwrap(),
                "KNIT_TOKEN=y\n",
                "移行済みの設定を上書きしない"
            );
            std::fs::remove_dir_all(base).unwrap();
        }
    }
