//! Android へ送る仮想 HID(キーボード・マウス)の記述子と入力レポート。
//! 記述子は scrcpy 4.1(app/src/hid)が実機で使っているものに合わせた。キーボードだけは
//! JIS の ¥・_・かな・英数(使用番号 0x87〜0x91)を送れるよう、使用範囲を 0xFF まで広げている

pub const KEYBOARD_ID: u16 = 1;
pub const MOUSE_ID: u16 = 2;

/// 5 ボタン・相対 X/Y・縦ホイール・横ホイール(AC Pan)。レポートは 5 バイト
pub const MOUSE_DESC: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00, // Mouse / Pointer
    0x05, 0x09, 0x19, 0x01, 0x29, 0x05, 0x15, 0x00, 0x25, 0x01, 0x95, 0x05, 0x75, 0x01, 0x81,
    0x02, // ボタン 1〜5
    0x95, 0x01, 0x75, 0x03, 0x81, 0x01, // 余白 3 ビット
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x03,
    0x81, 0x06, // X・Y・ホイール(相対)
    0x05, 0x0C, 0x0A, 0x38, 0x02, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x01, 0x81,
    0x06, // 横ホイール
    0xC0, 0xC0,
];

/// 修飾 8 ビット・予約 1 バイト・同時押し 6 キー。LED の出力レポートを持つ
pub const KEYBOARD_DESC: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, // Keyboard
    0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81,
    0x02, // 修飾
    0x75, 0x08, 0x95, 0x01, 0x81, 0x01, // 予約
    0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x75, 0x01, 0x95, 0x05, 0x91, 0x02, // LED
    0x75, 0x03, 0x95, 0x01, 0x91, 0x01, // LED の余白
    0x05, 0x07, 0x19, 0x00, 0x29, 0xFF, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x06, 0x81,
    0x00, // キー配列
    0xC0,
];

/// Mac の仮想キーコード(kVK_*)→ HID キーボード使用番号。修飾キーは含めない
pub fn mac_kc_to_hid(kc: u16) -> Option<u8> {
    Some(match kc {
        0 => 0x04,  // A
        1 => 0x16,  // S
        2 => 0x07,  // D
        3 => 0x09,  // F
        4 => 0x0B,  // H
        5 => 0x0A,  // G
        6 => 0x1D,  // Z
        7 => 0x1B,  // X
        8 => 0x06,  // C
        9 => 0x19,  // V
        10 => 0x64, // ISO §
        11 => 0x05, // B
        12 => 0x14, // Q
        13 => 0x1A, // W
        14 => 0x08, // E
        15 => 0x15, // R
        16 => 0x1C, // Y
        17 => 0x17, // T
        18 => 0x1E, // 1
        19 => 0x1F, // 2
        20 => 0x20, // 3
        21 => 0x21, // 4
        22 => 0x23, // 6
        23 => 0x22, // 5
        24 => 0x2E, // =
        25 => 0x26, // 9
        26 => 0x24, // 7
        27 => 0x2D, // -
        28 => 0x25, // 8
        29 => 0x27, // 0
        30 => 0x30, // ]
        31 => 0x12, // O
        32 => 0x18, // U
        33 => 0x2F, // [
        34 => 0x0C, // I
        35 => 0x13, // P
        36 => 0x28, // Return
        37 => 0x0F, // L
        38 => 0x0D, // J
        39 => 0x34, // '
        40 => 0x0E, // K
        41 => 0x33, // ;
        42 => 0x31, // \
        43 => 0x36, // ,
        44 => 0x38, // /
        45 => 0x11, // N
        46 => 0x10, // M
        47 => 0x37, // .
        48 => 0x2B, // Tab
        49 => 0x2C, // Space
        50 => 0x35, // `
        51 => 0x2A, // Delete(後退)
        53 => 0x29, // Escape
        57 => 0x39, // Caps Lock
        64 => 0x6C, // F17
        65 => 0x63, // テンキー .
        67 => 0x55, // テンキー *
        69 => 0x57, // テンキー +
        71 => 0x53, // テンキー Clear(Num Lock の位置)
        72 => 0x80, // 音量+
        73 => 0x81, // 音量-
        74 => 0x7F, // ミュート
        75 => 0x54, // テンキー /
        76 => 0x58, // テンキー Enter
        78 => 0x56, // テンキー -
        79 => 0x6D, // F18
        80 => 0x6E, // F19
        81 => 0x67, // テンキー =
        82 => 0x62, // テンキー 0
        83 => 0x59, // テンキー 1
        84 => 0x5A,
        85 => 0x5B,
        86 => 0x5C,
        87 => 0x5D,
        88 => 0x5E,
        89 => 0x5F,  // テンキー 7
        90 => 0x6F,  // F20
        91 => 0x60,  // テンキー 8
        92 => 0x61,  // テンキー 9
        93 => 0x89,  // JIS ¥(International3)
        94 => 0x87,  // JIS _(International1)
        95 => 0x85,  // JIS テンキー ,
        96 => 0x3E,  // F5
        97 => 0x3F,  // F6
        98 => 0x40,  // F7
        99 => 0x3C,  // F3
        100 => 0x41, // F8
        101 => 0x42, // F9
        102 => 0x91, // JIS 英数(LANG2 → Android の EISU)
        103 => 0x44, // F11
        104 => 0x90, // JIS かな(LANG1 → Android の KANA)
        105 => 0x68, // F13
        106 => 0x6B, // F16
        107 => 0x69, // F14
        109 => 0x43, // F10
        110 => 0x65, // メニュー(Application)
        111 => 0x45, // F12
        113 => 0x6A, // F15
        114 => 0x49, // Help(Insert の位置)
        115 => 0x4A, // Home
        116 => 0x4B, // Page Up
        117 => 0x4C, // 前方削除
        118 => 0x3D, // F4
        119 => 0x4D, // End
        120 => 0x3B, // F2
        121 => 0x4E, // Page Down
        122 => 0x3A, // F1
        123 => 0x50, // ←
        124 => 0x4F, // →
        125 => 0x51, // ↓
        126 => 0x52, // ↑
        _ => return None,
    })
}

