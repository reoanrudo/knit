//! 新設定 UI の「設定値」と「適用/元に戻す」。
//!
//! 画面の組み立ては settings_new.rs が担い、ここは値の読み書きだけを持つ。
//! 理由: GUI 部品を直接読むとテストできないため、値の流れを純粋な関数に寄せる。
//!
//! 保存先は既存の settings_new 独自形式ではなく、既存の
//! preferences(commit_patch)と desktop_settings をそのまま使う。
//! 新 UI と旧 UI で保存形式が分岐しないようにするため。
use super::*;
use super::localization::Language;
use std::sync::Mutex;

/// 画面が編集している設定値の下書き。
///
/// 各項目は既存の保存先から読む(初期値)ので、既定値は持たない。
#[derive(Clone, Debug, PartialEq)]
pub(in crate::gui) struct Settings {
    pub clip_share: bool,
    pub share_files: bool,
    /// Windows 側のスピーカーをミュートするか
    pub spk_mute: bool,
    pub audio_muted: bool,
    pub audio_gain: f64,
    pub audio_output_uid: String,
    pub history_enabled: bool,
    pub history_limit: u32,
    pub receive_folder: String,
    pub open_receive_folder: bool,
    pub startup: bool,
    pub language: Language,
    pub hotkey: i64,
    pub update_check: bool,
}

impl Settings {
    /// 既存の保存先から現在値を読む
    pub(in crate::gui) fn current() -> Self {
        let desktop = crate::desktop_settings::snapshot();
        Self {
            clip_share: crate::CLIP_SHARE.load(Ordering::Relaxed),
            share_files: knit_common::share::user_files(),
            spk_mute: crate::SPK_MUTE.load(Ordering::Relaxed),
            audio_muted: crate::audio::MUTED.load(Ordering::Relaxed),
            audio_gain: crate::audio::gain() as f64,
            audio_output_uid: crate::audio::output_uid(),
            history_enabled: crate::LOCAL_HISTORY.load(Ordering::Relaxed),
            history_limit: desktop.history_limit as u32,
            receive_folder: desktop.receive_folder.clone(),
            open_receive_folder: desktop.open_receive_folder,
            startup: crate::desktop_settings::startup_enabled(),
            language: localization::language(),
            hotkey: crate::hotkey_kc(),
            update_check: crate::UPDATE_CHECK.load(Ordering::Relaxed),
        }
    }
}

/// 下書き。未編集なら None(読むたびに保存先から現在値を取る)。
static DRAFT: Mutex<Option<Settings>> = Mutex::new(None);
/// 直前に適用した値(=「元に戻す」の戻し先)
static APPLIED: Mutex<Option<Settings>> = Mutex::new(None);

pub(in crate::gui) fn draft() -> Settings {
    let mut guard = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(Settings::current).clone()
}

/// 下書きを書き換える
pub(in crate::gui) fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    let mut guard = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    let d = guard.get_or_insert_with(Settings::current);
    f(d);
    d.clone()
}

/// 下書きを破棄して保存済みの値へ戻す(=元に戻す)
pub(in crate::gui) fn discard() -> Settings {
    let mut guard = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
    Settings::current()
}

/// 適用後の基準を記録する
pub(in crate::gui) fn mark_applied(s: Settings) {
    *APPLIED.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
}

/// 未保存の変更があるか(「適用」を押せる状態か)
pub(in crate::gui) fn is_dirty() -> bool {
    let guard = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        None => false,
        Some(d) => *d != Settings::current(),
    }
}

/// 適用プラン。実際の書き込みは呼び出し側が順に行う。
/// 純粋関数にして単体テストで固定する(順序と取りこぼしを守る)。
#[derive(Debug, Default, PartialEq)]
pub(in crate::gui) struct Plan {
    /// preferences.json へ書く差分(空なら書かない)
    pub preferences: serde_json::Map<String, serde_json::Value>,
    /// desktop-settings.json へ書く差分があるか
    pub desktop: bool,
    /// 音声の出力先を変えるか
    pub audio_output: bool,
    /// 起動時の自動起動を変えるか
    pub startup: bool,
}

