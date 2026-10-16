//! 持ち運べる設定アーカイブ。認証・接続先・OSのログイン登録は含めない。
//! データをすべて検証した後、desktop → screen → preferencesの順に適用する。
//! 役割/辺/旧共通範囲は既存のlive接続を変更しないため、importの対象外。
use super::*;
use serde_json::{json, Map, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024;
// 認証や将来の副作用付きキーを、snapshot/外部JSONから無条件で転送しない。
const PREFERENCE_KEYS: &[&str] = &[
    "version",
    "ui_language",
    "hotkey",
    "side",
    "client_role",
    "hotkey_only",
    "edge_taps",
    "delay",
    "double_tap",
    "scroll_div",
    "mouse_scale",
    "scroll_flip",
    "scroll_compat",
    "cmd_alt",
    "spk_mute",
    "clip_share",
    "share_files",
    "local_history",
    "audio_muted",
    "audio_gain",
    "audio_output_uid",
    "android_pinch",
    "android_navigation",
    "layout_range",
    "own_name",
    "diag_log",
    "update_check",
    "env_overrides",
];
const PRESERVED_KEYS: &[&str] = &["client_role", "side", "layout_range", "env_overrides"];

fn sandboxed() -> bool {
    cfg!(test) || UI_PREVIEW.load(Ordering::Relaxed)
}

pub(super) fn export_to(path: &Path) -> Result<(), String> {
    // パス取得やsnapshotより先に拒否し、preview/testが実configの移行まで起こさない。
    if sandboxed() {
        return Err("プレビュー・テストでは実ファイルへの設定書き出しを行いません".into());
    }
    write_archive(path, &archive_snapshot())
}

pub(super) fn import_from(path: &Path) -> Result<(), String> {
    if sandboxed() {
        return Err("プレビュー・テストでは実ファイルからの設定読み込みを行いません".into());
    }
    let value = read_archive(path)?;
    let plan = ImportPlan::parse(&value)?;
    // GUIのメインスレッドが入口。別のarchive適用同士も重ねない。
    static APPLY: Mutex<()> = Mutex::new(());
    let _apply = APPLY.lock().unwrap_or_else(|e| e.into_inner());
    apply_plan(&plan, &mut LiveSettings)
}

fn filtered_preferences(value: &Value, importing: bool) -> Value {
    Value::Object(
        value
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| {
                PREFERENCE_KEYS.contains(&key.as_str())
                    && (!importing || !PRESERVED_KEYS.contains(&key.as_str()))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn archive_snapshot() -> Value {
    compose_archive(
        &preferences::snapshot(),
        &crate::desktop_settings::snapshot(),
        crate::screen_settings::snapshot_json(),
    )
}

fn compose_archive(
    preferences: &Value,
    desktop: &crate::desktop_settings::Settings,
    screen: Value,
) -> Value {
    json!({
        "version": 2,
        "preferences": filtered_preferences(preferences, false),
        "desktop": {
            "notify_connection": desktop.notify_connection,
            "notify_transfer": desktop.notify_transfer,
            "receive_folder": desktop.receive_folder,
            "open_receive_folder": desktop.open_receive_folder,
            "history_limit": desktop.history_limit,
        },
        "screen": screen,
    })
}

#[derive(Debug)]
struct ImportPlan {
    preferences: Value,
    desktop: Option<crate::desktop_settings::Settings>,
    screen: Option<Value>,
}

impl ImportPlan {
    fn parse(value: &Value) -> Result<Self, String> {
        let map = value
            .as_object()
            .ok_or("設定アーカイブはJSONオブジェクトが必要です")?;
        match map.get("version").and_then(Value::as_u64) {
            Some(1) => Ok(Self {
                preferences: validate_preferences(value)?,
                desktop: None,
                screen: None,
            }),
            Some(2) => {
                check_keys(
                    map,
                    &["version", "preferences", "desktop", "screen"],
                    "アーカイブ",
                )?;
                if let Some(preferences) = value["preferences"].as_object() {
                    check_keys(preferences, PREFERENCE_KEYS, "preferences")?;
                }
                let preferences = validate_preferences(&value["preferences"])?;
                let desktop = parse_desktop(&value["desktop"])?;
                crate::screen_settings::validate_json(&value["screen"])?;
                Ok(Self {
                    preferences,
                    desktop: Some(desktop),
                    screen: Some(value["screen"].clone()),
                })
            }
            _ => Err("対応していないKnit設定アーカイブの版です（version 1 / 2のみ）".into()),
        }
    }
}

fn check_keys(map: &Map<String, Value>, allowed: &[&str], name: &str) -> Result<(), String> {
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{name}: 未対応の項目です: {key}"));
    }
    Ok(())
}

fn validate_preferences(value: &Value) -> Result<Value, String> {
    let map = value
        .as_object()
        .ok_or("preferencesはオブジェクトが必要です")?;
    if map.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("preferencesの版はversion 1が必要です".into());
    }
    // 除外するlive接続のキーも、値が不正ならファイル全体を拒否する。
    preferences::validate_patch(value)?;
    for (key, low, high) in [
        ("hotkey", 1, 127),
        ("side", 0, 7),
        ("edge_taps", 1, 3),
        ("double_tap", 100, 3000),
    ] {
        if let Some(value) = map.get(key) {
            if !value.as_u64().is_some_and(|n| (low..=high).contains(&n)) {
                return Err(format!("{key}: {low}〜{high}の整数が必要です"));
            }
        }
    }
    if let Some(value) = map.get("delay") {
        if !value
            .as_u64()
            .is_some_and(|n| n == 0 || (50..=1000).contains(&n))
        {
            return Err("delay: 0または50〜1000の整数が必要です".into());
        }
    }
    if let Some(value) = map.get("scroll_div") {
        if !value
            .as_f64()
            .is_some_and(|n| n.is_finite() && (20.0..=240.0).contains(&n))
        {
            return Err("scroll_div: 20〜240の有限値が必要です".into());
        }
    }
    if let Some(value) = map.get("env_overrides") {
        if !value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string))
        {
            return Err("env_overrides: 文字列の配列が必要です".into());
        }
    }
    if let Some(value) = map.get("own_name").and_then(Value::as_str) {
        if knit_common::proto::safe_peer_name(value.trim()) != value.trim() {
            return Err("own_name: 制御文字のない48文字以内の表示名が必要です".into());
        }
    }
    // 旧v1の未知キーは従来どおり無視するが、commitやexportへは流さない。
    Ok(filtered_preferences(value, true))
}

