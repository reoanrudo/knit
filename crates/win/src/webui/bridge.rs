//! 設定画面(WebView2)の状態と操作。ui/app.html が期待する JSON を組み立て、
//! ページからの操作を既存の Rust 側(共有・音声・役割・設定・更新)へ委譲する。
//! Mac 側 gui/webui.rs と同じ分担で、このファイルは値の変換だけを持つ。
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// 診断の実行中フラグ(ページの「Checking…」表示に使う)
static DIAG_RUNNING: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------- 状態

pub(super) fn state() -> Value {
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    let rtt = crate::RTT_MS.load(Ordering::Relaxed);
    let cap = knit_common::share::env_cap();
    let registered = crate::tray::registered();
    let host_mode = crate::tray::effective_host_mode();
    let (upd_state, upd_version, upd_reason, upd_enabled) = crate::updater::ui_state();
    json!({
        "os": "win",
        "preview": crate::tray::preview_mode(),
        "version": env!("CARGO_PKG_VERSION"),
        "myName": my_name(),
        "settings": settings(),
        "caps": { "clip": cap.clip, "files": cap.files, "audio": cap.audio },
        // Windows 版に音声の出力先選択は無い(行は出さない)
        "outputs": [],
        "outputStatus": Value::Null,
        "receiveRestart": false,
        // Windows にアクセシビリティ許可は無い(診断ページの行は出さない)
        "perm": { "ax": true },
        "conn": {
            "connected": connected,
            "rtt": rtt,
            "route": if connected { crate::tray::route_label().unwrap_or("") } else { "" },
            "status": status_word(connected, registered),
            "role": if host_mode { "host" } else { "client" },
            "roleLocked": crate::tray::role_env_fixed(),
            "roleNote": crate::tray::role_note(),
            "host": crate::tray::host_text(),
            "hostError": "",
        },
        "saveError": "",
        "addrs": knit_common::discover::local_ipv4s()
            .iter()
            .map(|ip| ip.to_string())
            .collect::<Vec<_>>(),
        "displays": displays(),
        "peers": peers(),
        "update": {
            "state": upd_state,
            "version": upd_version,
            "reason": upd_reason,
            "enabled": upd_enabled,
        },
        "diag": { "running": DIAG_RUNNING.load(Ordering::Relaxed) },
        "win": {
            "side": crate::SIDE_W.load(Ordering::Relaxed),
            "unlockSync": crate::tray::UNLOCK_SYNC.load(Ordering::Relaxed),
            "unlockSecretSet": crate::tray::unlock_secret_stored(),
        },
    })
}

fn settings() -> Value {
    let s = crate::receive_dir();
    json!({
        // Windows 版は install.bat(タスク スケジューラ)で起動を登録する
        "startup": true,
        "hotkey": 105,
        "language": "ja",
        "update_check": true,
        "clip_share": knit_common::share::user_clip(),
        "share_files": knit_common::share::user_files(),
        "history_enabled": true,
        "history_limit": 50,
        "receive_folder": s.to_string_lossy(),
        "open_receive_folder": false,
        "audio_on": crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed),
        "audio_gain": 1.0,
        "audio_output_uid": "",
        "spk_mute": crate::SPK_MUTE_MODE.load(Ordering::Relaxed),
        "notify_transfer": true,
        "notify_clipboard": true,
        "notify_connection": true,
        "notify_update": true,
        // 環境変数で固定されている項目(ページは操作を無効にして注記する)
        "locked": {
            "hotkey": false,
            "update_check": false,
            "clip_share": knit_common::share::env_cap().clip == false,
            "history_enabled": false,
            "audio_on": !knit_common::share::env_cap().audio,
            "audio_gain": false,
            "spk_mute": false,
            "ui_language": true,
        },
    })
}

fn my_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "このPC".into())
}

fn status_word(connected: bool, registered: bool) -> String {
    if connected {
        return "Connected".into();
    }
    if !registered {
        return "Not paired".into();
    }
    crate::next_retry_line().unwrap_or_else(|| "Searching…".into())
}

/// このPCの画面(仮想スクリーン全体を1枚として返す)
fn displays() -> Vec<Value> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    let (x, y, w, h) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    vec![json!({
        "x": x, "y": y, "w": w.max(1), "h": h.max(1), "main": true, "name": "このPC",
    })]
}

/// 相手(Mac)。接続していない・相手が分からない間は空
fn peers() -> Vec<Value> {
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    let ip = crate::peer_ip().map(|ip| ip.to_string()).unwrap_or_default();
    if ip.is_empty() && !connected {
        return Vec::new();
    }
    let name = crate::conn::peer_display();
    let screen = displays()
        .first()
        .cloned()
        .unwrap_or_else(|| json!({"w": 1920, "h": 1080}));
    let (w, h) = (
        screen["w"].as_i64().unwrap_or(1920) as i64,
        screen["h"].as_i64().unwrap_or(1080) as i64,
    );
    vec![json!({
        "id": "peer",
        "name": name,
        "alias": "",
        "label": name,
        "ip": ip,
        "active": true,
        "ver": 0,
        "side": crate::SIDE_W.load(Ordering::Relaxed),
        "monitor": Value::Null,
        "source": [0.0, 1.0],
        "target": [0.0, 1.0],
        "sens": 1.0,
        "screen": [w, h],
        "monitors": [{ "x": 0, "y": 0, "w": w, "h": h, "name": name }],
    })]
}

