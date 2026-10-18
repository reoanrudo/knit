//! 遅延提供(版 17)の Windows 側。**データを待たずにドロップを受理する**ための部品。
//!
//! 背景(実測): 現行は全ファイルの到着後に OLE ドラッグを始めるため、Wi-Fi 実効
//! 30MB/s では 1GB=34秒・10GB=5分41秒 のあいだ物理ボタンを押し続けることになる。
//! 遅延提供では目録(`Msg::Manifest`)だけで `DoDragDrop` を開始し、内容は
//! `CFSTR_FILECONTENTS` の `IStream` として**届いた分から**返す。
//!
//! このファイルは**純粋な部分だけ**を持つ(記述子の組み立てと段取り)。COM の
//! vtbl 配線は `dragdrop.rs` 側に置く。判断は `knit_common::drag::lazy` の
//! 状態機械にあり、ここは与えられた値をバイト列にするだけ。
#![cfg(windows)]

use knit_common::proto::ManifestEntry;

/// `FILEDESCRIPTORW` の大きさ(バイト)。MSDN の並びどおり:
/// dwFlags(4) + clsid(16) + sizel(8) + pointl(8) + dwFileAttributes(4) +
/// ftCreation(8) + ftLastAccess(8) + ftLastWrite(8) + sizeHigh/Low(8) + cFileName(520)
pub const FILEDESCRIPTORW_SIZE: usize = 592;
/// `FILEGROUPDESCRIPTORW` のヘッダ(DWORD cItems)
pub const GROUP_HEADER_SIZE: usize = 4;
/// `cFileName` の上限(WCHAR 単位。終端 NUL を含む)
pub const MAX_FILE_NAME: usize = 260;

/// dwFlags: 属性・サイズ・進行 UI・Unicode を使うと宣言する
const FD_ATTRIBUTES: u32 = 0x0000_0004;
const FD_FILESIZE: u32 = 0x0000_0040;
const FD_PROGRESSUI: u32 = 0x0000_4000;
const FD_UNICODE: u32 = 0x8000_0000;
/// フォルダを表す属性(`kind == 1`)
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;

/// 遅延提供を実際に使うか。**既定は無効**(実機で挙動を確かめてから既定を反転する)。
/// 有効時のみ目録を相手へ送るので、無効なら現行方式のまま何も変わらない
pub fn lazy_enabled() -> bool {
    matches!(std::env::var("KNIT_LAZY").as_deref(), Ok("1") | Ok("on") | Ok("true"))
}

/// 目録から `FILEGROUPDESCRIPTORW` を組み立てる(リトルエンディアン)。
/// 相手(Explorer 等)はこれだけでドラッグの見た目と受け入れ判定ができる
pub fn descriptor_bytes(entries: &[ManifestEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(GROUP_HEADER_SIZE + entries.len() * FILEDESCRIPTORW_SIZE);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        let is_dir = e.kind == 1;
        // フォルダはサイズを持たないので FD_FILESIZE を立てない
        let mut flags = FD_ATTRIBUTES | FD_PROGRESSUI | FD_UNICODE;
        if !is_dir {
            flags |= FD_FILESIZE;
        }
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&[0u8; 16]); // clsid(未使用)
        out.extend_from_slice(&[0u8; 8]); // sizel(未使用)
        out.extend_from_slice(&[0u8; 8]); // pointl(未使用)
        let attrs = if is_dir { FILE_ATTRIBUTE_DIRECTORY } else { 0x0080 }; // FILE_ATTRIBUTE_NORMAL
        out.extend_from_slice(&attrs.to_le_bytes());
        out.extend_from_slice(&[0u8; 24]); // 3 つの FILETIME(未指定)
        out.extend_from_slice(&((e.size >> 32) as u32).to_le_bytes());
        out.extend_from_slice(&((e.size & 0xFFFF_FFFF) as u32).to_le_bytes());
        // cFileName(WCHAR[260])。UTF-16LE で詰め、残りは NUL。
        // 収まらない名前は 259 文字で切る(終端 NUL を必ず残す)
        let mut used = 0usize;
        for unit in e.rel.encode_utf16().take(MAX_FILE_NAME - 1) {
            out.extend_from_slice(&unit.to_le_bytes());
            used += 1;
        }
        out.extend(std::iter::repeat(0u8).take((MAX_FILE_NAME - used) * 2));
    }
    debug_assert_eq!(out.len(), GROUP_HEADER_SIZE + entries.len() * FILEDESCRIPTORW_SIZE);
    out
}

/// 目録の件数を読む(受信側が照合に使う。壊れた入力を弾く)
pub fn descriptor_count(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < GROUP_HEADER_SIZE {
        return None;
    }
    let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if GROUP_HEADER_SIZE + n * FILEDESCRIPTORW_SIZE != bytes.len() {
        return None;
    }
    Some(n)
}

/// 遅延提供で受け取るファイルの置き場を決める(1件分)。
/// **相対名をそのまま連結しない**: `..`・絶対パス・ドライブ名・空要素は受けない
/// (受信側の既存の無害化と同じ方針。ここを通った名前だけを staging 配下に作る)
pub fn staged_path(root: &std::path::Path, rel: &str) -> Option<std::path::PathBuf> {
    let normalized = rel.replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') || normalized.contains(':') {
        return None;
    }
    let mut out = root.to_path_buf();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

