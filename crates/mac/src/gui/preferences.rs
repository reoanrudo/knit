//! GUI設定だけを保存し、認証情報や接続先の設定と分離する。
use super::*;
use serde_json::{json, Value};

fn path() -> Option<std::path::PathBuf> {
    knit_common::envutil::config_dir().map(|dir| dir.join("preferences.json"))
}

pub(super) fn has_overrides() -> bool {
    [
        "KNIT_SIDE",
        "KNIT_SWITCH_MODE",
        "KNIT_EDGE_TAPS",
        "KNIT_HOTKEY_KC",
        "KNIT_SWITCH_DELAY",
        "KNIT_DOUBLE_TAP_MS",
        "KNIT_SCROLL_DIV",
        "KNIT_MOUSE_SCALE",
        "KNIT_SCROLL_FLIP",
        "KNIT_SCROLL_COMPAT",
        "KNIT_CMD_ALT",
        "KNIT_MUTE_SPK",
        "KNIT_CLIP",
        "KNIT_ROLE",
        "KNIT_LOCAL_HISTORY",
        "KNIT_AUDIO",
        "KNIT_AUDIO_GAIN",
        "KNIT_ANDROID",
        "KNIT_ANDROID_ADB",
        "KNIT_ANDROID_GAIN",
        "KNIT_ANDROID_SCROLL_FLIP",
    ]
    .iter()
    .any(|key| crate::envutil::get(key).is_some())
}

pub(super) fn snapshot() -> Value {
    json!({
        "version": 1, "hotkey": crate::hotkey_kc(), "side": crate::SIDE.load(Ordering::Relaxed),
        "client_role": crate::CLIENT_ROLE.load(Ordering::Relaxed),
        "hotkey_only": crate::HOTKEY_ONLY.load(Ordering::Relaxed),
        "edge_taps": crate::EDGE_TAPS.load(Ordering::Relaxed),
        "delay": crate::SWITCH_DELAY_MS.load(Ordering::Relaxed),
        "double_tap": crate::DOUBLE_TAP_MS.load(Ordering::Relaxed),
        "scroll_div": crate::scroll_div(), "mouse_scale": crate::mouse_scale(),
        "scroll_flip": crate::SCROLL_FLIP.load(Ordering::Relaxed),
        "scroll_compat": crate::SCROLL_COMPAT.load(Ordering::Relaxed),
        "cmd_alt": crate::CMD_ALT.load(Ordering::Relaxed),
        "spk_mute": crate::SPK_MUTE.load(Ordering::Relaxed),
        "clip_share": crate::CLIP_SHARE.load(Ordering::Relaxed),
        "share_files": knit_common::share::user_files(),
        "local_history": crate::LOCAL_HISTORY.load(Ordering::Relaxed),
        "audio_muted": crate::audio::MUTED.load(Ordering::Relaxed),
        "audio_gain": crate::audio::gain() as f64,
        "android_pinch": crate::trackpad::ENABLED.load(Ordering::Relaxed),
        "android_navigation": crate::trackpad::NAV_ENABLED.load(Ordering::Relaxed),
        "layout_range": *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner())
    })
}

/// 画面に触れない保存(別スレッドから呼ぶ用)
pub(super) fn save_quiet() {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    if let Some(path) = path() {
        if let Err(e) = write_to(&path, &snapshot()) {
            eprintln!("[prefs] save failed: {e}");
        }
    }
}

pub(super) fn save() {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    let result = (|| -> std::io::Result<()> {
        let path = path().ok_or_else(|| std::io::Error::other("HOME がありません"))?;
        write_to(&path, &snapshot())
    })();
    let status = if result.is_ok() {
        if has_overrides() {
            "保存済み · 再起動時は環境変数・envファイルの指定が優先されます"
        } else {
            "変更を保存しました"
        }
    } else {
        "設定を保存できません。ログを確認してください"
    };
    unsafe {
        prefs::set_save_status(status);
    }
    if let Err(e) = result {
        eprintln!("[prefs] save failed: {e}");
    }
}

/// 設定一覧を JSON 文字列で返す(Windows の設定画面への応答)
pub(super) fn snapshot_json() -> String {
    snapshot().to_string()
}

