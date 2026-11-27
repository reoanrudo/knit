//! Settings window (WKWebView). ui/app.html renders and takes input; this module reads and
//! writes values by delegating to the existing stores and actions.
//!
//! - app -> page: `knit.state(json)` (every second, only when changed), `knit.result(id, ok, msg)`,
//!   `knit.diag(text)`
//! - page -> app: a JSON string `{"id":n,"cmd":"…",…}` delivered by webui.m to knit_webui_on_message
//!
//! In preview (--preview-ui) nothing is saved, sent or restarted; values live in this process only.
//! Everything runs on the main thread (AppKit event delivery and the 1-second timer).
use super::*;
use serde_json::{json, Map, Value};
use std::ffi::{c_char, CStr, CString};

const HTML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/app.html"));

unsafe extern "C" {
    fn knit_webui_show(html: *const c_char, title: *const c_char, width: f64, height: f64);
    fn knit_webui_eval(js: *const c_char);
    fn knit_webui_visible() -> i32;
    fn knit_webui_close();
    /// mode: 0 = open file, 1 = save file, 2 = choose folder
    fn knit_webui_choose_path(mode: i32, name: *const c_char) -> *mut c_char;
    fn knit_webui_free(p: *mut c_char);
    fn knit_webui_open_url(url: *const c_char);
    fn knit_webui_copy(text: *const c_char);
    fn knit_run_later(f: extern "C" fn());
}

