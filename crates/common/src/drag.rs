//! 本線の操作と、別接続で届くファイルを受け渡しIDで結びつける。
use std::path::PathBuf;

pub const MAX_FILES: usize = 64;

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
        if self.pending.is_some()
            || id == 0
            || count == 0
            || count > MAX_FILES
            || !position.is_finite()
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

#[cfg(test)]
mod tests {
    use super::*;
    fn files() -> Vec<PathBuf> {
        vec![PathBuf::from("received.txt")]
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
        assert!(!state.offer(1, MAX_FILES + 1, 0.0));
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