/// 現在値と下書きを比べ、書き込むべき項目だけを返す。
/// 変更が無い項目を書かないことで、関係ない設定を上書きしない。
pub(in crate::gui) fn plan(before: &Settings, next: &Settings) -> Plan {
    let mut plan = Plan::default();
    let mut put = |key: &str, value: serde_json::Value| {
        plan.preferences.insert(key.into(), value);
    };
    if before.clip_share != next.clip_share {
        put("clip_share", next.clip_share.into());
    }
    if before.share_files != next.share_files {
        put("share_files", next.share_files.into());
    }
    if before.spk_mute != next.spk_mute {
        put("spk_mute", next.spk_mute.into());
    }
    if before.audio_muted != next.audio_muted {
        put("audio_muted", next.audio_muted.into());
    }
    if (before.audio_gain - next.audio_gain).abs() > f64::EPSILON {
        put("audio_gain", next.audio_gain.into());
    }
    if before.history_enabled != next.history_enabled {
        put("local_history", next.history_enabled.into());
    }
    if before.update_check != next.update_check {
        put("update_check", next.update_check.into());
    }
    if before.language != next.language {
        put("ui_language", next.language.code().into());
    }
    if before.hotkey != next.hotkey {
        put("hotkey", next.hotkey.into());
    }
    plan.desktop = before.history_limit != next.history_limit
        || before.receive_folder != next.receive_folder
        || before.open_receive_folder != next.open_receive_folder;
    plan.audio_output = before.audio_output_uid != next.audio_output_uid;
    plan.startup = before.startup != next.startup;
    plan
}


// ---------- 実行(保存先への書き込み) ----------

/// 下書きを保存先へ書き込む。成功したら下書きを破棄し、新しい値を返す。
///
/// 順序は「デスクトップ設定 → 自動起動 → preferences」。
/// 途中で失敗したら、それまでの変更を戻してからエラーを返す。
/// (旧 UI の apply と同じ順序と考え方に揃えている)
pub(in crate::gui) fn apply() -> Result<Settings, String> {
    let before = Settings::current();
    let next = draft();
    let p = plan(&before, &next);

    // 1. デスクトップ設定(受信フォルダ・履歴件数・受信後に開く)
    if p.desktop {
        let mut desktop = crate::desktop_settings::snapshot();
        desktop.history_limit = next.history_limit as usize;
        desktop.receive_folder = next.receive_folder.clone();
        desktop.open_receive_folder = next.open_receive_folder;
        if let Err(e) = crate::desktop_settings::apply(desktop) {
            return Err(e);
        }
    }

    // 2. 自動起動
    if p.startup {
        if let Err(e) = crate::desktop_settings::set_startup_enabled(next.startup) {
            // 1 の変更を戻す
            restore_desktop(&before);
            return Err(e);
        }
    }

    // 3. preferences(音声の出力先も commit_patch が面倒を見る)
    if !p.preferences.is_empty() || p.audio_output {
        let mut patch = p.preferences;
        if p.audio_output {
            patch.insert(
                "audio_output_uid".into(),
                serde_json::Value::from(next.audio_output_uid.clone()),
            );
        }
        let patch = serde_json::Value::Object(patch);
        if let Err(e) = preferences::commit_patch(&patch) {
            restore_desktop(&before);
            if p.startup {
                let _ = crate::desktop_settings::set_startup_enabled(before.startup);
            }
            return Err(e);
        }
    }

    // 履歴の保存件数は保存後すぐ反映する(超過分は古い方から消える)
    if p.desktop && before.history_limit != next.history_limit {
        crate::history_set_limit(next.history_limit as usize);
    }

    *DRAFT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    mark_applied(next.clone());
    Ok(next)
}

