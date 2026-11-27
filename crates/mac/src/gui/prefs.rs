//! メニューバーと保存処理が使う、設定まわりの小さな共通部品。
//! 設定画面そのものは webui.rs(値の読み書き)と ui/app.html(見た目)が担う。
//! 旧 AppKit 版の設定画面(prefs/refined*・settings_new)は WebView 版へ置き換えて削除した。
use super::*;
use std::sync::Mutex;
use super::localization::{self, Language};

/// 保存エラーの出元。設定画面には出元ごとに最新の 1 件を出す
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SaveErrorSource {
    /// 接続先(KNIT_HOST)の保存
    Host,
    /// preferences.json 自体が壊れていた(起動時に退避して初期設定で起動した)。
    /// 次の保存成功で外れる(preferences::save が解除する)
    PrefsFile,
    /// peer-sides.json(端末の配置と名前)が壊れて退避された(tap が検知して
    /// gui::report_peer_sides_error から設定する)。保存の成功で外れる
    PeerSidesFile,
}

/// 出元ごとの未解決エラー。同じ出元の更新だけを置き換える
static SAVE_ERROR_NOTE: Mutex<Vec<(SaveErrorSource, String)>> = Mutex::new(Vec::new());
/// 直近の保存状態の文言(設定画面の表示用に保持する)
static LAST_SAVE_STATUS: Mutex<String> = Mutex::new(String::new());
/// 役割切替の進行(0=通常 1=環境変数で固定 2=確認待ち 3=確認済み 4=タイムアウト)。
/// 切替ハンドラと Ack 待ちスレッドが書き、設定画面が role_note_text で読む
pub(super) static ROLE_NOTE_KIND: AtomicUsize = AtomicUsize::new(0);

/// 保存状態行へ並べる環境変数キー列。2件までは全部、3件以上は先頭+「ほかN件」
fn env_keys_summary(keys: &[&str]) -> String {
    let px = |s: &str| s.chars().map(|c| if c.is_ascii() { 6 } else { 11 }).sum::<usize>();
    if keys.len() <= 2 {
        keys.join("・")
    } else if px(&keys[..2].join("・")) <= 200 {
        format!("{} ほか{}件", keys[..2].join("・"), keys.len() - 2)
    } else {
        format!("{} ほか{}件", keys[0], keys.len() - 1)
    }
}

/// 保存を明示的に行った直後のステータス文言(固定されている環境変数キーも伝える)
pub(super) fn save_status_saved(keys: &[&str]) -> String {
    match keys.len() {
        0 => "変更を保存しました".to_string(),
        _ => format!("保存済み・起動時は環境変数 {} が優先", env_keys_summary(keys)),
    }
}

pub(super) unsafe fn set_save_status(text: &str) {
    *LAST_SAVE_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = text.into();
}

/// 役割の表示は設定画面が毎秒の状態送信で行うため、ここでは何もしない
/// (旧画面のラジオボタン同期の呼び出し口を残している)
pub(super) unsafe fn sync_role() {}

/// 役割切替の進行の文言(設定画面の「このPCの役割」の注記)
pub(super) fn role_note_text() -> Option<&'static str> {
    match ROLE_NOTE_KIND.load(Ordering::Relaxed) {
        2 => Some("切り替えを相手へ送りました。相手の適用を確認しています…"),
        3 => Some("相手の適用を確認しました。接続を張り直します…"),
        4 => Some("相手の適用確認が取れないため、時間経過で接続を張り直します…"),
        _ => None,
    }
}

pub(super) fn host_validation_error(bad: &str) -> String {
    format!(
        "192.168.1.23の形式で入力してください({}はIPアドレスとして読めません)",
        knit_common::history::truncate_chars(bad, 30)
    )
}

/// 状態行が「自動で再接続します」を出して良いか。PAIRED は接続キーの保存だけでも
/// 立つため、接続中 PEERS と peer-sides.json の保存件数で登録の実体が 1件でも
/// あるときだけ true にする(メニューの状態行と設定画面が共用する)
pub(super) fn paired_registered(paired: bool, peers_live: usize, saved: usize) -> bool {
    paired && (peers_live > 0 || saved > 0)
}

pub(super) fn set_save_error(source: SaveErrorSource, note: String) {
    let mut errors = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner());
    errors.retain(|(existing, _)| *existing != source);
    errors.push((source, note));
}

