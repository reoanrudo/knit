//! クリップボードのテキストを論理時計で順序付ける。
//! 仕様: docs/cleanroom/2026-10-29-clip-ordering.md。
//! ローカルのコピーでは物理時刻と論理時刻の最大値を使い、同時刻なら
//! カウンタを進める。受信時は相手の時刻も含めた最大値とカウンタを更新し、
//! (論理時刻、カウンタ、端末 ID) の辞書順で新旧を決める。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClipStamp {
    pub l: u64,
    pub c: u32,
    pub node: String,
}

pub struct ClipClock {
    l: u64,
    c: u32,
    node: String,
}

impl ClipClock {
    pub fn new(node: String) -> Self {
        Self { l: 0, c: 0, node }
    }

    fn increment(&mut self, c: u32) {
        if let Some(next) = c.checked_add(1) {
            self.c = next;
        } else {
            self.l = self.l.checked_add(1).expect("論理時計が上限に達しました");
            self.c = 0;
        }
    }

    pub fn now(&mut self, pt: u64) -> ClipStamp {
        if pt > self.l {
            self.l = pt;
            self.c = 0;
        } else {
            self.increment(self.c);
        }
        self.stamp()
    }

    pub fn observe(&mut self, remote: &ClipStamp, pt: u64) {
        let old_l = self.l;
        let old_c = self.c;
        self.l = old_l.max(remote.l).max(pt);
        if self.l == old_l && self.l == remote.l {
            self.increment(old_c.max(remote.c));
        } else if self.l == old_l {
            self.increment(old_c);
        } else if self.l == remote.l {
            self.increment(remote.c);
        } else {
            self.c = 0;
        }
    }

    fn stamp(&self) -> ClipStamp {
        ClipStamp {
            l: self.l,
            c: self.c,
            node: self.node.clone(),
        }
    }
}

pub struct ClipState {
    clock: ClipClock,
    latest: Option<ClipStamp>,
}
impl ClipState {
    pub fn new(node: String) -> Self {
        Self {
            clock: ClipClock::new(node),
            latest: None,
        }
    }
    pub fn local_copy(&mut self, pt: u64) -> ClipStamp {
        let stamp = self.clock.now(pt);
        self.latest = Some(stamp.clone());
        stamp
    }
    pub fn latest(&self) -> Option<ClipStamp> {
        self.latest.clone()
    }

    pub fn receive(&mut self, remote: Option<ClipStamp>, pt: u64) -> bool {
        let Some(stamp) = remote else {
            self.local_copy(pt);
            return true;
        };
        self.clock.observe(&stamp, pt);
        if self.latest.as_ref().is_some_and(|latest| stamp <= *latest) {
            return false;
        }
        self.latest = Some(stamp);
        true
    }
}

/// 最後の同期後にクリップボードが変更されたかを判定する。
pub fn has_pending_local_copy<T: Eq>(current: T, synced: T) -> bool {
    current != synced
}

pub fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

pub fn device_state() -> &'static std::sync::Mutex<ClipState> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<ClipState>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(ClipState::new(crate::proto::device_id())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_message_legacy_and_timestamp_round_trip() {
        use crate::proto::Msg;
        let old: Msg = serde_json::from_str(r#"{"t":"clip","text":"hello"}"#).unwrap();
        assert!(matches!(old, Msg::Clip { ref text, ts: None } if text == "hello"));
        let stamp = ClipStamp {
            l: 100,
            c: 2,
            node: "a".into(),
        };
        let message = Msg::Clip {
            text: "hello".into(),
            ts: Some(stamp.clone()),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(
            matches!(serde_json::from_str::<Msg>(&json).unwrap(), Msg::Clip { text, ts: Some(ts) } if text == "hello" && ts == stamp)
        );
        assert!(!serde_json::to_string(&old).unwrap().contains("ts"));
    }

    #[test]
    fn pending_local_copy_depends_on_sync_marker() {
        assert!(!has_pending_local_copy(42_isize, 42));
        assert!(has_pending_local_copy(43_isize, 42));
        assert!(!has_pending_local_copy(5_u32, 5));
        assert!(has_pending_local_copy(0_u32, u32::MAX));
    }

    #[test]
    fn pending_local_copy_precedes_remote_ordering() {
        let mut state = ClipState::new("local".into());
        let remote = ClipStamp {
            l: 100,
            c: 0,
            node: "remote".into(),
        };
        if has_pending_local_copy(2_u32, 1) {
            state.local_copy(101);
        }
        assert!(!state.receive(Some(remote), 101));
    }

    #[test]
    fn clock_advances_and_survives_wall_clock_rollback() {
        let mut clock = ClipClock::new("a".into());
        let first = clock.now(100);
        let second = clock.now(100);
        let third = clock.now(1);
        assert!(first < second && second < third);
        let remote = ClipStamp {
            l: 200,
            c: 4,
            node: "b".into(),
        };
        clock.observe(&remote, 50);
        assert!(clock.now(50) > remote);
    }

    #[test]
    fn stamps_have_total_order_and_node_breaks_ties() {
        let a = ClipStamp {
            l: 10,
            c: 1,
            node: "a".into(),
        };
        let b = ClipStamp {
            node: "b".into(),
            ..a.clone()
        };
        let c = ClipStamp {
            l: 11,
            c: 0,
            node: "a".into(),
        };
        assert!(a < b && b < c && a < c);
    }

    #[test]
    fn receive_accepts_new_and_legacy_but_rejects_old() {
        let mut state = ClipState::new("a".into());
        let first = state.local_copy(100);
        let newer = ClipStamp {
            l: 101,
            c: 0,
            node: "b".into(),
        };
        assert!(state.receive(Some(newer), 100));
        assert!(!state.receive(Some(first), 100));
        assert!(state.receive(None, 100));
    }

    #[test]
    fn concurrent_copies_converge() {
        let mut a = ClipState::new("a".into());
        let mut b = ClipState::new("b".into());
        let a_stamp = a.local_copy(100);
        let b_stamp = b.local_copy(100);
        let mut a_text = "a";
        let mut b_text = "b";
        if a.receive(Some(b_stamp), 100) {
            a_text = "b";
        }
        if b.receive(Some(a_stamp), 100) {
            b_text = "a";
        }
        assert_eq!(a_text, b_text);
        assert_eq!(a_text, "b");
    }
}