/// 「このファイルは受け取り終わったか」。フォルダ(kind=1)は存在だけでよい。
/// 途中の大きさでも読めるようにするのは IStream 側の役目で、ここは完了判定だけ
pub fn is_ready(path: &std::path::Path, want: u64, kind: u8) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => {
            if kind == 1 {
                m.is_dir()
            } else if want == 0 {
                // 0 バイトのファイルは「存在すれば完了」(空ファイルは成長しない)
                m.is_file()
            } else {
                m.is_file() && m.len() >= want
            }
        }
        Err(_) => false,
    }
}

/// 受信側が**実際に公開したパス**を探す。受信側は同名衝突で「名前 (n).ext」へ
/// ずらす(`files::unique_final`)ため、宣言名が無いときは (n) 付きを**サイズ一致で**探す。
/// 見つからなければ宣言名をそのまま返す(呼び出し側が待って期限で失敗させる)
pub fn resolve_received(root: &std::path::Path, rel: &str, size: u64, kind: u8) -> std::path::PathBuf {
    let Some(declared) = staged_path(root, rel) else {
        // 名前が不正(そもそも受信側も作らない)。呼び出し側の待ちで期限切れになる
        return root.join(rel.replace('/', "\\"));
    };
    if is_ready(&declared, size, kind) {
        return declared;
    }
    // 「stem (n).ext」を順に試す(受信側の採番は 1 から連番)
    let parent = declared.parent().unwrap_or(root);
    let stem = declared
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = declared
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    for n in 1..100u32 {
        let name = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = parent.join(name);
        if is_ready(&candidate, size, kind) {
            return candidate;
        }
    }
    declared
}