/// 修飾キー単体のキーコード(押下状態は Key のフラグで届くため、配列には載せない)
fn is_modifier_kc(kc: u16) -> bool {
    matches!(kc, 54..=56 | 58..=63)
}

/// Knit の修飾フラグ。対応は Windows 版と同じで、⌘→Ctrl・⌥→Alt・control→Meta、
/// 右⌘→右 Ctrl。cmd_alt(Mac の設定「⌘を Alt に」)なら ⌘ と ⌥ の行き先を入れ替える
#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub opt: bool,
    pub cmd: bool,
    pub shift: bool,
    pub rcmd: bool,
}

const L_CTRL: u8 = 0x01;
const L_SHIFT: u8 = 0x02;
const L_ALT: u8 = 0x04;
const L_META: u8 = 0x08;
const R_CTRL: u8 = 0x10;

fn mod_bits(m: Mods, cmd_alt: bool) -> u8 {
    let (cmd_bit, opt_bit) = if cmd_alt {
        (L_ALT, L_CTRL)
    } else {
        (L_CTRL, L_ALT)
    };
    let mut b = 0;
    if m.cmd {
        b |= cmd_bit;
    }
    if m.opt {
        b |= opt_bit;
    }
    if m.ctrl {
        b |= L_META;
    }
    if m.shift {
        b |= L_SHIFT;
    }
    if m.rcmd {
        b |= R_CTRL;
    }
    b
}

/// 押下中のキーを保持し、キーボードの入力レポート(8 バイト)を作る
#[derive(Default)]
pub struct Keyboard {
    keys: Vec<u8>,
    mods: u8,
    pub cmd_alt: bool,
}

impl Keyboard {
    /// 1 キーの押下・解放を反映する。レポートに変化がなければ None
    pub fn key(&mut self, kc: u16, down: bool, m: Mods) -> Option<[u8; 8]> {
        let before = (self.mods, self.keys.clone());
        self.mods = mod_bits(m, self.cmd_alt);
        if !is_modifier_kc(kc) {
            if let Some(u) = mac_kc_to_hid(kc) {
                self.keys.retain(|&k| k != u);
                if down {
                    // 7 キー目以降は最も古い押下を押し出す(HID の 6 キー制限)
                    if self.keys.len() == 6 {
                        self.keys.remove(0);
                    }
                    self.keys.push(u);
                }
            }
        }
        if (self.mods, &self.keys) == (before.0, &before.1) {
            return None;
        }
        Some(self.report())
    }

    /// 全キーを離した状態へ戻す(Mac が制御を取り戻した時・切断時)
    pub fn release_all(&mut self) -> [u8; 8] {
        self.keys.clear();
        self.mods = 0;
        self.report()
    }

    fn report(&self) -> [u8; 8] {
        let mut r = [0u8; 8];
        r[0] = self.mods;
        for (i, k) in self.keys.iter().enumerate() {
            r[2 + i] = *k;
        }
        r
    }
}

/// Android のポインタ位置を推定しながら、Knit の絶対座標を相対移動のレポートへ直す。
///
/// UHID のマウスは相対移動しか送れず、Android 側は加速をかける。そのため推定位置は
/// 実際のポインタとずれるが、画面端では両方とも端で止まる。Mac が推定位置を端に置いた時は
/// 端へ余分に押し込み、実際のポインタも端へ揃える(戻る判定は Mac 側の推定位置で行う)
pub struct Pointer {
    w: f64,
    h: f64,
    cur: (f64, f64),
    frac: (f64, f64),
    buttons: u8,
    /// 推定 1px あたりに送るカウント数(Android 側の倍率の逆数。KNIT_ANDROID_GAIN で調整)
    gain: f64,
}

