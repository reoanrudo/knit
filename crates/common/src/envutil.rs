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

    /// 旧名称の設定フォルダを、新しいフォルダへ丸ごと複製する。
    /// 旧版へ戻しても設定が残るよう、移動ではなく複製にする。
    /// 「最後まで通った」証として new 内へ完了マーカー(migrated-from-旧名)を
    /// 置き、マーカーが無ければ(初回またはコピー途中のプロセス死亡)最初から
    /// やり直す。コピーは上書きのため冪数で、部分コピーが中途半端に残った
    /// 状態からでも再試行で完全な状態に収束する
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
        // 完了マーカーのパス。old の名前ごとに固有にする
        let marker = old.file_name().and_then(|n| n.to_str()).map(|n| {
            new.join(format!("migrated-from-{n}"))
        });
        let Some(marker) = marker else { return };
        if marker.exists() || !old.is_dir() {
            return;
        }
        if let Err(e) = copy(old, new) {
            // 失敗しても部分コピーは消さない: 失敗の原因がディスク容量等なら
            // 削除も失敗しやすく、またマーカーが無いため次回は最初から上書きで
            // やり直される(途中のファイルも完全なコピーで置き換わる)
            eprintln!("[config] 旧設定 {} の移行に失敗: {e}(次回の起動でやり直します)", old.display());
            return;
        }
        // 全ファイルのコピーが通ってから初めてマーカーを置く(ここで初回確定)
        let stamped = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let _ = std::fs::write(&marker, stamped);
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

    /// 設定ファイルの KEY=VALUE 行を書き換える(無ければ追記)。
    /// Windows 側の .env 書き換え(tray の save_host_to_env)と同じ挙動を共通化した
    /// もので、同じキーと旧名称のキー(TSUNAGU_*・SEAMLESS_*)の行は新しい 1 行に
    /// まとめる(get の解決順序に古い指定が残らないようにする)。
    /// value が空のときは行を書かない=未指定の既定へ戻す。
    /// 書き込みは一時ファイル+リネームのため、失敗しても元の内容が残る
    pub fn set_env_value(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
        let mut removes = vec![format!("{key}=")];
        removes.extend(legacy_keys(key).iter().map(|k| format!("{k}=")));
        let mut lines: Vec<String> = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !removes.iter().any(|r| t.starts_with(r.as_str()))
            })
            .map(|l| l.to_string())
            .collect();
        if !value.is_empty() {
            lines.push(format!("{key}={value}"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| std::io::Error::other("保存先のフォルダがありません"))?;
        std::fs::create_dir_all(parent)?;
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, lines.join("\n") + "\n")?;
        std::fs::rename(&tmp, path)
    }

    /// env ファイルから Knit の設定行(KNIT_* と旧名称の TSUNAGU_*・SEAMLESS_*)を
    /// すべて取り除く(設定の初期化用)。コメント行など Knit 以外の行が混ざって
    /// いても保持する。ファイルが無ければ何もしない(=既に指定が無い状態のため
    /// 素通り)。書き込みは set_env_value と同じ一時ファイル+リネームのため、
    /// 失敗しても元の内容が残る
    pub fn clear_knit_lines(path: &Path) -> std::io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let is_knit_line = |l: &str| {
            let t = l.trim_start();
            ["KNIT_", "TSUNAGU_", "SEAMLESS_"]
                .iter()
                .any(|prefix| t.starts_with(prefix))
        };
        let lines: Vec<String> = std::fs::read_to_string(path)?
            .lines()
            .filter(|l| !is_knit_line(l))
            .map(|l| l.to_string())
            .collect();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| std::io::Error::other("保存先のフォルダがありません"))?;
        std::fs::create_dir_all(parent)?;
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, lines.join("\n") + "\n")?;
        std::fs::rename(&tmp, path)
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

        /// set_env_value の一連の動作(追記・書き換え・旧キーの統合・削除)。
        /// ファイルは都度新しく作る(テスト間で共有しない)
        #[test]
        fn set_env_value_rewrites_appends_and_clears() {
            let path = std::env::temp_dir().join(format!(
                "knit-setenv-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            // 既存ファイルが無ければ追記する(コメント行も書ける)
            set_env_value(&path, "KNIT_HOST", "192.168.1.23").unwrap();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "KNIT_HOST=192.168.1.23\n"
            );
            // 既存行は置き換え、関係ない行(コメント・他キー)はそのまま残る
            std::fs::write(
                &path,
                "# 接続先\nKNIT_TOKEN=abc\nKNIT_HOST=10.0.0.1\nTSUNAGU_HOST=10.0.0.2\n",
            )
            .unwrap();
            set_env_value(&path, "KNIT_HOST", "192.168.1.99,100.100.10.9").unwrap();
            let after = std::fs::read_to_string(&path).unwrap();
            assert!(after.contains("# 接続先"), "コメント行は保持: {after}");
            assert!(after.contains("KNIT_TOKEN=abc"), "他キーは保持: {after}");
            assert!(after.contains("KNIT_HOST=192.168.1.99,100.100.10.9"));
            assert!(
                !after.contains("10.0.0.1") && !after.contains("10.0.0.2"),
                "新旧の KNIT_HOST 行は新しい 1 行にまとまる: {after}"
            );
            assert!(
                after.lines().count() == 3,
                "旧名称行も削除されて 3 行になる: {after}"
            );
            // 空値は行ごと削除(未指定=既定の挙動へ戻す)
            set_env_value(&path, "KNIT_HOST", "").unwrap();
            let cleared = std::fs::read_to_string(&path).unwrap();
            assert!(!cleared.contains("KNIT_HOST"));
            assert!(cleared.contains("KNIT_TOKEN=abc"));
            // 末尾に改行が無い既存ファイルでも失われる行が出ない
            std::fs::write(&path, "KNIT_TOKEN=abc").unwrap();
            set_env_value(&path, "KNIT_HOST", "1.2.3.4").unwrap();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "KNIT_TOKEN=abc\nKNIT_HOST=1.2.3.4\n"
            );
            std::fs::remove_file(&path).unwrap();
        }

        /// clear_knit_lines: KNIT_*(新旧名称)の行だけが消え、コメントと他ツールの
        /// 行は残る。ファイルが無い場合は作らない(初期化の冪等性)
        #[test]
        fn clear_knit_lines_removes_only_knit_entries() {
            let path = std::env::temp_dir().join(format!(
                "knit-clearenv-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            // ファイルが無いとき: 素通り(空ファイルを作らない)
            clear_knit_lines(&path).unwrap();
            assert!(!path.exists(), "存在しないファイルは作らない");
            std::fs::write(
                &path,
                "# Knit 設定\nKNIT_HOST=10.0.0.1\nTSUNAGU_HOST=10.0.0.2\nSEAMLESS_DESK_TOKEN=x\nOTHER_TOOL=1\n",
            )
            .unwrap();
            clear_knit_lines(&path).unwrap();
            let after = std::fs::read_to_string(&path).unwrap();
            assert!(after.contains("# Knit 設定"), "コメント行は保持: {after}");
            assert!(after.contains("OTHER_TOOL=1"), "他ツールの行は保持: {after}");
            assert!(!after.contains("KNIT_HOST"), "新名称の行は消える: {after}");
            assert!(!after.contains("TSUNAGU_"), "旧名称の行も消える: {after}");
            assert!(!after.contains("SEAMLESS_"), "最旧名称の行も消える: {after}");
            // 再実行しても冪等(全て消えた状態で変化しない)
            clear_knit_lines(&path).unwrap();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                after,
                "初期化済みの再実行は変化させない"
            );
            std::fs::remove_file(&path).unwrap();
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
            assert!(
                new.join("migrated-from-tsunagu").exists(),
                "完了後にマーカーが置かれる"
            );
            std::fs::write(new.join("env"), "KNIT_TOKEN=y\n").unwrap();
            migrate_dir(&old, &new);
            assert_eq!(
                std::fs::read_to_string(new.join("env")).unwrap(),
                "KNIT_TOKEN=y\n",
                "移行済みの設定を上書きしない"
            );
            std::fs::remove_dir_all(base).unwrap();
        }

        /// コピー途中でプロセスが死んだ状態(一部ファイルだけ転写済み・マーカー無し)
        /// から再試行すると、全ファイルが完全なコピーで揃う
        #[test]
        fn partial_migration_is_retried_until_complete() {
            let base =
                std::env::temp_dir().join(format!("knit-migrate-partial-{}", std::process::id()));
            let (old, new) = (base.join("tsunagu"), base.join("knit"));
            std::fs::create_dir_all(old.join("images")).unwrap();
            std::fs::write(old.join("env"), "TSUNAGU_TOKEN=x\n").unwrap();
            std::fs::write(old.join("images/a.png"), b"png").unwrap();
            std::fs::write(old.join("images/b.png"), b"png2").unwrap();
            // 途中で死んだ状況の再現: env だけが中途半端に転写済み
            std::fs::create_dir_all(&new).unwrap();
            std::fs::write(new.join("env"), "TSUNAGU_TOKEN=x\nTRUNCAT").unwrap();
            migrate_dir(&old, &new);
            // 再試行で全ファイルが揃い、部分コピーは完全なコピーへ置き換わる
            assert_eq!(
                std::fs::read_to_string(new.join("env")).unwrap(),
                "TSUNAGU_TOKEN=x\n",
                "部分コピーは本の内容で上書きされる"
            );
            assert_eq!(std::fs::read(new.join("images/a.png")).unwrap(), b"png");
            assert_eq!(std::fs::read(new.join("images/b.png")).unwrap(), b"png2");
            assert!(new.join("migrated-from-tsunagu").exists());
            // マーカーが有る状態での再呼び出しは何もしない(new 側の変更が保たれる)
            std::fs::write(new.join("images/a.png"), b"edited").unwrap();
            migrate_dir(&old, &new);
            assert_eq!(
                std::fs::read(new.join("images/a.png")).unwrap(),
                b"edited",
                "完了済みの再移行は上書きしない"
            );
            std::fs::remove_dir_all(base).unwrap();
        }
    }
