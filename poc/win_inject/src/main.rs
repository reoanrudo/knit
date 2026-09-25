// poc/win_inject: SendInput・クリップボードのSSH経由ヘッドレス検証
// すべて「自分で読み取れる状態」で検証する(ユーザー操作不要)
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::process::exit;
use windows_sys::Win32::Foundation::{GetLastError, POINT};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VK_Z,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
};

const CF_UNICODETEXT: u32 = 13;
const INPUT_MOUSE: u32 = 0;
const MOUSEEVENTF_MOVE: u32 = 0x0001;
const MOUSEEVENTF_ABSOLUTE: u32 = 0x8000;

const DESKTOP_FULL_ACCESS: u32 = 0x01FF;

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenInputDesktop(dwFlags: u32, fInherit: bool, dwDesiredAccess: u32) -> *mut core::ffi::c_void;
    fn SetThreadDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
    fn CloseDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
}

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    macro_rules! check {
        ($name:expr, $ok:expr, $detail:expr) => {
            if $ok {
                pass += 1;
                println!("[PASS] {}: {}", $name, $detail);
            } else {
                fail += 1;
                println!("[FAIL] {}: {}", $name, $detail);
            }
        };
    }

    // 0) 入力デスクトップへの接続(SSH起動プロセスは未接続の場合がある)
    let desk = unsafe { OpenInputDesktop(0, false, DESKTOP_FULL_ACCESS) };
    let desk_ok = !desk.is_null();
    if desk_ok {
        let s = unsafe { SetThreadDesktop(desk) };
        check!("attach_input_desktop", s != 0, "OpenInputDesktop+SetThreadDesktop ok".to_string());
    } else {
        check!("attach_input_desktop", false, "OpenInputDesktop returned null".to_string());
    }

    // 1) 画面サイズ取得
    let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    check!("screen_size", w > 0 && h > 0, format!("{w}x{h}"));

    // 2) マウス注入: SendInput(絶対移動) → GetCursorPosで確認
    let before = {
        let mut p = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut p) };
        p
    };
    let target = (w / 2, h / 2);
    let moved = send_mouse_move(target.0, target.1, w, h);
    let after = {
        let mut p = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut p) };
        p
    };
    check!(
        "sendinput_mouse",
        moved && (after.x - target.0).abs() <= 2 && (after.y - target.1).abs() <= 2,
        format!(
            "before=({},{}) after=({},{}) target=({},{})",
            before.x, before.y, after.x, after.y, target.0, target.1
        )
    );

    // 3) キー注入: 'Z' down → GetAsyncKeyStateで押下確認 → up
    let down = send_key(VK_Z as u16, false);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let state = unsafe { GetAsyncKeyState(VK_Z as i32) };
    let pressed_during = (state as u16) & 0x8000 != 0;
    let up = send_key(VK_Z as u16, true);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let state2 = unsafe { GetAsyncKeyState(VK_Z as i32) };
    let released_after = (state2 as u16) & 0x8000 == 0;
    check!(
        "sendinput_key",
        down && pressed_during && up && released_after,
        format!(
            "down_ret={down} pressed={pressed_during} up_ret={up} released={released_after}"
        )
    );

    // 4) クリップボード: 書き込み→読み出し一致
    let test_str = "seamless-desk-POC-12345";
    let written = clipboard_write(test_str);
    let read_back = clipboard_read();
    check!(
        "clipboard_roundtrip",
        written && read_back.as_deref() == Some(test_str),
        format!("written={written} read={read_back:?}")
    );

    println!(
        "--- POC result: pass={pass} fail={fail} (last_error={})",
        unsafe { GetLastError() }
    );
    if fail > 0 {
        exit(1);
    }
}

// INPUT は type(u32) + 4バイトパッド + 32バイト共用体 = 40バイト。
// 共用体を手動パックするための固定レイアウト。
#[repr(C)]
#[derive(Clone, Copy)]
struct InputBuf {
    itype: u32,     // @0
    _pad: u32,      // @4 (共用体の8バイトアラインによるパッド)
    body: [u32; 6], // @8..32 (MOUSEINPUT: dx,dy,mouseData,dwFlags,time,pad)
    extra: usize,   // @32..40 (MOUSEINPUT.dwExtraInfo の位置)
}

fn send_mouse_move(x: i32, y: i32, sw: i32, sh: i32) -> bool {
    assert_eq!(std::mem::size_of::<InputBuf>(), std::mem::size_of::<INPUT>());
    let nx = ((x as i64 * 65535) / (sw as i64 - 1)) as i32;
    let ny = ((y as i64 * 65535) / (sh as i64 - 1)) as i32;
    let input = InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [
            nx as u32,
            ny as u32,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
            0,
            0,
        ],
        extra: 0,
    };
    let sent = unsafe {
        SendInput(
            1,
            &input as *const _ as *const INPUT,
            std::mem::size_of::<InputBuf>() as i32,
        )
    };
    sent == 1
}

fn send_key(vk: u16, up: bool) -> bool {
    // KEYBDINPUT: wVk@8, wScan@10, dwFlags@12, time@16 (dwExtraInfo=0でよい)
    let input = InputBuf {
        itype: INPUT_KEYBOARD,
        _pad: 0,
        body: [vk as u32, if up { KEYEVENTF_KEYUP } else { 0 }, 0, 0, 0, 0],
        extra: 0,
    };
    let sent = unsafe {
        SendInput(
            1,
            &input as *const _ as *const INPUT,
            std::mem::size_of::<InputBuf>() as i32,
        )
    };
    sent == 1
}

fn clipboard_write(s: &str) -> bool {
    let mut utf16: Vec<u16> = s.encode_utf16().collect();
    utf16.push(0);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let bytes = utf16.len() * 2;
        let hglob = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if hglob.is_null() {
            CloseClipboard();
            return false;
        }
        let ptr = GlobalLock(hglob);
        if ptr.is_null() {
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr as *mut u16, utf16.len());
        GlobalUnlock(hglob);
        let ok = !SetClipboardData(CF_UNICODETEXT, hglob).is_null();
        CloseClipboard();
        ok
    }
}

fn clipboard_read() -> Option<String> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT) as *mut core::ffi::c_void;
        let result = if h.is_null() {
            None
        } else {
            let ptr = GlobalLock(h) as *const u16;
            if ptr.is_null() {
                None
            } else {
                let mut len = 0usize;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                Some(String::from_utf16_lossy(slice))
            }
        };
        if !h.is_null() {
            GlobalUnlock(h);
        }
        CloseClipboard();
        result
    }
}