extern "C" fn open_registration() {
    unsafe { setup::show_registration(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Last state sent to the page (skip identical pushes)
static LAST_STATE: Mutex<String> = Mutex::new(String::new());
/// Preview-only values (never saved). Keys are `settings.<name>`
static PREVIEW: Mutex<Option<Map<String, Value>>> = Mutex::new(None);

fn preview() -> bool {
    UI_PREVIEW.load(Ordering::Relaxed)
}

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// Open the settings window (or bring it to front). page: gen / conn / share / notify / diag / upd
pub(super) fn show(page: Option<&str>) {
    // Preview can open a given page: --preview-page <0-5>
    let arg_page = std::env::args()
        .skip_while(|a| a != "--preview-page")
        .nth(1)
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|i| ["gen", "conn", "share", "notify", "diag", "upd"].get(i).copied());
    let page = page.or(arg_page.filter(|_| preview()));
    let boot = json!({
        "os": "mac",
        "preview": preview(),
        "version": env!("CARGO_PKG_VERSION"),
        "page": page.unwrap_or("gen"),
    });
    let html = HTML.replacen("/*KNIT_BOOT*/", &format!("window.KNIT_BOOT={boot};"), 1);
    LAST_STATE.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let html = cstring(&html);
    let title = cstring("Knit Settings");
    unsafe { knit_webui_show(html.as_ptr(), title.as_ptr(), 920.0, 660.0) };
}

pub(super) fn close() {
    unsafe { knit_webui_close() };
}

pub(super) fn visible() -> bool {
    unsafe { knit_webui_visible() != 0 }
}

fn eval(js: &str) {
    let js = cstring(js);
    unsafe { knit_webui_eval(js.as_ptr()) };
}

/// 1-second refresh (from refresh_status): push only while visible and changed
pub(super) fn tick() {
    if visible() {
        push(false);
    }
}

fn push(force: bool) {
    let s = state().to_string();
    {
        let mut last = LAST_STATE.lock().unwrap_or_else(|e| e.into_inner());
        if !force && *last == s {
            return;
        }
        *last = s.clone();
    }
    eval(&format!("window.knit&&knit.state({s})"));
}

fn reply(id: u64, ok: bool, message: &str) {
    eval(&format!("window.knit&&knit.result({id},{ok},{})", Value::String(message.to_string())));
}

/// Show diagnostics in the window. Returns false when hidden (caller falls back to an alert)
pub(super) fn show_diag(text: &str) -> bool {
    if !visible() {
        return false;
    }
    eval(&format!("window.knit&&knit.diag({})", Value::String(text.to_string())));
    true
}

// ---------------------------------------------------------------- state

fn state() -> Value {
    let s = settings_model::Settings::current();
    let locked = preferences::override_keys();
    let cap = knit_common::share::env_cap();
    let (upd_state, upd_version, upd_reason, upd_enabled) = crate::updater::ui_state();
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    let on = |a: &std::sync::atomic::AtomicBool| a.load(Ordering::Relaxed);
    let mut v = json!({
        "os": "mac",
        "preview": preview(),
        "version": env!("CARGO_PKG_VERSION"),
        "myName": my_name(),
        "settings": {
            "startup": s.startup,
            "hotkey": s.hotkey,
            "language": s.language.code(),
            "update_check": s.update_check,
            "clip_share": s.clip_share,
            "share_files": s.share_files,
            "history_enabled": s.history_enabled,
            "history_limit": s.history_limit,
            "receive_folder": crate::desktop_settings::receive_dir().to_string_lossy(),
            "open_receive_folder": s.open_receive_folder,
            "audio_on": !s.audio_muted,
            "audio_gain": (s.audio_gain * 100.0).round() / 100.0,
            "audio_output_uid": s.audio_output_uid,
            "spk_mute": s.spk_mute,
            "notify_transfer": on(&crate::NOTIFY_TRANSFER),
            "notify_clipboard": on(&crate::NOTIFY_CLIPBOARD),
            "notify_connection": on(&crate::NOTIFY_CONNECTION),
            "notify_update": on(&crate::NOTIFY_UPDATE),
            "locked": {
                "hotkey": locked.contains(&"KNIT_HOTKEY_KC"),
                "update_check": crate::envutil::get("KNIT_AUTO_UPDATE").as_deref() == Some("0"),
                "clip_share": locked.contains(&"KNIT_CLIP"),
                "history_enabled": locked.contains(&"KNIT_LOCAL_HISTORY"),
                "audio_on": locked.contains(&"KNIT_AUDIO"),
                "audio_gain": locked.contains(&"KNIT_AUDIO_GAIN"),
                "spk_mute": locked.contains(&"KNIT_MUTE_SPK"),
            },
        },
        "caps": { "clip": cap.clip, "files": cap.files, "audio": cap.audio },
        "outputs": crate::audio::output_devices().into_iter().map(|(uid, name)| json!([uid, name])).collect::<Vec<_>>(),
        "outputStatus": crate::audio::output_status(),
        "receiveRestart": crate::desktop_settings::receive_dir_restart_required(),
        "perm": { "ax": unsafe { AXIsProcessTrusted() } },
        "conn": {
            "connected": connected,
            "rtt": crate::RTT_MS.load(Ordering::Relaxed),
            "route": if connected { crate::route_label() } else { "" },
            "status": status_word(connected),
            "role": if crate::effective_client_role() { "client" } else { "host" },
            "roleLocked": crate::role_env_fixed(),
            "roleNote": prefs::role_note_text(),
            "host": crate::envutil::get("KNIT_HOST").unwrap_or_default(),
            "hostError": prefs::save_error_for(prefs::SaveErrorSource::Host),
        },
        "saveError": prefs::save_error_summary(),
        "addrs": knit_common::discover::local_ipv4s().iter().map(|ip| ip.to_string()).collect::<Vec<_>>(),
        "displays": displays(),
        "peers": peers(),
        "update": { "state": upd_state, "version": upd_version, "reason": upd_reason, "enabled": upd_enabled },
        "diag": { "running": diag_running() },
    });
    if preview() {
        overlay_preview(&mut v);
    }
    v
}

pub(super) fn my_name() -> String {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        match std::process::Command::new("scutil").args(["--get", "ComputerName"]).output() {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => "Mac".to_string(),
        }
    })
    .clone()
}

fn status_word(connected: bool) -> String {
    if connected {
        return "Connected".into();
    }
    if !prefs::paired_registered(
        crate::PAIRED.load(Ordering::Relaxed),
        crate::PEERS.lock().unwrap_or_else(|e| e.into_inner()).len(),
        crate::tap::saved_peer_count(),
    ) {
        return "Not paired".into();
    }
    crate::next_retry_line().unwrap_or_else(|| "Searching…".into())
}

fn displays() -> Vec<Value> {
    crate::tap::mac_displays()
        .iter()
        .map(|d| json!({"x": d.x, "y": d.y, "w": d.w, "h": d.h, "main": d.main, "name": d.name}))
        .collect()
}

