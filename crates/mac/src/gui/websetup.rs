//! Setup and pairing window (modal WKWebView, ui/setup.html).
//!
//! Runs modally so it works before the app's main run loop starts (first launch, permission
//! check) and from the menu/settings ("Pair a device…"). The pairing invitation lives here;
//! its events are drained every 250 ms by `knit_setup_tick` and pushed to the page.
use super::*;
use knit_common::pairing::{Approval, Event, Invitation, LIFETIME};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};

const HTML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/setup.html"));

unsafe extern "C" {
    fn knit_setup_run(html: *const c_char, width: f64, height: f64) -> i64;
    fn knit_setup_stop(code: i64);
    fn knit_setup_eval(js: *const c_char);
    fn knit_webui_open_url(url: *const c_char);
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mode {
    /// Welcome -> permission -> pairing -> startup
    First,
    /// Pairing only (menu / settings "Pair a device…")
    Pair,
    /// Accessibility only (permission missing on a later launch)
    Permission,
}

/// Exit codes of the modal window
pub(super) const FINISHED: i64 = 1;
pub(super) const LATER: i64 = 2;

struct Session {
    token: Option<String>,
    preview: bool,
    invitation: Option<Invitation>,
    until: std::time::SystemTime,
    code: String,
    /// idle / wait / confirm / done / ended
    phase: &'static str,
    message: String,
    request: Option<Approval>,
    peer: Option<String>,
    registered: bool,
    startup: Option<bool>,
    /// Preview only: when the fake request arrives (ms since epoch)
    demo_at: u64,
    last: String,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// Show the window modally. Returns (exit code, whether a device registered).
pub(super) fn run(mode: Mode, token: Option<String>, preview: bool) -> (i64, bool) {
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        token,
        preview,
        invitation: None,
        until: std::time::SystemTime::now(),
        code: String::new(),
        phase: "idle",
        message: String::new(),
        request: None,
        peer: None,
        registered: false,
        startup: None,
        demo_at: 0,
        last: String::new(),
    });
    let boot = json!({
        "mode": match mode { Mode::First => "first", Mode::Pair => "pair", Mode::Permission => "perm" },
        "preview": preview,
    });
    let html = cstring(&HTML.replacen("/*KNIT_BOOT*/", &format!("window.KNIT_BOOT={boot};"), 1));
    let code = unsafe { knit_setup_run(html.as_ptr(), 640.0, 560.0) };
    // Dropping the session closes the invitation listener; an unanswered request is denied.
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    let registered = session.as_ref().is_some_and(|s| s.registered);
    if let Some(Session { request: Some(r), .. }) = session {
        r.deny();
    }
    (code, registered)
}

/// Label for the switch hotkey (shared with the tray)
pub(super) fn hotkey_label() -> String {
    match crate::HOTKEY_KC.load(Ordering::Relaxed) {
        97 => "F6".into(),
        100 => "F8".into(),
        105 => "F13".into(),
        54 => "Right ⌘".into(),
        n => format!("Key {n}"),
    }
}

fn state(s: &Session) -> Value {
    let remain = knit_common::pairing::remaining(s.until, std::time::SystemTime::now()).as_secs();
    json!({
        "preview": s.preview,
        "ax": unsafe { AXIsProcessTrusted() },
        "startup": s.startup.unwrap_or_else(crate::desktop_settings::startup_enabled),
        "hotkey": hotkey_label(),
        "addrs": knit_common::discover::local_ipv4s().iter().map(|ip| ip.to_string()).collect::<Vec<_>>(),
        "pair": {
            "phase": s.phase,
            "code": s.code,
            "remain": remain,
            "message": s.message,
            "peer": s.peer,
            "request": s
                .request
                .as_ref()
                .map(|r| json!({"name": r.name, "addr": r.peer.ip().to_string(), "sas": r.sas}))
                .or_else(|| (s.preview && s.phase == "confirm").then(preview_request)),
        },
    })
}

fn push(s: &mut Session, force: bool) {
    let text = state(s).to_string();
    if !force && text == s.last {
        return;
    }
    s.last = text.clone();
    let js = cstring(&format!("window.knit&&knit.state({text})"));
    unsafe { knit_setup_eval(js.as_ptr()) };
}