// ---------------------------------------------------------------- 操作

/// プレビュー起動(--preview-ui)では保存・送信・再起動をしない(画面の確認だけ)。
/// 見た目と文言の確認中に実設定や接続を変えないため、Mac 側と同じ扱いにする
const PREVIEW_NOTE: &str = "プレビューのため実行しません";

pub(super) fn handle(cmd: &str, msg: &Value) -> Result<String, String> {
    let mutates = matches!(
        cmd,
        "set" | "role" | "saveHost" | "reconnect" | "unlock" | "historyClear" | "openFolder"
            | "restart" | "resetReg" | "resetAll" | "quit"
    );
    if mutates && crate::tray::preview_mode() {
        return Ok(PREVIEW_NOTE.to_string());
    }
    match cmd {
        "ready" => Ok(String::new()),
        "set" => set(msg["key"].as_str().unwrap_or(""), &msg["value"]),
        "role" => {
            crate::tray::switch_role(!msg["client"].as_bool().unwrap_or(false));
            Ok(String::new())
        }
        "saveHost" => crate::tray::save_host(msg["text"].as_str().unwrap_or("").trim()),
        "reconnect" => {
            crate::tray::restart("つなぎ直すため Knit を再起動します");
        }
        "diag" => {
            start_diag();
            Ok(String::new())
        }
        "update" => {
            crate::updater::on_click();
            Ok(String::new())
        }
        "unlock" => crate::tray::set_unlock_secret(msg["text"].as_str().unwrap_or("")),
        "historyClear" => {
            crate::history_clear();
            Ok("履歴をすべて消しました".into())
        }
        "openFolder" => {
            crate::tray::post_menu(crate::tray::MENU_OPENFOLDER);
            Ok(String::new())
        }
        "log" => {
            crate::tray::post_menu(crate::tray::MENU_OPENLOG);
            Ok(String::new())
        }
        "restart" => {
            crate::tray::post_menu(crate::tray::MENU_RESTART);
            Ok(String::new())
        }
        "resetReg" => {
            crate::tray::post_menu(crate::tray::MENU_RESETREG);
            Ok(String::new())
        }
        "resetAll" => {
            crate::tray::post_menu(crate::tray::MENU_RESETSETTINGS);
            Ok(String::new())
        }
        "quit" => {
            crate::tray::post_menu(crate::tray::MENU_QUIT);
            Ok(String::new())
        }
        "copy" => {
            let text = msg["text"].as_str().unwrap_or("");
            if crate::clipboard::clipboard_write_text(text) {
                Ok("コピーしました".into())
            } else {
                Err("コピーできませんでした".into())
            }
        }
        other => Err(format!("この操作は Windows 版にありません: {other}")),
    }
}

fn set(key: &str, value: &Value) -> Result<String, String> {
    let flag = || value.as_bool().ok_or_else(|| "値が正しくありません".to_string());
    let cap = knit_common::share::env_cap();
    match key {
        "clip_share" => {
            if !cap.clip {
                return Err("KNIT_SHARE の設定で無効になっています".into());
            }
            knit_common::share::set_user_clip(flag()?);
        }
        "share_files" => {
            if !cap.files {
                return Err("KNIT_SHARE の設定で無効になっています".into());
            }
            knit_common::share::set_user_files(flag()?);
        }
        "audio_on" => {
            if !knit_common::share::env_cap().audio {
                return Err("KNIT_SHARE の設定で無効になっています".into());
            }
            crate::audio::AUDIO_ENABLED.store(flag()?, Ordering::Relaxed);
        }
        "spk_mute" => crate::SPK_MUTE_MODE.store(flag()?, Ordering::Relaxed),
        "win_side" => {
            let side = value
                .as_u64()
                .filter(|n| *n <= 3)
                .ok_or("画面の位置が正しくありません")? as u8;
            crate::SIDE_W.store(side, Ordering::Relaxed);
            crate::state::set_mac_pref("side", json!(side));
            return Ok(String::new());
        }
        "unlock_sync" => crate::tray::UNLOCK_SYNC.store(flag()?, Ordering::Relaxed),
        "ui_language" => {
            return match value.as_str() {
                Some("ja") => Ok(String::new()),
                _ => Err("Windows 版は日本語のみです".into()),
            };
        }
        other => return Err(format!("この設定は Windows 版では変えられません: {other}")),
    }
    crate::tray::save_preferences()
}

fn start_diag() {
    if DIAG_RUNNING.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        let report = crate::diag::run();
        DIAG_RUNNING.store(false, Ordering::Relaxed);
        let text = serde_json::to_string(&report).unwrap_or_else(|_| "\"\"".into());
        super::eval_async(format!("window.knit&&knit.diag({text})"));
    });
}
