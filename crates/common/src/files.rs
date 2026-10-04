    //! 受信ファイルの保存(Mac/Windows 共通)。名前の無害化と同名回避を一箇所に置き、
    //! 両側で規則が食い違う(Windows だけ上書きしていた)事故を防ぐ
    use std::path::{Path, PathBuf};

    /// 1 ファイルの受信上限(送信側の合計上限と同じ)
    pub const MAX_FILE: u64 = 10 * 1024 * 1024 * 1024;

    /// 1 コンポーネント(ファイル名)の上限。Windows の実制限と同じ UTF-16 units
    /// で数える(絵文字など BMP 外の文字はサロゲートペアで 2 units。文字数や
    /// バイト数では実制限とずれ、Windows だけ最終リネームに失敗する)
    pub const MAX_NAME_UNITS: usize = 255;

    /// 相対パス(rel)全体の上限(UTF-16 units)。深い日本語フォルダ(1 文字 1 unit)
    /// も現実的な範囲で受け付ける。旧判定(バイト数 480・文字数 200)は日本語
    /// 200 文字=600 バイトが正当なのに弾かれていたため、実制限の単位へ揃えた
    pub const MAX_REL_UNITS: usize = 1000;

    /// 名前の長さを UTF-16 units で数える(Windows のファイル名・パス制限の単位)
    pub fn utf16_len(s: &str) -> usize {
        s.encode_utf16().count()
    }

    /// UTF-16 units 上限内へ切り詰める。char 単位で足すため、サロゲートペアの
    /// 途中(上位サロゲートだけ残る位置)では切らない
    fn truncate_utf16(s: &str, max_units: usize) -> String {
        let mut out = String::new();
        let mut units = 0usize;
        for c in s.chars() {
            let cu = c.len_utf16();
            if units + cu > max_units {
                break;
            }
            out.push(c);
            units += cu;
        }
        out
    }

    /// 受信一時ファイル(.knit-<16進16桁>.part)の形式判定。掃除(sweep_temp_files)
    /// と名前の無害化(sanitize)が同じ規則を共有する: 片方だけ広いと、相手から
    /// 届いたファイルが「残骸」と誤認されて消える隙間が残る。
    /// 桁の部分は create_temp が `{:016x}` で作るのと同じ小文字の 16 進だけを
    /// 認める(大文字・前後の余分は本物の形式ではない)
    fn is_temp_name(name: &str) -> bool {
        let Some(hex) = name.strip_prefix(".knit-") else {
            return false;
        };
        let Some(hex) = hex.strip_suffix(".part") else {
            return false;
        };
        hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() && b <= b'f')
    }

    /// 相手から届いたファイル名を、どちらの OS でも安全な単一の名前へ変換する。
    /// パス区切り・予約文字・制御文字は '_'、末尾の '.' と空白は除去(Windows が
    /// 受け付けないため。先頭の '.' は Unix の隠しファイルに意味があるので残す)、
    /// Windows の予約デバイス名(CON/NUL/COM1 等)は先頭に '_' を付ける。
    /// 一時ファイルと同一形式(.knit-<hex16>.part)の名前も先頭に '_' を付ける
    /// (そのままだと起動時の掃除に消されるため)
    pub fn sanitize(name: &str) -> String {
        let mut s: String = name
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                c if c.is_control() => '_',
                c => c,
            })
            .collect();
        s = s
            .trim_start_matches(|c: char| c.is_whitespace())
            .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
            .to_string();
        if utf16_len(&s) > MAX_NAME_UNITS {
            s = truncate_utf16(&s, MAX_NAME_UNITS);
            // 切り詰めで末尾に '.' や空白が露出すると、Windows が保存時に黙って
            // 剥がす/失敗するため、切り詰めた後にもう一度末尾を整える
            s = s
                .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
                .to_string();
        }
        if s.is_empty() {
            return "file".into();
        }
        let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit());
        if reserved || is_temp_name(&s) {
            s.insert(0, '_');
        }
        s
    }

    /// ファイル群の指紋(パス+合計サイズ)。同一コピーの再検出・エコーバック判定に
    /// 使う。両 OS で同じ形式にするためここへ置く
    pub fn key(paths: &[String]) -> String {
        let sizes: u64 = paths
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
            .sum();
        format!("{}|{sizes}", paths.join("\u{1}"))
    }

    /// 相手 PC から来たファイルに「外部から入手した」印を付ける。これが無いと、
    /// 受信した実行ファイルや .app が OS の警告(SmartScreen / Gatekeeper)なしで開ける。
    /// 付与できなくても受信自体は続ける(NTFS 以外のドライブ等)
    pub fn mark_untrusted(path: &Path) {
        #[cfg(windows)]
        {
            let mut ads = path.as_os_str().to_owned();
            ads.push(":Zone.Identifier");
            let _ = std::fs::write(ads, "[ZoneTransfer]\r\nZoneId=3\r\n");
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            unsafe extern "C" {
                fn setxattr(
                    path: *const core::ffi::c_char,
                    name: *const core::ffi::c_char,
                    value: *const core::ffi::c_void,
                    size: usize,
                    position: u32,
                    options: i32,
                ) -> i32;
            }
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // 形式: フラグ;時刻(16進);取得元アプリ;UUID(省略可)
            let value = format!("0081;{secs:x};Knit;");
            let Ok(p) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
                return;
            };
            unsafe {
                setxattr(
                    p.as_ptr(),
                    c"com.apple.quarantine".as_ptr(),
                    value.as_ptr() as *const core::ffi::c_void,
                    value.len(),
                    0,
                    0,
                );
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = path;
    }

    /// 相対パス付きの受信ファイル名(dir/sub/file.txt。版 14 のフォルダ対応)を
    /// 検証して無害化する。コンポーネント毎に sanitize し、パス遷移(..)・絶対パス・
    /// 空コンポーネント・深すぎるパスは受け付けない(受信側の書き込み先を
    /// 受信フォルダの外へ出させない)。不正な場合は None。
    /// 長さは UTF-16 units で MAX_REL_UNITS まで(深い日本語フォルダも通す)
    pub fn sanitize_rel(name: &str) -> Option<String> {
        if utf16_len(name) > MAX_REL_UNITS {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        for comp in name.split('/') {
            if comp.is_empty() {
                return None; // 連続する区切り・先頭末尾の区切りを許さない
            }
            let s = sanitize(comp);
            if s.is_empty() || s == "." || s == ".." {
                return None; // sanitize で「.」「..」に戻る文字列も許さない
            }
            parts.push(s);
        }
        if parts.len() > 16 {
            return None;
        }
        Some(parts.join("/"))
    }

    /// dir のあるドライブの空きバイト数。取得できない環境では None(確認を省略する)
    pub fn available_bytes(dir: &Path) -> Option<u64> {
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            // statfs64 の先頭レイアウト: [f_bsize u32][f_iosize i32][f_blocks u64]
            // [f_bfree u64][f_bavail u64]。構造体全体は配列を含み数 KB あるため、
            // 十分なバッファへ書かせて先頭フィールドだけ読む
            unsafe extern "C" {
                fn statfs64(path: *const core::ffi::c_char, buf: *mut u8) -> i32;
            }
            let Ok(p) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
                return None;
            };
            let mut buf = vec![0u8; 4096];
            if unsafe { statfs64(p.as_ptr(), buf.as_mut_ptr()) } != 0 {
                return None;
            }
            let field = |off: usize| -> u64 {
                buf[off..off + 8].try_into().map(u64::from_le_bytes).unwrap_or(0)
            };
            let bsize = u32::from_le_bytes(
                buf[0..4].try_into().ok()?,
            ) as u64;
            let bavail = field(24);
            Some(bsize.saturating_mul(bavail))
        }
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn GetDiskFreeSpaceExW(
                    directory: *const u16,
                    free_bytes_available: *mut u64,
                    total_bytes: *mut u64,
                    total_free_bytes: *mut u64,
                ) -> i32;
            }
            let wide: Vec<u16> = dir
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
            if unsafe {
                GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free)
            } == 0
            {
                return None;
            }
            Some(avail)
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = dir;
            None
        }
    }

    /// 一時ファイル名用の擬似乱数(予測不可能性は不要。衝突回避だけが目的)
    fn temp_salt() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        t ^ (std::process::id() as u64).rotate_left(32)
            ^ COUNTER.fetch_add(0x9E3779B97F4A7C15, Ordering::Relaxed)
    }

    /// parent 内へ一時ファイル(.knit-*.part)を作る。最終名への書き込みを防ぎ、
    ///完了するまで受信フォルダに不完全な実体が見えないようにする
    pub fn create_temp(parent: &Path) -> Option<(std::fs::File, PathBuf)> {
        std::fs::create_dir_all(parent).ok()?;
        for _ in 0..8 {
            let cand = parent.join(format!(".knit-{:016x}.part", temp_salt()));
            if let Ok(f) = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&cand)
            {
                return Some((f, cand));
            }
        }
        None
    }

    /// 受信フォルダに残った一時ファイル(.knit-<16進16桁>.part)を掃除する。
    /// プロセスの異常終了(exit で Drop が走らない経路・クラッシュ・kill)で
    /// 残った残骸を、起動時(まだ受信が始まっていないため安全)に消す保険。
    /// 形式は create_temp が作る名前と厳密に一致する物だけ(相手から届いた
    /// ファイルがたまたま .part で終わる名前でも消さない)。
    /// 戻り値は消した件数(ログ・通知の判断用)
    pub fn sweep_temp_files(dir: &Path) -> usize {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut removed = 0usize;
        for e in rd.flatten() {
            let Some(name) = e.file_name().to_str().map(|n| n.to_string()) else {
                continue;
            };
            if is_temp_name(&name) && std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// リネーム先の候補パスを組み立てる("名前 (n).拡張子"。n=0 は素の名前)
    fn candidate(parent: &Path, stem: &str, ext: &str, i: u32) -> PathBuf {
        if i == 0 {
            parent.join(format!("{stem}{ext}"))
        } else {
            parent.join(format!("{stem} ({i}){ext}"))
        }
    }

    /// 受信ファイルの最終パスを予約する。実体はまだ作らない(原子的配置のため)。
    /// taken には同じバッチで既に予約済みのパスを渡す(一時ファイルだから
    /// ディスク上の存在確認だけでは重なりを検出できない)
    pub fn unique_final(base: &Path, rel: &str, taken: &[PathBuf]) -> Option<PathBuf> {
        let rel_path = Path::new(rel);
        let name = rel_path.file_name()?.to_str()?.to_string();
        let parent_rel = rel_path.parent()?;
        let parent = if parent_rel.as_os_str().is_empty() {
            base.to_path_buf()
        } else {
            base.join(parent_rel)
        };
        std::fs::create_dir_all(&parent).ok()?;
        let p = Path::new(&name);
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let ext = p
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        for i in 0..1000u32 {
            let cand = candidate(&parent, &stem, &ext, i);
            if !cand.exists() && !taken.contains(&cand) {
                return Some(cand);
            }
        }
        None
    }

    /// 一時ファイルを最終名へ原子的に公開する(リネーム)。公開先が既に埋まって
    /// いたら "名前 (n).ext" を探す。失敗した場合は呼び出し元が一時ファイルを削除する
    pub fn publish(temp: &Path, desired: &Path) -> std::io::Result<PathBuf> {
        let parent = desired
            .parent()
            .ok_or_else(|| std::io::Error::other("publish: desired has no parent"))?;
        let name = desired
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| std::io::Error::other("publish: desired has no file name"))?;
        let p = Path::new(name);
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let ext = p
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let mut last_err = None;
        for i in 0..1000u32 {
            let cand = candidate(parent, &stem, &ext, i);
            // Unix の rename は上書きするため、確認してから移す(同一プロセス内の
            // 受信フォルダなので競合の窓は実質的に無い)
            if cand.exists() {
                last_err = Some(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "destination already exists",
                ));
                continue;
            }
            match std::fs::rename(temp, &cand) {
                Ok(()) => return Ok(cand),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last_err = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| std::io::Error::other("publish: no free name")))
    }

    /// 相手のファイルの更新日時を復元する(秒精度。失敗しても転送成否には影響しない)。
    /// 相手が指定した異常に大きな値では加算がオーバーフローして panic するため、
    /// 範囲外の値は無視する
    pub fn set_mtime(path: &Path, unix_secs: u64) {
        if unix_secs == 0 {
            return;
        }
        let Some(t) = std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(unix_secs))
        else {
            return;
        };
        if let Ok(f) = std::fs::File::options().write(true).open(path) {
            let _ = f.set_modified(t);
        }
    }

