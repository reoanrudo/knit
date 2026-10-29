use std::io::{self, Read, Write};

pub const PORT_OFFSET: u16 = 5;
pub const PORT: u16 = crate::proto::PORT + PORT_OFFSET;
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
pub const CONFIG: u8 = 1;
pub const KEYFRAME: u8 = 2;
pub const DELTA: u8 = 3;

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    pub codec: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u8,
    pub timestamp_us: u64,
    pub body: Vec<u8>,
}

fn valid_kind(kind: u8) -> bool {
    matches!(kind, CONFIG | KEYFRAME | DELTA)
}

pub fn write_frame(w: &mut impl Write, kind: u8, timestamp_us: u64, body: &[u8]) -> io::Result<()> {
    if !valid_kind(kind) || body.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "不正な動画フレーム",
        ));
    }
    w.write_all(&[kind])?;
    w.write_all(&timestamp_us.to_le_bytes())?;
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(body)
}

pub fn read_frame(r: &mut impl Read) -> io::Result<Frame> {
    let mut header = [0u8; 13];
    r.read_exact(&mut header)?;
    let kind = header[0];
    let timestamp_us = u64::from_le_bytes(header[1..9].try_into().unwrap());
    let len = u32::from_le_bytes(header[9..13].try_into().unwrap()) as usize;
    if !valid_kind(kind) || len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "不正な動画フレーム",
        ));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    Ok(Frame {
        kind,
        timestamp_us,
        body,
    })
}

/// AVCC の長さ付き NAL を Annex B の開始コード付き NAL に変換する。
pub fn avcc_to_annex_b(mut data: &[u8], length_size: usize) -> io::Result<Vec<u8>> {
    if !(1..=4).contains(&length_size) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "不正な NAL 長"));
    }
    let mut out = Vec::new();
    while !data.is_empty() {
        if data.len() < length_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "NAL 長が途中で切れている",
            ));
        }
        let len = data[..length_size]
            .iter()
            .fold(0usize, |n, &b| (n << 8) | b as usize);
        data = &data[length_size..];
        if len == 0
            || len > data.len()
            || out.len().saturating_add(4).saturating_add(len) > MAX_FRAME
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "不正な NAL データ",
            ));
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[..len]);
        data = &data[len..];
    }
    Ok(out)
}

/// 戻り値は (フレームを捨てるか, キーフレーム待ちを続けるか)。
pub fn drop_decision(congested: bool, waiting_for_keyframe: bool, keyframe: bool) -> (bool, bool) {
    if congested {
        (true, true)
    } else if waiting_for_keyframe && !keyframe {
        (true, true)
    } else {
        (false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn video_config_serializes_expected_json() {
        let config = Config {
            w: 1920,
            h: 1080,
            fps: 60,
            codec: "h264".into(),
        };
        assert_eq!(
            serde_json::to_string(&config).unwrap(),
            r#"{"w":1920,"h":1080,"fps":60,"codec":"h264"}"#
        );
    }

    #[test]
    fn video_frame_roundtrip() {
        let mut wire = Vec::new();
        let body = br#"{"w":1920,"h":1080,"fps":60,"codec":"h264"}"#;
        write_frame(&mut wire, CONFIG, 123, body).unwrap();
        let frame = read_frame(&mut Cursor::new(wire)).unwrap();
        assert_eq!(
            (frame.kind, frame.timestamp_us, frame.body.as_slice()),
            (CONFIG, 123, body.as_slice())
        );
    }

    #[test]
    fn video_frame_rejects_oversized_and_corrupt_data() {
        assert!(write_frame(&mut Vec::new(), KEYFRAME, 0, &vec![0; MAX_FRAME + 1]).is_err());
        let mut wire = vec![KEYFRAME];
        wire.extend_from_slice(&0u64.to_le_bytes());
        wire.extend_from_slice(&((MAX_FRAME + 1) as u32).to_le_bytes());
        assert!(read_frame(&mut Cursor::new(wire)).is_err());
        assert!(read_frame(&mut Cursor::new(vec![9; 13])).is_err());
        assert!(read_frame(&mut Cursor::new(vec![KEYFRAME; 3])).is_err());
        let mut truncated = vec![DELTA];
        truncated.extend_from_slice(&0u64.to_le_bytes());
        truncated.extend_from_slice(&2u32.to_le_bytes());
        truncated.push(1);
        assert!(read_frame(&mut Cursor::new(truncated)).is_err());
    }

    #[test]
    fn avcc_to_annex_b_converts_multiple_nals_and_rejects_truncation() {
        assert_eq!(
            avcc_to_annex_b(&[0, 0, 0, 2, 0x65, 1, 0, 0, 0, 1, 0x41], 4).unwrap(),
            vec![0, 0, 0, 1, 0x65, 1, 0, 0, 0, 1, 0x41]
        );
        assert!(avcc_to_annex_b(&[0, 0, 0, 2, 1], 4).is_err());
        assert!(avcc_to_annex_b(&[0, 0], 4).is_err());
        assert!(avcc_to_annex_b(&[0], 0).is_err());
    }

    #[test]
    fn congestion_discards_delta_until_keyframe() {
        assert_eq!(drop_decision(false, false, false), (false, false));
        assert_eq!(drop_decision(true, false, false), (true, true));
        assert_eq!(drop_decision(false, true, false), (true, true));
        assert_eq!(drop_decision(false, true, true), (false, false));
        assert_eq!(drop_decision(true, true, true), (true, true));
    }
}
