//! Native enrollment: discovery is a hint; only PAKE authentication grants trust.
use super::*;
use knit_common::{
    credentials,
    pairing::{self, Candidate},
};
use std::sync::{atomic::AtomicBool, mpsc, Arc, Mutex};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
static RESULT: Mutex<Option<String>> = Mutex::new(None);
static FIELD: AtomicUsize = AtomicUsize::new(0);
static HOST: AtomicUsize = AtomicUsize::new(0);
static LIST: AtomicUsize = AtomicUsize::new(0);
static HINT: AtomicUsize = AtomicUsize::new(0);
static CONNECT: AtomicUsize = AtomicUsize::new(0);
static SEARCH: AtomicUsize = AtomicUsize::new(0);
static PREVIEW: AtomicBool = AtomicBool::new(false);
static CLOSING: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static CANDIDATES: Mutex<Vec<Candidate>> = Mutex::new(Vec::new());
static EVENTS: Mutex<Option<mpsc::Receiver<UiEvent>>> = Mutex::new(None);
static CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
const SEARCH_ID: u32 = 2010;
enum UiEvent {
    Found(Vec<Candidate>),
    Registered(Result<String, String>),
}
unsafe fn text_of(field: &AtomicUsize) -> String {
    let mut text = [0u16; 160];
    let len = GetWindowTextW(
        field.load(Ordering::Relaxed) as HWND,
        text.as_mut_ptr(),
        text.len() as i32,
    );
    String::from_utf16_lossy(&text[..len.max(0) as usize])
}
unsafe fn hint(text: &str) {
    set_text(HINT.load(Ordering::Relaxed), text);
}
unsafe fn enable_controls(enabled: bool) {
    for field in [&FIELD, &HOST, &LIST, &CONNECT, &SEARCH] {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
            field.load(Ordering::Relaxed) as HWND,
            enabled as i32,
        );
    }
}
unsafe fn search() {
    if BUSY.swap(true, Ordering::Relaxed) {
        return;
    }
    enable_controls(false);
    hint("登録を受け付けているMacを探しています…");
    let (tx, rx) = mpsc::channel();
    *EVENTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
    let preview = PREVIEW.load(Ordering::Relaxed);
    std::thread::spawn(move || {
        let peers = if preview {
            vec![Candidate {
                name: "Mac · 確認用".into(),
                address: "192.168.1.10:24904".parse().unwrap(),
            }]
        } else {
            pairing::discover().unwrap_or_default()
        };
        let _ = tx.send(UiEvent::Found(peers));
    });
}
unsafe fn connect() {
    if BUSY.load(Ordering::Relaxed) {
        return;
    }
    let code = match pairing::parse_code(&text_of(&FIELD)) {
        Ok(c) => c,
        Err(_) => {
            hint("Macに表示された6桁の数字を入力してください。");
            return;
        }
    };
    let host = text_of(&HOST);
    let address = if !host.trim().is_empty() {
        match pairing::manual_address(&host) {
            Ok(a) => a,
            Err(_) => {
                hint("IPアドレスを確認してください。例：192.168.1.10");
                return;
            }
        }
    } else {
        let selected = SendMessageW(LIST.load(Ordering::Relaxed) as HWND, CB_GETCURSEL, 0, 0);
        let peers = CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
        match peers.get(selected as usize) {
            Some(p) => p.address,
            None => {
                hint("Macで「Windowsを登録」を開き、再検索してください。IPでも指定できます。");
                return;
            }
        }
    };
    if PREVIEW.load(Ordering::Relaxed) {
        hint("入力を確認しました。確認モードでは接続・保存しません。");
        return;
    }
    BUSY.store(true, Ordering::Relaxed);
    enable_controls(false);
    hint("コードを確認し、登録しています…");
    let cancel = Arc::new(AtomicBool::new(false));
    *CANCEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
    let (tx, rx) = mpsc::channel();
    *EVENTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
    std::thread::spawn(move || {
        let mut save_failed = false;
        let result = pairing::enroll_cancellable(address, &code, cancel, |token, peer| {
            let result = credentials::save_peer(peer).and_then(|_| credentials::save(token));
            save_failed = result.is_err(); result
        }).map_err(|e| {
            if save_failed { "登録情報を保存できませんでした。Windowsのユーザーと保存先を確認してください。".into() }
            else if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) { "Macに接続できません。登録画面・ネットワーク・IPを確認してください。".into() }
            else { "コードを確認できませんでした。Macの画面を確認し、期限切れなら新しいコードでやり直してください。".into() }
        });
        let _ = tx.send(UiEvent::Registered(result));
    });
}
unsafe fn poll(hwnd: HWND) {
    let event = EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|r| r.try_recv().ok());
    let Some(event) = event else {
        return;
    };
    BUSY.store(false, Ordering::Relaxed);
    CANCEL.lock().unwrap_or_else(|e| e.into_inner()).take();
    if CLOSING.load(Ordering::Relaxed) {
        DestroyWindow(hwnd);
        return;
    }
    enable_controls(true);
    match event {
        UiEvent::Found(peers) => {
            let list = LIST.load(Ordering::Relaxed) as HWND;
            SendMessageW(list, CB_RESETCONTENT, 0, 0);
            for peer in &peers {
                SendMessageW(
                    list,
                    CB_ADDSTRING,
                    0,
                    wide(&format!("{}  ·  {}", peer.name, peer.address.ip())).as_ptr() as isize,
                );
            }
            // Multiple results require an explicit choice, never silently pair the first device.
            if peers.len() == 1 {
                SendMessageW(list, CB_SETCURSEL, 0, 0);
            }
            hint(if peers.is_empty() {
                "Macが見つかりません。Macで登録画面を開くか、IPを入力してください。"
            } else {
                "Macの画面に表示された6桁コードを入力してください。"
            });
            let focus = if peers.len() == 1 {
                FIELD.load(Ordering::Relaxed) as HWND
            } else if peers.is_empty() {
                HOST.load(Ordering::Relaxed) as HWND
            } else {
                list
            };
            *CANDIDATES.lock().unwrap_or_else(|e| e.into_inner()) = peers;
            windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(focus);
        }
        UiEvent::Registered(Ok(token)) => {
            *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(token);
            DestroyWindow(hwnd);
        }
        UiEvent::Registered(Err(message)) => hint(&message),
    }
}
unsafe fn close(hwnd: HWND) {
    if BUSY.load(Ordering::Relaxed) {
        CLOSING.store(true, Ordering::Relaxed);
        if let Some(cancel) = CANCEL.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            cancel.store(true, Ordering::SeqCst);
        }
        hint("登録を終了しています…");
    } else {
        DestroyWindow(hwnd);
    }
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND if wp as u32 & 0xffff == MENU_SAVEHOST => {
            connect();
            0
        }
        WM_COMMAND if wp as u32 & 0xffff == SEARCH_ID => {
            search();
            0
        }
        WM_COMMAND if wp as u32 & 0xffff == 1011 => {
            close(hwnd);
            0
        }
        WM_TIMER => {
            poll(hwnd);
            0
        }
        WM_CLOSE => {
            close(hwnd);
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            PostQuitMessage(0);
            0
        }
        WM_DRAWITEM => status_wndproc(hwnd, msg, wp, lp),
        WM_CTLCOLORSTATIC => {
            let hdc = wp as *mut core::ffi::c_void;
            SetTextColor(hdc, rgb(CLR_TEXT));
            SetBkMode(hdc, TRANSPARENT_BK);
            static BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
            *BRUSH.get_or_init(|| CreateSolidBrush(rgb(CLR_CARD)) as usize) as LRESULT
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
pub fn first_run(preview: bool) -> Option<String> {
    unsafe {
        PREVIEW.store(preview, Ordering::Relaxed);
        CLOSING.store(false, Ordering::Relaxed);
        BUSY.store(false, Ordering::Relaxed);
        *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
        }
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("KnitFirstConnection");
        let brush = CreateSolidBrush(rgb(CLR_CARD));
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hIcon: LoadIconW(instance, 1usize as *const u16),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: brush,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let style = WS_CAPTION | WS_SYSMENU;
        let mut bounds = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 680,
            bottom: 560,
        };
        AdjustWindowRect(&mut bounds, style, 0);
        let window = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            wide("Knit — はじめての接続").as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if window.is_null() {
            UnregisterClassW(class.as_ptr(), instance);
            DeleteObject(brush);
            return None;
        }
        let font = segoe_font(false, 16);
        let heading = segoe_font(true, 28);
        let small = segoe_font(false, 14);
        let codefont = segoe_font(true, 30);
        let create = |class: &str, text: &str, x, y, w, h, id: u32, style, font| {
            let child = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                w,
                h,
                window,
                id as usize as _,
                instance,
                std::ptr::null(),
            );
            SendMessageW(child, WM_SETFONT, font as usize, 1);
            child
        };
        create("STATIC", "Macと、つなぐ。", 36, 28, 608, 44, 0, 0, heading);
        create("STATIC", "Macで「Windowsを登録」を開いてください。\n6桁のコードで登録すると、次回から自動でつながります。", 36, 83, 608, 52, 0, 0, font);
        create("STATIC", "1   接続するMac", 36, 158, 608, 26, 0, 0, font);
        let list = create(
            "COMBOBOX",
            "",
            36,
            193,
            466,
            180,
            2,
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            font,
        );
        LIST.store(list as usize, Ordering::Relaxed);
        let searchbutton = create(
            "BUTTON",
            "再検索",
            518,
            191,
            126,
            32,
            SEARCH_ID,
            0xB | WS_TABSTOP,
            font,
        );
        SEARCH.store(searchbutton as usize, Ordering::Relaxed);
        create(
            "STATIC",
            "見つからない場合は、MacのIP",
            36,
            245,
            270,
            24,
            0,
            0,
            small,
        );
        let host = create(
            "EDIT",
            "",
            318,
            240,
            326,
            30,
            3,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            font,
        );
        HOST.store(host as usize, Ordering::Relaxed);
        SendMessageW(host, 0x00C5, 64, 0);
        create(
            "STATIC",
            "2   Macに表示されたコード",
            36,
            306,
            608,
            26,
            0,
            0,
            font,
        );
        let field = create(
            "EDIT",
            "",
            36,
            342,
            240,
            52,
            1,
            WS_BORDER | WS_TABSTOP | ES_CENTER as u32 | ES_AUTOHSCROLL as u32,
            codefont,
        );
        FIELD.store(field as usize, Ordering::Relaxed);
        SendMessageW(field, 0x00C5, 16, 0);
        create(
            "STATIC",
            "コードは5分間有効です。\n暗号化の鍵はアプリが保護して保存します。",
            300,
            345,
            344,
            48,
            0,
            0,
            small,
        );
        let status = create("STATIC", "", 36, 420, 608, 48, 0, 0, small);
        HINT.store(status as usize, Ordering::Relaxed);
        create(
            "BUTTON",
            "あとで",
            370,
            492,
            120,
            36,
            1011,
            0xB | WS_TABSTOP,
            font,
        );
        let button = create(
            "BUTTON",
            "登録する",
            506,
            492,
            138,
            36,
            MENU_SAVEHOST,
            0xB | WS_TABSTOP,
            font,
        );
        CONNECT.store(button as usize, Ordering::Relaxed);
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        SetTimer(window, 1, 100, None);
        search();
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_KEYDOWN && msg.wParam == 13 {
                SendMessageW(window, WM_COMMAND, MENU_SAVEHOST as usize, 0);
                continue;
            }
            if msg.message == WM_KEYDOWN && msg.wParam == 27 {
                SendMessageW(window, WM_CLOSE, 0, 0);
                continue;
            }
            if IsDialogMessageW(window, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for f in [font, heading, small, codefont] {
            DeleteObject(f);
        }
        UnregisterClassW(class.as_ptr(), instance);
        DeleteObject(brush);
        EVENTS.lock().unwrap_or_else(|e| e.into_inner()).take();
        RESULT.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}
pub fn error(text: &str) {
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Knit — 接続の準備").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
