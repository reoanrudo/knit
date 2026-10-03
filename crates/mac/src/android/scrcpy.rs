//! scrcpy サーバー(端末側で app_process として動く部品)の制御チャネルの形式。
//! scrcpy 4.1 の ControlMessageReader / DeviceMessageWriter に合わせている。
//! 数値はすべてビッグエンディアン。サーバーは起動引数の版がサーバー自身の版と
//! 一致しないと起動しないため、版の違うサーバーとは組み合わせない

use std::io::{self, Read};

const TYPE_INJECT_KEYCODE: u8 = 0;
const TYPE_INJECT_TOUCH_EVENT: u8 = 2;
const TYPE_INJECT_SCROLL_EVENT: u8 = 3;
const TYPE_SET_CLIPBOARD: u8 = 9;
const TYPE_UHID_CREATE: u8 = 12;
const TYPE_UHID_INPUT: u8 = 13;
const TYPE_UHID_DESTROY: u8 = 14;

/// サーバーが受け付ける 1 メッセージの上限(256KiB)から、クリップボード設定の
/// 見出し(種別 1・連番 8・貼り付け 1・長さ 4 バイト)を引いたもの
pub const CLIPBOARD_TEXT_MAX: usize = (1 << 18) - 14;

/// Android の KeyEvent.KEYCODE_*(音量操作に使う)
pub const KEYCODE_VOLUME_UP: u32 = 24;
pub const KEYCODE_VOLUME_DOWN: u32 = 25;
pub const KEYCODE_VOLUME_MUTE: u32 = 164;

pub fn inject_keycode(down: bool, keycode: u32) -> Vec<u8> {
    inject_keycode_with_meta(down, keycode, 0)
}

pub fn inject_keycode_with_meta(down: bool, keycode: u32, meta: u32) -> Vec<u8> {
    let mut m = vec![TYPE_INJECT_KEYCODE, if down { 0 } else { 1 }];
    m.extend_from_slice(&keycode.to_be_bytes());
    m.extend_from_slice(&0u32.to_be_bytes()); // repeat
    m.extend_from_slice(&meta.to_be_bytes());
    m
}

/// スクロール注入(scrcpy 4.1 ControlMessageReader.parseInjectScrollEvent と同じ形式:
/// type 3 + 位置 x/y(i32)・画面 w/h(u16)+ 横/縦(i16 固定小数点、サーバー側で
/// 16 を掛けて [-16,16] のスクロール量へ)+ buttons i32)。
/// h/v は 1.0 が約 1 ノッチ。video=false 起動ではサーバーが位置を生の画面座標として
/// そのまま使う(PositionMapper 未設定のため。イベントは破棄されない)
pub fn inject_scroll(x: i32, y: i32, sw: u16, sh: u16, h: f64, v: f64) -> Vec<u8> {
    // デコードは raw/32767*16 のため、1.0 = raw 2048。範囲は [-16, 16]
    let enc = |s: f64| -> i16 {
        (s.clamp(-16.0, 16.0) * 2048.0).round().clamp(-32768.0, 32767.0) as i16
    };
    let mut m = vec![TYPE_INJECT_SCROLL_EVENT];
    m.extend_from_slice(&x.to_be_bytes());
    m.extend_from_slice(&y.to_be_bytes());
    m.extend_from_slice(&sw.to_be_bytes());
    m.extend_from_slice(&sh.to_be_bytes());
    m.extend_from_slice(&enc(h).to_be_bytes());
    m.extend_from_slice(&enc(v).to_be_bytes());
    m.extend_from_slice(&0i32.to_be_bytes()); // buttons(押下中ボタンの再現はしない)
    m
}

pub fn inject_touch(action: u8, pointer: u64, x: i32, y: i32, sw: u16, sh: u16) -> Vec<u8> {
    let mut m = vec![TYPE_INJECT_TOUCH_EVENT, action];
    m.extend_from_slice(&pointer.to_be_bytes());
    m.extend_from_slice(&x.to_be_bytes());
    m.extend_from_slice(&y.to_be_bytes());
    m.extend_from_slice(&sw.to_be_bytes());
    m.extend_from_slice(&sh.to_be_bytes());
    m.extend_from_slice(&(if action == 1 {0u16} else {u16::MAX}).to_be_bytes());
    m.extend_from_slice(&0u32.to_be_bytes());
    m.extend_from_slice(&0u32.to_be_bytes());
    m
}