fn peers() -> Vec<Value> {
    let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let active = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    peers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let ss = crate::screen_settings::get(&p.id, p.side);
            let alias = p.alias.clone().filter(|a| !a.trim().is_empty());
            json!({
                "id": p.id,
                "name": p.name,
                "alias": alias.clone().unwrap_or_default(),
                "label": alias.unwrap_or_else(|| p.name.clone()),
                "ip": p.ip.to_string(),
                "active": i == active,
                "ver": p.ver,
                "side": p.side,
                "monitor": p.edge_monitor.filter(|&m| m != usize::MAX),
                "source": [ss.source.0, ss.source.1],
                "target": [ss.target.0, ss.target.1],
                "sens": (ss.sensitivity * 10.0).round() / 10.0,
                "screen": [p.screen.0, p.screen.1],
                "monitors": p.monitors.iter().map(|m| json!({"x": m.x, "y": m.y, "w": m.w, "h": m.h, "name": m.name})).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// Preview: pretend to be connected and layer the values the user changed
fn overlay_preview(v: &mut Value) {
    let guard = PREVIEW.lock().unwrap_or_else(|e| e.into_inner());
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    v["conn"]["connected"] = json!(true);
    v["conn"]["rtt"] = json!(4 + secs % 3);
    v["conn"]["route"] = json!("Preview");
    v["conn"]["status"] = json!("Connected (preview)");
    if let Some(pv) = guard.as_ref() {
        for (key, value) in pv {
            if let Some(name) = key.strip_prefix("settings.") {
                v["settings"][name] = value.clone();
            }
        }
    }
}

fn preview_set(key: &str, value: Value) {
    let mut guard = PREVIEW.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(Map::new).insert(format!("settings.{key}"), value);
}

// ---------------------------------------------------------------- commands

/// Message from the page (called by webui.m on the main thread)
#[unsafe(no_mangle)]
pub extern "C" fn knit_webui_on_message(json: *const c_char) {
    if json.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(json) }.to_string_lossy().into_owned();
    let Ok(msg) = serde_json::from_str::<Value>(&text) else {
        eprintln!("[webui] unreadable message: {text}");
        return;
    };
    let id = msg["id"].as_u64().unwrap_or(0);
    let cmd = msg["cmd"].as_str().unwrap_or("");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { handle(cmd, &msg) }));
    match result {
        Ok(Ok(message)) => reply(id, true, &message),
        Ok(Err(message)) => reply(id, false, &message),
        Err(_) => reply(id, false, "内部エラーが起きました。ログを確かめてください"),
    }
    push(true);
}

/// Window closed (windowWillClose: in webui.m)
#[unsafe(no_mangle)]
pub extern "C" fn knit_webui_on_close() {
    if preview() {
        eprintln!("[ui-check] PASS: preview window closed; process exits without a status item");
        crate::clip::flush_image_store();
        std::process::exit(0);
    }
}

const PREVIEW_NOTE: &str = "プレビューのため実行しません";

unsafe fn handle(cmd: &str, msg: &Value) -> Result<String, String> {
    let peer_id = || msg["peer"].as_str().unwrap_or("").to_string();
    let null = std::ptr::null_mut();
    match cmd {
        "ready" => Ok(String::new()),
        "set" => set(msg["key"].as_str().unwrap_or(""), &msg["value"]),
        "layout" => layout(&peer_id(), msg),
        "sens" => sensitivity(&peer_id(), msg["value"].as_f64().unwrap_or(1.0)),
        "rename" => rename(&peer_id(), msg["name"].as_str().unwrap_or("")),
        "reconnect" => reconnect(&peer_id()),
        "role" => role(msg["client"].as_bool().unwrap_or(false)),
        "saveHost" if preview() => Ok(PREVIEW_NOTE.into()),
        "saveHost" => super::save_host_text(msg["text"].as_str().unwrap_or("").trim()),
        "folder" => choose_folder(),
        "openFolder" => open_url(&format!("file://{}", crate::desktop_settings::receive_dir().to_string_lossy())),
        "openAx" => open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"),
        "openNotify" => open_url("x-apple.systempreferences:com.apple.Notifications-Settings.extension"),
        "testNotify" if preview() => Ok(PREVIEW_NOTE.into()),
        "testNotify" => {
            crate::notify("Knit", "通知のテストです。このように表示されます");
            Ok("テスト通知を送りました".into())
        }
        "diag" if preview() => {
            show_diag("Preview: 診断は実行しません。");
            Ok(String::new())
        }
        "diag" => {
            imp_diag(null, null, null);
            Ok(String::new())
        }
        "update" if preview() => Ok(PREVIEW_NOTE.into()),
        "update" => {
            crate::updater::on_click();
            Ok(String::new())
        }
        "export" => export(),
        "import" => import(),
        "log" => save_log(),
        _ if preview() && matches!(cmd, "restart" | "pair" | "resetReg" | "resetAll") => Ok(PREVIEW_NOTE.into()),
        "restart" => {
            imp_restart(null, null, null);
            Ok(String::new())
        }
        "pair" => {
            knit_run_later(open_registration);
            Ok(String::new())
        }
        "resetReg" => {
            setup::reset_registration(null, null, null);
            Ok(String::new())
        }
        "resetAll" => {
            setup::reset_all_settings(null, null, null);
            Ok(String::new())
        }
        "copy" => {
            let t = cstring(msg["text"].as_str().unwrap_or(""));
            knit_webui_copy(t.as_ptr());
            Ok("コピーしました".into())
        }
        "quit" if preview() => std::process::exit(0),
        "quit" => {
            imp_quit(null, null, null);
            Ok(String::new())
        }
        other => Err(format!("Unknown command: {other}")),
    }
}

