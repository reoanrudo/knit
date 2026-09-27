//! 初回導入は通常の通信・入力フックを起動する前に完了させる。
use super::*;
use tsunagu_common::credentials;

unsafe fn alert(title: &str, body: &str, buttons: &[&str], key: Option<&str>) -> isize {
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 0);
    msg0_void(app, sel(c"finishLaunching"));
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(a, sel(c"setMessageText:"), nsstring(title));
    msg1_void_id(a, sel(c"setInformativeText:"), nsstring(body));
    for button in buttons {
        let _: ID = crate::msg1_id(a, sel(c"addButtonWithTitle:"), nsstring(button));
    }
    if let Some(key) = key {
        let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let field = f(
            msg0(objc_getClass(c"NSTextField".as_ptr()), sel(c"alloc")),
            sel(c"initWithFrame:"),
            NSRect {
                x: 0.0,
                y: 0.0,
                w: 440.0,
                h: 60.0,
            },
        );
        msg1_void_id(field, sel(c"setStringValue:"), nsstring(key));
        msg1_void_u8(field, sel(c"setEditable:"), 0);
        msg1_void_u8(field, sel(c"setSelectable:"), 1);
        msg1_void_id(
            field,
            sel(c"setAccessibilityLabel:"),
            nsstring("Windowsで入力する6桁コード"),
        );
        let font_fn: unsafe extern "C" fn(ID, SEL, f64) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let font = font_fn(
            objc_getClass(c"NSFont".as_ptr()),
            sel(c"systemFontOfSize:"),
            36.0,
        );
        msg1_void_id(field, sel(c"setFont:"), font);
        msg1_void_i64(field, sel(c"setAlignment:"), 1);
        msg1_void_u8(field, sel(c"setBezeled:"), 0);
        msg1_void_u8(field, sel(c"setDrawsBackground:"), 0);
        msg1_void_id(a, sel(c"setAccessoryView:"), field);
        msg0_void(field, sel(c"release"));
    }
    PAIR_ALERT.store(a as usize, Ordering::Relaxed);
    let result = crate::msg0_isize(a, sel(c"runModal"));
    PAIR_ALERT.store(0, Ordering::Relaxed);
    msg0_void(a, sel(c"release"));
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 1);
    result
}
pub fn error(message: &str) {
    unsafe {
        alert("接続の準備を完了できません", message, &["閉じる"], None);
    }
}
pub fn first_run(preview: bool) -> Option<String> {
    unsafe {
        if alert("MacとWindowsを、ひとつの手元で。", "近くのMacをWindowsから選び、6桁のコードを入力するだけ。\n\n登録後は自動でつながります。暗号化の鍵はアプリが管理します。", &["Windowsを登録", "あとで"], None) != 1000 { return None; }
    }
    let token = match credentials::generate() {
        Ok(t) => t,
        Err(_) => {
            error("登録の準備ができませんでした。もう一度起動してください。");
            return None;
        }
    };
    if !preview && credentials::save(&token).is_err() {
        error("キーチェーンに保存できませんでした。アクセス許可を確認してください。");
        return None;
    }
    show_invitation(&token, preview);
    Some(token)
}

