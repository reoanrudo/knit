    //! 受信ファイルの保存(Mac/Windows 共通)。名前の無害化と同名回避を一箇所に置き、
    //! 両側で規則が食い違う(Windows だけ上書きしていた)事故を防ぐ
    use std::path::{Path, PathBuf};

    /// 1 ファイルの受信上限(送信側の合計上限と同じ)
    pub const MAX_FILE: u64 = 10 * 1024 * 1024 * 1024;

    /// 相手から届いたファイル名を、どちらの OS でも安全な単一の名前へ変換する。
    /// パス区切り・予約文字・制御文字は '_'、末尾の '.' と空白は除去(Windows が
    /// 受け付けないため。先頭の '.' は Unix の隠しファイルに意味があるので残す)、
    /// Windows の予約デバイス名(CON/NUL/COM1 等)は先頭に '_' を付ける
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
        if s.chars().count() > 200 {
            s = s.chars().take(200).collect();
        }
        if s.is_empty() {
            return "file".into();
        }
        let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit());
        if reserved {
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
    /// 受信フォルダの外へ出させない)。不正な場合は None
    pub fn sanitize_rel(name: &str) -> Option<String> {
        if name.len() > 480 || name.chars().count() > 200 {
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
