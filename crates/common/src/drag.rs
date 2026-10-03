//! 本線の操作と、別接続で届くファイルを受け渡しIDで結びつける。
use std::path::PathBuf;

pub const MAX_FILES: usize = 64;
/// フォルダ展開後の件数上限(版 14 で導入)。掴みドラッグの予告(DragOffer)は
/// 展開後の件数で送るため、受け側もこの上限まで受け付ける
pub const MAX_BATCH_FILES: usize = 512;

struct Pending {
    id: u64,
    count: usize,
    position: f64,
    files: Option<Vec<PathBuf>>,
    committed: bool,
}

#[derive(Default)]
pub struct Incoming {
    pending: Option<Pending>,
}

impl Incoming {
    pub const fn new() -> Self {
        Self { pending: None }
    }
    pub fn offer(&mut self, id: u64, count: usize, position: f64) -> bool {
        if self.pending.is_some() || id == 0 || count == 0 || count > MAX_BATCH_FILES || !position.is_finite()
        {
            return false;
        }
        self.pending = Some(Pending {
            id,
            count,
            position: position.clamp(0.0, 1.0),
            files: None,
            committed: false,
        });
        true
    }
    pub fn receive(&mut self, id: u64, files: Vec<PathBuf>) -> Result<(), Vec<PathBuf>> {
        if let Some(p) = &mut self.pending {
            if p.id == id && !p.committed && p.files.is_none() && files.len() == p.count {
                p.files = Some(files);
                return Ok(());
            }
        }
        Err(files)
    }
    pub fn commit(&mut self, id: u64) -> Option<(Vec<PathBuf>, f64)> {
        let p = self.pending.as_mut()?;
        if p.id != id || p.committed {
            return None;
        }
        let files = p.files.take()?;
        p.committed = true;
        Some((files, p.position))
    }
    pub fn cancel(&mut self, id: u64) -> Vec<PathBuf> {
        if self.pending.as_ref().is_some_and(|p| p.id == id) {
            self.reset()
        } else {
            Vec::new()
        }
    }
    pub fn reset(&mut self) -> Vec<PathBuf> {
        self.pending
            .take()
            .and_then(|p| p.files)
            .unwrap_or_default()
    }
}

/// 相手から掴んだまま越えてきた操作。本線の予告と別接続で届くファイルを
/// 受け渡しIDで結びつけ、同じ押下が続いている間に届いた転送だけをドラッグにする。
/// 離した・制御が戻った後に届いた転送は、通常の受信として扱わせる。
pub struct Carried {
    expected: Option<(u64, usize, bool)>,
    /// 予告が届く前に離しが観測された(予告が遅れて到着しても Released のままにする)
    released_once: bool,
}

impl Default for Carried {
    fn default() -> Self {
        Self::new()
    }
}

/// 予告された掴みドラッグの転送を受け渡しIDと件数で照合した結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// 同じ押下が続いており、件数も予告と一致。掴みドラッグを開始してよい
    Carried,
    /// 押下が既に終わっている(離した後・制御が戻った後・取消後)。通常の受信として扱う
    Released,
    /// 予告の件数と一致しない。ドラッグにせず、不足を通知する
    Mismatch {
        expected: usize,
        received: usize,
    },
    /// このIDの予告が無い(消化済み・別の押下・旧版)。通常の受信として扱う
    Unknown,
}

impl Carried {
    pub const fn new() -> Self {
        Self {
            expected: None,
            released_once: false,
        }
    }
    pub fn announce(&mut self, id: u64, count: usize) {
        if id != 0 && count > 0 {
            // 先行した離しが観測済みなら、そのまま Released として扱う
            // (announce が released を無条件 false で上書きすると、転送完了が
            // 予告より先に届いた稀な順序で受信ファイルが相手のクリップボードを
            // 上書きする)
            self.expected = Some((id, count, self.released_once));
        }
    }
    pub fn release(&mut self) {
        self.released_once = true;
        if let Some((_, _, released)) = &mut self.expected {
            *released = true;
        }
    }
    pub fn claim(&mut self, id: u64, received: usize) -> Claim {
        // 不一致(別の押下の遅延転送)では予告を残す: 直後に届く正しい転送を
        // 弹かないようにする
        match self.expected {
            Some((expected, count, released)) if expected == id => {
                self.expected = None;
                if released {
                    Claim::Released
                } else if received == count {
                    Claim::Carried
                } else {
                    Claim::Mismatch {
                        expected: count,
                        received,
                    }
                }
            }
            _ => Claim::Unknown,
        }
    }
}

