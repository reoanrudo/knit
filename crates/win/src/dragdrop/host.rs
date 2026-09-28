//! OLEのマウス捕捉を所有する、ドラッグ中だけ存在するウィンドウ。
use super::*;
use std::sync::atomic::AtomicBool;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct Host {
    window: HWND,
    previous: HWND,
}

pub fn cancelled() -> bool {
    !crate::CONNECTED.load(Ordering::Relaxed) || !edge::CONTROLLED.load(Ordering::Relaxed)
}

/// OLE のモーダルループは SendInput の共有マウス入力を拾わないため、
/// 捕捉窓(capture window)へ明示的に投稿して中継する(ネイティブ試験で実証)。
/// 中継先は進行中のドラッグスレッドの捕捉窓に限定し、進行中でなければ何もしない。
unsafe fn post_to_capture(message: u32, wp: WPARAM) -> bool {
    let thread = super::DRAG_THREAD.load(Ordering::Relaxed);
    if thread == 0 {
        return false;
    }
    let mut info: GUITHREADINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndCapture.is_null() {
        return false;
    }
    let mut point = std::mem::zeroed();
    GetCursorPos(&mut point);
    ScreenToClient(info.hwndCapture, &mut point);
    let coordinates = ((point.y as u16 as u32) << 16) | point.x as u16 as u32;
    PostMessageW(info.hwndCapture, message, wp, coordinates as LPARAM) != 0
}

/// 掴み移動。捕捉窓の解釈に合わせ、押下中であることを wParam で示す
pub fn relay_move() {
    unsafe { post_to_capture(WM_MOUSEMOVE, MK_LBUTTON as usize) };
}

/// ボタン解放=ドロップ
pub fn relay_up() {
    super::release_expected();
    if unsafe { post_to_capture(WM_LBUTTONUP, 0) } {
        println!("[drag] ボタン解放を OLE の捕捉窓へ中継");
    }
}

/// 操作モード離脱・切断。Esc を捕まえたOLEがキャンセル経路で終わる
pub fn relay_cancel() {
    super::release_expected();
    if unsafe { post_to_capture(WM_KEYDOWN, 0x1B as usize) } {
        println!("[drag] キャンセル(Esc)を OLE の捕捉窓へ中継");
    }
}

unsafe extern "system" fn proc(window: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_TIMER {
        // ネットワークの終了状態でもOLEを起こす。背景化・切断でUpが
        // OLEのキューへ届かなかった場合も、古いドラッグを残さない。
        let capture = GetCapture();
        if !capture.is_null() && capture != window {
            if cancelled() {
                PostMessageW(capture, WM_KEYDOWN, 0x1B, 0);
            } else if !crate::BTN_W[0].load(Ordering::Relaxed) {
                let mut point = std::mem::zeroed();
                GetCursorPos(&mut point);
                ScreenToClient(capture, &mut point);
                let coordinates = ((point.y as u16 as u32) << 16) | point.x as u16 as u32;
                PostMessageW(capture, WM_LBUTTONUP, 0, coordinates as LPARAM);
            }
        }
        return 0;
    }
    DefWindowProcW(window, message, wp, lp)
}

impl Host {
    pub unsafe fn create() -> Option<Self> {
        if ACTIVE.swap(true, Ordering::Relaxed) {
            return None;
        }
        let class: Vec<u16> = "KnitDragSource\0".encode_utf16().collect();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let mut point = std::mem::zeroed();
        GetCursorPos(&mut point);
        let previous = GetForegroundWindow();
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TOPMOST,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            point.x - 8,
            point.y - 8,
            16,
            16,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if window.is_null() {
            ACTIVE.store(false, Ordering::Relaxed);
            return None;
        }
        let host = Self { window, previous };
        SetLayeredWindowAttributes(window, 0, 1, LWA_ALPHA);
        ShowWindow(window, SW_SHOWNORMAL);
        SetForegroundWindow(window);
        if GetForegroundWindow() != window {
            println!("[drag] source window could not receive foreground input");
            ACTIVE.store(false, Ordering::Relaxed);
            return None;
        }
        // OLE のモーダルループが入力を拾えるよう、押下の所有権をドラッグ
        // スレッド側に置いておく(背景のSTAからは他アプリ上のUpを受け取れない)
        SetCapture(window);
        SetTimer(window, 1, 16, None);
        Some(host)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            KillTimer(self.window, 1);
            ReleaseCapture();
            let restore = GetForegroundWindow() == self.window;
            DestroyWindow(self.window);
            if restore && !self.previous.is_null() {
                SetForegroundWindow(self.previous);
            }
            ACTIVE.store(false, Ordering::Relaxed);
        }
    }
}