struct PairingUi {
    invitation: tsunagu_common::pairing::Invitation,
    until: std::time::Instant,
    message: String,
}
static PAIRING: std::sync::Mutex<Option<PairingUi>> = std::sync::Mutex::new(None);
static PAIR_ALERT: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn poll_pairing(_s: ID, _c: SEL, _timer: ID) {
    use tsunagu_common::pairing::Event;
    let mut state = PAIRING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ui) = state.as_mut() else {
        return;
    };
    let a = PAIR_ALERT.load(Ordering::Relaxed) as ID;
    if a.is_null() {
        return;
    }
    let mut terminal = None;
    while let Ok(event) = ui.invitation.events.try_recv() {
        match event {
            Event::Registered(_) => { terminal = Some("登録が完了しました。Windowsから自動で接続します。"); }
            Event::Expired => terminal = Some("コードの有効期限が切れました。閉じて「Windowsを登録」から新しいコードを作成してください。"),
            Event::Locked => terminal = Some("3回の接続試行に達しました。閉じて新しいコードを作成してください。"),
            Event::AttemptFailed(left) => ui.message = format!("接続を確認できませんでした。コードを確認してください（残り{left}回）。"),
        }
    }
    if let Some(message) = terminal {
        msg1_void_id(a, sel(c"setInformativeText:"), nsstring(message));
        // Remove the expired/used code immediately; leave the result readable.
        let field = msg0(a, sel(c"accessoryView"));
        if !field.is_null() {
            msg1_void_u8(field, sel(c"setHidden:"), 1);
        }
        msg1_void_id(a, sel(c"setAccessoryView:"), std::ptr::null_mut());
        msg0_void(a, sel(c"layout"));
        *state = None;
        return;
    }
    let seconds = ui
        .until
        .saturating_duration_since(std::time::Instant::now())
        .as_secs();
    let body = format!("WindowsでTsunaguを開き、このMacを選んでコードを入力してください。\n\n有効期限：{}分{:02}秒 • 登録できるのは1台です。\n{}", seconds / 60, seconds % 60, ui.message);
    msg1_void_id(a, sel(c"setInformativeText:"), nsstring(&body));
    msg0_void(a, sel(c"layout"));
}
fn show_invitation(token: &str, preview: bool) {
    use tsunagu_common::pairing::{Invitation, LIFETIME};
    if preview {
        unsafe {
            alert("Windowsとつなぐ", "Windowsで近くのMacを選び、この6桁を入力します。\n\nコードは5分間有効です。\nこれは画面確認用です。通信・保存は行いません。", &["閉じる"], Some("123 456"));
        }
        return;
    }
    let port = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|a| a[0] == "--port")
        .and_then(|a| a[1].parse().ok())
        .unwrap_or(24900);
    let invitation = match Invitation::open(token.into(), port) {
        Ok(i) => i,
        Err(_) => {
            error(
                "登録の待受を開始できませんでした。別の登録画面が開いていないか確認してください。",
            );
            return;
        }
    };
    let code = format!("{} {}", &invitation.code[..3], &invitation.code[3..]);
    *PAIRING.lock().unwrap_or_else(|e| e.into_inner()) = Some(PairingUi {
        invitation,
        until: std::time::Instant::now() + LIFETIME,
        message: "この画面を閉じると登録の受付を終了します。".into(),
    });
    unsafe {
        let class_name = c"TsunaguPairingTarget";
        let mut cls = objc_getClass(class_name.as_ptr());
        if cls.is_null() {
            cls =
                objc_allocateClassPair(objc_getClass(c"NSObject".as_ptr()), class_name.as_ptr(), 0);
            class_addMethod(
                cls,
                sel(c"pollPairing:"),
                poll_pairing as *const () as usize,
                c"v@:@".as_ptr(),
            );
            objc_registerClassPair(cls);
        }
        let target = msg0(cls, sel(c"new"));
        let timer = msg5_timer(
            objc_getClass(c"NSTimer".as_ptr()),
            sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            0.25,
            target,
            sel(c"pollPairing:"),
            std::ptr::null_mut(),
            1,
        );
        let runloop = msg0(objc_getClass(c"NSRunLoop".as_ptr()), sel(c"mainRunLoop"));
        msg2_void_id_id(
            runloop,
            sel(c"addTimer:forMode:"),
            timer,
            nsstring("NSModalPanelRunLoopMode"),
        );
        // alert() publishes the active alert for this timer on the main thread.
        alert(
            "Windowsとつなぐ",
            "WindowsでこのMacを選び、6桁のコードを入力してください。\nコードは5分間有効です。",
            &["閉じる"],
            Some(&code),
        );
        msg0_void(timer, sel(c"invalidate"));
        msg0_void(target, sel(c"release"));
    }
    PAIRING.lock().unwrap_or_else(|e| e.into_inner()).take();
}
pub(super) unsafe extern "C" fn show_registration(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        let _ = first_run(true);
        return;
    }
    let token = crate::envutil::get("TSUNAGU_TOKEN")
        .filter(|t| !t.is_empty())
        .or_else(|| credentials::load().ok().flatten());
    match token {
        Some(token) if credentials::parse_key(&token).is_ok_and(|normalized| normalized == token) => show_invitation(&token, false),
        _ => error("この接続は旧形式の設定を使っています。既存の接続を保持するため、6桁コードによる追加登録は行いません。"),
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
pub fn ensure_permission() -> bool {
    while !unsafe { AXIsProcessTrusted() } {
        let choice = unsafe {
            alert("このMacから操作する準備","TsunaguでWindowsを操作するには、Macのアクセシビリティ権限が必要です。\n\nシステム設定でTsunaguを許可し、この画面に戻って確認してください。",&["システム設定を開く","許可したので確認","あとで"],None)
        };
        if choice == 1002 {
            return false;
        }
        if choice == 1000 {
            unsafe {
                let url=crate::msg1_id(objc_getClass(c"NSURL".as_ptr()),sel(c"URLWithString:"),nsstring("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"));
                let workspace = msg0(
                    objc_getClass(c"NSWorkspace".as_ptr()),
                    sel(c"sharedWorkspace"),
                );
                let open: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
                    std::mem::transmute(crate::objc_msgSend as *const () as usize);
                open(workspace, sel(c"openURL:"), url);
            }
        }
    }
    true
}

/// Debug-only end-to-end UI probe: ephemeral credential, no OS-store access.
#[cfg(debug_assertions)]
pub fn probe_invitation() {
    if let Ok(token) = credentials::generate() {
        show_invitation(&token, false);
    }
}
