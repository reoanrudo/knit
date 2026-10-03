//! 操作補助(UAC の確認画面・ロック画面・管理者権限のアプリへの入力)。
//!
//! 通常の knit-win(ユーザー権限)の SendInput は、保護デスクトップと昇格した窓へ
//! 届かない。これを埋めるため、同じ exe を SYSTEM 権限の別プロセスとして常駐させ、
//! 入力の注入だけを肩代わりさせる。
//!
//! - supervisor(`--input-supervisor`): SYSTEM のスケジュールタスクで常駐。操作中の
//!   コンソールセッションに SYSTEM トークンで helper を起動し、落ちたら起こし直す
//! - helper(`--input-helper`): 名前付きパイプで knit-win から注入要求を受け、注入のたびに
//!   現在の入力デスクトップへ切り替えて SendInput / SetCursorPos する
//! - client(通常の knit-win): 直接注入が届かない間だけパイプへ中継する
//!
//! 導入は管理者権限で `knit-win.exe --install-input-helper`(install.bat から呼ぶ)。
//! SYSTEM の入力注入経路は UAC の「はい」も押せる強い権限なので、接続できる相手を
//! 次で絞る: パイプの DACL(対話ユーザーと SYSTEM のみ・リモート拒否)、同一
//! セッションの確認、接続元の exe パスが導入時に管理者が記録した値と一致すること。
use crate::input::InputBuf;
use core::ffi::c_void;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED,
    INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{
    DuplicateTokenEx, SecurityImpersonation, SetTokenInformation, TokenPrimary, TokenSessionId,
    SECURITY_ATTRIBUTES, TOKEN_ALL_ACCESS,
};
use windows_sys::Win32::Storage::FileSystem::{
    ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, CreateProcessAsUserW, GetCurrentProcess, GetCurrentProcessId, OpenProcess,
    OpenProcessToken, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;

const PIPE_NAME: &str = r"\\.\pipe\knit-input";
/// 接続直後に helper が送る合図(版)。食い違う相手とは話さない
const MAGIC: &[u8; 4] = b"KNH1";
const FRAME_INPUT: u8 = 1;
const FRAME_CURSOR: u8 = 2;
const FRAME_PING: u8 = 3;
const TASK_NAME: &str = "knit_input";
const HELPER_EXE: &str = "knit-win-input.exe";
const ALLOWED_FILE: &str = "allowed-client.txt";

/// SYSTEM の補助は stdout を持たないため、導入先(管理者だけが書ける場所)のログへ書く。
/// 肥大化を避け 256KB を超えたら作り直す
fn hlog(msg: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let path = exe.with_file_name("helper.log");
    if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "{t} {msg}");
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ======================================================================
// client(通常の knit-win)
// ======================================================================

static CONN: Mutex<Option<std::fs::File>> = Mutex::new(None);
static UP: AtomicBool = AtomicBool::new(false);

/// 操作補助へ接続済みか
pub(crate) fn is_connected() -> bool {
    UP.load(Ordering::Relaxed)
}

fn drop_conn(slot: &mut Option<std::fs::File>) {
    *slot = None;
    UP.store(false, Ordering::Relaxed);
}

fn try_connect() {
    use std::io::Read;
    let Ok(mut f) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
    else {
        return;
    };
    let mut magic = [0u8; 4];
    if f.read_exact(&mut magic).is_err() || &magic != MAGIC {
        return;
    }
    let mut g = CONN.lock().unwrap_or_else(|e| e.into_inner());
    *g = Some(f);
    UP.store(true, Ordering::Relaxed);
    println!("[helper] 操作補助に接続しました(UAC・管理者権限のアプリも操作できます)");
}

/// 接続の維持スレッドを起動する(起動時に 1 回)。未導入なら静かに待ち続ける
pub(crate) fn start_client() {
    std::thread::spawn(|| loop {
        if is_connected() {
            // 生存確認。切れていれば書き込み失敗で検知して接続を捨てる
            let mut g = CONN.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(f) = g.as_mut() {
                if f.write_all(&[FRAME_PING]).is_err() {
                    drop_conn(&mut g);
                    println!("[helper] 操作補助との接続が切れました");
                }
            }
        } else {
            try_connect();
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}

fn send_frame(frame: &[u8]) -> bool {
    let mut g = CONN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(f) = g.as_mut() else { return false };
    if f.write_all(frame).is_ok() {
        true
    } else {
        drop_conn(&mut g);
        false
    }
}

/// INPUT を操作補助へ中継する。未接続・失敗なら false(呼び出し側が直接注入に戻る)
pub(crate) fn send_input(buf: &InputBuf) -> bool {
    let mut frame = [0u8; 1 + std::mem::size_of::<InputBuf>()];
    frame[0] = FRAME_INPUT;
    // SAFETY: InputBuf は padding の無い repr(C)(input.rs のサイズ検査で保証)
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (buf as *const InputBuf).cast::<u8>(),
            std::mem::size_of::<InputBuf>(),
        )
    };
    frame[1..].copy_from_slice(bytes);
    send_frame(&frame)
}

pub(crate) fn send_cursor(x: i32, y: i32) -> bool {
    let mut frame = [0u8; 9];
    frame[0] = FRAME_CURSOR;
    frame[1..5].copy_from_slice(&x.to_le_bytes());
    frame[5..9].copy_from_slice(&y.to_le_bytes());
    send_frame(&frame)
}

// ======================================================================
// helper(SYSTEM・ユーザーのセッション内)
// ======================================================================

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenInputDesktop(flags: u32, inherit: bool, access: u32) -> *mut c_void;
    fn CloseDesktop(h: *mut c_void) -> i32;
    fn SetThreadDesktop(h: *mut c_void) -> i32;
    fn GetUserObjectInformationW(
        h: *mut c_void,
        index: i32,
        info: *mut c_void,
        len: u32,
        needed: *mut u32,
    ) -> i32;
}

fn desktop_name(h: *mut c_void) -> String {
    let mut buf = [0u16; 64];
    let mut need = 0u32;
    let ok = unsafe {
        GetUserObjectInformationW(h, 2, buf.as_mut_ptr().cast(), std::mem::size_of_val(&buf) as u32, &mut need)
    };
    if ok == 0 {
        return "?".into();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

struct DeskCache {
    cur: *mut c_void,
    at: Instant,
    name: String,
}

impl DeskCache {
    /// 現在の入力デスクトップへ切り替える。UAC の表示・消滅に追従するため
    /// 20ms を超えたら取り直す(毎回の取得は mouse move の高頻度で無駄になる)
    fn follow(&mut self) {
        if !self.cur.is_null() && self.at.elapsed() < Duration::from_millis(20) {
            return;
        }
        self.at = Instant::now();
        unsafe {
            let d = OpenInputDesktop(0, false, 0x01FF);
            if d.is_null() {
                hlog(&format!("OpenInputDesktop 失敗 err={}", GetLastError()));
                return;
            }
            if SetThreadDesktop(d) != 0 {
                if !self.cur.is_null() {
                    CloseDesktop(self.cur);
                }
                self.cur = d;
                let n = desktop_name(d);
                if n != self.name {
                    hlog(&format!("入力デスクトップ: {} -> {n}", self.name));
                    self.name = n;
                }
            } else {
                hlog(&format!("SetThreadDesktop 失敗 err={}", GetLastError()));
                CloseDesktop(d);
            }
        }
    }
}

fn read_exact(h: *mut c_void, buf: &mut [u8]) -> bool {
    let mut got = 0usize;
    while got < buf.len() {
        let mut n = 0u32;
        let ok = unsafe {
            ReadFile(
                h,
                buf[got..].as_mut_ptr(),
                (buf.len() - got) as u32,
                &mut n,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || n == 0 {
            return false;
        }
        got += n as usize;
    }
    true
}

fn write_all(h: *mut c_void, buf: &[u8]) -> bool {
    let mut n = 0u32;
    unsafe {
        WriteFile(h, buf.as_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut()) != 0
            && n as usize == buf.len()
    }
}

fn normalize_path(p: &str) -> String {
    p.trim().trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase()
}

/// 接続元が許可された knit-win か(同一セッション・導入時に記録した exe パス)
fn client_allowed(pipe: *mut c_void) -> bool {
    unsafe {
        let mut pid = 0u32;
        if GetNamedPipeClientProcessId(pipe, &mut pid) == 0 || pid == 0 {
            return false;
        }
        let (mut cs, mut ms) = (u32::MAX, u32::MAX - 1);
        if ProcessIdToSessionId(pid, &mut cs) == 0
            || ProcessIdToSessionId(GetCurrentProcessId(), &mut ms) == 0
            || cs != ms
        {
            println!("[helper] 別セッションからの接続を拒否しました");
            return false;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return false;
        }
        let client = normalize_path(&String::from_utf16_lossy(&buf[..len as usize]));
        let allowed = std::env::current_exe()
            .ok()
            .and_then(|e| std::fs::read_to_string(e.with_file_name(ALLOWED_FILE)).ok())
            .map(|s| normalize_path(&s));
        if allowed.as_deref() != Some(client.as_str()) {
            println!("[helper] 許可されていない接続元を拒否しました: {client}");
            return false;
        }
        true
    }
}

/// 1 クライアントの注入要求を処理する。切断・不正で戻る
fn serve(pipe: *mut c_void) {
    let mut desk = DeskCache {
        cur: std::ptr::null_mut(),
        at: Instant::now(),
        name: String::new(),
    };
    hlog("クライアント接続");
    let mut kind = [0u8; 1];
    while read_exact(pipe, &mut kind) {
        match kind[0] {
            FRAME_INPUT => {
                let mut b = [0u8; std::mem::size_of::<InputBuf>()];
                if !read_exact(pipe, &mut b) {
                    return;
                }
                let itype = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                // マウス(0)とキーボード(1)だけ。ハードウェア入力は受けない
                if itype > 1 {
                    println!("[helper] 不正な入力種別 {itype}。接続を切ります");
                    return;
                }
                desk.follow();
                let n = unsafe { SendInput(1, b.as_ptr().cast::<INPUT>(), b.len() as i32) };
                // 移動以外(クリック・キー)と失敗だけ記録する(移動は高頻度)
                let flags = u32::from_le_bytes([b[20], b[21], b[22], b[23]]);
                if n != 1 || itype == 1 || flags & 0x7FFF_FFFE != 0 && itype == 0 && flags & 0x0001 == 0 {
                    hlog(&format!(
                        "SendInput type={itype} flags={flags:#x} -> {n} (err={}) desk={}",
                        unsafe { GetLastError() },
                        desk.name
                    ));
                }
            }
            FRAME_CURSOR => {
                let mut b = [0u8; 8];
                if !read_exact(pipe, &mut b) {
                    return;
                }
                let x = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                let y = i32::from_le_bytes([b[4], b[5], b[6], b[7]]);
                desk.follow();
                unsafe { SetCursorPos(x, y) };
            }
            FRAME_PING => {}
            other => {
                println!("[helper] 不明なフレーム {other}。接続を切ります");
                return;
            }
        }
    }
}

/// `--input-helper`: パイプで待ち受けて注入する(SYSTEM・ユーザーのセッション内)
pub(crate) fn run_helper() -> i32 {
    // 対話ユーザー(IU)と SYSTEM(SY)だけ。リモート接続は PIPE_REJECT_REMOTE_CLIENTS
    let sddl = wide("D:(A;;GRGW;;;IU)(A;;GA;;;SY)");
    let mut psd: *mut c_void = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut psd,
            std::ptr::null_mut(),
        )
    } == 0
    {
        eprintln!("[helper] セキュリティ記述子を作れません");
        return 1;
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: psd,
        bInheritHandle: 0,
    };
    // 先に同名のパイプを作った第三者になりすまされないよう、最初の 1 個を自分で作る
    let name = wide(PIPE_NAME);
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            &sa,
        )
    };
    unsafe { LocalFree(psd) };
    if pipe == INVALID_HANDLE_VALUE {
        eprintln!("[helper] パイプを作れません(err={})", unsafe { GetLastError() });
        return 1;
    }
    println!("[helper] 待受開始 {PIPE_NAME}");
    loop {
        let ok = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
        if ok != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
            if client_allowed(pipe) && write_all(pipe, MAGIC) {
                serve(pipe);
            }
        }
        unsafe { DisconnectNamedPipe(pipe) };
    }
}