#[cfg(test)]
mod tests {
    use super::*;

    /// 切り詰めの上限は Windows の実制限と同じ UTF-16 units(255)。日本語(BMP 内)
    /// は 1 文字 1 unit のため 255 文字まで、絵文字は 1 文字 2 units のため
    /// 127 文字まで入る
    #[test]
    fn sanitize_truncates_to_255_utf16_units() {
        let jp = "あ".repeat(MAX_NAME_UNITS);
        assert_eq!(sanitize(&jp).chars().count(), MAX_NAME_UNITS);
        let jp_over = "あ".repeat(MAX_NAME_UNITS + 10);
        assert_eq!(utf16_len(&sanitize(&jp_over)), MAX_NAME_UNITS);

        // 絵文字 128 個 = 256 units > 255。旧判定(文字数 200)では 128 文字が
        // そのまま通り、Windows の 255 units 制限に引っかかっていた
        let emoji = "😀".repeat(128);
        let cut = sanitize(&emoji);
        assert_eq!(utf16_len(&cut), MAX_NAME_UNITS - 1, "最後の絵文字(2 units)を入れると超えるため 254 units で止まる");
        assert_eq!(cut.chars().count(), 127, "サロゲートペアの途中で切らない");
    }

    /// 切り詰めた位置の末尾に '.' や空白が露出したら、切り詰め後にもう一度
    /// 末尾を整える(Windows は末尾の '.'・空白を受け付けず、保存時に黙って
    /// 剥がされたり失敗したりする)
    #[test]
    fn sanitize_retrims_dots_exposed_by_truncation() {
        // 254 文字 + ".bb" = 258 units → 255 units で切ると末尾が '.' になる
        let name = format!("{}..bb", "a".repeat(254));
        let cut = sanitize(&name);
        assert!(!cut.ends_with('.'), "切り詰め後の末尾に '.' を残さない: {cut:?}");
        assert!(!cut.ends_with(' '), "切り詰め後の末尾に空白を残さない");
        // 切り詰めで全て剥がれた(空白・ドットだけの長い名前)場合は既定名へ落ちる
        assert_eq!(sanitize(&"  ".repeat(300)), "file");
    }

