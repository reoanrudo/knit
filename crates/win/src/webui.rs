//! 設定画面(WebView2)。ui/app.html を表示し、値の読み書きと実処理は
//! `bridge` が既存の Rust 側へ委譲する。Mac 側(gui/webui.rs + webui.m)と同じ
//! 分担で、このファイルは窓と WebView2 の寿命だけを持つ。
//!
//! - app -> page: `knit.state(json)`(1秒ごと・変化時のみ)、`knit.result(id,ok,msg)`、
//!   `knit.diag(text)`
//! - page -> app: `chrome.webview.postMessage` で届く JSON 文字列
//!   `{"id":n,"cmd":"…",…}`
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    CoreWebView2EnvironmentOptions, ExecuteScriptCompletedHandler, WebMessageReceivedEventHandler,
};
use windows::core::{w, HSTRING, PCWSTR};
use windows::core::Interface;
use windows::Win32::Foundation::{E_POINTER, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, PostMessageW, RegisterClassW,
    SetForegroundWindow, ShowWindow, CW_USEDEFAULT, SW_SHOW, WM_CLOSE, WM_SIZE, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};

mod bridge;

const HTML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/app.html"));
const CLASS: windows::core::PCWSTR = w!("KnitWebSettings");
/// 別スレッド(診断)から UI スレッドへ JS を渡すための独自メッセージ
const WM_APP_EVAL: u32 = 0x8000 + 1;

thread_local! {
    /// 生成に成功した WebView2。UI スレッド専用(COM は Send でない)
    static WEBVIEW: RefCell<Option<ICoreWebView2>> = const { RefCell::new(None) };
    static CONTROLLER: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
}

static HWND_UI: AtomicUsize = AtomicUsize::new(0);
static INIT_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 直近に送った状態(同一内容の push を省く)
static LAST_STATE: Mutex<String> = Mutex::new(String::new());

fn hwnd() -> HWND {
    HWND(HWND_UI.load(Ordering::Relaxed) as *mut core::ffi::c_void)
}

fn visible() -> bool {
    let h = hwnd();
    !h.0.is_null() && unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(h).as_bool() }
}

/// 設定画面を開く(既にあれば前面へ)。トレイのメニューと左クリックから呼ぶ
pub(crate) unsafe fn show() {
    let existing = hwnd();
    if !existing.0.is_null() {
        let _ = ShowWindow(existing, SW_SHOW);
        let _ = SetForegroundWindow(existing);
        with_controller(|c| {
            let _ = c.SetIsVisible(true);
        });
        push(true);
        return;
    }
    let hinst = match GetModuleHandleW(PCWSTR::null()) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[webui] GetModuleHandleW 失敗: {e}");
            return;
        }
    };
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    RegisterClassW(&wc);
    let (w, h) = (1000, 720);
    let hwnd = match CreateWindowExW(
        Default::default(),
        CLASS,
        w!("Knit 設定"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        w,
        h,
        None,
        None,
        Some(hinst.into()),
        None,
    ) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[webui] 設定画面を作成できません: {e}");
            return;
        }
    };
    HWND_UI.store(hwnd.0 as usize, Ordering::Relaxed);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
    start_webview(hwnd);
}

/// 1秒ごとの更新(トレイの WM_TIMER から)。見えている時だけ送る
pub(crate) fn tick() {
    if visible() {
        push(false);
    }
}

fn start_webview(hwnd: HWND) {
    if INIT_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    // WebView2 は呼び出しスレッドが STA であることを要求する(未初期化だと
    // CreateCoreWebView2EnvironmentWithOptions が CO_E_NOTINITIALIZED で失敗する)
    let hr = unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        )
    };
    // RPC_E_CHANGED_MODE は既に別モデルで初期化済みのスレッド。WebView2 は
    // STA を要求するため、その場合は作成が失敗して下のログに残る
    if hr.is_err() {
        eprintln!("[webui] COM(STA) を初期化できません: {hr:?}");
    }
    let dir = knit_common::envutil::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("webview2");
    let _ = std::fs::create_dir_all(&dir);
    let user_data = HSTRING::from(dir.to_string_lossy().to_string());
    let options: ICoreWebView2EnvironmentOptions =
        CoreWebView2EnvironmentOptions::default().into();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |error_code, environment| {

            error_code?;
            let environment =
                environment.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |error_code, controller| {

                    error_code?;
                    let controller =
                        controller.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                    if let Err(e) = on_controller(hwnd, controller) {
                        eprintln!("[webui] ページの準備に失敗: {e}");
                    }
                    Ok(())
                },
            ));
            unsafe { environment.CreateCoreWebView2Controller(hwnd, &handler)? };
            Ok(())
        },
    ));
    let result = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &user_data,
            &options,
            &handler,
        )
    };
    if let Err(e) = result {
        eprintln!("[webui] WebView2 を開始できません: {e}");
    }
}