// ======================================================================
// supervisor(SYSTEM・セッション 0 のタスク)
// ======================================================================

/// SYSTEM トークンを複製して操作中のセッションへ付け替え、helper を起動する
fn spawn_in_session(exe: &str, session: u32) -> Option<*mut c_void> {
    unsafe {
        let mut tok = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut tok) == 0 {
            return None;
        }
        let mut dup = std::ptr::null_mut();
        let ok = DuplicateTokenEx(
            tok,
            TOKEN_ALL_ACCESS,
            std::ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut dup,
        );
        CloseHandle(tok);
        if ok == 0 {
            return None;
        }
        if SetTokenInformation(
            dup,
            TokenSessionId,
            (&session as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        ) == 0
        {
            CloseHandle(dup);
            return None;
        }
        let mut desktop = wide("winsta0\\default");
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.lpDesktop = desktop.as_mut_ptr();
        let mut cmd = wide(&format!("\"{exe}\" --input-helper"));
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let ok = CreateProcessAsUserW(
            dup,
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        );
        CloseHandle(dup);
        if ok == 0 {
            return None;
        }
        CloseHandle(pi.hThread);
        Some(pi.hProcess)
    }
}

/// `--input-supervisor`: 操作中のセッションに helper を常に 1 つ立てておく
pub(crate) fn run_supervisor() -> i32 {
    let m = wide("Global\\KnitInputSupervisor");
    unsafe {
        CreateMutexW(std::ptr::null(), 0, m.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return 0;
        }
    }
    let Ok(exe) = std::env::current_exe() else {
        return 1;
    };
    let exe = exe.to_string_lossy().into_owned();
    let mut child: Option<(*mut c_void, u32)> = None;
    loop {
        let sid = unsafe { WTSGetActiveConsoleSessionId() };
        if let Some((h, s)) = child {
            let alive = unsafe { WaitForSingleObject(h, 0) } == WAIT_TIMEOUT;
            if !alive || s != sid {
                unsafe {
                    if alive {
                        TerminateProcess(h, 0);
                    }
                    CloseHandle(h);
                }
                child = None;
            }
        }
        // セッション 0 は GUI の無いサービス用。操作中のコンソールが無い間は待つ
        if child.is_none() && sid != 0 && sid != u32::MAX {
            child = spawn_in_session(&exe, sid).map(|h| (h, sid));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

// ======================================================================
// 導入・撤去(管理者権限)
// ======================================================================

fn install_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into()))
        .join("Knit")
}