const EDGE_PUSH: i32 = 127;

impl Pointer {
    pub fn new(w: f64, h: f64, gain: f64) -> Self {
        Self {
            w: w.max(1.0),
            h: h.max(1.0),
            cur: (w / 2.0, h / 2.0),
            frac: (0.0, 0.0),
            buttons: 0,
            gain: if gain > 0.0 { gain } else { 1.0 },
        }
    }

    /// Knit の MouseAbs(0..1 正規化)を反映する
    pub fn move_abs(&mut self, nx: f64, ny: f64) -> Vec<[u8; 5]> {
        let t = (nx.clamp(0.0, 1.0) * self.w, ny.clamp(0.0, 1.0) * self.h);
        let dx = (t.0 - self.cur.0) * self.gain + self.frac.0;
        let dy = (t.1 - self.cur.1) * self.gain + self.frac.1;
        let (mut ix, mut iy) = (dx.trunc() as i32, dy.trunc() as i32);
        self.frac = (dx - ix as f64, dy - iy as f64);
        self.cur = t;
        // Mac は相手画面の推定位置を [0, 幅-2] に収める。端に置かれたら実物も端へ押し込む
        if t.0 <= 1.0 {
            ix -= EDGE_PUSH;
        } else if t.0 >= self.w - 3.0 {
            ix += EDGE_PUSH;
        }
        if t.1 <= 1.0 {
            iy -= EDGE_PUSH;
        } else if t.1 >= self.h - 3.0 {
            iy += EDGE_PUSH;
        }
        self.rel(ix, iy)
    }

    /// 相対移動(Knit の相対モード)
    pub fn move_rel(&mut self, dx: f64, dy: f64) -> Vec<[u8; 5]> {
        let dx = dx * self.gain + self.frac.0;
        let dy = dy * self.gain + self.frac.1;
        let (ix, iy) = (dx.trunc() as i32, dy.trunc() as i32);
        self.frac = (dx - ix as f64, dy - iy as f64);
        self.cur.0 = (self.cur.0 + ix as f64 / self.gain).clamp(0.0, self.w);
        self.cur.1 = (self.cur.1 + iy as f64 / self.gain).clamp(0.0, self.h);
        self.rel(ix, iy)
    }

    /// 画面を移ってきた時の位置合わせ。入ってきた辺へ押し付けて実物を揃え、
    /// 辺に沿った向きは推定位置からの差分で動かす
    pub fn warp(&mut self, nx: f64, ny: f64) -> Vec<[u8; 5]> {
        let t = (nx.clamp(0.0, 1.0) * self.w, ny.clamp(0.0, 1.0) * self.h);
        let pin = |v: f64, len: f64| -> Option<(f64, i32)> {
            if v <= len * 0.02 {
                Some((0.0, -1))
            } else if v >= len * 0.98 {
                Some((len - 2.0, 1))
            } else {
                None
            }
        };
        let full = |len: f64| ((len * self.gain) / 127.0).ceil() as i32 * 127 + 127;
        let mut out = Vec::new();
        match (pin(t.0, self.w), pin(t.1, self.h)) {
            (Some((x, s)), _) => {
                out.extend(self.rel(s * full(self.w), 0));
                self.cur.0 = x;
            }
            (None, Some((y, s))) => {
                out.extend(self.rel(0, s * full(self.h)));
                self.cur.1 = y;
            }
            (None, None) => {}
        }
        self.frac = (0.0, 0.0);
        out.extend(self.move_abs(nx, ny));
        out
    }

    pub fn button(&mut self, btn: u8, down: bool) -> Option<[u8; 5]> {
        let bit = match btn {
            0 => 0x01,
            1 => 0x02,
            2 => 0x04,
            3 => 0x08,
            4 => 0x10,
            _ => return None,
        };
        let before = self.buttons;
        if down {
            self.buttons |= bit;
        } else {
            self.buttons &= !bit;
        }
        (before != self.buttons).then_some([self.buttons, 0, 0, 0, 0])
    }

    /// 推定位置(inject 系メッセージの位置指定に使う)
    pub fn pos(&self) -> (i32, i32) {
        (self.cur.0.round() as i32, self.cur.1.round() as i32)
    }

    /// 画面サイズ(inject 系メッセージの位置指定に使う)
    pub fn size(&self) -> (u16, u16) {
        (self.w.min(u16::MAX as f64) as u16, self.h.min(u16::MAX as f64) as u16)
    }

    pub fn release_all(&mut self) -> [u8; 5] {
        self.buttons = 0;
        [0, 0, 0, 0, 0]
    }

