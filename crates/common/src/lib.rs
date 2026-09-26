// 共通プロトコル定義(JSON Lines over TCP)
pub mod envutil {
    //! 設定値の参照: 環境変数 > 実行ファイル同階層の .env > ~/.config/tsunagu/env。
    //! 配布形態(.app バンドル埋め込み / exe 同梱 .env / ホーム設定)のどれでも
    //! 同一コードで動かすための仕組み。KEY=VALUE 形式(1行1エントリ、# はコメント)。
    //! 旧名称(v0.7 以前の seamless-desk)の環境変数・設定パスもフォールバックで
    //! 読むため、既存環境を書き換えずにそのまま移行できる。

    use std::sync::OnceLock;

    /// 旧名称(v0.7 以前)へのキー変換。
    /// TSUNAGU_TOKEN ← SEAMLESS_DESK_TOKEN、TSUNAGU_X ← SEAMLESS_X
    fn legacy_key(key: &str) -> String {
        if key == "TSUNAGU_TOKEN" {
            return "SEAMLESS_DESK_TOKEN".to_string();
        }
        match key.strip_prefix("TSUNAGU_") {
            Some(rest) => format!("SEAMLESS_{rest}"),
            None => String::new(),
        }
    }

    fn entries() -> &'static Vec<(String, String)> {
        static E: OnceLock<Vec<(String, String)>> = OnceLock::new();
        E.get_or_init(|| {
            let mut v = Vec::new();
            let mut paths = Vec::new();
            if let Ok(exe) = std::env::current_exe() {
                if let Some(d) = exe.parent() {
                    paths.push(d.join(".env"));
                    // .app バンドル配布用: Contents/Resources/.env
                    // (MacOS/ 内に置くと codesign の署名対象になって失敗するため)
                    if let Some(res) = d.parent().map(|p| p.join("Resources/.env")) {
                        paths.push(res);
                    }
                }
            }
            for key in ["HOME", "USERPROFILE"] {
                if let Some(home) = std::env::var_os(key) {
                    let cfg = std::path::Path::new(&home).join(".config");
                    paths.push(cfg.join("tsunagu/env"));
                    // 旧名称時代の設定パス(v0.7 からの移行措置)
                    paths.push(cfg.join("seamless-desk/env"));
                }
            }
            for p in paths {
                let Ok(s) = std::fs::read_to_string(&p) else { continue };
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

    /// 環境変数を第一優先とし、未設定なら設定ファイル群から検索する。
    /// 旧名称のキー(SEAMLESS_*)も最後に確認する(v0.7 設定からの移行)
    pub fn get(key: &str) -> Option<String> {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return Some(v);
            }
        }
        let legacy = legacy_key(key);
        let hit = entries()
            .iter()
            .find(|(k, _)| k == key)
            .or_else(|| {
                if legacy.is_empty() {
                    None
                } else {
                    entries().iter().find(|(k, _)| *k == legacy)
                }
            })
            .map(|(_, v)| v.clone());
        // 旧名称の環境変数も受け入れる(スクリプト側の書き換え漏れ保険)
        if hit.is_none() && !legacy.is_empty() {
            if let Ok(v) = std::env::var(&legacy) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        hit
    }
}

pub mod proto {
    use serde::{Deserialize, Serialize};

    pub const PORT: u16 = 24900;
    /// プロトコル版。9: Leave 追加・Focus/Minimize 削除・版交渉(MIN_VERSION)導入
    pub const VERSION: u32 = 9;
    /// 接続を受け入れる最小の相手版。新しいメッセージは未知として無視される
    /// (decode が None を返す)ため、MIN_VERSION 以上なら新旧混在でも通信できる。
    /// 片側だけ更新された状態で接続拒否が続く事故を防ぐ
    pub const MIN_VERSION: u32 = 9;

    /// 相手の版を受け入れてよいか
    pub fn compatible(peer: u32) -> bool {
        peer >= MIN_VERSION
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "t")]
    pub enum Msg {
        #[serde(rename = "hello")]
        Hello {
            ver: u32,
            name: String,
            token: String,
            /// 送信側の画面幅/高さ(px)。スケール自動算出と絶対座標送信に使う
            #[serde(default)]
            w: i32,
            #[serde(default)]
            h: i32,
        },
        #[serde(rename = "hello_ok")]
        HelloOk { name: String, w: i32, h: i32 },
        #[serde(rename = "key")]
        Key {
            kc: u16,
            down: bool,
            ctrl: bool,
            opt: bool,
            cmd: bool,
            shift: bool,
            /// 翻訳済みキー(⌘]→Tab 等)。Win 側の ⌘Tab→Alt+Tab 変換など
            /// 「生の Mac 入力」前提の特殊処理を適用しない
            #[serde(default)]
            tr: bool,
        },
        #[serde(rename = "mouse_move")]
        MouseMove { dx: f64, dy: f64 },
        /// カーソル絶対位置(0..1 正規化)。Windows 側は MOUSEEVENTF_ABSOLUTE で注入し、
        /// ポインタ加速曲線を通さず Mac の速度感をそのまま再現する
        #[serde(rename = "mouse_abs")]
        MouseAbs { nx: f64, ny: f64 },
        #[serde(rename = "mouse_btn")]
        MouseButton { btn: u8, down: bool },
        #[serde(rename = "scroll")]
        Scroll { dx: f64, dy: f64 },
        /// Windows 左端到達による復帰通知。ny = 復帰時のカーソル高さ(0..1、Mac 側復帰位置へ反映)
        #[serde(rename = "return")]
        Return {
            #[serde(default)]
            ny: f64,
        },
        /// クリップボード同期(プレーンテキスト)
        #[serde(rename = "clip")]
        Clip { text: String },
        /// クリップボード同期(バイナリ、base64)。kind 例: "image/dib"
        #[serde(rename = "clip_data")]
        ClipData { kind: String, data: String },
        /// Mac が制御を取り戻した(Windows を離れた)。Windows は押下中の全キー・
        /// ボタン・Alt+Tab を解放する。ホットキー/Mac 内完結の左端復帰/切断など
        /// Windows が自力で検知できない離脱経路のための後片付け合図
        #[serde(rename = "leave")]
        Leave,
        /// カーソル絶対ワープ(0..1 正規化。切替時に相手画面の対応位置へ飛ばす)
        #[serde(rename = "warp")]
        Warp { nx: f64, ny: f64 },
        #[serde(rename = "ping")]
        Ping {
            /// 送信時刻(unix ms)。Pong にエコーバックされ RTT 測定に使う
            #[serde(default)]
            ts: u64,
        },
        #[serde(rename = "pong")]
        Pong {
            #[serde(default)]
            ts: u64,
        },
        /// 設定同期: ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)と
        /// Windows スピーカーのミュート(true=接続中ミュート=Mac のみ発音)。
        /// 接続確立時とメニュー切替時に Mac→Windows へ送る
        #[serde(rename = "cfg")]
        Cfg {
            cmd_alt: bool,
            #[serde(default)]
            spk_mute: bool,
            /// Windows 画面の位置(0=Macの右/1=左/2=上/3=下。Deskflow の links 相当)
            #[serde(default)]
            side: u8,
            /// クリップボード共有。false の間は Windows 側も送らない(相手任せにしない)
            #[serde(default = "default_true")]
            clip: bool,
        },
        /// Windows の音量制御(0=up / 1=down / 2=ミュート)。Mac メニューから送る
        #[serde(rename = "vol")]
        Vol { op: u8 },
        /// 接続品質通知: Mac が測定した RTT(ms)を Windows 側の表示へ回す
        #[serde(rename = "stat")]
        Stat { rtt: u64 },
        /// ファイル送信(双方向)。begin → chunk(base64, 生3MB以下) → end の順。
        /// 受信側は Downloads\Tsunagu へ保存しファイル参照をクリップボードへ
        #[serde(rename = "file_begin")]
        FileBegin { name: String, size: u64 },
        #[serde(rename = "file_chunk")]
        FileChunk { data: String },
        #[serde(rename = "file_end")]
        FileEnd,
        /// ファイル一括送信の終了合図(全ファイルの FileEnd 後に 1 回)。
        /// 受信側はこの時点でクリップボードへファイル参照を載せ通知する
        #[serde(rename = "file_batch_end")]
        FileBatchEnd,
        /// ファイル掴みドラッグ越境の開始合図。Mac が Finder 等のファイルドラッグ
        /// 中に境界を越えた時に FileBegin 群の前に送る。受信側は続くファイル群を
        /// 「ドロップ用」(クリップボードではなく OLE ドラッグで渡す)と扱う
        #[serde(rename = "file_drop_begin")]
        FileDropBegin,
        /// ファイル掴みドラッグ越域の転送完了合図(FileBatchEnd の後)。
        /// 受信側はこの時点で押下中ボタンの継続を前提に DoDragDrop を開始する
        #[serde(rename = "file_drop_end")]
        FileDropEnd,
        #[serde(rename = "bye")]
        Bye,
    }

    fn default_true() -> bool {
        true
    }

    pub fn encode(msg: &Msg) -> String {
        let mut s = serde_json::to_string(msg).unwrap_or_default();
        s.push('\n');
        s
    }

    pub fn decode(line: &str) -> Option<Msg> {
        serde_json::from_str(line.trim()).ok()
    }
}

pub mod files {
    //! 受信ファイルの保存(Mac/Windows 共通)。名前の無害化と同名回避を一箇所に置き、
    //! 両側で規則が食い違う(Windows だけ上書きしていた)事故を防ぐ
    use std::path::{Path, PathBuf};