    /// rel の上限は UTF-16 units で 1000。深い日本語フォルダ(旧判定の
    /// バイト数 480 では 160 文字で弾かれていた)も通る
    #[test]
    fn sanitize_rel_accepts_deep_japanese_paths_up_to_1000_units() {
        // 日本語 200 文字 = 200 units(旧: 600 バイト > 480 で拒否されていた)
        assert_eq!(sanitize_rel(&"あ".repeat(200)).as_deref(), Some("あ".repeat(200).as_str()));
        // 深い階層の合計で判定する(コンポーネント毎ではない)
        let deep = vec!["あ".repeat(60); 3].join("/");
        assert_eq!(utf16_len(&deep), 182);
        assert!(sanitize_rel(&deep).is_some());
        // 境界: ちょうど 1000 units は通る
        let just = vec!["あ".repeat(997), "aa".to_string()].join("/");
        assert_eq!(utf16_len(&just), MAX_REL_UNITS);
        assert!(sanitize_rel(&just).is_some());
        // 1001 units は拒否
        let over = "あ".repeat(MAX_REL_UNITS + 1);
        assert!(sanitize_rel(&over).is_none());
    }

    /// rel 内の各コンポーネントは sanitize(255 units 切り詰め)を通る
    #[test]
    fn sanitize_rel_truncates_each_component() {
        let rel = format!("{}/{}", "あ".repeat(300), "b.txt");
        let got = sanitize_rel(&rel).unwrap();
        let parts: Vec<&str> = got.split('/').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(utf16_len(parts[0]), MAX_NAME_UNITS);
        assert_eq!(parts[1], "b.txt");
    }

