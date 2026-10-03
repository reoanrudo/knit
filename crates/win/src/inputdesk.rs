//! 通常の SendInput が届かない状態の監視。
//! - 入力デスクトップが Default 以外(UAC の確認画面・ロック画面などの保護デスクトップ)
//! - 前面窓が自分より高い権限(管理者として実行したアプリ等。UIPI で入力が捨てられる)
//!
//! どちらも SendInput は成功を返しながら黙って捨てられるため、戻り値では判定できない。
//! 100ms 周期で状態を見て NEEDS_HELPER に反映し、注入側(input.rs)が操作補助
//! (helper.rs)へ経路を切り替える。補助が無い時は Mac へ制御を返す(固まらない)。
use crate::helper;
use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// 直接の注入が届かない状態か(true の間は操作補助へ中継する)
pub(crate) static NEEDS_HELPER: AtomicBool = AtomicBool::new(false);

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenInputDesktop(flags: u32, inherit: bool, access: u32) -> *mut c_void;
    fn CloseDesktop(h: *mut c_void) -> i32;
    fn GetUserObjectInformationW(
        h: *mut c_void,
        index: i32,
        info: *mut c_void,
        len: u32,
        needed: *mut u32,
    ) -> i32;
}

/// 入力デスクトップが通常の Default か。開けない(保護デスクトップは権限不足で
/// 開けない)場合は false。開けたが名前が取れない時は判断できないので true
fn input_desktop_is_default() -> bool {
    const DESKTOP_READOBJECTS: u32 = 0x0001;
    const UOI_NAME: i32 = 2;
    unsafe {
        let h = OpenInputDesktop(0, false, DESKTOP_READOBJECTS);
        if h.is_null() {
            return false;
        }
        let mut buf = [0u16; 64];
        let mut need = 0u32;
        let ok = GetUserObjectInformationW(
            h,
            UOI_NAME,
            buf.as_mut_ptr().cast(),
            std::mem::size_of_val(&buf) as u32,
            &mut need,
        );
        CloseDesktop(h);
        if ok == 0 {
            return true;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len]).eq_ignore_ascii_case("default")
    }
}

/// プロセスのトークンが昇格済みか。照会に失敗したら None
pub(crate) fn process_elevated(process: *mut c_void) -> Option<bool> {
    unsafe {
        let mut tok = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut tok) == 0 {
            return None;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            tok,
            TokenElevation,
            (&mut e as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        );
        CloseHandle(tok);
        (ok != 0).then_some(e.TokenIsElevated != 0)
    }
}

pub(crate) fn self_elevated() -> bool {
    process_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

#[derive(PartialEq, Clone, Copy)]
enum Fg {
    Normal,
    /// 昇格済みと確認できた
    Elevated,
    /// プロセスを開けない(SYSTEM 等のより高い権限の可能性)。補助があれば使うが、
    /// 確証が無いので Mac へ制御を返す根拠にはしない
    Unknown,
}

fn foreground_class() -> Fg {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return Fg::Normal;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return Fg::Normal;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return Fg::Unknown;
        }
        let r = process_elevated(h);
        CloseHandle(h);
        match r {
            Some(true) => Fg::Elevated,
            Some(false) => Fg::Normal,
            None => Fg::Unknown,
        }
    }
}

/// 監視スレッドを起動する(起動時に 1 回)
pub(crate) fn start() {
    std::thread::spawn(|| {
        let elevated_self = self_elevated();
        let mut reported = false;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let secure = !input_desktop_is_default();
            let fg = if elevated_self {
                Fg::Normal
            } else {
                foreground_class()
            };
            NEEDS_HELPER.store(secure || fg != Fg::Normal, Ordering::Relaxed);
            let blocked = secure || fg == Fg::Elevated;
            if !blocked {
                reported = false;
            } else if !reported && !helper::is_connected() {
                reported = true;
                fall_back_to_mac(secure);
            }
        }
    });
}

/// 操作補助が無く届かない時、Mac から操作中なら制御を Mac へ返す。
/// Mac のカーソルが Windows の見えない所で固まるのを防ぐ
fn fall_back_to_mac(secure: bool) {
    use crate::dragdrop::edge::CONTROLLED;
    use knit_common::proto::Msg;
    let what = if secure {
        "UAC やロック画面など保護された画面"
    } else {
        "管理者権限で動いているアプリ"
    };
    println!("[desk] {what}は Mac から操作できません(操作補助が未導入)。制御を Mac へ返します");
    if CONTROLLED.load(Ordering::Relaxed) {
        crate::state::send_main_msg(&Msg::Return { ny: 0.5 });
        crate::tray::notify(
            "Knit",
            &format!("{what}は Mac から操作できないため、操作を Mac へ戻しました。Windows 側で直接操作してください"),
        );
    }
}
