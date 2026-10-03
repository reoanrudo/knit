use knit_common::proto::TabletAction as Action;

// 3 本指の操作は、指がふれただけで発火しないよう大きめの移動と明確な向きを要求する。
const SWIPE: f64 = 0.24;
const DOMINANCE: f64 = 2.5;
const HOLD_MS: u64 = 550;

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub id: i32,
    pub x: f64,
    pub y: f64,
}
struct Stroke {
    start: Vec<Point>,
    edge: i8,
    fired: bool,
    lifting: bool,
    pause: Option<(u64, f64, f64)>,
}
#[derive(Default)]
pub struct Recognizer {
    stroke: Option<Stroke>,
    blocked: bool,
    last_ms: u64,
}
impl Recognizer {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn frame(&mut self, points: &[Point], now: u64) -> Option<Action> {
        // 長い途切れや指の追加は取消。次の操作へ前の停止時間を持ち越さない。
        if self.stroke.is_some() && now.saturating_sub(self.last_ms) > 700 {
            self.stroke = None;
            self.blocked = !points.is_empty();
        }
        self.last_ms = now;
        if points.is_empty() {
            let home = self
                .stroke
                .as_ref()
                .is_some_and(|s| s.pause.is_some() && !s.fired);
            self.reset();
            return home.then_some(Action::Home);
        }
        if points.len() > 3
            || points.iter().any(|p| {
                !p.x.is_finite()
                    || !p.y.is_finite()
                    || !(0.0..=1.0).contains(&p.x)
                    || !(0.0..=1.0).contains(&p.y)
            })
            || points
                .iter()
                .enumerate()
                .any(|(i, p)| points[..i].iter().any(|q| q.id == p.id))
        {
            self.stroke = None;
            self.blocked = true;
        }
        if self.blocked {
            return None;
        }
        let upgrade = self
            .stroke
            .as_ref()
            .is_some_and(|s| s.start.len() == 2 && points.len() == 3 && !s.fired);
        if upgrade {
            self.stroke = None;
        }
        if self.stroke.is_none() {
            if points.len() < 2 {
                return None;
            }
            let edge = if points.len() != 2 {
                0
            } else if points.iter().all(|p| p.x <= 0.12) {
                1
            } else if points.iter().all(|p| p.x >= 0.88) {
                -1
            } else {
                0
            };
            self.stroke = Some(Stroke {
                start: points.to_vec(),
                edge,
                fired: false,
                lifting: false,
                pause: None,
            });
            return None;
        }
        let s = self.stroke.as_mut().unwrap();
        if points.len() < s.start.len() {
            s.lifting = true;
            return None;
        }
        if s.lifting || points.len() != s.start.len() {
            self.blocked = true;
            self.stroke = None;
            return None;
        }
        let mut moves = Vec::with_capacity(points.len());
        for first in &s.start {
            let Some(p) = points.iter().find(|p| p.id == first.id) else {
                self.blocked = true;
                self.stroke = None;
                return None;
            };
            moves.push((p.x - first.x, p.y - first.y));
        }
        if s.fired {
            return None;
        }
        let n = moves.len() as f64;
        let dx = moves.iter().map(|p| p.0).sum::<f64>() / n;
        let dy = moves.iter().map(|p| p.1).sum::<f64>() / n;
        // 指が別々の方向へ動くピンチを、平行移動として判定しない。
        if moves
            .iter()
            .any(|p| (p.0 - dx).abs() > 0.06 || (p.1 - dy).abs() > 0.06)
        {
            return None;
        }
        let action = if s.start.len() == 2 {
            (s.edge != 0 && dx * s.edge as f64 >= 0.10 && dx.abs() > dy.abs() * 1.7)
                .then_some(Action::Back)
        } else if s.pause.is_none() && dx.abs() >= SWIPE && dx.abs() > dy.abs() * DOMINANCE {
            Some(if dx < 0.0 {
                Action::PreviousApp
            } else {
                Action::NextApp
            })
        } else if s.pause.is_none() && dy <= -SWIPE && dy.abs() > dx.abs() * DOMINANCE {
            Some(Action::Screenshot)
        } else if dy >= SWIPE && dy.abs() > dx.abs() * DOMINANCE {
            let pause = s.pause.get_or_insert((now, dx, dy));
            if (dx - pause.1).abs() > 0.025 || (dy - pause.2).abs() > 0.025 {
                *pause = (now, dx, dy);
            }
            (now.saturating_sub(pause.0) >= HOLD_MS).then_some(Action::Recents)
        } else {
            None
        };
        if action.is_some() {
            s.fired = true;
        }
        action
    }
    pub fn suppress_scroll(&self, horizontal: bool) -> bool {
        self.stroke
            .as_ref()
            .is_some_and(|s| s.start.len() == 3 || (horizontal && s.edge != 0))
    }
    pub fn tick(&mut self, now: u64) -> Option<Action> {
        if self.stroke.is_some() && now.saturating_sub(self.last_ms) > 700 {
            self.stroke = None;
            self.blocked = true;
            return None;
        }
        let s = self.stroke.as_mut()?;
        if !s.fired && !s.lifting && s.pause.is_some_and(|p| now.saturating_sub(p.0) >= HOLD_MS) {
            s.fired = true;
            return Some(Action::Recents);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn points(n: usize, x: f64, y: f64) -> Vec<Point> {
        (0..n)
            .map(|id| Point {
                id: id as i32,
                x: x + id as f64 * 0.025,
                y,
            })
            .collect()
    }
    #[test]
    fn stationary_hold_uses_timer_and_stale_frames_cancel() {
        let mut r = Recognizer::default();
        r.frame(&points(3, 0.4, 0.3), 0);
        r.frame(&points(3, 0.4, 0.56), 80);
        assert_eq!(r.tick(629), None);
        assert_eq!(r.tick(630), Some(Action::Recents));
        assert_eq!(r.tick(640), None);
        assert_eq!(r.frame(&[], 650), None);
        r.frame(&points(3, 0.4, 0.3), 750);
        r.frame(&points(3, 0.4, 0.56), 830);
        r.frame(&points(2, 0.4, 0.56), 850);
        assert_eq!(r.tick(1150), None);
        assert_eq!(r.frame(&[], 1170), Some(Action::Home));
        r.frame(&points(3, 0.4, 0.3), 1250);
        r.frame(&points(3, 0.4, 0.56), 1330);
        assert_eq!(r.tick(2150), None);
        assert_eq!(r.frame(&points(3, 0.4, 0.2), 2160), None);
        assert_eq!(r.frame(&[], 2170), None);
    }
    #[test]
    fn center_scroll_and_pinch_do_not_trigger_back() {
        let mut r = Recognizer::default();
        r.frame(&points(2, 0.4, 0.4), 0);
        assert_eq!(r.frame(&points(2, 0.6, 0.4), 30), None);
        assert!(!r.suppress_scroll(true));
        r.frame(&[], 40);
        r.frame(&points(2, 0.03, 0.4), 50);
        let mut pinch = points(2, 0.03, 0.4);
        pinch[0].x -= 0.025;
        pinch[1].x += 0.15;
        assert_eq!(r.frame(&pinch, 70), None);
    }
    #[test]
    fn inward_edge_swipe_fires_once_from_either_edge() {
        for (start, end) in [(0.02, 0.2), (0.94, 0.75)] {
            let mut r = Recognizer::default();
            r.frame(&points(2, start, 0.4), 0);
            assert!(r.suppress_scroll(true));
            assert!(!r.suppress_scroll(false));
            assert_eq!(r.frame(&points(2, end, 0.4), 30), Some(Action::Back));
            assert_eq!(r.frame(&points(2, end, 0.4), 40), None);
            assert_eq!(r.frame(&[], 50), None);
        }
    }
    #[test]
    fn three_finger_horizontal_and_down_are_distinct() {
        for (x, y, action) in [
            (0.1, 0.5, Action::PreviousApp),
            (0.75, 0.5, Action::NextApp),
            (0.4, 0.2, Action::Screenshot),
        ] {
            let mut r = Recognizer::default();
            r.frame(&points(3, 0.4, 0.5), 0);
            assert!(r.suppress_scroll(false));
            assert_eq!(r.frame(&points(3, x, y), 30), Some(action));
            assert_eq!(r.frame(&points(3, x, y), 60), None);
            assert_eq!(r.frame(&[], 100), None);
        }
    }
    #[test]
    fn upward_release_goes_home_but_pause_only_opens_recents() {
        let mut r = Recognizer::default();
        r.frame(&points(3, 0.4, 0.3), 0);
        assert_eq!(r.frame(&points(3, 0.4, 0.56), 80), None);
        assert_eq!(r.frame(&points(2, 0.4, 0.56), 120), None);
        assert_eq!(r.frame(&[], 140), Some(Action::Home));
        r.frame(&points(3, 0.4, 0.3), 200);
        r.frame(&points(3, 0.4, 0.56), 280);
        assert_eq!(r.frame(&points(3, 0.4, 0.57), 840), Some(Action::Recents));
        assert_eq!(r.frame(&[], 870), None);
    }
    #[test]
    fn moving_up_without_pause_and_invalid_contacts_do_not_open_recents() {
        let mut r = Recognizer::default();
        r.frame(&points(3, 0.4, 0.2), 0);
        r.frame(&points(3, 0.4, 0.4), 50);
        assert_eq!(r.frame(&points(3, 0.4, 0.6), 500), None);
        assert_eq!(r.frame(&[], 550), Some(Action::Home));
        r.frame(&points(3, 0.4, 0.3), 600);
        r.frame(&points(3, 0.4, 0.5), 650);
        r.frame(&points(4, 0.4, 0.5), 700);
        assert_eq!(r.frame(&[], 750), None);
        r.frame(&points(3, f64::NAN, 0.3), 800);
        assert_eq!(r.frame(&points(3, 0.4, 0.5), 850), None);
        assert_eq!(r.frame(&[], 900), None);
    }
}