fn parse_desktop(value: &Value) -> Result<crate::desktop_settings::Settings, String> {
    let map = value.as_object().ok_or("desktopはオブジェクトが必要です")?;
    check_keys(
        map,
        &[
            "notify_connection",
            "notify_transfer",
            "receive_folder",
            "open_receive_folder",
            "history_limit",
        ],
        "desktop",
    )?;
    let boolean = |key: &str| {
        value[key]
            .as_bool()
            .ok_or_else(|| format!("desktop.{key}: 真偽値が必要です"))
    };
    let receive_folder = value["receive_folder"]
        .as_str()
        .ok_or("desktop.receive_folder: 文字列が必要です")?;
    if receive_folder.contains('\0')
        || (!receive_folder.is_empty() && !Path::new(receive_folder).is_absolute())
    {
        return Err(
            "受信フォルダは既存の絶対パスで指定してください（空欄はDownloads/Knit）".into(),
        );
    }
    // 新キーは任意: 無ければ既定値のまま読み込む(旧アーカイブとの互換性)
    let open_receive_folder = value
        .get("open_receive_folder")
        .map(|v| {
            v.as_bool()
                .ok_or("desktop.open_receive_folder: 真偽値が必要です".to_string())
        })
        .transpose()?
        .unwrap_or(false);
    let history_limit = value
        .get("history_limit")
        .map(|v| {
            let n = v
                .as_u64()
                .ok_or("desktop.history_limit: 10/50/100/200のいずれかが必要です".to_string())?;
            if [10u64, 50, 100, 200].contains(&n) {
                Ok(n as usize)
            } else {
                Err("desktop.history_limit: 10/50/100/200のいずれかが必要です".to_string())
            }
        })
        .transpose()?
        .unwrap_or(50);
    Ok(crate::desktop_settings::Settings {
        notify_connection: boolean("notify_connection")?,
        notify_transfer: boolean("notify_transfer")?,
        receive_folder: receive_folder.into(),
        open_receive_folder,
        history_limit,
    })
}