/// コントローラができたら HTML を流し込み、メッセージ受けと初期状態を配線する
fn on_controller(hwnd: HWND, controller: ICoreWebView2Controller) -> windows::core::Result<()> {
    unsafe {
        // 白い一瞬のちらつきを避ける(ICoreWebView2Controller2 以降)
        if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 255,
                R: 249,
                G: 249,
                B: 249,
            });
        }
        // コントローラを保存してから大きさを合わせる(保存前に resize すると
        // with_controller が空振りし、WebView が 0×0 のまま何も描かれない)
        CONTROLLER.with(|c| *c.borrow_mut() = Some(controller.clone()));
        resize(hwnd);
        controller.SetIsVisible(true)?;
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        let mut nav_token = Default::default();
        webview.add_NavigationCompleted(
            &webview2_com::NavigationCompletedEventHandler::create(Box::new(|_sender, args| {
                if let Some(args) = args {
                    let mut ok = windows::core::BOOL(0);
                    let ok = args.IsSuccess(&mut ok).map(|_| ok.as_bool()).unwrap_or(false);
                    if !ok {
                        eprintln!("[webui] ページの読み込みに失敗しました");
                    }
                }
                Ok(())
            })),
            &mut nav_token,
        )?;
        let mut token = Default::default();
        webview.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(|_sender, args| {
                if let Some(args) = args {
                    let mut raw = windows::core::PWSTR::null();
                    args.TryGetWebMessageAsString(&mut raw)?;
                    on_message(&webview2_com::take_pwstr(raw));
                }
                Ok(())
            })),
            &mut token,
        )?;
        WEBVIEW.with(|w| *w.borrow_mut() = Some(webview.clone()));
        let boot = serde_json::json!({
            "os": "win",
            "preview": crate::tray::preview_mode(),
            "version": env!("CARGO_PKG_VERSION"),
            "page": "gen",
        });
        let html = HTML.replacen("/*KNIT_BOOT*/", &format!("window.KNIT_BOOT={boot};"), 1);
        webview.NavigateToString(&HSTRING::from(html))?;
    }
    Ok(())
}

fn with_controller(f: impl FnOnce(&ICoreWebView2Controller)) {
    CONTROLLER.with(|c| {
        if let Some(controller) = c.borrow().as_ref() {
            f(controller);
        }
    });
}

/// 窓の大きさに子ウィンドウを合わせる
unsafe fn resize(hwnd: HWND) {
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err() {
        return;
    }
    with_controller(|c| {
        if let Err(e) = c.SetBounds(RECT {
            left: 0,
            top: 0,
            right: rect.right,
            bottom: rect.bottom,
        }) {
            eprintln!("[webui] 大きさを合わせられません: {e}");
        }
    });
}

/// ページから届いた JSON。1件ずつ処理し、必ず結果を返す(返さないと UI が固まる)
fn on_message(text: &str) {
    let Ok(msg) = serde_json::from_str::<serde_json::Value>(text) else {
        eprintln!("[webui] 読めないメッセージ: {text}");
        return;
    };
    let id = msg["id"].as_u64().unwrap_or(0);
    let cmd = msg["cmd"].as_str().unwrap_or("");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bridge::handle(cmd, &msg)));
    match result {
        Ok(Ok(message)) => reply(id, true, &message),
        Ok(Err(message)) => reply(id, false, &message),
        Err(_) => reply(id, false, "内部エラーが起きました。ログを確かめてください"),
    }
    push(true);
}

fn reply(id: u64, ok: bool, message: &str) {
    if !message.is_empty() || !ok {
        let m = serde_json::to_string(message).unwrap_or_else(|_| "\"\"".into());
        eval(&format!("window.knit&&knit.result({id},{ok},{m})"));
    } else {
        eval(&format!("window.knit&&knit.result({id},true)"));
    }
}

fn push(force: bool) {
    if !visible() {
        return;
    }
    let state = bridge::state().to_string();
    {
        let mut last = LAST_STATE.lock().unwrap_or_else(|e| e.into_inner());
        if !force && *last == state {
            return;
        }
        *last = state.clone();
    }
    eval(&format!("window.knit&&knit.state({state})"));
}

fn eval(js: &str) {
    WEBVIEW.with(|w| {
        let borrow = w.borrow();
        let Some(webview) = borrow.as_ref() else {
            return;
        };
        let js = HSTRING::from(js);
        let handler = ExecuteScriptCompletedHandler::create(Box::new(|_error, _result| Ok(())));
        let _ = unsafe { webview.ExecuteScript(&js, &handler) };
    });
}

/// 別スレッドから UI スレッドへ JS を渡す(診断の完了通知)。窓が無ければ捨てる
pub(super) fn eval_async(js: String) {
    let hwnd = hwnd();
    if hwnd.0.is_null() {
        return;
    }
    let ptr = Box::into_raw(Box::new(js));
    unsafe {
        if PostMessageW(Some(hwnd), WM_APP_EVAL, WPARAM(0), LPARAM(ptr as isize)).is_err() {
            drop(Box::from_raw(ptr));
        }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            resize(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            // 閉じる=隠す。次に開くときは作り直さず、そのまま見せる
            with_controller(|c| {
                let _ = c.SetIsVisible(false);
            });
            let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
            LRESULT(0)
        }
        WM_APP_EVAL => {
            let js = Box::from_raw(lparam.0 as *mut String);
            eval(&js);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