    /// 1 ファイルの受信上限(送信側の合計上限と同じ)
    pub const MAX_FILE: u64 = 200 * 1024 * 1024;

    /// 相手から届いたファイル名を、どちらの OS でも安全な単一の名前へ変換する。
    /// パス区切り・予約文字・制御文字は '_'、先頭末尾の '.' と空白は除去、
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
        s = s.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
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
            let value = format!("0081;{secs:x};Tsunagu;");
            let Ok(p) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
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

    /// dir 内に新規ファイルを作る。同名があれば「名前 (n).拡張子」で回避する
    pub fn create_unique(dir: &Path, name: &str) -> Option<(std::fs::File, PathBuf)> {
        std::fs::create_dir_all(dir).ok()?;
        let base = sanitize(name);
        let p = Path::new(&base);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        for i in 0..1000u32 {
            let cand = if i == 0 { dir.join(&base) } else { dir.join(format!("{stem} ({i}){ext}")) };
            if let Ok(f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&cand) {
                mark_untrusted(&cand);
                return Some((f, cand));
            }
        }
        None
    }
}

pub mod b64 {
    /// 小さな base64 実装(依存追加なし。クリップボード画像の運搬用)
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const R: [u8; 256] = {
        let mut t = [255u8; 256];
        let mut i = 0;
        while i < 64 {
            t[T[i] as usize] = i as u8;
            i += 1;
        }
        t
    };

    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
        for c in data.chunks(3) {
            let b = [*c.first().unwrap_or(&0), *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(T[(n >> 18 & 63) as usize] as char);
            out.push(T[(n >> 12 & 63) as usize] as char);
            out.push(if c.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
            out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
        }
        out
    }

    pub fn decode(s: &str) -> Option<Vec<u8>> {
        let b: Vec<u8> = s.bytes().filter(|c| *c != b'\n' && *c != b'\r').collect();
        if b.len() % 4 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(b.len() / 4 * 3);
        for c in b.chunks(4) {
            let mut n: u32 = 0;
            let mut pad = 0;
            for (i, ch) in c.iter().enumerate() {
                if *ch == b'=' {
                    n <<= 6;
                    pad += 1;
                } else {
                    let v = R[*ch as usize];
                    if v == 255 {
                        return None;
                    }
                    n = (n << 6) | v as u32;
                    let _ = i;
                }
            }
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Some(out)
    }
}

pub mod keymap {
    /// Mac keycode(HIToolbox)→ Windows 仮想キーコード(VK)
    pub fn mac_kc_to_win_vk(kc: u16) -> Option<u16> {
        let vk = match kc {
            // アルファベット(Mac keycode はレイアウト依存しない物理キー)
            0 => 0x41,   // A
            11 => 0x42,  // B
            8 => 0x43,   // C
            2 => 0x44,   // D
            14 => 0x45,  // E
            3 => 0x46,   // F
            5 => 0x47,   // G
            4 => 0x48,   // H
            34 => 0x49,  // I
            38 => 0x4A,  // J
            40 => 0x4B,  // K
            37 => 0x4C,  // L
            46 => 0x4D,  // M
            45 => 0x4E,  // N
            31 => 0x4F,  // O
            35 => 0x50,  // P
            12 => 0x51,  // Q
            15 => 0x52,  // R
            1 => 0x53,   // S
            17 => 0x54,  // T
            32 => 0x55,  // U
            9 => 0x56,   // V
            13 => 0x57,  // W
            7 => 0x58,   // X
            16 => 0x59,  // Y
            6 => 0x5A,   // Z
            // 数字row
            18 => 0x31, // 1
            19 => 0x32,
            20 => 0x33,
            21 => 0x34,
            23 => 0x35,
            22 => 0x36,
            26 => 0x37,
            28 => 0x38,
            25 => 0x39,
            29 => 0x30, // 0
            // 記号
            33 => 0xDB, // [
            30 => 0xDD, // ]
            39 => 0xBA, // ;
            41 => 0xDE, // '
            42 => 0xDC, // \
            43 => 0xBC, // ,
            47 => 0xBE, // .
            44 => 0xBF, // /
            50 => 0xC0, // `
            93 => 0xDC, // ¥(Mac JIS)→ Win バックスラッシュ/円記号
            27 => 0xBD, // -(US)/ー(JIS 長音)→ Win -[OEM_MINUS]
            94 => 0xBD, // _(Mac JIS)→ Win -
            // 制御・編集
            36 => 0x0D, // Return
            48 => 0x09, // Tab
            49 => 0x20, // Space
            51 => 0x08, // Delete(Backspace)
            53 => 0x1B, // Escape
            117 => 0x2E, // Forward Delete
            115 => 0x24, // Home
            119 => 0x23, // End
            116 => 0x21, // PageUp
            121 => 0x22, // PageDown
            123 => 0x25, // Left
            124 => 0x27, // Right
            125 => 0x28, // Down
            126 => 0x26, // Up
            // Fキー
            122 => 0x70, // F1
            120 => 0x71, // F2
            99 => 0x72,  // F3
            118 => 0x73, // F4
            96 => 0x74,  // F5
            97 => 0x75,  // F6
            98 => 0x76,  // F7
            100 => 0x77, // F8
            101 => 0x78, // F9
            109 => 0x79, // F10
            103 => 0x7A, // F11
            111 => 0x7B, // F12
            // テンキー
            82 => 0x60, // Num0
            83 => 0x61,
            84 => 0x62,
            85 => 0x63,
            86 => 0x64,
            87 => 0x65,
            88 => 0x66,
            89 => 0x67,
            91 => 0x68,
            92 => 0x69, // Num9
            65 => 0x6E, // Num .
            67 => 0x6A, // Num *
            69 => 0x6B, // Num +
            78 => 0x6D, // Num -
            75 => 0x6F, // Num /
            71 => 0x0C, // Clear
            76 => 0x0D, // テンキー Enter(Win 側で拡張キーフラグを付けて区別する)
            57 => 0x14, // Caps Lock(Mac 側は押下ごとに down+up の組で送る)
            114 => 0x2D, // Help(Mac の Ins 位置)→ Insert
            // F13〜F20(F13 は既定ホットキーのため通常は Mac 側で握られる)
            105 => 0x7C,
            107 => 0x7D,
            113 => 0x7E,
            106 => 0x7F,
            64 => 0x80,
            79 => 0x81,
            80 => 0x82,
            90 => 0x83,
            _ => return None,
        };
        Some(vk)
    }
}

pub mod charmap {
    /// 送信テスト用: Mac keycode → 表示文字(英数字・記号のみ)
    pub fn mac_kc_to_char(kc: u16) -> Option<char> {
        let c = match kc {
            0 => 'A', 11 => 'B', 8 => 'C', 2 => 'D', 14 => 'E', 3 => 'F', 5 => 'G',
            4 => 'H', 34 => 'I', 38 => 'J', 40 => 'K', 37 => 'L', 46 => 'M', 45 => 'N',
            31 => 'O', 35 => 'P', 12 => 'Q', 15 => 'R', 1 => 'S', 17 => 'T', 32 => 'U',
            9 => 'V', 13 => 'W', 7 => 'X', 16 => 'Y', 6 => 'Z',
            18 => '1', 19 => '2', 20 => '3', 21 => '4', 23 => '5', 22 => '6',
            26 => '7', 28 => '8', 25 => '9', 29 => '0',
            39 => ';', 41 => '\'', 43 => ',', 47 => '.', 44 => '/', 33 => '[', 30 => ']',
            49 => ' ', 36 => '\n', 48 => '\t',
            _ => return None,
        };
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::keymap::mac_kc_to_win_vk;
    use super::proto::*;

    #[test]
    fn keymap_covers_full_keyboard_and_numpad_enter() {
        assert_eq!(mac_kc_to_win_vk(76), Some(0x0D));
        assert_eq!(mac_kc_to_win_vk(57), Some(0x14));
        assert_eq!(mac_kc_to_win_vk(114), Some(0x2D));
        assert_eq!(mac_kc_to_win_vk(90), Some(0x83));
        // 数字row の取り違え実績(21=4, 23=5)の回帰防止
        assert_eq!(mac_kc_to_win_vk(21), Some(0x34));
        assert_eq!(mac_kc_to_win_vk(23), Some(0x35));
        assert_eq!(mac_kc_to_win_vk(200), None);
    }

    #[test]
    fn received_file_names_are_neutralized() {
        use super::files::sanitize;
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize("a\\b:c.txt"), "a_b_c.txt");
        assert_eq!(sanitize("  .hidden  "), "hidden");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("com1"), "_com1");
        assert_eq!(sanitize("console.txt"), "console.txt");
        assert_eq!(sanitize(""), "file");
        assert_eq!(sanitize("報告書.pdf"), "報告書.pdf");
    }

    #[test]
    fn same_name_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("tsunagu-files-{}", std::process::id()));
        let (_, a) = super::files::create_unique(&dir, "x.txt").unwrap();
        let (_, b) = super::files::create_unique(&dir, "x.txt").unwrap();
        assert_ne!(a, b);
        assert!(b.to_string_lossy().ends_with("x (1).txt"));
        #[cfg(target_os = "macos")]
        {
            let out = std::process::Command::new("xattr").arg("-p").arg("com.apple.quarantine").arg(&a).output().unwrap();
            assert!(String::from_utf8_lossy(&out.stdout).contains(";Tsunagu;"), "quarantine 属性が付いていない");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_negotiation_accepts_same_or_newer() {
        assert!(compatible(VERSION));
        assert!(compatible(VERSION + 5));
        assert!(!compatible(MIN_VERSION - 1));
    }

    #[test]
    fn unknown_message_is_ignored_not_fatal() {
        assert!(decode("{\"t\":\"future_feature\",\"x\":1}").is_none());
        assert!(matches!(decode(&encode(&Msg::Leave)), Some(Msg::Leave)));
        assert!(matches!(decode("{\"t\":\"return\"}"), Some(Msg::Return { .. })));
    }
}