pub fn set_clipboard(text: &str) -> Vec<u8> {
    let raw = truncate_utf8(text.as_bytes(), CLIPBOARD_TEXT_MAX);
    let mut m = vec![TYPE_SET_CLIPBOARD];
    m.extend_from_slice(&0u64.to_be_bytes()); // 連番 0 = 受領確認を求めない
    m.push(0); // 貼り付けまではしない
    m.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    m.extend_from_slice(raw);
    m
}

pub fn uhid_create(id: u16, name: &str, desc: &[u8]) -> Vec<u8> {
    let name = truncate_utf8(name.as_bytes(), 255);
    let mut m = vec![TYPE_UHID_CREATE];
    m.extend_from_slice(&id.to_be_bytes());
    m.extend_from_slice(&0u16.to_be_bytes()); // vendor
    m.extend_from_slice(&0u16.to_be_bytes()); // product
    m.push(name.len() as u8);
    m.extend_from_slice(name);
    m.extend_from_slice(&(desc.len() as u16).to_be_bytes());
    m.extend_from_slice(desc);
    m
}

pub fn uhid_input(id: u16, data: &[u8]) -> Vec<u8> {
    let mut m = vec![TYPE_UHID_INPUT];
    m.extend_from_slice(&id.to_be_bytes());
    m.extend_from_slice(&(data.len() as u16).to_be_bytes());
    m.extend_from_slice(data);
    m
}

pub fn uhid_destroy(id: u16) -> Vec<u8> {
    let mut m = vec![TYPE_UHID_DESTROY];
    m.extend_from_slice(&id.to_be_bytes());
    m
}