    fn rel(&self, mut dx: i32, mut dy: i32) -> Vec<[u8; 5]> {
        let mut out = Vec::new();
        while dx != 0 || dy != 0 {
            let sx = dx.clamp(-127, 127);
            let sy = dy.clamp(-127, 127);
            out.push([self.buttons, sx as i8 as u8, sy as i8 as u8, 0, 0]);
            dx -= sx;
            dy -= sy;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(reports: &[[u8; 5]]) -> (i32, i32) {
        reports.iter().fold((0, 0), |(x, y), r| {
            (x + r[1] as i8 as i32, y + r[2] as i8 as i32)
        })
    }

    #[test]
    fn letters_and_modifiers_map_like_the_windows_side() {
        let mut kb = Keyboard::default();
        let cmd = Mods {
            cmd: true,
            ..Default::default()
        };
        // ⌘ 単体の押下は修飾だけ変わる(⌘→左 Ctrl)
        assert_eq!(kb.key(55, true, cmd), Some([L_CTRL, 0, 0, 0, 0, 0, 0, 0]));
        // ⌘C → Ctrl+C
        assert_eq!(kb.key(8, true, cmd), Some([L_CTRL, 0, 0x06, 0, 0, 0, 0, 0]));
        assert_eq!(kb.key(8, false, cmd), Some([L_CTRL, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(kb.key(55, false, Mods::default()), Some([0; 8]));
        // 変化のないイベントはレポートを出さない
        assert_eq!(kb.key(55, false, Mods::default()), None);
    }

    #[test]
    fn cmd_alt_swaps_cmd_and_option_targets() {
        let mut kb = Keyboard {
            cmd_alt: true,
            ..Default::default()
        };
        let m = Mods {
            cmd: true,
            ..Default::default()
        };
        assert_eq!(kb.key(55, true, m).unwrap()[0], L_ALT);
    }

    #[test]
    fn jis_keys_reach_android_kana_and_eisu() {
        assert_eq!(mac_kc_to_hid(104), Some(0x90));
        assert_eq!(mac_kc_to_hid(102), Some(0x91));
        assert_eq!(mac_kc_to_hid(93), Some(0x89));
        // 修飾キーは配列に載せない
        let mut kb = Keyboard::default();
        let r = kb.key(
            56,
            true,
            Mods {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(r, Some([L_SHIFT, 0, 0, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn seventh_key_pushes_out_the_oldest() {
        let mut kb = Keyboard::default();
        for kc in [0u16, 1, 2, 3, 4, 5] {
            kb.key(kc, true, Mods::default());
        }
        let r = kb.key(6, true, Mods::default()).unwrap();
        assert_eq!(&r[2..], &[0x16, 0x07, 0x09, 0x0B, 0x0A, 0x1D]);
    }

    #[test]
    fn abs_moves_become_relative_reports_in_127_steps() {
        let mut p = Pointer::new(1000.0, 800.0, 1.0);
        let r = p.move_abs(0.8, 0.5); // 500→800
        assert_eq!(sum(&r), (300, 0));
        assert!(r.iter().all(|x| (x[1] as i8).unsigned_abs() <= 127));
    }

    #[test]
    fn reaching_an_edge_pushes_the_real_pointer_into_it() {
        let mut p = Pointer::new(1000.0, 800.0, 1.0);
        let r = p.move_abs(0.0, 0.5);
        assert_eq!(sum(&r), (-500 - EDGE_PUSH, 0));
        // 端に留まる間も押し込み続ける(加速でずれた実物を揃えるため)
        let r = p.move_abs(0.0, 0.5);
        assert_eq!(sum(&r), (-EDGE_PUSH, 0));
    }

    #[test]
    fn sub_pixel_moves_carry_over() {
        let mut p = Pointer::new(1000.0, 1000.0, 1.0);
        let mut total = 0;
        for i in 1..=10 {
            total += sum(&p.move_abs(0.5 + i as f64 * 0.0004, 0.5)).0;
        }
        assert_eq!(total, 4);
    }

    #[test]
    fn warp_from_the_left_edge_pins_x_then_moves_y() {
        let mut p = Pointer::new(1000.0, 800.0, 1.0);
        let r = p.warp(0.0, 0.25);
        let (x, y) = sum(&r);
        assert!(x <= -1000, "左端へ押し付ける: {x}");
        assert_eq!(y, -200); // 400→200
    }

    #[test]
    fn buttons_are_held_across_moves() {
        let mut p = Pointer::new(100.0, 100.0, 1.0);
        assert_eq!(p.button(0, true), Some([1, 0, 0, 0, 0]));
        assert_eq!(p.button(0, true), None);
        assert_eq!(p.move_abs(0.6, 0.5)[0][0], 1);
    }
}
