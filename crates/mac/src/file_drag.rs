//! 物理的な押下ごとに、ドラッグ用ペーストボードの変化と読み出し結果を結びつける。
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    generation: u64,
    pub(crate) baseline: isize,
}

enum Phase {
    Idle,
    Pressed { probe: Probe, moved: bool },
    Ready(Vec<PathBuf>),
    HandedOff,
}

pub(crate) struct FileDrag {
    generation: u64,
    phase: Phase,
}

impl FileDrag {
    pub(crate) const fn new() -> Self {
        Self {
            generation: 0,
            phase: Phase::Idle,
        }
    }

    pub(crate) fn begin(&mut self, baseline: Option<isize>) {
        self.generation = self.generation.wrapping_add(1);
        self.phase = match baseline {
            Some(baseline) => Phase::Pressed {
                probe: Probe {
                    generation: self.generation,
                    baseline,
                },
                moved: false,
            },
            None => Phase::Idle,
        };
    }

    pub(crate) fn moved(&mut self) {
        if let Phase::Pressed { moved, .. } = &mut self.phase {
            *moved = true;
        }
    }

    pub(crate) fn probe(&self) -> Option<Probe> {
        match self.phase {
            Phase::Pressed { probe, moved: true } => Some(probe),
            _ => None,
        }
    }

    pub(crate) fn complete(&mut self, probe: Probe, count: isize, files: Vec<PathBuf>) -> bool {
        // URL読み出し中に離す・押し直す・越境する場合、古い結果を採用しない。
        if self.probe() != Some(probe) || count == probe.baseline || files.is_empty() {
            return false;
        }
        self.phase = Phase::Ready(files);
        true
    }

    pub(crate) fn ready(&self) -> bool {
        matches!(self.phase, Phase::Ready(_))
    }

    pub(crate) fn take(&mut self) -> Option<Vec<PathBuf>> {
        if !self.ready() {
            return None;
        }
        match std::mem::replace(&mut self.phase, Phase::HandedOff) {
            Phase::Ready(files) => Some(files),
            _ => unreachable!(),
        }
    }

    pub(crate) fn end(&mut self) {
        self.phase = Phase::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<PathBuf> {
        vec![PathBuf::from("dragged.txt")]
    }

    #[test]
    fn captures_a_drag_that_started_before_the_first_poll() {
        let mut drag = FileDrag::new();
        drag.begin(Some(40)); // MouseDown時点。その後、Finderが41へ更新する。
        drag.moved();
        let probe = drag.probe().unwrap();
        assert!(drag.complete(probe, 41, files()));
        assert!(
            drag.ready(),
            "最初の監視より前に始めたドラッグも境界を越えられる"
        );
        assert_eq!(drag.take(), Some(files()));
    }

    #[test]
    fn does_not_reuse_a_cancelled_drag_pasteboard() {
        let mut drag = FileDrag::new();
        drag.begin(Some(41)); // 前回キャンセルしたファイルURLは41のまま残っている。
        drag.moved();
        assert!(!drag.complete(drag.probe().unwrap(), 41, files()));
        assert!(
            !drag.ready(),
            "残骸で通常のクリックやウィンドウ移動を越境させない"
        );
    }

    #[test]
    fn discards_a_late_read_after_release_and_a_new_press() {
        let mut drag = FileDrag::new();
        drag.begin(Some(40));
        drag.moved();
        let old = drag.probe().unwrap();
        drag.end();
        assert!(!drag.complete(old, 41, files()));
        drag.begin(Some(41));
        drag.moved();
        assert!(
            !drag.complete(old, 42, files()),
            "別の押下で古い読み出しを受け付けない"
        );
        assert!(drag.complete(drag.probe().unwrap(), 42, files()));
    }

    #[test]
    fn requires_drag_movement_and_file_urls() {
        let mut drag = FileDrag::new();
        drag.begin(Some(1));
        assert!(
            drag.probe().is_none(),
            "クリックだけではファイルの読み出しを始めない"
        );
        drag.moved();
        assert!(!drag.complete(drag.probe().unwrap(), 2, Vec::new()));
        assert!(
            !drag.ready(),
            "テキストやウィンドウのドラッグをファイルと判定しない"
        );
        drag.begin(None);
        drag.moved();
        assert!(
            drag.probe().is_none(),
            "基準値が取得できなければ残骸を渡さない"
        );
    }

    #[test]
    fn hands_off_once_and_clears_on_release() {
        let mut drag = FileDrag::new();
        drag.begin(Some(10));
        drag.moved();
        let probe = drag.probe().unwrap();
        assert!(drag.complete(probe, 11, files()));
        assert_eq!(drag.take(), Some(files()));
        assert!(!drag.ready());
        assert!(drag.take().is_none());
        assert!(drag.probe().is_none());
        assert!(
            !drag.complete(probe, 11, files()),
            "同じ押下のまま二重送信しない"
        );
        drag.end();
        drag.begin(Some(11));
        drag.moved();
        assert!(drag.complete(drag.probe().unwrap(), 12, files()));
        drag.end();
        assert!(!drag.ready());
        assert!(drag.take().is_none());
    }
}