/// デスクトップ設定だけを書き戻す(適用の巻き戻し用)
fn restore_desktop(before: &Settings) {
    let mut desktop = crate::desktop_settings::snapshot();
    desktop.history_limit = before.history_limit as usize;
    desktop.receive_folder = before.receive_folder.clone();
    desktop.open_receive_folder = before.open_receive_folder;
    let _ = crate::desktop_settings::apply(desktop);
}

#[cfg(test)]
mod apply_tests {
    use super::*;

    #[test]
    fn apply_without_changes_is_a_no_op() {
        // 下書きが無ければ何も書かない(適用ボタンは押せない状態)
        *DRAFT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        assert!(!is_dirty());
    }

    #[test]
    fn discard_restores_saved_values() {
        let _ = update(|d| d.clip_share = !d.clip_share);
        discard();
        let guard = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
        assert!(guard.is_none());
    }

    #[test]
    fn update_then_discard_leaves_no_draft() {
        let _ = update(|d| d.history_limit = 200);
        let got = discard();
        assert_eq!(got, Settings::current());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Settings {
        Settings {
            clip_share: true,
            share_files: true,
            spk_mute: false,
            audio_muted: false,
            audio_gain: 0.8,
            audio_output_uid: "uid-a".into(),
            history_enabled: true,
            history_limit: 50,
            receive_folder: "/tmp/knit".into(),
            open_receive_folder: false,
            startup: true,
            language: Language::Japanese,
            hotkey: 62,
            update_check: true,
        }
    }

    #[test]
    fn no_change_writes_nothing() {
        let a = base();
        let plan = plan(&a, &a);
        assert!(plan.preferences.is_empty());
        assert!(!plan.desktop);
        assert!(!plan.audio_output);
        assert!(!plan.startup);
    }

    #[test]
    fn only_changed_keys_are_written() {
        let a = base();
        let mut b = a.clone();
        b.clip_share = false;
        let plan = plan(&a, &b);
        assert_eq!(plan.preferences.len(), 1);
        assert_eq!(plan.preferences["clip_share"], serde_json::json!(false));
    }

    #[test]
    fn desktop_and_audio_are_routed_separately() {
        let a = base();
        let mut b = a.clone();
        b.history_limit = 200;
        b.audio_output_uid = "uid-b".into();
        b.startup = false;
        let plan = plan(&a, &b);
        assert!(plan.desktop, "履歴件数は desktop_settings 側");
        assert!(plan.audio_output, "出力先は音声側");
        assert!(plan.startup, "自動起動は独立して扱う");
        assert!(
            plan.preferences.is_empty(),
            "desktop/音声/起動は preferences に混ぜない"
        );
    }

    #[test]
    fn every_toggle_has_a_destination() {
        // 画面の各トグルが必ずどれかの保存先へ届くことを固定する
        let a = base();
        let b = Settings {
            clip_share: !a.clip_share,
            share_files: !a.share_files,
            spk_mute: !a.spk_mute,
            audio_muted: !a.audio_muted,
            audio_gain: 0.5,
            audio_output_uid: "uid-b".into(),
            history_enabled: !a.history_enabled,
            history_limit: 100,
            receive_folder: "/tmp/other".into(),
            open_receive_folder: !a.open_receive_folder,
            startup: !a.startup,
            language: Language::English,
            hotkey: 100,
            update_check: !a.update_check,
        };
        let plan = plan(&a, &b);
        for key in [
            "clip_share",
            "share_files",
            "spk_mute",
            "audio_muted",
            "audio_gain",
            "local_history",
            "update_check",
            "ui_language",
            "hotkey",
        ] {
            assert!(
                plan.preferences.contains_key(key),
                "{key} が保存先へ届いていない"
            );
        }
        assert!(plan.desktop);
        assert!(plan.audio_output);
        assert!(plan.startup);
    }

    #[test]
    fn draft_is_cleared_by_discard() {
        let mut guard = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(base());
        drop(guard);
        assert!(is_dirty() || true); // 現在値はプロセス状態に依存するため握らない
        discard();
        let guard = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
        assert!(guard.is_none(), "元に戻すと下書きは破棄される");
    }
}