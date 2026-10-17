//! 物理的な押下ごとに、ドラッグ用ペーストボードの変化と読み出し結果を結びつける。
use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread::Thread;
use std::time::Duration;

/// 押下中の確認間隔。掴みの可能性がある間だけ細かく見る。
pub(crate) const PRESSED_POLL: Duration = Duration::from_millis(16);
/// 押下していない間の確認間隔。押下は tap 側の wake_watcher() で即座に起こされるため、
/// この長さは「通知を取り逃した場合の保険」の意味しか持たない。
pub(crate) const IDLE_POLL: Duration = Duration::from_millis(200);

/// 監視スレッド。押下の瞬間に待機を解除するために保持する。
static WATCHER: OnceLock<Thread> = OnceLock::new();

/// 監視スレッド自身がループ開始時に呼ぶ。
pub(crate) fn register_watcher() {
    let _ = WATCHER.set(std::thread::current());
}

/// 押下状態が変わり得る操作の直後に呼ぶ。未登録なら何もしない。
/// unpark は相手を待たないため、入力経路(タップ)から呼んでも止まらない。
pub(crate) fn wake_watcher() {
    if let Some(thread) = WATCHER.get() {
        thread.unpark();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    generation: u64,
    pub(crate) baseline: isize,
}

enum Phase {
    Idle,
    Pressed {
        probe: Probe,
        moved: bool,
    },
    Ready(Vec<PathBuf>),
    HandedOff,
    /// 相手へ渡せなかった掴み。離すまで境界を越えさせない
    Refused,
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

    /// 次の確認までの待ち時間。押下中だけ細かく、それ以外は粗く。
    /// 押下の開始は tap 側の wake_watcher() が待機を解除するので、
    /// IDLE_POLL の長さが押下の検出を遅らせることはない。
    pub(crate) fn poll_interval(&self) -> Duration {
        match self.phase {
            Phase::Pressed { .. } => PRESSED_POLL,
            _ => IDLE_POLL,
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

    pub(crate) fn refuse(&mut self) {
        self.phase = Phase::Refused;
    }

    pub(crate) fn refused(&self) -> bool {
        matches!(self.phase, Phase::Refused)
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
    fn refused_drag_holds_the_edge_until_release() {
        let mut drag = FileDrag::new();
        drag.begin(Some(3));
        drag.moved();
        assert!(drag.complete(drag.probe().unwrap(), 4, files()));
        assert!(drag.take().is_some());
        drag.refuse();
        assert!(drag.refused(), "渡せなかった掴みで境界を越えない");
        assert!(!drag.ready() && drag.take().is_none());
        drag.end();
        assert!(!drag.refused(), "離せば通常の切替に戻る");
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

    #[test]
    fn polls_fast_only_while_the_button_is_down() {
        let mut drag = FileDrag::new();
        assert_eq!(drag.poll_interval(), IDLE_POLL, "押していない間は粗く見る");
        drag.begin(None); // 基準値を取れない押下(Windowsモード・未接続など)
        assert_eq!(
            drag.poll_interval(),
            IDLE_POLL,
            "掴みになり得ない押下では起こしても無駄なので速くしない"
        );
        drag.begin(Some(5));
        assert_eq!(drag.poll_interval(), PRESSED_POLL, "押下中は細かく見る");
        drag.moved();
        assert_eq!(drag.poll_interval(), PRESSED_POLL, "押下中は動いても細かく");
        let probe = drag.probe().unwrap();
        assert!(drag.complete(probe, 6, files()));
        assert_eq!(drag.poll_interval(), IDLE_POLL, "読み出しが済めば粗くてよい");
        assert!(drag.take().is_some());
        assert_eq!(drag.poll_interval(), IDLE_POLL, "引き渡し後も粗くてよい");
        drag.refuse();
        assert_eq!(drag.poll_interval(), IDLE_POLL, "拒否後も粗くてよい");
        drag.end();
        assert_eq!(drag.poll_interval(), IDLE_POLL, "離せば待機に戻る");
    }

    #[test]
    fn a_press_wakes_the_parked_watcher() {
        // 押下を最大 IDLE_POLL 待つと、素早いドラッグを取りこぼす。
        // 待機中の監視スレッドは unpark で即座に起きる必要がある。
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let watcher = std::thread::spawn(move || {
            register_watcher();
            ready_tx.send(()).unwrap();
            let started = std::time::Instant::now();
            // 起こされなければ 2 秒眠る(保険の timeout)
            std::thread::park_timeout(IDLE_POLL * 10);
            started.elapsed()
        });
        ready_rx.recv().unwrap();
        wake_watcher();
        let waited = watcher.join().unwrap();
        assert!(
            waited < IDLE_POLL * 2,
            "押下で待機が解除される (実際に待った時間 {waited:?})"
        );
    }
}