/// 予告(DragOffer)の遅着を待って claim を再試行する。本線と大容量経路は
/// 別 TCP で相対順序が保証されないため、小さい転送ではファイル実体が予告より
/// 先に届き得る。try_claim が Unknown 以外(Carried/Released/Mismatch)を返したら
/// 即座に返し、deadline_ms を過ぎたら最後の Unknown を返す(5ms 毎に受け直す)。
/// 呼び出し元は大容量経路の受信スレッドで動く前提で、本線の受信は止めない
pub fn await_claim(deadline_ms: u64, mut try_claim: impl FnMut() -> Claim) -> Claim {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    loop {
        let outcome = try_claim();
        if !matches!(outcome, Claim::Unknown) || std::time::Instant::now() >= deadline {
            return outcome;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn files() -> Vec<PathBuf> {
        vec![PathBuf::from("received.txt")]
    }

    #[test]
    fn early_release_survives_a_late_announce() {
        // 転送完了が予告より先に届く稀な順序: announce が released を消さないこと
        let mut carried = Carried::new();
        carried.release();
        carried.announce(9, 2);
        assert_eq!(carried.claim(9, 2), Claim::Released, "先行した離しは予告到着後も有効");
    }

    #[test]
    fn carried_drag_starts_only_while_the_same_press_continues() {
        let mut carried = Carried::new();
        assert_eq!(carried.claim(5, 1), Claim::Unknown, "予告のない転送はドラッグにしない");
        carried.announce(5, 1);
        assert_eq!(carried.claim(5, 1), Claim::Carried);
        assert_eq!(carried.claim(5, 1), Claim::Unknown, "同じ転送で二度開始しない");
        carried.announce(6, 1);
        carried.release();
        assert_eq!(carried.claim(6, 1), Claim::Released, "離した後に届いた転送は通常の受信へ");
        carried.announce(0, 1);
        assert_eq!(carried.claim(0, 1), Claim::Unknown);
    }

    #[test]
    fn late_transfer_of_an_older_drag_does_not_take_over_the_next_one() {
        let mut carried = Carried::new();
        carried.announce(1, 1);
        carried.announce(2, 1);
        assert_eq!(carried.claim(1, 1), Claim::Unknown, "前の操作の転送を新しい押下に混ぜない");
        assert_eq!(carried.claim(2, 1), Claim::Carried);
    }

    /// 大容量経路の完了が本線の予告(DragOffer)より先に届く順序入れ替わり(H2)
    /// の受け皿: 予告の遅着を待って同じ押下のドラッグとして扱う
    #[test]
    fn await_claim_picks_up_a_late_announce_within_the_deadline() {
        let carried = std::sync::Arc::new(std::sync::Mutex::new(Carried::new()));
        let late = std::sync::Arc::clone(&carried);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            late.lock().unwrap().announce(9, 1);
        });
        let outcome = await_claim(1000, || carried.lock().unwrap().claim(9, 1));
        assert_eq!(outcome, Claim::Carried, "遅れた予告でも待ち時間内ならドラッグにする");
    }

    #[test]
    fn await_claim_falls_back_to_unknown_when_no_announce_arrives() {
        let carried = std::sync::Mutex::new(Carried::new());
        let started = std::time::Instant::now();
        let outcome = await_claim(60, || carried.lock().unwrap().claim(9, 1));
        assert_eq!(outcome, Claim::Unknown, "予告が届かなければ通常の受信へ");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(60),
            "deadline までは待つ"
        );
    }

    #[test]
    fn await_claim_returns_immediately_once_the_press_is_released() {
        let carried = std::sync::Mutex::new(Carried::new());
        carried.lock().unwrap().announce(9, 1);
        carried.lock().unwrap().release();
        let started = std::time::Instant::now();
        let outcome = await_claim(1000, || carried.lock().unwrap().claim(9, 1));
        assert_eq!(outcome, Claim::Released, "離した後の転送は待たずに通常の受信へ");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "released で待ちを引き延ばさない"
        );
    }

    /// 予告の件数と届いた件数が一致しない転送はドラッグにしない(M1:
    /// 欠けた掴みをそのままドロップさせると、一部だけ渡ったことに気付けない)
    #[test]
    fn a_partial_transfer_is_not_carried_as_a_drag() {
        let mut carried = Carried::new();
        carried.announce(3, 5);
        assert_eq!(
            carried.claim(3, 4),
            Claim::Mismatch {
                expected: 5,
                received: 4
            },
            "予告より少ない受信はドラッグにしない"
        );
        assert_eq!(carried.claim(3, 5), Claim::Unknown, "不一致で消化済み");
        // 多い場合も(送信側の誤りとして)通常の受信へ
        carried.announce(4, 1);
        assert_eq!(
            carried.claim(4, 2),
            Claim::Mismatch {
                expected: 1,
                received: 2
            }
        );
    }

    #[test]
    fn commits_only_the_prepared_operation_once() {
        let mut state = Incoming::new();
        assert!(state.offer(1, 1, 0.4));
        assert!(state.commit(1).is_none());
        assert_eq!(state.receive(1, files()), Ok(()));
        assert!(state.commit(2).is_none());
        assert_eq!(state.commit(1), Some((files(), 0.4)));
        assert!(state.commit(1).is_none());
        assert!(!state.offer(2, 1, 0.5));
        state.cancel(1);
        assert!(state.offer(2, 1, 0.5));
    }

    #[test]
    fn cancellation_does_not_attach_late_files_to_the_next_drag() {
        let mut state = Incoming::new();
        assert!(state.offer(7, 1, 0.0));
        state.cancel(7);
        assert!(state.offer(8, 1, 1.0));
        assert_eq!(state.receive(7, files()), Err(files()));
        state.cancel(7);
        assert_eq!(state.receive(8, files()), Ok(()));
        assert_eq!(state.cancel(8), files());
        assert!(state.commit(8).is_none());
    }

    #[test]
    fn rejects_invalid_or_incomplete_offers_and_cleans_on_disconnect() {
        let mut state = Incoming::new();
        assert!(!state.offer(0, 1, 0.0));
        assert!(!state.offer(1, 0, 0.0));
        assert!(!state.offer(1, MAX_BATCH_FILES + 1, 0.0));
        assert!(!state.offer(1, 1, f64::NAN));
        assert!(state.offer(1, 2, 2.0));
        assert_eq!(state.receive(1, files()), Err(files()));
        state.reset();
        assert!(state.offer(2, 1, -1.0));
        assert_eq!(state.receive(2, files()), Ok(()));
        assert_eq!(state.reset(), files());
        assert!(state.commit(2).is_none());
    }
}