/// Every 250 ms while the window is open (timer in webui.m)
#[unsafe(no_mangle)]
pub extern "C" fn knit_setup_tick() {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return };
    if s.preview && s.demo_at != 0 && crate::now_ms() >= s.demo_at {
        s.demo_at = 0;
        s.phase = "confirm";
    }
    let mut ended = None;
    if let Some(inv) = s.invitation.as_ref() {
        while let Ok(event) = inv.events.try_recv() {
            match event {
                // One confirmation at a time; turn away anything that arrives meanwhile
                Event::Approval(r) if s.request.is_some() => r.deny(),
                Event::Approval(r) => {
                    s.phase = "confirm";
                    s.message.clear();
                    s.request = Some(r);
                }
                Event::Withdrawn => {
                    s.request = None;
                    s.phase = "wait";
                    s.message = "相手が登録の要求を取り消しました。".into();
                }
                Event::AttemptFailed(left) => {
                    s.message = format!("接続を確認できませんでした。コードを確かめてください（残り {left} 回）。");
                }
                Event::Registered(addr) => {
                    s.registered = true;
                    s.phase = "done";
                    if s.peer.is_none() {
                        s.peer = Some(addr.ip().to_string());
                    }
                }
                Event::Expired => ended = Some("コードの有効期限が切れました。New Code で作り直せます。"),
                Event::Locked => ended = Some("3回の接続試行に達しました。New Code で作り直せます。"),
            }
        }
    }
    if let Some(text) = ended {
        s.invitation = None;
        s.request = None;
        s.phase = "ended";
        s.message = text.into();
    }
    push(s, false);
}

/// Message from setup.html
#[unsafe(no_mangle)]
pub extern "C" fn knit_setup_on_message(json: *const c_char) {
    if json.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(json) }.to_string_lossy().into_owned();
    let Ok(msg) = serde_json::from_str::<Value>(&text) else { return };
    let cmd = msg["cmd"].as_str().unwrap_or("").to_string();
    // Stopping the modal must happen without holding the session lock
    let stop = match cmd.as_str() {
        "finish" => Some(FINISHED),
        "later" => Some(LATER),
        "close" => Some(0),
        _ => None,
    };
    if let Some(code) = stop {
        unsafe { knit_setup_stop(code) };
        return;
    }
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return };
    match cmd.as_str() {
        "openAx" => {
            let url = cstring("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
            unsafe { knit_webui_open_url(url.as_ptr()) };
        }
        "pairStart" | "newCode" => open_invitation(s),
        "pairStop" => {
            s.invitation = None;
            s.phase = "idle";
        }
        "allow" | "deny" => {
            let allow = cmd == "allow";
            if let Some(r) = s.request.take() {
                s.peer = Some(r.name.clone());
                if allow { r.approve() } else { r.deny() }
            }
            if s.preview {
                s.peer = Some("DESKTOP-PREVIEW".into());
                if allow {
                    s.registered = true;
                }
            }
            s.phase = if allow && s.preview { "done" } else { "wait" };
            s.message = if allow { "許可しました。登録を待っています…".into() } else { "要求を拒否しました。".into() };
        }
        "startup" => {
            let on = msg["value"].as_bool().unwrap_or(true);
            if !s.preview {
                if let Err(e) = crate::desktop_settings::set_startup_enabled(on) {
                    s.message = e;
                }
            }
            s.startup = Some(on);
        }
        _ => {}
    }
    push(s, true);
}

fn open_invitation(s: &mut Session) {
    s.request = None;
    s.message.clear();
    s.until = std::time::SystemTime::now() + LIFETIME;
    if s.preview {
        s.code = "123 456".into();
        s.phase = "wait";
        s.demo_at = crate::now_ms() + 3000;
        return;
    }
    let Some(token) = s.token.clone() else {
        s.phase = "ended";
        s.message = "接続キーが無いため登録を始められません。".into();
        return;
    };
    let Some(registry) = crate::ensure_registry(&token).map(|r| r.clone()) else {
        s.phase = "ended";
        s.message = "端末登録の台帳を開けませんでした。データ置き場を確かめてからやり直してください。".into();
        return;
    };
    let port = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|a| a[0] == "--port")
        .and_then(|a| a[1].parse().ok())
        .unwrap_or(24900);
    s.invitation = None; // close any previous listener before reopening the port
    match Invitation::open(registry, port) {
        Ok(inv) => {
            s.code = format!("{} {}", &inv.code[..3], &inv.code[3..]);
            s.invitation = Some(inv);
            s.phase = "wait";
        }
        Err(_) => {
            s.phase = "ended";
            s.message = "登録の待ち受けを始められませんでした。別の登録画面が開いていないか確かめてください。".into();
        }
    }
}

/// The fake request shown in preview
fn preview_request() -> Value {
    json!({"name": "DESKTOP-PREVIEW", "addr": "192.168.1.42", "sas": "4829"})
}
