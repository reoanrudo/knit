//! GUI設定だけを保存し、認証情報や接続先の設定と分離する。
use super::*;
use serde_json::{json, Value};

fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/tsunagu/preferences.json"))
}

pub(super) fn has_overrides() -> bool {
    [
        "TSUNAGU_SIDE",
        "TSUNAGU_SWITCH_MODE",
        "TSUNAGU_EDGE_TAPS",
        "TSUNAGU_HOTKEY_KC",
        "TSUNAGU_SWITCH_DELAY",
        "TSUNAGU_DOUBLE_TAP_MS",
        "TSUNAGU_SCROLL_DIV",
        "TSUNAGU_MOUSE_SCALE",
        "TSUNAGU_EDGE_PX",
        "TSUNAGU_SCROLL_FLIP",
        "TSUNAGU_SCROLL_COMPAT",
        "TSUNAGU_CMD_ALT",
        "TSUNAGU_MUTE_SPK",
        "TSUNAGU_CLIP",
    ]
    .iter()
    .any(|key| crate::envutil::get(key).is_some())
}

pub(super) fn snapshot() -> Value {
    json!({
        "version": 1, "hotkey": crate::hotkey_kc(), "side": crate::SIDE.load(Ordering::Relaxed),
        "hotkey_only": crate::HOTKEY_ONLY.load(Ordering::Relaxed),
        "edge_taps": crate::EDGE_TAPS.load(Ordering::Relaxed),
        "delay": crate::SWITCH_DELAY_MS.load(Ordering::Relaxed),
        "double_tap": crate::DOUBLE_TAP_MS.load(Ordering::Relaxed),
        "scroll_div": crate::scroll_div(), "mouse_scale": crate::mouse_scale(),
        "edge_px": crate::edge_px(), "scroll_flip": crate::SCROLL_FLIP.load(Ordering::Relaxed),
        "scroll_compat": crate::SCROLL_COMPAT.load(Ordering::Relaxed),
        "cmd_alt": crate::CMD_ALT.load(Ordering::Relaxed),
        "spk_mute": crate::SPK_MUTE.load(Ordering::Relaxed),
        "clip_share": crate::CLIP_SHARE.load(Ordering::Relaxed),
        "audio_muted": crate::audio::MUTED.load(Ordering::Relaxed),
        "layout_range": *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner())
    })
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

fn write_to(path: &std::path::Path, value: &Value) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("保存先がありません"))?;
    std::fs::create_dir_all(parent)?;
    tsunagu_common::history::restrict_dir(parent);
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    f.sync_all()?;
    drop(f);
    // 履歴と同じく所有者のみ(600)で保存する(umask 既定の 644 に任せない)
    tsunagu_common::history::restrict(&tmp);
    std::fs::rename(tmp, path)
}

pub(super) fn restore() {
    let Some(path) = path() else { return };
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    // 旧版が 644 で作ったファイルを読めたら 600 へ是正する
    tsunagu_common::history::restrict(&path);
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
    boolean("hotkey_only", &crate::HOTKEY_ONLY);
    boolean("scroll_flip", &crate::SCROLL_FLIP);
    boolean("scroll_compat", &crate::SCROLL_COMPAT);
    boolean("cmd_alt", &crate::CMD_ALT);
    boolean("spk_mute", &crate::SPK_MUTE);
    boolean("clip_share", &crate::CLIP_SHARE);
    boolean("audio_muted", &crate::audio::MUTED);
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
    if let Some(n) = v["edge_px"].as_f64().filter(|n| n.is_finite()) {
        crate::set_edge_px(n);
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
    #[test]
    fn restored_values_are_bounded_and_invalid_layout_is_ignored() {
        let before = snapshot();
        *crate::LAY_RANGE.lock().unwrap() = (0.2, 0.8);
        apply(
            &json!({"scroll_div": -20.0, "mouse_scale": 99.0, "edge_taps": 0,
            "delay": 9000, "side": 999, "layout_range": [0.9, 0.1]}),
        );
        assert_eq!(crate::scroll_div(), 20.0);
        assert_eq!(crate::mouse_scale(), 3.0);
        assert_eq!(crate::EDGE_TAPS.load(Ordering::Relaxed), 1);
        assert_eq!(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed), 5000);
        assert_eq!(*crate::LAY_RANGE.lock().unwrap(), (0.2, 0.8));
        apply(&before);
        let dir = std::env::temp_dir().join(format!(
            "tsunagu-prefs-test-{}-{}",
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
        assert!(write_to(&path.join("invalid.json"), &before).is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            changed
        );
        std::fs::remove_dir_all(dir).unwrap();
        apply(&before);
    }
}
