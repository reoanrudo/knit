use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
static STATES: OnceLock<Mutex<HashMap<String, (bool, bool, bool)>>> = OnceLock::new();
pub(crate) fn remember(id: &str, control: bool, keyboard: bool, japanese: bool) {
    STATES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_owned(), (control, keyboard, japanese));
}
pub(crate) fn get(id: &str) -> Option<(bool, bool)> {
    STATES
        .get()?
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .map(|(control, keyboard, _)| (*control, *keyboard))
}
pub(crate) fn japanese(id: &str) -> bool {
    STATES
        .get()
        .and_then(|states| {
            states
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .map(|(_, keyboard, japanese)| *keyboard && *japanese)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tablet_conversion_requires_selected_keyboard_and_ready_engine() {
        let id = "android-app-japanese-capability-test";
        assert!(!japanese(id));
        remember(id, true, true, false);
        assert_eq!(get(id), Some((true, true)));
        assert!(!japanese(id));
        remember(id, true, false, true);
        assert!(!japanese(id));
        remember(id, true, true, true);
        assert!(japanese(id));
    }
}