/// UTF-8 の文字の途中で切らないよう、max バイト以下の境界で切り詰める
fn truncate_utf8(b: &[u8], max: usize) -> &[u8] {
    if b.len() <= max {
        return b;
    }
    let mut end = max;
    while end > 0 && (b[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    &b[..end]
}

/// 端末から届くメッセージ
#[derive(Debug, PartialEq)]
pub enum DeviceMsg {
    /// 端末のクリップボードが変わった
    Clipboard(String),
    AckClipboard,
    /// キーボードの LED 状態など(使わない)
    UhidOutput,
}

pub fn read_device_msg(r: &mut impl Read) -> io::Result<DeviceMsg> {
    let mut t = [0u8; 1];
    r.read_exact(&mut t)?;
    match t[0] {
        0 => {
            let mut len = [0u8; 4];
            r.read_exact(&mut len)?;
            let len = u32::from_be_bytes(len) as usize;
            if len > CLIPBOARD_TEXT_MAX + 14 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "clipboard too large",
                ));
            }
            let mut buf = vec![0u8; len];
            r.read_exact(&mut buf)?;
            Ok(DeviceMsg::Clipboard(
                String::from_utf8_lossy(&buf).into_owned(),
            ))
        }
        1 => {
            let mut seq = [0u8; 8];
            r.read_exact(&mut seq)?;
            Ok(DeviceMsg::AckClipboard)
        }
        2 => {
            let mut head = [0u8; 4];
            r.read_exact(&mut head)?;
            let size = u16::from_be_bytes([head[2], head[3]]) as usize;
            let mut buf = vec![0u8; size];
            r.read_exact(&mut buf)?;
            Ok(DeviceMsg::UhidOutput)
        }
        t => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown device message type {t}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_message_matches_the_control_reader() {
        let down = inject_touch(0,1,100,200,3048,2032);
        assert_eq!(down.len(),32);
        assert_eq!(&down[0..2], &[2,0]);
        assert_eq!(&down[2..10], &1u64.to_be_bytes());
        assert_eq!(&down[10..14], &100i32.to_be_bytes());
        assert_eq!(&down[14..18], &200i32.to_be_bytes());
        assert_eq!(&down[18..20], &3048u16.to_be_bytes());
        assert_eq!(&down[20..22], &2032u16.to_be_bytes());
        assert_eq!(&down[22..24], &[255,255]);
        assert_eq!(&down[24..32], &[0;8]);
        let up = inject_touch(1,1,100,200,3048,2032);
        assert_eq!(&up[22..24], &[0,0]);
    }

    #[test]
    fn uhid_create_layout_matches_the_server_reader() {
        let m = uhid_create(2, "Knit", &[0xAA, 0xBB]);
        assert_eq!(
            m,
            vec![12, 0, 2, 0, 0, 0, 0, 4, b'K', b'n', b'i', b't', 0, 2, 0xAA, 0xBB]
        );
    }

    #[test]
    fn uhid_input_and_destroy_layout() {
        assert_eq!(uhid_input(1, &[9, 8]), vec![13, 0, 1, 0, 2, 9, 8]);
        assert_eq!(uhid_destroy(1), vec![14, 0, 1]);
    }

    #[test]
    fn set_clipboard_layout() {
        let m = set_clipboard("あ");
        assert_eq!(m[0], 9);
        assert_eq!(&m[1..9], &[0; 8]);
        assert_eq!(m[9], 0);
        assert_eq!(&m[10..14], &3u32.to_be_bytes());
        assert_eq!(&m[14..], "あ".as_bytes());
    }

    #[test]
    fn clipboard_is_cut_on_a_char_boundary() {
        let s = "あ".repeat(CLIPBOARD_TEXT_MAX / 3 + 10);
        let m = set_clipboard(&s);
        let body = &m[14..];
        assert!(body.len() <= CLIPBOARD_TEXT_MAX);
        assert!(std::str::from_utf8(body).is_ok());
    }

    #[test]
    fn inject_keycode_layout() {
        assert_eq!(
            inject_keycode(true, KEYCODE_VOLUME_UP),
            vec![0, 0, 0, 0, 0, 24, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(inject_keycode(false, 24)[1], 1);
    }

    #[test]
    fn inject_scroll_layout() {
        // type 3 + x/y i32 + 画面 w/h u16 + 横/縦 i16 + buttons i32 = 21 バイト
        let m = inject_scroll(100, 200, 3048, 2032, 0.05, -0.1);
        assert_eq!(m.len(), 21);
        assert_eq!(m[0], 3);
        assert_eq!(i32::from_be_bytes([m[1], m[2], m[3], m[4]]), 100);
        assert_eq!(i32::from_be_bytes([m[5], m[6], m[7], m[8]]), 200);
        assert_eq!(u16::from_be_bytes([m[9], m[10]]), 3048);
        assert_eq!(u16::from_be_bytes([m[11], m[12]]), 2032);
        // 1.0 = 2048。0.05 ノッチ = 102、-0.1 ノッチ = -205
        assert_eq!(i16::from_be_bytes([m[13], m[14]]), 102);
        assert_eq!(i16::from_be_bytes([m[15], m[16]]), -205);
        assert_eq!(i32::from_be_bytes([m[17], m[18], m[19], m[20]]), 0);
        // 範囲外は ±16 ノッチに丸まる
        let m = inject_scroll(0, 0, 100, 100, 99.0, -99.0);
        assert_eq!(i16::from_be_bytes([m[13], m[14]]), 32767);
        assert_eq!(i16::from_be_bytes([m[15], m[16]]), -32768);
    }

    #[test]
    fn device_messages_parse_in_sequence() {
        let mut wire = vec![0u8];
        wire.extend_from_slice(&5u32.to_be_bytes());
        wire.extend_from_slice(b"hello");
        wire.push(2);
        wire.extend_from_slice(&[0, 1, 0, 1, 0x02]);
        wire.push(1);
        wire.extend_from_slice(&7u64.to_be_bytes());
        let mut r = &wire[..];
        assert_eq!(
            read_device_msg(&mut r).unwrap(),
            DeviceMsg::Clipboard("hello".into())
        );
        assert_eq!(read_device_msg(&mut r).unwrap(), DeviceMsg::UhidOutput);
        assert_eq!(read_device_msg(&mut r).unwrap(), DeviceMsg::AckClipboard);
        assert!(read_device_msg(&mut r).is_err());
    }
}