fn schtasks(args: &[&str]) -> bool {
    std::process::Command::new("schtasks")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `--install-input-helper`: 管理者だけが書ける場所へ exe を複製し、SYSTEM タスクを登録する
pub(crate) fn install() -> i32 {
    if !crate::inputdesk::self_elevated() {
        println!("INPUT_HELPER_NEEDS_ADMIN: 管理者として実行してください");
        return 2;
    }
    let (Ok(me), dir) = (std::env::current_exe(), install_dir()) else {
        return 1;
    };
    let dest = dir.join(HELPER_EXE);
    // 旧版が動いていると上書きできないため先に止める
    let _ = schtasks(&["/End", "/TN", TASK_NAME]);
    let _ = std::process::Command::new("taskkill")
        .args(["/IM", HELPER_EXE, "/F"])
        .output();
    std::thread::sleep(Duration::from_millis(500));
    if let Err(e) = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::copy(&me, &dest).map(|_| ()))
        .and_then(|_| std::fs::write(dir.join(ALLOWED_FILE), me.to_string_lossy().as_bytes()))
    {
        println!("INPUT_HELPER_FAILED: 配置に失敗しました: {e}");
        return 1;
    }
    let tr = format!("\"{}\" --input-supervisor", dest.display());
    let created = schtasks(&[
        "/Create", "/TN", TASK_NAME, "/TR", &tr, "/SC", "ONSTART", "/RU", "SYSTEM", "/RL",
        "HIGHEST", "/F",
    ]);
    if !created || !schtasks(&["/Run", "/TN", TASK_NAME]) {
        println!("INPUT_HELPER_FAILED: タスクの登録/起動に失敗しました");
        return 1;
    }
    println!("INPUT_HELPER_INSTALLED");
    0
}

/// `--uninstall-input-helper`
pub(crate) fn uninstall() -> i32 {
    let _ = schtasks(&["/End", "/TN", TASK_NAME]);
    let _ = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]);
    let _ = std::process::Command::new("taskkill")
        .args(["/IM", HELPER_EXE, "/F"])
        .output();
    std::thread::sleep(Duration::from_millis(500));
    let dir = install_dir();
    if dir.exists() && std::fs::remove_dir_all(&dir).is_err() {
        println!("INPUT_HELPER_UNINSTALL_PARTIAL: {} を削除できません(管理者として実行してください)", dir.display());
        return 1;
    }
    println!("INPUT_HELPER_UNINSTALLED");
    0
}
