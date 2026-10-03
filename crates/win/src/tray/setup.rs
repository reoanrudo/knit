//! Native enrollment: discovery is a hint. Trust comes from either the typed code (PAKE) or the
//! user comparing the confirmation number on both screens (approval).
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
/// 確認番号を出して、持ち主が「つなぐ」を押すのを待っている依頼(承認方式)
static PENDING: Mutex<Option<pairing::Pending>> = Mutex::new(None);
/// 確認番号の候補(4つ)のボタン。Mac の画面に出ている番号を選んでもらう
static CHOICES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
/// コード入力欄まわり(候補を出している間は隠す)
static CODE_WIDGETS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
const CHOICE_ID: u32 = 2020;
const SEARCH_ID: u32 = 2010;
enum UiEvent {
    Found(Vec<Candidate>),
    Approval(Result<pairing::Pending, String>),
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
/// 依頼を取り消し、番号の候補を隠してコード欄へ戻す
unsafe fn clear_pending() {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    for c in &CHOICES {
        ShowWindow(c.load(Ordering::Relaxed) as HWND, SW_HIDE);
    }
    for w in CODE_WIDGETS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        ShowWindow(*w as HWND, SW_SHOW);
    }
    set_text(CONNECT.load(Ordering::Relaxed), "登録する");
}
unsafe fn show_choices(list: &[String]) {
    for w in CODE_WIDGETS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        ShowWindow(*w as HWND, SW_HIDE);
    }
    for (c, text) in CHOICES.iter().zip(list) {
        let h = c.load(Ordering::Relaxed);
        set_text(h, text);
        ShowWindow(h as HWND, SW_SHOW);
    }
}
/// 候補のどれかが押された。Mac に出ている番号(本物)を選んだ時だけ、登録を進める
unsafe fn choose(index: usize) {
    if BUSY.load(Ordering::Relaxed) {
        return;
    }
    let picked = text_of(&CHOICES[index]);
    let correct = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|p| p.sas == picked);
    match correct {
        Some(true) => {
            for c in &CHOICES {
                ShowWindow(c.load(Ordering::Relaxed) as HWND, SW_HIDE);
            }
            confirm_pending();
        }
        Some(false) => {
            clear_pending();
            hint("番号が違います。Macの画面に出ている番号を確かめて、「登録する」からやり直してください。");
        }
        None => {}
    }
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
    clear_pending();
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
/// 接続先(入力したIP、または選んだMac)。決まらなければ利用者向けの説明を返す
unsafe fn target_address() -> Result<std::net::SocketAddr, &'static str> {
    let host = text_of(&HOST);
    if !host.trim().is_empty() {
        return pairing::manual_address(&host).map_err(|_| "IPアドレスを確認してください。例：192.168.1.10");
    }
    let selected = SendMessageW(LIST.load(Ordering::Relaxed) as HWND, CB_GETCURSEL, 0, 0);
    let peers = CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
    peers
        .get(selected as usize)
        .map(|p| p.address)
        .ok_or("Macの設定「接続」で「端末を登録…」を開き、再検索してください。IPでも指定できます。")
}
fn save_error_message(e: &std::io::Error, save_failed: bool) -> String {
    if save_failed { "登録情報を保存できませんでした。Windowsのユーザーと保存先を確認してください。".into() }
    else if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) { "Macに接続できません。登録画面・ネットワーク・IPを確認してください。".into() }
    else if e.kind() == std::io::ErrorKind::PermissionDenied { "Macで許可されませんでした。もう一度やり直してください。".into() }
    else { "登録できませんでした。Macの画面を確認し、期限切れなら新しく開いてやり直してください。".into() }
}
/// 承認方式: Mac に「つなぎたい」と伝え、確認番号を受け取る。持ち主が押すまで何も保存しない
unsafe fn request_approval() {
    if PREVIEW.load(Ordering::Relaxed) {
        hint("確認モードでは、Macへ接続しません。");
        return;
    }
    let address = match target_address() {
        Ok(a) => a,
        Err(message) => {
            hint(message);
            return;
        }
    };
    clear_pending();
    BUSY.store(true, Ordering::Relaxed);
    enable_controls(false);
    hint("Macに確認を求めています…");
    let cancel = Arc::new(AtomicBool::new(false));
    *CANCEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
    let (tx, rx) = mpsc::channel();
    *EVENTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
    let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows".into());
    std::thread::spawn(move || {
        let result = pairing::request_approval(address, &name, cancel)
            .map_err(|e| save_error_message(&e, false));
        let _ = tx.send(UiEvent::Approval(result));
    });
}
/// 確認番号を見比べた持ち主が「つなぐ」を押した。長期キーを受け取って保存する
unsafe fn confirm_pending() {
    let Some(pending) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    BUSY.store(true, Ordering::Relaxed);
    enable_controls(false);
    hint("登録しています…");
    let cancel = Arc::new(AtomicBool::new(false));
    *CANCEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel);
    let (tx, rx) = mpsc::channel();
    *EVENTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
    std::thread::spawn(move || {
        let mut save_failed = false;
        let result = pending
            .confirm(false, |token, peer| {
                let result = credentials::save_peer(peer).and_then(|_| credentials::save(token));
                save_failed = result.is_err();
                result
            })
            .map_err(|e| save_error_message(&e, save_failed));
        let _ = tx.send(UiEvent::Registered(result));
    });
}
unsafe fn connect() {
    if BUSY.load(Ordering::Relaxed) {
        return;
    }
    // コード欄が空なら承認方式(確認番号を見比べる)。コードが入っていれば従来どおりコードで登録する
    if text_of(&FIELD).trim().is_empty() {
        if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            hint("Macの画面に出ている番号を、下の4つから選んでください。");
        } else {
            request_approval();
        }
        return;
    }
    clear_pending();
    let code = match pairing::parse_code(&text_of(&FIELD)) {
        Ok(c) => c,
        Err(_) => {
            hint("Macに表示された6桁の数字を入力してください。");
            return;
        }
    };
    let address = match target_address() {
        Ok(a) => a,
        Err(message) => {
            hint(message);
            return;
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
        }).map_err(|e| save_error_message(&e, save_failed));
        let _ = tx.send(UiEvent::Registered(result));
    });
}
/// 6桁が入力され、送り先も決まっていれば、ボタンを押さずに登録を始める。
/// 同じコード・送り先の組で再度自動送信はしない(失敗後に繰り返し送らない。直したら再開する)
unsafe fn auto_submit() {
    static LAST: Mutex<String> = Mutex::new(String::new());
    if BUSY.load(Ordering::Relaxed) || CLOSING.load(Ordering::Relaxed) {
        return;
    }
    let Ok(code) = pairing::parse_code(&text_of(&FIELD)) else {
        // コードを消したら、同じコードをもう一度入れた時に送れるようにする
        LAST.lock().unwrap_or_else(|e| e.into_inner()).clear();
        return;
    };
    let host = text_of(&HOST);
    let selected = SendMessageW(LIST.load(Ordering::Relaxed) as HWND, CB_GETCURSEL, 0, 0);
    let has_target = !host.trim().is_empty()
        || (selected >= 0 && !CANDIDATES.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    if !has_target {
        return;
    }
    let key = format!("{code}|{}|{selected}", host.trim());
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if *last == key {
        return;
    }
    *last = key;
    drop(last);
    connect();
}
unsafe fn poll(hwnd: HWND) {
    auto_submit();
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
                "Macが見つかりました。確認番号が出るまでお待ちください。"
            });
            let focus = if peers.len() == 1 {
                FIELD.load(Ordering::Relaxed) as HWND
            } else if peers.is_empty() {
                HOST.load(Ordering::Relaxed) as HWND
            } else {
                list
            };
            let single = peers.len() == 1;
            *CANDIDATES.lock().unwrap_or_else(|e| e.into_inner()) = peers;
            windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(focus);
            // Macが1台だけ見つかったら、選ぶ手間なしで自動で確認を求める(押すまで何も保存されない)
            if single && text_of(&FIELD).trim().is_empty() && text_of(&HOST).trim().is_empty() {
                request_approval();
            }
        }
        UiEvent::Approval(Ok(pending)) => match pairing::number_choices(&pending.sas) {
            Ok(list) => {
                hint(&format!(
                    "Mac「{}」(アドレス {})の画面に出ている番号を、下から選んでください。Macに何も出ていない、または心当たりがなければ、閉じてください。",
                    pending.server_name,
                    pending.peer().ip()
                ));
                *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(pending);
                show_choices(&list);
            }
            Err(_) => hint("安全な乱数を取得できませんでした。もう一度やり直してください。"),
        },
        UiEvent::Approval(Err(message)) => {
            clear_pending();
            hint(&message)
        }
        UiEvent::Registered(Ok(token)) => {
            *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(token);
            DestroyWindow(hwnd);
        }
        UiEvent::Registered(Err(message)) => hint(&message),
    }
}
unsafe fn close(hwnd: HWND) {
    // 確認せずに閉じたら、依頼は取り消す(相手には何も保存されない)
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
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
        WM_COMMAND if (CHOICE_ID..CHOICE_ID + 4).contains(&(wp as u32 & 0xffff)) => {
            choose(((wp as u32 & 0xffff) - CHOICE_ID) as usize);
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
            SetTextColor(hdc, rgb(theme().text));
            SetBkMode(hdc, TRANSPARENT_BK);
            static BRUSHES: std::sync::OnceLock<[usize; 2]> = std::sync::OnceLock::new();
            let b = BRUSHES.get_or_init(|| {
                [
                    CreateSolidBrush(rgb(LIGHT.card)) as usize,
                    CreateSolidBrush(rgb(DARK.card)) as usize,
                ]
            });
            b[DARK_MODE.load(Ordering::Relaxed) as usize] as LRESULT
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
pub fn first_run(preview: bool) -> Option<String> {
    unsafe {
        sync_theme();
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
        let brush = CreateSolidBrush(rgb(theme().card));
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hIcon: LoadIconW(instance, std::ptr::dangling::<u16>()),
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
        create("STATIC", "Macの設定「接続」で「端末を登録…」を開いてください。見つかると、確認番号が出ます。\n一度つなげば、次回から自動でつながります。", 36, 83, 608, 52, 0, 0, font);
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
        let code_label = create(
            "STATIC",
            "コード(自動でつながらない時だけ)",
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
        let code_note = create(
            "STATIC",
            "確認番号が出た時は、コードは要りません。\n接続キーはアプリが保護して保存します。",
            300,
            345,
            344,
            48,
            0,
            0,
            small,
        );
        *CODE_WIDGETS.lock().unwrap_or_else(|e| e.into_inner()) =
            vec![code_label as usize, field as usize, code_note as usize];
        // Mac の画面に出ている番号を選ぶための4つのボタン(確認中だけ、コード欄の場所に出す)
        for (i, slot) in CHOICES.iter().enumerate() {
            let choice = create(
                "BUTTON",
                "",
                36 + i as i32 * 156,
                342,
                144,
                52,
                CHOICE_ID + i as u32,
                0xB | WS_TABSTOP,
                codefont,
            );
            ShowWindow(choice, SW_HIDE);
            slot.store(choice as usize, Ordering::Relaxed);
        }
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
                // 確認番号を見比べる前に、Enterの反射で許可しない(マウスで「つなぐ」を押す)
                if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                    continue;
                }
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