// 副作用を注入できるようにし、失敗順・rollbackを実設定なしで検証する。
trait SettingsBackend {
    fn preferences(&self) -> Value;
    fn desktop(&self) -> crate::desktop_settings::Settings;
    fn screen(&self) -> Value;
    fn apply_desktop(&mut self, value: crate::desktop_settings::Settings) -> Result<(), String>;
    fn import_screen(&mut self, value: &Value) -> Result<(), String>;
    fn restore_screen(&mut self, value: &Value) -> Result<(), String>;
    fn commit_preferences(&mut self, value: &Value) -> Result<(), String>;
}

struct LiveSettings;
impl SettingsBackend for LiveSettings {
    fn preferences(&self) -> Value {
        filtered_preferences(&preferences::snapshot(), true)
    }
    fn desktop(&self) -> crate::desktop_settings::Settings {
        crate::desktop_settings::snapshot()
    }
    fn screen(&self) -> Value {
        crate::screen_settings::snapshot_json()
    }
    fn apply_desktop(&mut self, value: crate::desktop_settings::Settings) -> Result<(), String> {
        crate::desktop_settings::apply(value)
    }
    fn import_screen(&mut self, value: &Value) -> Result<(), String> {
        crate::screen_settings::import_json(value)
    }
    fn restore_screen(&mut self, value: &Value) -> Result<(), String> {
        crate::screen_settings::restore_json(value)
    }
    fn commit_preferences(&mut self, value: &Value) -> Result<(), String> {
        preferences::commit_patch(value)
    }
}

fn patch_changes(before: &Value, patch: &Value) -> bool {
    patch.as_object().is_some_and(|map| {
        map.iter()
            .any(|(key, value)| before.get(key) != Some(value))
    })
}

fn apply_plan(plan: &ImportPlan, backend: &mut impl SettingsBackend) -> Result<(), String> {
    let before_preferences = backend.preferences();
    let before_desktop = backend.desktop();
    let before_screen = backend.screen();
    let mut desktop_changed = false;
    let mut screen_changed = false;
    let result: Result<(), String> = (|| {
        // desktopの実フォルダ・権限・転送中検査も、後段の変更より先に完了する。
        if let Some(desktop) = &plan.desktop {
            if desktop != &before_desktop {
                backend
                    .apply_desktop(desktop.clone())
                    .map_err(|e| format!("desktop: {e}"))?;
                desktop_changed = true;
            }
        }
        if let Some(screen) = &plan.screen {
            if patch_changes(&before_screen["peers"], &screen["peers"]) {
                backend
                    .import_screen(screen)
                    .map_err(|e| format!("screen: {e}"))?;
                screen_changed = true;
            }
        }
        if patch_changes(&before_preferences, &plan.preferences) {
            backend
                .commit_preferences(&plan.preferences)
                .map_err(|e| format!("preferences: {e}"))?;
        }
        Ok(())
    })();
    let Err(error) = result else { return Ok(()) };
    let mut rollback_errors = Vec::new();
    // 各APIは失敗安全が基本。念のため部分適用が検出された場合だけpreferencesも戻す。
    // 成功したpreferencesの後には失敗し得る操作を置かない（音声出力の再操作を減らす）。
    if backend.preferences() != before_preferences {
        if let Err(e) = backend.commit_preferences(&before_preferences) {
            rollback_errors.push(format!("preferences: {e}"));
        }
    }
    if screen_changed {
        // import_jsonはmergeなので、追加peerの削除も含むexact復元APIを使う。
        if let Err(e) = backend.restore_screen(&before_screen) {
            rollback_errors.push(format!("screen: {e}"));
        }
    }
    if desktop_changed {
        if let Err(e) = backend.apply_desktop(before_desktop) {
            rollback_errors.push(format!("desktop: {e}"));
        }
    }
    if rollback_errors.is_empty() {
        Err(format!("{error}。読み込みを中止し、変更を元に戻しました"))
    } else {
        Err(format!(
            "{error}。設定の復元にも失敗しました: {}",
            rollback_errors.join(" / ")
        ))
    }
}

// macOSのO_NOFOLLOW/O_NONBLOCK。Linuxでは純粋なhelperの単体試験にも使える。
#[cfg(target_os = "macos")]
const READ_FLAGS: i32 = 0x100 | 0x4;
#[cfg(not(target_os = "macos"))]
const READ_FLAGS: i32 = 0x20000 | 0x800;

