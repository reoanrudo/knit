//! Menu bar popover (WKWebView in an NSPopover, ui/tray.html).
//! Left click opens the popover; right / control / option click opens the classic menu,
//! which also stays one click away behind "More…".
use super::*;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};

const HTML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/tray.html"));

unsafe extern "C" {
    fn knit_tray_attach(item: ID, menu: ID, html: *const c_char, width: f64, height: f64);
    fn knit_tray_eval(js: *const c_char);
    fn knit_tray_shown() -> i32;
    fn knit_tray_resize(height: f64);
    fn knit_tray_close();
    fn knit_tray_show_menu();
    fn knit_webui_open_url(url: *const c_char);
    fn knit_run_later(f: extern "C" fn());
    fn knit_tray_open();
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

static LAST: Mutex<String> = Mutex::new(String::new());
/// Preview only: pretend to control the peer (the real toggle needs a connection)
static PREVIEW_CONTROLLING: AtomicBool = AtomicBool::new(false);

fn preview() -> bool {
    UI_PREVIEW.load(Ordering::Relaxed)
}

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// Replace the status item's menu with the popover (called once from gui::start)
pub(super) unsafe fn attach(item: ID, menu: ID) {
    let html = cstring(HTML);
    knit_tray_attach(item, menu, html.as_ptr(), 340.0, 360.0);
    // Preview: --preview-tray opens the popover right away (for screenshots and checks)
    if preview() && std::env::args().any(|a| a == "--preview-tray") {
        knit_run_later(open_tray);
    }
}

extern "C" fn open_tray() {
    unsafe { knit_tray_open() };
}

/// 1-second refresh while the popover is open
pub(super) fn tick() {
    if unsafe { knit_tray_shown() } != 0 {
        push(false);
    }
}

fn state() -> Value {
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    let (peer, peers_live) = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        let p = peers.get(act).or_else(|| peers.first());
        (p.map(|p| p.alias.clone().filter(|a| !a.trim().is_empty()).unwrap_or_else(|| p.name.clone())), peers.len())
    };
    let paired = prefs::paired_registered(crate::PAIRED.load(Ordering::Relaxed), peers_live, crate::tap::saved_peer_count());
    let (upd, version, _, _) = crate::updater::ui_state();
    let mut v = json!({
        "me": webui::my_name(),
        "peer": peer,
        "paired": paired,
        "connected": connected,
        "controlling": crate::WIN_MODE.load(Ordering::Relaxed),
        "ax": unsafe { AXIsProcessTrusted() },
        "rtt": crate::RTT_MS.load(Ordering::Relaxed),
        "route": if connected { crate::route_label() } else { "" },
        "retry": crate::next_retry_line(),
        "hotkey": websetup::hotkey_label(),
        "edge": !crate::HOTKEY_ONLY.load(Ordering::Relaxed),
        "update": if upd == "available" { version } else { None },
    });
    if preview() {
        v["paired"] = json!(true);
        v["connected"] = json!(true);
        v["controlling"] = json!(PREVIEW_CONTROLLING.load(Ordering::Relaxed));
        v["rtt"] = json!(4);
        v["route"] = json!("Preview");
    }
    v
}

fn push(force: bool) {
    let s = state().to_string();
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if !force && *last == s {
            return;
        }
        *last = s.clone();
    }
    let js = cstring(&format!("window.knit&&knit.state({s})"));
    unsafe { knit_tray_eval(js.as_ptr()) };
}

/// The popover is about to open: send fresh state first
#[unsafe(no_mangle)]
pub extern "C" fn knit_tray_will_show() {
    push(true);
}

extern "C" fn open_registration() {
    unsafe { setup::show_registration(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
}

/// Message from tray.html
#[unsafe(no_mangle)]
pub extern "C" fn knit_tray_on_message(json: *const c_char) {
    if json.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(json) }.to_string_lossy().into_owned();
    let Ok(msg) = serde_json::from_str::<Value>(&text) else { return };
    let null = std::ptr::null_mut();
    unsafe {
        match msg["cmd"].as_str().unwrap_or("") {
            "ready" => {}
            "size" => {
                knit_tray_resize(msg["h"].as_f64().unwrap_or(360.0));
                return;
            }
            "goPeer" if preview() => PREVIEW_CONTROLLING.store(true, Ordering::Relaxed),
            "goHome" if preview() => PREVIEW_CONTROLLING.store(false, Ordering::Relaxed),
            "goPeer" => {
                if !crate::WIN_MODE.load(Ordering::Relaxed) {
                    crate::do_toggle("menu bar");
                }
                knit_tray_close();
            }
            "goHome" => {
                if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
                    crate::leave_win_mode_cursor_unlock(None);
                }
            }
            "edge" => {
                let edge = msg["value"].as_bool().unwrap_or(true);
                if preview() {
                    crate::HOTKEY_ONLY.store(!edge, Ordering::Relaxed);
                } else if let Err(e) = preferences::commit_patch(&json!({ "hotkey_only": !edge })) {
                    crate::notify("Knit", &e);
                }
            }
            "settings" => {
                knit_tray_close();
                show_prefs_page(msg["page"].as_str());
            }
            "pair" => {
                knit_tray_close();
                knit_run_later(open_registration);
            }
            "diag" => {
                knit_tray_close();
                if !preview() {
                    imp_diag(null, null, null);
                }
            }
            "update" if !preview() => crate::updater::on_click(),
            "openAx" => {
                let url = cstring("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
                knit_webui_open_url(url.as_ptr());
            }
            "menu" => knit_tray_show_menu(),
            "quit" if preview() => std::process::exit(0),
            "quit" => imp_quit(null, null, null),
            _ => {}
        }
    }
    push(true);
}