/// Windows から届いた設定変更を適用する。許可したキーだけを範囲検査つきで反映し、
/// 画面には触れず(別スレッドから呼ばれる)、ファイルへ保存する
pub(super) fn apply_remote(json: &str) {
    const ALLOWED: [&str; 21] = [
        "hotkey", "side", "hotkey_only", "edge_taps", "delay", "double_tap", "scroll_div",
        "mouse_scale", "scroll_flip", "scroll_compat", "cmd_alt", "spk_mute", "clip_share",
        "share_files", "local_history", "audio_muted", "audio_gain", "layout_range",
        "android_pinch", "android_navigation", "peer_layout_reset",
    ];
    if json.len() > 4096 || UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) else {
        return;
    };
    let mut filtered = serde_json::Map::new();
    for (k, v) in map {
        if ALLOWED.contains(&k.as_str()) {
            filtered.insert(k, v);
        }
    }
    // Android アプリは音声非対応。その接続中に音声設定を書き換えない
    if crate::active_peer_is_android_app() {
        filtered.remove("audio_muted");
    }
    // 配置の完全リセット(辺とモニター指定の両方を既定へ戻す。
    // Mac 設定画面の「配置を初期化」と同じ効果)
    let layout_reset = filtered.remove("peer_layout_reset").is_some_and(|v| v == json!(true));
    let v = Value::Object(filtered);
    if layout_reset {
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        if act != usize::MAX {
            crate::set_peer_side(act, 0, None);
        }
    }
    apply(&v);
    // 画面の部品を介さない設定(Android のジェスチャ)は専用の入口で反映する
    if let Some(on) = v["android_pinch"].as_bool() {
        crate::trackpad::set_enabled(on);
    }
    if let Some(on) = v["android_navigation"].as_bool() {
        crate::trackpad::set_navigation(on);
    }
    // 試験が利用者の設定ファイルを上書きしないよう、試験では保存しない
    if cfg!(test) {
        return;
    }
    if let Some(path) = path() {
        if let Err(e) = write_to(&path, &snapshot()) {
            eprintln!("[prefs] remote save failed: {e}");
        }
    }
}

fn write_to(path: &std::path::Path, value: &Value) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("保存先がありません"))?;
    std::fs::create_dir_all(parent)?;
    knit_common::history::restrict_dir(parent);
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    // 履歴と同じく 0600 で作成する(作成後に chmod する方式だと共有 Mac で
    // 僅かな時間他ユーザーに読まれる窓が残る)
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(tmp, path)
}

pub(super) fn restore() {
    let Some(path) = path() else { return };
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    // 旧版が 644 で作ったファイルを読めたら 600 へ是正する
    knit_common::history::restrict(&path);
    let v: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[prefs] invalid preferences: {e}");
            return;
        }
    };
    if v["version"].as_u64() != Some(1) {
        return;
    }
    apply(&v);
}