fn open_url(url: &str) -> Result<String, String> {
    let u = cstring(url);
    unsafe { knit_webui_open_url(u.as_ptr()) };
    Ok(String::new())
}

fn set(key: &str, value: &Value) -> Result<String, String> {
    if preview() {
        let name = if key == "ui_language" { "language" } else { key };
        preview_set(name, value.clone());
        return Ok(String::new());
    }
    let flag = || value.as_bool().ok_or_else(|| "値が正しくありません".to_string());
    // Notification categories live in preferences.json only
    if let Some(kind) = key.strip_prefix("notify_") {
        if !matches!(kind, "transfer" | "clipboard" | "connection" | "update") {
            return Err(format!("Unknown setting: {key}"));
        }
        preferences::commit_patch(&json!({ key: flag()? }))?;
        return Ok(String::new());
    }
    let mut next = settings_model::Settings::current();
    match key {
        "startup" => next.startup = flag()?,
        "hotkey" => {
            next.hotkey = value.as_i64().filter(|k| (1..=127).contains(k)).ok_or("キーコードは1〜127で指定してください")?
        }
        "ui_language" => {
            next.language = match value.as_str() {
                Some("ja") => localization::Language::Japanese,
                Some("en") => localization::Language::English,
                _ => return Err("言語が正しくありません".into()),
            }
        }
        "update_check" => next.update_check = flag()?,
        "clip_share" => next.clip_share = flag()?,
        "share_files" => next.share_files = flag()?,
        "history_enabled" => next.history_enabled = flag()?,
        "history_limit" => {
            next.history_limit = value
                .as_u64()
                .filter(|n| [10, 50, 100, 200].contains(n))
                .ok_or("保存件数は 10 / 50 / 100 / 200 から選んでください")? as u32
        }
        "open_receive_folder" => next.open_receive_folder = flag()?,
        "audio_on" => next.audio_muted = !flag()?,
        "audio_gain" => next.audio_gain = value.as_f64().ok_or("値が正しくありません")?.clamp(0.0, 2.0),
        "audio_output_uid" => next.audio_output_uid = value.as_str().unwrap_or("").to_string(),
        "spk_mute" => next.spk_mute = flag()?,
        other => return Err(format!("Unknown setting: {other}")),
    }
    settings_model::save(&next)?;
    crate::send_cfg();
    Ok(match key {
        "startup" if next.startup => "ログイン項目に登録しました".into(),
        "startup" => "ログイン項目から外しました".into(),
        _ => String::new(),
    })
}

fn choose_folder() -> Result<String, String> {
    if preview() {
        return Ok(PREVIEW_NOTE.into());
    }
    let Some(path) = choose(2, "") else {
        return Ok(String::new());
    };
    let mut next = settings_model::Settings::current();
    next.receive_folder = path.to_string_lossy().into_owned();
    settings_model::save(&next)?;
    Ok(if crate::desktop_settings::receive_dir_restart_required() {
        "保存しました。受信先は Knit の再起動後に切り替わります".into()
    } else {
        "受信フォルダを変えました".into()
    })
}

