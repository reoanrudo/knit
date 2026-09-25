// 共通プロトコル定義(JSON Lines over TCP)
pub mod proto {
    use serde::{Deserialize, Serialize};

    pub const PORT: u16 = 24900;
    pub const VERSION: u32 = 2; // 2: Clip同期・Return ny 追加(旧バイナリ混在の早期検知用)

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
        #[serde(rename = "focus")]
        Focus { title: String },
        #[serde(rename = "minimize")]
        Minimize { title: String },
        /// カーソル絶対ワープ(0..1 正規化。切替時に相手画面の対応位置へ飛ばす)
        #[serde(rename = "warp")]
        Warp { nx: f64, ny: f64 },
        #[serde(rename = "ping")]
        Ping,
        #[serde(rename = "pong")]
        Pong,
        #[serde(rename = "bye")]
        Bye,
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