/// 明示的な外部パス専用。testはこのhelperに一時ファイルだけを渡す。
fn read_archive(path: &Path) -> Result<Value, String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|e| format!("ファイルを確認できません: {e}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("設定アーカイブにはリンクではなく通常ファイルを指定してください".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(READ_FLAGS)
        .open(path)
        .map_err(|e| format!("ファイルを読めません: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err("設定アーカイブは4MiB以下の通常ファイルが必要です".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("ファイルを読めません: {e}"))?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err("設定アーカイブは4MiB以下が必要です".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("設定ファイルとして読めません: {e}"))
}

struct TemporaryFile(PathBuf);
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 既存parentのみ使用。外部parentをmkdir/chmodせず、0600排他的temp→renameで保存。
fn write_archive(path: &Path, value: &Value) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|e| format!("設定を変換できません: {e}"))?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err("設定アーカイブが4MiBを超えるため書き出せません".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("保存先フォルダを確認できません: {e}"))?;
    if !parent.is_dir() {
        return Err("保存先の親はフォルダが必要です".into());
    }
    let name = path.file_name().ok_or("保存先ファイル名がありません")?;
    let target = parent.join(name);
    match std::fs::symlink_metadata(&target) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("保存先はリンクではない通常ファイルを指定してください".into());
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("保存先を確認できません: {e}")),
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let temp_path = parent.join(temp_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp_path)
        .map_err(|e| format!("書き出し用ファイルを作れません: {e}"))?;
    let _temporary = TemporaryFile(temp_path.clone());
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("設定を書き出せません: {e}"))?;
    std::fs::rename(&temp_path, &target)
        .map_err(|e| format!("設定の書き出しを確定できません: {e}"))?;
    // rename後のsync失敗で「未保存」とは返さない（保存済みファイルと結果を一致させる）。
    if let Ok(dir) = File::open(&parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive() -> Value {
        json!({"version":2,"preferences":{"version":1,"ui_language":"en","audio_gain":1.25,
            "client_role":true,"side":7,"layout_range":[0.1,0.8]},
            "desktop":{"notify_connection":false,"notify_transfer":false,"receive_folder":"/knit-test/inbox","open_receive_folder":true,"history_limit":100},
            "screen":{"version":1,"peers":{"new-peer":{"source":[0.1,0.8],"target":[0.2,0.9],"sensitivity":1.5}}}})
    }

    #[test]
    fn version_two_validates_all_sections_and_preserves_live_keys() {
        let plan = ImportPlan::parse(&archive()).unwrap();
        for key in PRESERVED_KEYS {
            assert!(plan.preferences.get(*key).is_none(), "{key}");
        }
        assert_eq!(plan.preferences["audio_gain"], json!(1.25));
        let desktop = plan.desktop.unwrap();
        assert_eq!(desktop.receive_folder, "/knit-test/inbox");
        assert!(desktop.open_receive_folder);
        assert_eq!(desktop.history_limit, 100);
        for pointer in [
            "/desktop/notify_connection",
            "/desktop/open_receive_folder",
            "/desktop/history_limit",
            "/preferences/audio_gain",
            "/screen/peers/new-peer/sensitivity",
        ] {
            let mut invalid = archive();
            *invalid.pointer_mut(pointer).unwrap() = json!("invalid");
            assert!(ImportPlan::parse(&invalid).is_err(), "{pointer}");
        }
        for invalid in [
            json!([]),
            json!({"version":3}),
            json!({"version":2}),
            json!({"version":2,"preferences":{},"desktop":{},"screen":{}}),
        ] {
            assert!(ImportPlan::parse(&invalid).is_err());
        }
        let mut invalid = archive();
        invalid["desktop"]["receive_folder"] = json!("relative");
        assert!(ImportPlan::parse(&invalid).is_err());
        let mut invalid = archive();
        invalid["desktop"]["history_limit"] = json!(30);
        assert!(ImportPlan::parse(&invalid).is_err());
        // 新キーが無い旧アーカイブも既定値で読める
        let mut legacy = archive();
        legacy["desktop"]
            .as_object_mut()
            .unwrap()
            .remove("open_receive_folder");
        legacy["desktop"]
            .as_object_mut()
            .unwrap()
            .remove("history_limit");
        let desktop = ImportPlan::parse(&legacy).unwrap().desktop.unwrap();
        assert!(!desktop.open_receive_folder && desktop.history_limit == 50);
        let mut invalid = archive();
        invalid["startup_enabled"] = json!(true);
        assert!(ImportPlan::parse(&invalid).is_err());
        let mut invalid = archive();
        invalid["preferences"]["token"] = json!("must-not-import");
        assert!(ImportPlan::parse(&invalid).is_err());
    }

    #[test]
    fn snapshot_contains_only_portable_sections_without_os_registration() {
        // 実設定や環境変数の参照も避けるため、明示したsnapshotだけを合成する。
        let snapshot = compose_archive(
            &json!({"version":1,"audio_output_uid":"test-output","startup_enabled":true,"token":"secret"}),
            &crate::desktop_settings::Settings::default(),
            json!({"version":1,"peers":{}}),
        );
        assert_eq!(snapshot["version"], 2);
        assert!(snapshot["preferences"]["audio_output_uid"].is_string());
        assert_eq!(snapshot["screen"]["version"], 1);
        let desktop = snapshot["desktop"].as_object().unwrap();
        assert_eq!(desktop.len(), 5);
        assert!(!snapshot.to_string().contains("startup_enabled"));
        assert!(snapshot.get("authentication").is_none());
        assert!(ImportPlan::parse(&snapshot).is_ok());
    }

    #[test]
    fn legacy_preferences_are_partial_and_unknown_or_auth_keys_never_commit() {
        let plan = ImportPlan::parse(&json!({"version":1,"audio_gain":0.5,"client_role":true,
            "side":2,"layout_range":[0.2,0.8],"token":"secret","startup_enabled":true}))
        .unwrap();
        assert!(plan.desktop.is_none() && plan.screen.is_none());
        assert_eq!(plan.preferences, json!({"version":1,"audio_gain":0.5}));
        let exported = filtered_preferences(
            &json!({"version":1,"token":"secret","startup_enabled":true}),
            false,
        );
        assert_eq!(exported, json!({"version":1}));
        for (key, value) in [
            ("hotkey", json!(128)),
            ("side", json!(8)),
            ("edge_taps", json!(0)),
            ("delay", json!(1)),
            ("double_tap", json!(99)),
            ("scroll_div", json!(241)),
            ("client_role", json!("true")),
            ("layout_range", json!([0.8, 0.2])),
        ] {
            let mut value_map = json!({"version":1});
            value_map[key] = value;
            assert!(ImportPlan::parse(&value_map).is_err(), "{key}");
        }
    }

    struct FakeSettings {
        preferences: Value,
        desktop: crate::desktop_settings::Settings,
        screen: Value,
        calls: Vec<&'static str>,
        fail: Vec<&'static str>,
        mutate_preferences_on_failure: bool,
    }
    impl FakeSettings {
        fn new() -> Self {
            Self {
                preferences: json!({"version":1,"ui_language":"ja","audio_gain":1.0}),
                desktop: crate::desktop_settings::Settings::default(),
                screen: json!({"version":1,"peers":{"old-peer":{"source":[0.0,1.0],"target":[0.0,1.0],"sensitivity":1.0}}}),
                calls: Vec::new(),
                fail: Vec::new(),
                mutate_preferences_on_failure: false,
            }
        }
        fn call(&mut self, name: &'static str) -> Result<(), String> {
            self.calls.push(name);
            if self.fail.contains(&name) {
                Err(format!("injected {name}"))
            } else {
                Ok(())
            }
        }
    }
    impl SettingsBackend for FakeSettings {
        fn preferences(&self) -> Value {
            self.preferences.clone()
        }
        fn desktop(&self) -> crate::desktop_settings::Settings {
            self.desktop.clone()
        }
        fn screen(&self) -> Value {
            self.screen.clone()
        }
        fn apply_desktop(
            &mut self,
            value: crate::desktop_settings::Settings,
        ) -> Result<(), String> {
            self.call(if value == crate::desktop_settings::Settings::default() {
                "rollback-desktop"
            } else {
                "desktop"
            })?;
            self.desktop = value;
            Ok(())
        }
        fn import_screen(&mut self, value: &Value) -> Result<(), String> {
            self.call("screen")?;
            self.screen["peers"]
                .as_object_mut()
                .unwrap()
                .extend(value["peers"].as_object().unwrap().clone());
            Ok(())
        }
        fn restore_screen(&mut self, value: &Value) -> Result<(), String> {
            self.call("rollback-screen")?;
            self.screen = value.clone();
            Ok(())
        }
        fn commit_preferences(&mut self, value: &Value) -> Result<(), String> {
            let name = if value["ui_language"] == "ja" {
                "rollback-preferences"
            } else {
                "preferences"
            };
            let result = self.call(name);
            if result.is_ok() || self.mutate_preferences_on_failure {
                self.preferences
                    .as_object_mut()
                    .unwrap()
                    .extend(value.as_object().unwrap().clone());
            }
            result
        }
    }

    #[test]
    fn transaction_order_and_merge_preserve_unlisted_peers() {
        let mut backend = FakeSettings::new();
        apply_plan(&ImportPlan::parse(&archive()).unwrap(), &mut backend).unwrap();
        assert_eq!(backend.calls, ["desktop", "screen", "preferences"]);
        assert!(backend.screen["peers"].get("old-peer").is_some());
        assert!(backend.screen["peers"].get("new-peer").is_some());
    }

    #[test]
    fn failures_rollback_in_reverse_and_remove_added_peers() {
        for (failure, expected) in [
            ("desktop", vec!["desktop"]),
            ("screen", vec!["desktop", "screen", "rollback-desktop"]),
            (
                "preferences",
                vec![
                    "desktop",
                    "screen",
                    "preferences",
                    "rollback-screen",
                    "rollback-desktop",
                ],
            ),
        ] {
            let mut backend = FakeSettings::new();
            let before = (backend.preferences(), backend.desktop(), backend.screen());
            backend.fail = vec![failure];
            let error =
                apply_plan(&ImportPlan::parse(&archive()).unwrap(), &mut backend).unwrap_err();
            assert!(error.contains(failure));
            assert_eq!(backend.calls, expected);
            assert_eq!(
                (backend.preferences(), backend.desktop(), backend.screen()),
                before
            );
        }
    }

    #[test]
    fn rollback_failure_is_reported_without_skipping_other_restores() {
        let mut backend = FakeSettings::new();
        backend.fail = vec![
            "preferences",
            "rollback-preferences",
            "rollback-screen",
            "rollback-desktop",
        ];
        backend.mutate_preferences_on_failure = true;
        let error = apply_plan(&ImportPlan::parse(&archive()).unwrap(), &mut backend).unwrap_err();
        for failure in &backend.fail {
            assert!(error.contains(failure), "{error}");
        }
        assert_eq!(
            backend.calls,
            [
                "desktop",
                "screen",
                "preferences",
                "rollback-preferences",
                "rollback-screen",
                "rollback-desktop"
            ]
        );
    }

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "knit-archive-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            // new()が作成・canonicalizeした専用ディレクトリだけを削除する。
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn explicit_tempfile_helpers_roundtrip_0600_without_chmod_parent() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TestDirectory::new();
        std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = dir.0.join("settings.json");
        write_archive(&file, &archive()).unwrap();
        assert_eq!(read_archive(&file).unwrap(), archive());
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
            0o755
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_archive(&file, &json!({"version":1})).unwrap();
        assert_eq!(read_archive(&file).unwrap(), json!({"version":1}));
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
        assert!(write_archive(&dir.0.join("missing/settings.json"), &archive()).is_err());
        assert!(!dir.0.join("missing").exists());
    }

    #[test]
    fn links_and_invalid_sources_are_rejected_without_touching_victim() {
        use std::os::unix::fs::symlink;
        let dir = TestDirectory::new();
        let victim = dir.0.join("victim.json");
        let link = dir.0.join("link.json");
        std::fs::write(&victim, b"original").unwrap();
        symlink(&victim, &link).unwrap();
        assert!(write_archive(&link, &archive()).is_err());
        assert!(read_archive(&link).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"original");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(read_archive(&dir.0).is_err());
        assert!(read_archive(&victim).is_err());
        let large = dir.0.join("large.json");
        File::create(&large)
            .unwrap()
            .set_len(MAX_ARCHIVE_BYTES + 1)
            .unwrap();
        assert!(read_archive(&large).is_err());
    }

    #[test]
    fn public_io_is_disabled_before_access_in_tests() {
        assert!(sandboxed());
        let dir = TestDirectory::new();
        let file = dir.0.join("must-not-exist.json");
        assert!(export_to(&file).unwrap_err().contains("テスト"));
        assert!(import_from(&file).unwrap_err().contains("テスト"));
        assert!(!file.exists());
    }
}