/// 受け取り完了まで待つ(期限つき)。**呼び出し側のスレッドを長時間止めない**ため、
/// 期限を必ず入れる(IStream の Read から呼ぶ)
pub fn wait_ready(
    path: &std::path::Path,
    want: u64,
    kind: u8,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if is_ready(path, want, kind) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(rel: &str, size: u64, kind: u8) -> ManifestEntry {
        ManifestEntry { rel: rel.to_string(), size, kind }
    }

    #[test]
    fn descriptor_has_the_msdn_layout() {
        let bytes = descriptor_bytes(&[e("a.txt", 1234, 0), e("sub", 0, 1)]);
        assert_eq!(bytes.len(), GROUP_HEADER_SIZE + 2 * FILEDESCRIPTORW_SIZE);
        assert_eq!(descriptor_count(&bytes), Some(2), "件数を読める");
        // 1件目の dwFlags は属性・サイズ・進行UI・Unicode
        let flags = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert_eq!(flags & FD_ATTRIBUTES, FD_ATTRIBUTES);
        assert_eq!(flags & FD_UNICODE, FD_UNICODE, "名前は UTF-16 で入れる");
        assert_eq!(flags & FD_PROGRESSUI, FD_PROGRESSUI);
        assert_eq!(flags & FD_FILESIZE, FD_FILESIZE, "ファイルはサイズを持つ");
        // 2件目(フォルダ)はサイズを持たない
        let second = GROUP_HEADER_SIZE + FILEDESCRIPTORW_SIZE;
        let f2 = u32::from_le_bytes([
            bytes[second + 4],
            bytes[second + 5],
            bytes[second + 6],
            bytes[second + 7],
        ]);
        assert_eq!(f2 & FD_FILESIZE, 0, "フォルダにサイズを宣言しない");
    }

    #[test]
    fn names_are_utf16le_and_folders_get_the_directory_attribute() {
        let bytes = descriptor_bytes(&[e("資料/a.txt", 1, 0), e("資料/空", 0, 1)]);
        // 2件目の cFileName の先頭 3 WCHAR が「資」「料」「/」
        let second = GROUP_HEADER_SIZE + FILEDESCRIPTORW_SIZE;
        let name_off = second + FILEDESCRIPTORW_SIZE - MAX_FILE_NAME * 2;
        let w = |i: usize| u16::from_le_bytes([bytes[name_off + i * 2], bytes[name_off + i * 2 + 1]]);
        assert_eq!(w(0), '資' as u16);
        assert_eq!(w(1), '料' as u16);
        assert_eq!(w(2), '/' as u16);
        assert_eq!(w(6), 0, "終端は NUL");
        // 2件目はフォルダ属性
        let attrs_off = second + 4 + 16 + 8 + 8;
        let attrs = u32::from_le_bytes([
            bytes[attrs_off],
            bytes[attrs_off + 1],
            bytes[attrs_off + 2],
            bytes[attrs_off + 3],
        ]);
        assert_eq!(attrs & FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_DIRECTORY);
    }

    #[test]
    fn a_very_long_name_is_truncated_and_still_terminated() {
        let long = "あ".repeat(400);
        let bytes = descriptor_bytes(&[e(&long, 1, 0)]);
        let name_off = GROUP_HEADER_SIZE + FILEDESCRIPTORW_SIZE - MAX_FILE_NAME * 2;
        let last = u16::from_le_bytes([
            bytes[name_off + (MAX_FILE_NAME - 1) * 2],
            bytes[name_off + (MAX_FILE_NAME - 1) * 2 + 1],
        ]);
        assert_eq!(last, 0, "終端 NUL を必ず残す");
        assert_eq!(bytes.len(), GROUP_HEADER_SIZE + FILEDESCRIPTORW_SIZE, "長さは不変");
    }

    #[test]
    fn a_broken_descriptor_is_rejected() {
        assert_eq!(descriptor_count(&[]), None);
        assert_eq!(descriptor_count(&[1, 0, 0, 0]), None, "中身が足りない");
        // 件数だけ大きく書いた偽物
        let mut bad = vec![9u8, 0, 0, 0];
        bad.extend(std::iter::repeat(0u8).take(FILEDESCRIPTORW_SIZE));
        assert_eq!(descriptor_count(&bad), None, "件数と長さの不一致を弾く");
    }

    #[test]
    fn staged_paths_reject_escapes_and_accept_nested_names() {
        let root = std::path::Path::new("C:\\stage");
        let ok = staged_path(root, "資料/sub/a.txt").expect("入れ子は通す");
        assert_eq!(ok, root.join("資料").join("sub").join("a.txt"));
        // 区切りはどちらでも受ける(目録は `/`、Windows の実パスは `\`)
        assert_eq!(
            staged_path(root, "a\\b.txt").unwrap(),
            root.join("a").join("b.txt")
        );
        for bad in ["", "/etc/passwd", "C:/Windows/x", "..", "a/../../b", "a//b", "./a"] {
            assert!(staged_path(root, bad).is_none(), "受けない: {bad}");
        }
    }

    #[test]
    fn readiness_follows_the_declared_size() {
        let dir = std::env::temp_dir().join(format!("knit-lazy-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("grow.bin");
        let _ = std::fs::remove_file(&f);
        // まだ無い
        assert!(!is_ready(&f, 4, 0));
        std::fs::write(&f, b"ab").unwrap();
        assert!(!is_ready(&f, 4, 0), "宣言より小さいうちは未完了");
        std::fs::write(&f, b"abcd").unwrap();
        assert!(is_ready(&f, 4, 0), "宣言に達したら完了(超過も可)");
        // 0 バイト宣言は存在だけで完了(空ファイルは成長しない)
        let empty = dir.join("empty.txt");
        std::fs::write(&empty, b"").unwrap();
        assert!(is_ready(&empty, 0, 0), "空ファイルで待ち続けない");
        // フォルダは存在だけでよい
        let sub = dir.join("folder");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(is_ready(&sub, 0, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn waiting_gives_up_at_the_deadline_instead_of_hanging() {
        let dir = std::env::temp_dir().join(format!("knit-lazy-wait-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("never.bin");
        let _ = std::fs::remove_file(&f);
        let started = std::time::Instant::now();
        let ok = wait_ready(&f, 10, 0, std::time::Duration::from_millis(60));
        assert!(!ok, "来なければ false");
        assert!(started.elapsed() >= std::time::Duration::from_millis(50), "期限まで待つ");
        // 途中で現れれば true
        let f2 = dir.join("appears.bin");
        std::fs::write(&f2, b"0123456789").unwrap();
        assert!(wait_ready(&f2, 10, 0, std::time::Duration::from_millis(50)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 同名衝突で「名前 (n)」へずらされた実体を見つけられる(受信側の unique_final と対)
    #[test]
    fn a_renamed_receipt_is_found_by_size() {
        let dir = std::env::temp_dir().join(format!("knit-lazy-ren-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 既存の同名ファイル(前回の受け取り)
        std::fs::write(dir.join("report.pdf"), b"old").unwrap();
        // 今回の実体は「report (1).pdf」として公開される
        std::fs::write(dir.join("report (1).pdf"), b"0123456789").unwrap();
        let got = resolve_received(&dir, "report.pdf", 10, 0);
        assert_eq!(got.file_name().unwrap(), "report (1).pdf", "サイズ一致で見つける");
        // 宣言名がそのまま使えるときは動かさない
        std::fs::write(dir.join("fresh.txt"), b"abc").unwrap();
        let same = resolve_received(&dir, "fresh.txt", 3, 0);
        assert_eq!(same.file_name().unwrap(), "fresh.txt");
        // 見つからなければ宣言名を返す(呼び出し側が期限で失敗させる)
        let missing = resolve_received(&dir, "nope.txt", 5, 0);
        assert_eq!(missing.file_name().unwrap(), "nope.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lazy_is_off_until_the_owner_opts_in() {
        // 既定は無効。実機で確かめるまで現行方式のまま(危険な既定にしない)
        assert!(!lazy_enabled(), "既定は無効");
        std::env::set_var("KNIT_LAZY", "1");
        assert!(lazy_enabled(), "明示したときだけ有効");
        std::env::set_var("KNIT_LAZY", "0");
        assert!(!lazy_enabled(), "0 は無効");
        std::env::remove_var("KNIT_LAZY");
    }
}