/// Screen layout: save the edge (side), which of my monitors, and the source/target ranges,
/// then apply to the runtime. Preview runs the real path too (stores are memory-only there).
fn layout(id: &str, msg: &Value) -> Result<String, String> {
    let side = msg["side"].as_u64().filter(|s| *s <= 3).ok_or("辺の向きが正しくありません")? as u8;
    let monitor = msg["monitor"].as_u64().map(|m| m as usize);
    let range = |v: &Value| -> Result<(f64, f64), String> {
        let r = (v[0].as_f64().unwrap_or(-1.0), v[1].as_f64().unwrap_or(-1.0));
        crate::screen_settings::valid_range(r).then_some(r).ok_or_else(|| "範囲が正しくありません".to_string())
    };
    let source = range(&msg["source"])?;
    let target = range(&msg["target"])?;
    let _changes = crate::PEER_CHANGES.lock().unwrap_or_else(|e| e.into_inner());
    let (current_side, others) = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let me = peers.iter().find(|p| p.id == id).ok_or("この相手はもう接続していません")?;
        let others: Vec<(String, u8, Option<usize>)> =
            peers.iter().filter(|p| p.id != id).map(|p| (p.id.clone(), p.side, p.edge_monitor)).collect();
        (me.side, others)
    };
    // Two peers on the same stretch of the same edge would make the destination ambiguous
    for (other, other_side, other_monitor) in &others {
        if *other_side == side && *other_monitor == monitor {
            let o = crate::screen_settings::get(other, *other_side).source;
            if source.0 < o.1 && o.0 < source.1 {
                return Err("ほかの相手と同じ辺の同じ区間には置けません".into());
            }
        }
    }
    let before = crate::screen_settings::get(id, current_side);
    let next = crate::screen_settings::PeerSettings { source, target, sensitivity: before.sensitivity };
    next.validate()?;
    crate::screen_settings::set_many(&[(id.to_string(), next)])?;
    if let Err(e) = crate::tap::apply_peer_placements(&[(id.to_string(), side, monitor)]) {
        if let Err(r) = crate::screen_settings::set_many(&[(id.to_string(), before)]) {
            return Err(format!("{e}。範囲の復元にも失敗しました: {r}"));
        }
        return Err(e);
    }
    if !preview() {
        crate::send_cfg();
    }
    Ok(String::new())
}

fn sensitivity(id: &str, value: f64) -> Result<String, String> {
    let value = (value.clamp(0.2, 3.0) * 10.0).round() / 10.0;
    let side = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        peers.iter().find(|p| p.id == id).map(|p| p.side).ok_or("この相手はもう接続していません")?
    };
    let mut s = crate::screen_settings::get(id, side);
    s.sensitivity = value;
    crate::screen_settings::set_many(&[(id.to_string(), s)])?;
    Ok(String::new())
}

fn rename(id: &str, name: &str) -> Result<String, String> {
    if preview() {
        return Ok(PREVIEW_NOTE.into());
    }
    let name = name.trim();
    let computer = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        peers.iter().find(|p| p.id == id).map(|p| p.name.clone()).ok_or("この相手はもう接続していません")?
    };
    let alias = (!name.is_empty() && name != computer).then_some(name);
    crate::tap::set_peer_alias(id, alias)?;
    Ok("名前を変えました".into())
}

fn reconnect(id: &str) -> Result<String, String> {
    if preview() {
        return Ok(PREVIEW_NOTE.into());
    }
    let active = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        peers.get(act).is_some_and(|p| p.id == id)
    };
    if active {
        if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
            crate::leave_win_mode_cursor_unlock(None);
        }
        crate::conn::drop_stream("reconnect from settings");
    } else {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = peers.iter().find(|p| p.id == id).and_then(|p| p.writer.as_ref()) {
            w.shutdown();
        }
    }
    Ok("つなぎ直しています".into())
}

unsafe fn role(client: bool) -> Result<String, String> {
    if preview() {
        return Ok(PREVIEW_NOTE.into());
    }
    if role_env_guard() {
        return Err("KNIT_ROLE で固定されています".into());
    }
    set_role(client);
    Ok("役割を切り替えています。両方の端末で接続し直します".into())
}

fn choose(mode: i32, name: &str) -> Option<std::path::PathBuf> {
    let name = cstring(name);
    unsafe {
        let p = knit_webui_choose_path(mode, name.as_ptr());
        if p.is_null() {
            return None;
        }
        let path = CStr::from_ptr(p).to_string_lossy().into_owned();
        knit_webui_free(p);
        Some(std::path::PathBuf::from(path))
    }
}

fn export() -> Result<String, String> {
    if preview() {
        return Ok(PREVIEW_NOTE.into());
    }
    let Some(path) = choose(1, "Knit-settings.json") else {
        return Ok(String::new());
    };
    settings_archive::export_to(&path).map_err(|e| format!("書き出せません: {e}"))?;
    Ok("設定を書き出しました".into())
}

fn import() -> Result<String, String> {
    if preview() {
        return Ok(PREVIEW_NOTE.into());
    }
    let Some(path) = choose(0, "") else {
        return Ok(String::new());
    };
    settings_archive::import_from(&path)?;
    crate::send_cfg();
    Ok("設定を読み込みました".into())
}

fn save_log() -> Result<String, String> {
    if preview() {
        return Ok(PREVIEW_NOTE.into());
    }
    let Some(path) = choose(1, "Knit.log") else {
        return Ok(String::new());
    };
    std::fs::copy("/tmp/knit-mac.log", &path).map_err(|e| format!("ログを保存できません: {e}"))?;
    Ok("ログを書き出しました".into())
}