fn apply(v: &Value) {
    let boolean = |key: &str, atom: &AtomicBool| {
        if let Some(b) = v[key].as_bool() {
            atom.store(b, Ordering::Relaxed);
        }
    };
    if let Some(n) = v["hotkey"].as_i64().filter(|n| (1..=127).contains(n)) {
        crate::HOTKEY_KC.store(n, Ordering::Relaxed);
    }
    boolean("client_role", &crate::CLIENT_ROLE);
    boolean("hotkey_only", &crate::HOTKEY_ONLY);
    boolean("scroll_flip", &crate::SCROLL_FLIP);
    boolean("scroll_compat", &crate::SCROLL_COMPAT);
    boolean("cmd_alt", &crate::CMD_ALT);
    boolean("spk_mute", &crate::SPK_MUTE);
    boolean("clip_share", &crate::CLIP_SHARE);
    boolean("local_history", &crate::LOCAL_HISTORY);
    if let Some(on) = v["share_files"].as_bool() {
        knit_common::share::set_user_files(on);
    }
    boolean("audio_muted", &crate::audio::MUTED);
    if let Some(n) = v["audio_gain"].as_f64().filter(|n| n.is_finite()) {
        crate::audio::set_gain(n);
    }
    boolean("android_pinch", &crate::trackpad::ENABLED);
    boolean("android_navigation", &crate::trackpad::NAV_ENABLED);
    if let Some(n) = v["side"].as_u64().filter(|n| *n <= 7) {
        crate::set_side(n as u8);
    }
    if let Some(n) = v["edge_taps"].as_u64() {
        crate::EDGE_TAPS.store(n.clamp(1, 3) as u32, Ordering::Relaxed);
    }
    if let Some(n) = v["delay"].as_u64() {
        crate::SWITCH_DELAY_MS.store(n.min(5000), Ordering::Relaxed);
    }
    if let Some(n) = v["double_tap"].as_u64() {
        crate::DOUBLE_TAP_MS.store(n.clamp(100, 3000), Ordering::Relaxed);
    }
    if let Some(n) = v["scroll_div"].as_f64().filter(|n| n.is_finite()) {
        crate::set_scroll_div(n);
    }
    if let Some(n) = v["mouse_scale"].as_f64().filter(|n| n.is_finite()) {
        crate::set_mouse_scale(n);
    }
    if let (Some(a), Some(b)) = (v["layout_range"][0].as_f64(), v["layout_range"][1].as_f64()) {
        if a.is_finite() && b.is_finite() && a >= 0.0 && b <= 1.0 && a < b {
            *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = (a, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// この画面の設定は static そのもののため、テスト同士の干渉を直列化で防ぐ
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn remote_apply_ignores_unlisted_keys_and_oversized_input() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = snapshot();
        let role = crate::CLIENT_ROLE.load(Ordering::Relaxed);
        apply_remote(r#"{"client_role": true, "version": 9, "scroll_div": 140.0}"#);
        assert_eq!(crate::CLIENT_ROLE.load(Ordering::Relaxed), role);
        assert_eq!(crate::scroll_div(), 140.0);
        apply_remote(&format!(r#"{{"scroll_div": 40.0, "pad": "{}"}}"#, "x".repeat(5000)));
        assert_eq!(crate::scroll_div(), 140.0);
        apply(&before);
    }

    #[test]
    fn restored_values_are_bounded_and_invalid_layout_is_ignored() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = snapshot();
        *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = (0.2, 0.8);
        let side_before = crate::SIDE.load(Ordering::Relaxed);
        apply(
            &json!({"scroll_div": -20.0, "mouse_scale": 99.0, "edge_taps": 0,
            "delay": 9000, "side": 999, "layout_range": [0.9, 0.1]}),
        );
        assert_eq!(crate::scroll_div(), 20.0);
        assert_eq!(crate::mouse_scale(), 3.0);
        assert_eq!(crate::EDGE_TAPS.load(Ordering::Relaxed), 1);
        assert_eq!(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed), 5000);
        assert_eq!(
            crate::SIDE.load(Ordering::Relaxed),
            side_before,
            "side:999 は受理されない(apply の n <= 7 フィルタ)"
        );
        assert_eq!(*crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()), (0.2, 0.8));
        // 上限そのもの(7)は受理・直上(8)は拒否
        apply(&json!({"side": 8}));
        assert_eq!(crate::SIDE.load(Ordering::Relaxed), side_before, "side:8 は拒否");
        apply(&json!({"side": 7}));
        assert_eq!(crate::SIDE.load(Ordering::Relaxed), 7, "side:7 は受理");
        apply(&before);
        let dir = std::env::temp_dir().join(format!(
            "knit-prefs-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("preferences.json");
        write_to(&path, &before).unwrap();
        let mut changed = before.clone();
        changed["hotkey"] = json!(97);
        changed["side"] = json!(6);
        changed["scroll_div"] = json!(140.0);
        write_to(&path, &changed).unwrap();
        let loaded: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        apply(&loaded);
        assert_eq!(crate::hotkey_kc(), 97);
        assert_eq!(crate::SIDE.load(Ordering::Relaxed), 6);
        assert_eq!(crate::scroll_div(), 140.0);
        // ファイルの受け渡しの選択も保存・復元される(直後に元へ戻す)
        let mut share_off = before.clone();
        share_off["share_files"] = json!(false);
        apply(&share_off);
        assert!(!knit_common::share::user_files());
        assert_eq!(snapshot()["share_files"], json!(false));
        apply(&before);
        assert!(knit_common::share::user_files());
        assert!(write_to(&path.join("invalid.json"), &before).is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            changed
        );
        std::fs::remove_dir_all(dir).unwrap();
        apply(&before);
    }
}