    /// 受信名が一時ファイルと同一形式(.knit-<hex16>.part)なら先頭へ '_' を付け、
    /// 起動時の掃除に消されない名前へ変える。先頭の '.' 自体は Unix の隠しファイル
    /// として意味があるので、形式が違うドットファイルには触れない
    #[test]
    fn sanitize_renames_files_sharing_the_temp_name_shape() {
        assert_eq!(
            sanitize(".knit-0123456789abcdef.part"),
            "_.knit-0123456789abcdef.part"
        );
        assert_eq!(
            sanitize(".knit-ffffffffffffffff.part"),
            "_.knit-ffffffffffffffff.part"
        );
        // 16 桁でなければ一時ファイルの形式ではない: そのまま(隠しファイル扱い)
        assert_eq!(sanitize(".knit-0123456789abcde.part"), ".knit-0123456789abcde.part");
        assert_eq!(sanitize(".knit-0123456789abcdef.par"), ".knit-0123456789abcdef.par");
        assert_eq!(sanitize(".knit-notes.md"), ".knit-notes.md");
        assert_eq!(sanitize(".part"), ".part");
    }
}

/// 起動時の一時ファイル掃除(sweep_temp_files)の単体テスト
#[cfg(test)]
mod sweep_temp_tests {
    use super::sweep_temp_files;

