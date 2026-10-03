use std::io::{self, Write};

#[derive(Debug, Clone, Copy)]
pub struct Touch {
    pub action: u8,
    pub id: u64,
    pub x: i32,
    pub y: i32,
}
struct Fingers {
    center: (f64, f64),
    radius: f64,
    limit: f64,
}
#[derive(Default)]
pub struct Pinch {
    fingers: Option<Fingers>,
    last_ms: u64,
}

impl Pinch {
    // カーソル位置を拡大の中心として固定し、開始・終了で必ず両指を揃える。
    pub fn update(
        &mut self,
        phase: u8,
        delta: f64,
        pos: (i32, i32),
        size: (u16, u16),
        now: u64,
    ) -> Vec<Touch> {
        if phase >= 2 {
            return self.finish();
        }
        if !delta.is_finite() || size.0 < 16 || size.1 < 16 {
            return Vec::new();
        }
        let mut out = if phase == 0 {
            self.finish()
        } else {
            Vec::new()
        };
        if self.fingers.is_none() {
            if phase != 0 && delta.abs() < 0.0001 {
                return out;
            }
            let (w, h) = (size.0 as f64 - 1.0, size.1 as f64 - 1.0);
            let radius = w.min(h) * 0.08;
            let center = (
                (pos.0 as f64).clamp(radius + 1.0, w - radius - 1.0),
                (pos.1 as f64).clamp(radius + 1.0, h - radius - 1.0),
            );
            let limit = center.0.min(w - center.0).min(center.1).min(h - center.1) - 1.0;
            self.fingers = Some(Fingers {
                center,
                radius,
                limit,
            });
            out.extend(self.touches(0));
        }
        self.last_ms = now;
        if delta.abs() > 0.0001 {
            let f = self.fingers.as_mut().unwrap();
            f.radius = (f.radius * (1.0 + delta.clamp(-0.5, 0.5))).clamp(1.0, f.limit);
            out.extend(self.touches(2));
        }
        out
    }
    fn touches(&self, action: u8) -> Vec<Touch> {
        let Some(f) = &self.fingers else {
            return Vec::new();
        };
        [-1.0, 1.0]
            .into_iter()
            .enumerate()
            .map(|(id, sign)| Touch {
                action,
                id: id as u64,
                x: (f.center.0 + sign * f.radius).round() as i32,
                y: f.center.1.round() as i32,
            })
            .collect()
    }
    pub fn finish(&mut self) -> Vec<Touch> {
        let mut out = self.touches(1);
        out.reverse();
        self.fingers = None;
        out
    }
    pub fn expire(&mut self, now: u64) -> Vec<Touch> {
        if now.saturating_sub(self.last_ms) > 1500 {
            self.finish()
        } else {
            Vec::new()
        }
    }
}

pub fn write_to(writer: &mut impl Write, touches: Vec<Touch>, size: (u16, u16)) -> io::Result<()> {
    for t in touches {
        writer.write_all(&super::scrcpy::inject_touch(
            t.action, t.id, t.x, t.y, size.0, size.1,
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinch_expands_then_releases_both_fingers() {
        let mut p = Pinch::default();
        let start = p.update(0, 0.0, (500, 300), (1000, 600), 10);
        assert_eq!(start.len(), 2);
        assert!(start.iter().all(|t| t.action == 0));
        let moved = p.update(1, 0.2, (999, 599), (1000, 600), 20);
        assert!(moved[0].x < start[0].x && moved[1].x > start[1].x);
        assert_eq!(moved[0].y, start[0].y);
        let end = p.update(2, 0.0, (0, 0), (1000, 600), 30);
        assert_eq!(end.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 0]);
        assert!(end.iter().all(|t| t.action == 1));
        assert!(p.finish().is_empty());
    }
    #[test]
    fn edge_and_repeated_delta_never_send_offscreen_touches() {
        let mut p = Pinch::default();
        for n in 0..40 {
            let events = p.update(if n == 0 { 0 } else { 1 }, 0.5, (0, 0), (3048, 2032), n);
            assert!(events
                .iter()
                .all(|e| e.x >= 0 && e.x < 3048 && e.y >= 0 && e.y < 2032));
        }
        assert!(p.update(1, f64::NAN, (0, 0), (3048, 2032), 40).is_empty());
        assert_eq!(p.expire(2000).len(), 2);
    }
    #[test]
    fn missed_begin_and_cancel_are_recoverable() {
        let mut p = Pinch::default();
        assert_eq!(p.update(1, 0.1, (300, 200), (600, 400), 0).len(), 4);
        assert_eq!(p.update(3, 0.0, (300, 200), (600, 400), 20).len(), 2);
        assert!(p.update(1, 0.0, (300, 200), (600, 400), 30).is_empty());
    }
}

/// 画面上の3本指タッチはHyperOSのトラックパッド操作とは別物なので、同じOS操作へ変換する。
pub fn navigation_keys(action: knit_common::proto::TabletAction) -> Vec<u8> {
    use knit_common::proto::TabletAction::*;
    use super::scrcpy::inject_keycode_with_meta as key;
    let mut out=Vec::new();
    let code=match action {Back=>4, Home=>3, Recents=>187, Screenshot=>120, PreviousApp|NextApp=>0};
    if code!=0 { out.extend(key(true,code,0)); out.extend(key(false,code,0)); }
    else {
        let reverse=action==PreviousApp;
        out.extend(key(true,57,2)); // ALT_LEFT
        if reverse {out.extend(key(true,59,3));} // SHIFT_LEFT
        out.extend(key(true,61,if reverse {3} else {2})); // TAB
        out.extend(key(false,61,if reverse {3} else {2}));
        if reverse {out.extend(key(false,59,2));}
        out.extend(key(false,57,0));
    }
    out
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    use knit_common::proto::{TabletAction::*, Msg, encode, decode};
    #[test]
    fn settings_actions_roundtrip_and_leave_no_pressed_keys() {
        for action in [Back, Home, Recents, Screenshot, PreviousApp, NextApp] {
            let wire=encode(&Msg::TabletGesture {action});
            let Some(Msg::TabletGesture {action:decoded})=decode(&wire) else {panic!("gesture decode");};
            assert_eq!(decoded,action);
            let bytes=navigation_keys(decoded);
            let mut pressed=std::collections::BTreeSet::new();
            for packet in bytes.chunks_exact(14) {
                assert_eq!(packet[0],0);
                let code=u32::from_be_bytes(packet[2..6].try_into().unwrap());
                if packet[1]==0 {assert!(pressed.insert(code));} else {assert!(pressed.remove(&code));}
            }
            assert!(pressed.is_empty());
            assert_eq!(&bytes[bytes.len()-4..],&0u32.to_be_bytes());
        }
        assert_eq!(u32::from_be_bytes(navigation_keys(Screenshot)[2..6].try_into().unwrap()),120);
        let previous=navigation_keys(PreviousApp);
        assert_eq!(u32::from_be_bytes(previous[28+10..28+14].try_into().unwrap()),3);
        let next=navigation_keys(NextApp);
        assert_eq!(u32::from_be_bytes(next[14+10..14+14].try_into().unwrap()),2);
    }
}