/// 出元が一致するときだけ None へ(他の出元の未解決エラーは残す)
fn cleared_save_error(
    source: SaveErrorSource,
    current: Option<(SaveErrorSource, String)>,
) -> Option<(SaveErrorSource, String)> {
    match current {
        Some((s, _)) if s == source => None,
        other => other,
    }
}

/// 保存が成功したら、その出元のエラーだけを外す
pub(super) fn clear_save_error(source: SaveErrorSource) {
    let mut note = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner());
    *note = std::mem::take(&mut *note)
        .into_iter()
        .filter_map(|error| cleared_save_error(source, Some(error)))
        .collect();
}

/// 出元ごとの未解決エラー(設定画面で、その欄の下に出す)
pub(super) fn save_error_for(source: SaveErrorSource) -> Option<String> {
    let errors = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner());
    errors
        .iter()
        .rev()
        .find(|(s, _)| *s == source)
        .map(|(_, note)| localization::copy_for(localization::language(), note))
}

/// ファイルの退避など、欄を持たないエラーをまとめた文言
pub(super) fn save_error_summary() -> Option<String> {
    let errors = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner());
    save_error_display(localization::language(), &errors)
}

fn save_error_display(language: Language, errors: &[(SaveErrorSource, String)]) -> Option<String> {
    let notes: Vec<_> = errors
        .iter()
        .filter(|(s, _)| *s != SaveErrorSource::Host)
        .rev()
        .map(|(_, note)| localization::copy_for(language, note))
        .collect();
    (!notes.is_empty()).then(|| notes.join(" · "))
}

/// menuWillOpen:/menuDidClose: は NSMenuDelegate の固定メソッド名のため、
/// 履歴サブメニュー(再構築の保留制御)・登録台帳・メニューバー本体が共有する。
/// sender(menu)で区別する
pub(super) unsafe extern "C" fn peer_menu_open(_s: ID, _c: SEL, menu: ID) {
    if super::history_menu_tracking(menu) {
        super::history_menu_opened();
    } else if super::registry_menu_tracking(menu) {
        super::registry_menu_opened();
    } else if super::main_menu_tracking(menu) {
        super::main_menu_opened();
    }
}

pub(super) unsafe extern "C" fn peer_menu_close(_s: ID, _c: SEL, menu: ID) {
    if super::history_menu_tracking(menu) {
        super::history_menu_closed();
    } else if super::registry_menu_tracking(menu) {
        super::registry_menu_closed();
    } else if super::main_menu_tracking(menu) {
        super::main_menu_closed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_needs_a_real_registration() {
        assert!(!paired_registered(false, 1, 1));
        assert!(!paired_registered(true, 0, 0));
        assert!(paired_registered(true, 1, 0));
        assert!(paired_registered(true, 0, 2));
    }

    #[test]
    fn clearing_one_source_keeps_the_other() {
        let host = Some((SaveErrorSource::Host, "a".to_string()));
        assert_eq!(cleared_save_error(SaveErrorSource::Host, host.clone()), None);
        assert_eq!(cleared_save_error(SaveErrorSource::PrefsFile, host.clone()), host);
    }

    #[test]
    fn env_keys_summary_shortens_long_lists() {
        assert_eq!(env_keys_summary(&["A", "B"]), "A・B");
        assert_eq!(env_keys_summary(&["A", "B", "C"]), "A・B ほか1件");
        assert_eq!(save_status_saved(&[]), "変更を保存しました");
    }

    #[test]
    fn host_errors_are_not_in_the_summary() {
        let errors = vec![
            (SaveErrorSource::Host, "host".to_string()),
            (SaveErrorSource::PrefsFile, "file".to_string()),
        ];
        let text = save_error_display(Language::Japanese, &errors).unwrap();
        assert!(text.contains("file") && !text.contains("host"));
    }

    #[test]
    fn role_note_is_shown_only_while_switching() {
        ROLE_NOTE_KIND.store(0, Ordering::Relaxed);
        assert!(role_note_text().is_none());
        ROLE_NOTE_KIND.store(2, Ordering::Relaxed);
        assert!(role_note_text().is_some());
        ROLE_NOTE_KIND.store(0, Ordering::Relaxed);
    }
}
