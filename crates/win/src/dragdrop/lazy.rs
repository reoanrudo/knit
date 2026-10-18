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