    /// 一時掃除用の孤立ディレクトリを作る(並列テスト・連続実行での衝突を避ける)。
    /// 時刻の解像度は OS によって粗いため、連番も併せて一意性を担保する
    fn scratch() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "knit-sweep-{}-{}-{seq}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 残骸(.knit-*.part)だけが消え、受信済みファイルと .part 以外のドット
    /// ファイルは残る(受信フォルダの他の内容を誤って消さないこと)
    #[test]
    fn sweeps_only_leftover_part_files() {
        let dir = scratch();
        let part1 = dir.join(".knit-0000000000000001.part");
        let part2 = dir.join(".knit-ffffffffffffffff.part");
        std::fs::write(&part1, b"leftover").unwrap();
        std::fs::write(&part2, b"leftover2").unwrap();
        let keep = dir.join("受信済み.txt");
        std::fs::write(&keep, b"ok").unwrap();
        let dotfile = dir.join(".knit-notes.md");
        std::fs::write(&dotfile, b"ok").unwrap();
        let subdir = dir.join("資料");
        std::fs::create_dir_all(&subdir).unwrap();
        assert_eq!(sweep_temp_files(&dir), 2, ".part の残骸だけを数える");
        assert!(!part1.exists(), "残骸の一時ファイルは消える");
        assert!(!part2.exists(), "残骸の一時ファイルは消える");
        assert!(keep.exists(), "受信済みファイルは残る");
        assert!(dotfile.exists(), ".part 以外のドットファイルは残る");
        assert!(subdir.exists(), "フォルダは残る");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 存在しないディレクトリでは何も起きない(起動時の初回呼び出しでも安全)
    #[test]
    fn missing_directory_is_silent_noop() {
        let missing = std::env::temp_dir().join(format!(
            "knit-sweep-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert_eq!(sweep_temp_files(&missing), 0);
    }

    /// 掃除は create_temp が作る名前(.knit- + 16進16桁 + .part)と厳密一致する
    /// 物だけを消す。相手から届いたファイルがたまたま .knit-* や .part を
    /// 含む名前でも、形式が違えば残る(誤削除の回帰防止)
    #[test]
    fn sweep_spares_names_that_do_not_match_the_temp_shape_exactly() {
        let dir = scratch();
        let keep_hex15 = dir.join(".knit-0123456789abcde.part"); // 15桁
        let keep_hex17 = dir.join(".knit-0123456789abcdef0.part"); // 17桁
        let keep_upper = dir.join(".knit-0123456789ABCDEF.part"); // 大文字
        let keep_nonhex = dir.join(".knit-zzzzzzzzzzzzzzzz.part"); // 16桁だが hex でない
        let keep_partial = dir.join(".part");
        let keep_plain = dir.join(".knit-part");
        for f in [&keep_hex15, &keep_hex17, &keep_upper, &keep_nonhex, &keep_partial, &keep_plain] {
            std::fs::write(f, b"received").unwrap();
        }
        assert_eq!(sweep_temp_files(&dir), 0, "形式が違う物は 1 つも消さない");
        for f in [&keep_hex15, &keep_hex17, &keep_upper, &keep_nonhex, &keep_partial, &keep_plain] {
            assert!(f.exists(), "形式が違う名前は残る: {}", f.display());
        }
        // 対照: 厳密に一致する物だけは消える
        let real = dir.join(".knit-0000000000000000.part");
        std::fs::write(&real, b"leftover").unwrap();
        assert_eq!(sweep_temp_files(&dir), 1);
        assert!(!real.exists(), "本物の一時ファイルは消える");
        std::fs::remove_dir_all(&dir).ok();
    }
}
