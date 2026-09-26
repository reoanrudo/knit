// tsunagu-win: Windows 側サーバ。TCP で受けた入力イベントを SendInput で注入する。
// 必須: 対話セッション起動 + OpenInputDesktop(フル権限) + SetThreadDesktop
// v0.5: GUI サブシステム化(コンソール非依存)+タスクトレイ常駐+待受モード追加
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![windows_subsystem = "windows"]

mod audio;
mod dragdrop;
mod tray;

use tsunagu_common::bulk;
use tsunagu_common::keymap::mac_kc_to_win_vk;

static DEBUG_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
use tsunagu_common::proto::{compatible, decode, encode, Msg, PORT, VERSION};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::System::Threading::{PROCESS_INFORMATION, STARTUPINFOW};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VK_CONTROL, VK_MENU,
    VK_SHIFT, VK_LWIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetSystemMetrics, SendMessageW, SetCursorPos, SM_CXSCREEN,
    SM_CYSCREEN,
};

const INPUT_MOUSE: u32 = 0;
const MOUSEEVENTF_MOVE: u32 = 0x0001;
const VK_LBUTTON_SENTINEL: u16 = 0xFF; // 使用しない(ボタンは専用関数で)
const VK_XBUTTON2: u16 = 0x06;

// ---------- Win32 直宣言(desktop 接続) ----------
#[link(name = "user32")]
unsafe extern "system" {
    fn OpenInputDesktop(dwFlags: u32, fInherit: bool, dwDesiredAccess: u32) -> *mut core::ffi::c_void;
    fn SetThreadDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
    fn CloseDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
    // クリップボード
    fn OpenClipboard(hWndNewOwner: *mut core::ffi::c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(uFormat: u32) -> *mut core::ffi::c_void;
    fn SetClipboardData(uFormat: u32, hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GetClipboardSequenceNumber() -> u32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
}

// CF_HDROP からファイルパス群を列挙する(shell32)
#[link(name = "shell32")]
unsafe extern "system" {
    /// ifile=0xFFFFFFFF でファイル個数、それ以外はパス長(文字数・NUL 除外)
    fn DragQueryFileW(
        hdrop: *mut core::ffi::c_void,
        ifile: u32,
        lpszfile: *mut u16,
        cch: u32,
    ) -> u32;
}

// ---------- Win32 直宣言(グローバルメモリ) ----------
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> *mut core::ffi::c_void;
    fn GlobalLock(hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalUnlock(hMem: *mut core::ffi::c_void) -> i32;
    fn GlobalFree(hMem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalSize(hMem: *mut core::ffi::c_void) -> usize;
}

// ---------- Win32 直宣言(コンソール離脱) ----------
#[link(name = "user32")]
unsafe extern "system" {
    fn GetConsoleWindow() -> *mut core::ffi::c_void;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    /// windows-sys 0.59 の Threading モジュールに無いため直宣言(ABI は安定)
    fn CreateProcessW(
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *const core::ffi::c_void,
        thread_attributes: *const core::ffi::c_void,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *const core::ffi::c_void,
        current_directory: *const u16,
        startup_info: *mut STARTUPINFOW,
        process_information: *mut PROCESS_INFORMATION,
    ) -> i32;
}

// ---------- Win32 直宣言(IME 制御) ----------
#[link(name = "imm32")]
unsafe extern "system" {
    fn ImmGetContext(hwnd: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn ImmSetOpenStatus(himc: *mut core::ffi::c_void, fOpen: i32) -> i32;
    fn ImmReleaseContext(hwnd: *mut core::ffi::c_void, himc: *mut core::ffi::c_void) -> i32;
    /// ウィンドウのデフォルトIMEウィンドウを取得(他プロセスのウィンドウでも可)
    fn ImmGetDefaultIMEWnd(hwnd: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

const WM_IME_CONTROL: u32 = 0x283;
const IMC_SETOPENSTATUS: usize = 0x0006;

/// フォアグラウンドウィンドウの IME を開(かな)/閉じ(英数)する。
/// キーエミュレート(VK_KANJI 等)と違い方向指定が確実。
/// IME コンテキストが取れないウィンドウでは半角/全角キー相当の
/// VK_KANJI 注入へフォールバックする(トグル動作)。
fn ime_set_open(open: bool) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_null() {
            // 自ウィンドウを持たないプロセスは ImmGetContext が他プロセスの
            // ウィンドウに対して null を返すため、デフォルトIMEウィンドウへ
            // WM_IME_CONTROL(IMC_SETOPENSTATUS) を送る(方向指定が確実な定番手法)
            let ime_wnd = ImmGetDefaultIMEWnd(hwnd);
            if !ime_wnd.is_null() {
                SendMessageW(ime_wnd, WM_IME_CONTROL, IMC_SETOPENSTATUS, open as isize);
                println!("[ime] WM_IME_CONTROL open={open} -> sent");
                return;
            }
            println!("[ime] default IME wnd=null -> fallback");
        } else {
            println!("[ime] no foreground window -> fallback");
        }
        // フォールバック: 半角/全角キー(VK_KANJI)の押し離し(トグル動作)
        inject_key(0xF4, false);
        inject_key(0xF4, true);
    }
}

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const GMEM_MOVEABLE: u32 = 0x0002;
/// 最後に Mac から受信して書き込んだテキスト(エコーバック送信防止)
static LAST_RECV_CLIP: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// cmd+Tab → Alt+Tab 変換中(Alt を保持し、cmd 離下で確定する)
static ALT_TAB_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// トレイ/バルーン表示用の接続状態(セッション確立で true)
static CONNECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)。Mac から Cfg で同期される
static CMD_ALT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 接続中の Windows スピーカーミュート(true=Mac のみ発音。既定 ON)。
/// Mac から Cfg で同期される(TSUNAGU_MUTE_SPK=0 で初期無効化)
static SPK_MUTE_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Mac が測定した RTT(ms)。Mac から Stat で届く(ステータス窓の表示用)
static RTT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Windows 画面の位置(0=Macの右/1=左/2=上/3=下)。Mac から Cfg で同期
pub static SIDE_W: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// Windows 側で現在押下中のマウスボタン(後片付けの UP 注入を押下中のみに絞る。
/// 押されていないボタンへの UP は通常無害だが、一部アプリで意図しない
/// クリックとして扱われる懸念を排除する)
/// 送信チャネル(wtx)の共有: トレイの「Mac へ戻る」等から送るために
/// session の開始時に登録し、終了時に外す
pub static WTX: std::sync::Mutex<Option<std::sync::mpsc::Sender<String>>> =
    std::sync::Mutex::new(None);

/// 「Mac へ戻る」用の Return 行(高さは画面中央相当)
pub fn proto_return() -> String {
    encode(&Msg::Return { ny: 0.5 })
}

static BTN_W: [std::sync::atomic::AtomicBool; 3] = [
    std::sync::atomic::AtomicBool::new(false),
    std::sync::atomic::AtomicBool::new(false),
    std::sync::atomic::AtomicBool::new(false),
];
/// 最後に Mac から受信してクリップボードへ載せたファイル群の指紋(エコーバック防止)
static LAST_RECV_FILES: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// 累計ファイル受信数(ステータス窓の表示用)
pub static FILES_RX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const CLIP_MAX_CHARS: usize = 1024 * 1024; // 1MB
/// Mac のクリップボード共有設定(Cfg で同期)。OFF の間は Windows からも送らない
static CLIP_SHARE_W: AtomicBool = AtomicBool::new(true);
/// 最後に Mac と同期したクリップボードのシーケンス番号
static LAST_SYNC_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn clipboard_seq() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// パスワードマネージャ等が「監視・共有しないで」と付ける登録形式があるか
/// (KeePass/1Password/Bitwarden 等が付ける Windows の慣行)
fn clipboard_is_excluded() -> bool {
    ["ExcludeClipboardContentFromMonitorProcessing", "Clipboard Viewer Ignore"].iter().any(|n| {
        let w: Vec<u16> = n.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let fmt = RegisterClipboardFormatW(w.as_ptr());
            fmt != 0 && IsClipboardFormatAvailable(fmt) != 0
        }
    })
}

/// Mac へ制御が戻る時に Windows のクリップボードを渡す(Deskflow と同じ「画面を
/// 離れる時に同期」方式)。旧方式は 200ms ごとに本文と画像全体を読み、画像は毎回
/// base64 化して比較していた(常時の CPU・メモリ負荷。レビュー D-F1)。
/// シーケンス番号が変わっていない限りクリップボードを開きもしない
fn sync_clipboard_to_mac() {
    if !CLIP_SHARE_W.load(Ordering::Relaxed) {
        return;
    }
    let seq = clipboard_seq();
    if LAST_SYNC_SEQ.swap(seq, Ordering::Relaxed) == seq {
        return;
    }
    let Some(tx) = WTX.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    std::thread::spawn(move || {
        if clipboard_is_excluded() {
            println!("[clip] 秘匿指定のコピー(パスワード等)のため送りません");
            return;
        }
        if let Some(text) = clipboard_read_text() {
            let echo = LAST_RECV_CLIP
                .lock()
                .map(|g| g.as_deref() == Some(text.as_str()))
                .unwrap_or(false);
            if !text.is_empty() && text.len() <= CLIP_MAX_CHARS && !echo {
                println!("[clip] win->mac {} bytes", text.len());
                let _ = tx.send(encode(&Msg::Clip { text }));
            }
            return;
        }
        if let Some(dib) = clipboard_read_dib() {
            if dib.len() <= bulk::MAX_IMAGE {
                match BULK_LINK.send(|w| bulk::send_image(w, &dib)) {
                    Ok(()) => println!("[clip] win->mac image {}KB", dib.len() / 1024),
                    Err(e) => println!("[clip] win->mac image 送信失敗: {e}"),
                }
            }
            return;
        }
        if let Some(files) = clipboard_read_files() {
            let key = files_key(&files);
            let echo = LAST_RECV_FILES
                .lock()
                .map(|g| g.as_deref() == Some(key.as_str()))
                .unwrap_or(false);
            if !echo {
                println!("[file] CF_HDROP: {} files", files.len());
                send_files_to_mac(&files);
            }
        }
    });
}

fn clipboard_read_text() -> Option<String> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None; // 他プロセス占有中(次回ポーリングで再試行)
        }
        let h = GetClipboardData(CF_UNICODETEXT);
        let mut locked = false;
        let out = if h.is_null() {
            None // テキスト形式ではない(画像等)
        } else {
            let p = GlobalLock(h) as *const u16;
            if p.is_null() {
                None
            } else {
                locked = true;
                let mut len = 0usize;
                while *p.add(len) != 0 && len < CLIP_MAX_CHARS {
                    len += 1;
                }
                Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
            }
        };
        if locked {
            GlobalUnlock(h);
        }
        CloseClipboard();
        out
    }
}

/// クリップボードから画像(CF_DIB)の生バイトを読む(Windows→Mac 画像同期用)
fn clipboard_read_dib() -> Option<Vec<u8>> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_DIB);
        let out = if h.is_null() {
            None
        } else {
            let size = GlobalSize(h);
            let p = GlobalLock(h) as *const u8;
            if p.is_null() || size == 0 {
                None
            } else {
                Some(std::slice::from_raw_parts(p, size).to_vec())
            }
        };
        if !h.is_null() {
            GlobalUnlock(h);
        }
        CloseClipboard();
        out
    }
}

fn clipboard_write_text(s: &str) -> bool {
    let mut utf16: Vec<u16> = s.encode_utf16().collect();
    utf16.push(0);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, utf16.len() * 2);
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        let p = GlobalLock(h) as *mut u16;
        if p.is_null() {
            GlobalFree(h);
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), p, utf16.len());
        GlobalUnlock(h);
        if SetClipboardData(CF_UNICODETEXT, h).is_null() {
            GlobalFree(h); // 設定失敗時は呼び出し側の解放責任
            CloseClipboard();
            return false;
        }
        CloseClipboard();
        true
    }
}

/// ファイルパス群を CF_HDROP の HGLOBAL へパックする(クリップボード操作を
/// 含まない純粋な生成。ドラッグ越境の IDataObject::GetData でも使う)。
/// DROPFILES ヘッダ(20byte): pFiles=20, pt=(0,0), fNC=0, fWide=1 の後ろに
/// UTF16 パス群(\0 区切り、リスト終端に追加の \0)が続く
pub(crate) fn make_hdrop_global(paths: &[String]) -> *mut core::ffi::c_void {
    const HEAD: usize = 20;
    let mut w: Vec<u16> = Vec::new();
    for p in paths {
        w.extend(p.encode_utf16());
        w.push(0);
    }
    w.push(0); // リスト終端
    let total = HEAD + w.len() * 2;
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, total);
        if h.is_null() {
            return std::ptr::null_mut();
        }
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            GlobalFree(h);
            return std::ptr::null_mut();
        }
        std::ptr::write_bytes(p, 0, total);
        let buf = std::slice::from_raw_parts_mut(p, total);
        buf[0..4].copy_from_slice(&(HEAD as u32).to_le_bytes()); // pFiles
        buf[16..20].copy_from_slice(&1i32.to_le_bytes()); // fWide = UTF16
        std::ptr::copy_nonoverlapping(w.as_ptr(), p.add(HEAD) as *mut u16, w.len());
        GlobalUnlock(h);
        h
    }
}

/// ファイルパス群をクリップボードへ(CF_HDROP)。Mac からのファイル受信完了時に
/// 呼ぶ。受け取ったファイルは Windows 側でそのまま Ctrl+V で貼り付けられる
pub(crate) fn clipboard_write_files(paths: &[String]) -> bool {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = make_hdrop_global(paths);
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        if SetClipboardData(CF_HDROP, h).is_null() {
            GlobalFree(h); // 設定失敗時は呼び出し側の解放責任
            CloseClipboard();
            return false;
        }
        CloseClipboard();
        true
    }
}

/// クリップボードからファイル参照(CF_HDROP)のパス群を読む。
/// エクスプローラーでファイルをコピー(Ctrl+C)した際に載る形式
fn clipboard_read_files() -> Option<Vec<String>> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let mut out = None;
        let h = GetClipboardData(CF_HDROP);
        if !h.is_null() {
            const COUNT: u32 = 0xFFFF_FFFF; // iFile=-1 で個数問い合わせ
            let n = DragQueryFileW(h, COUNT, std::ptr::null_mut(), 0);
            let mut v = Vec::new();
            for i in 0..n.min(64) {
                let len = DragQueryFileW(h, i, std::ptr::null_mut(), 0);
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u16; len as usize + 1];
                DragQueryFileW(h, i, buf.as_mut_ptr(), buf.len() as u32);
                v.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
            if !v.is_empty() {
                out = Some(v);
            }
        }
        CloseClipboard();
        out
    }
}

/// ファイル群の指紋(パス+サイズ)。同一コピーの再検出・エコーバック判定に使う
fn files_key(paths: &[String]) -> String {
    let sizes: u64 = paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
        .sum();
    format!("{}|{sizes}", paths.join("\u{1}"))
}

/// ファイル群を Mac へ送る(大容量経路。本線の入力・ping を詰まらせない)
fn send_files_to_mac(paths: &[String]) {
    let paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
    let total = bulk::total_size(&paths);
    if total == 0 || total > bulk::MAX_TOTAL {
        println!("[file] win->mac skip(total={total} bytes)");
        return;
    }
    match BULK_LINK.send(|w| bulk::send_files(w, &paths, false)) {
        Ok(n) => println!("[file] win->mac {n} 件送信完了"),
        Err(e) => println!("[file] win->mac 送信失敗: {e}"),
    }
}

/// 画像を CF_DIB としてクリップボードへ(Mac からの画像受信)
fn clipboard_write_dib(dib: &[u8]) -> bool {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, dib.len());
        let p = if h.is_null() { std::ptr::null_mut() } else { GlobalLock(h) as *mut u8 };
        if p.is_null() {
            if !h.is_null() {
                GlobalFree(h);
            }
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(dib.as_ptr(), p, dib.len());
        GlobalUnlock(h);
        if SetClipboardData(CF_DIB, h).is_null() {
            GlobalFree(h);
            CloseClipboard();
            return false;
        }
        CloseClipboard();
        true
    }
}

/// 大容量経路(ファイル・画像)。本線とは別の TCP 接続
static BULK_LINK: bulk::Link = bulk::Link::new();
static BULK: std::sync::OnceLock<bulk::Endpoint> = std::sync::OnceLock::new();

/// 大容量経路の受信完了(Mac からのファイル・画像)
fn win_on_bulk(e: bulk::Event) {
    match e {
        bulk::Event::Files { paths, drop } => {
            let files: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            let n = files.len();
            FILES_RX.fetch_add(n as u64, Ordering::Relaxed);
            // 自分が渡す CF_HDROP を Mac へ送り返さない
            *LAST_RECV_FILES.lock().unwrap_or_else(|e| e.into_inner()) = Some(files_key(&files));
            if drop && BTN_W[0].load(Ordering::Relaxed) {
                // まだ押している=掴んだまま → 本物の OLE ドラッグを開始
                dragdrop::start(files);
                return;
            }
            if drop {
                println!("[drag] 転送完了時点でボタン非押下のため Ctrl+V 形式へフォールバック");
            }
            if clipboard_write_files(&files) {
                LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
                println!("[file] 受信完了: {n} 件(クリップボードに載せました)");
                tray::notify("tsunagu", &format!("ファイルを受信: {n} 件(Ctrl+V で貼り付け可)"));
            }
        }
        bulk::Event::Image(dib) => {
            if !CLIP_SHARE_W.load(Ordering::Relaxed) {
                return;
            }
            let ok = clipboard_write_dib(&dib);
            if ok {
                LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
            }
            println!("[clip] mac->win image {}KB {}", dib.len() / 1024, if ok { "ok" } else { "FAILED" });
        }
    }
}

// ---------- INPUT 手動パック(type+pad+32byte共用体=40byte) ----------
#[repr(C)]
#[derive(Clone, Copy)]
struct InputBuf {
    itype: u32,
    _pad: u32,
    body: [u32; 6],
    extra: usize,
}

fn send_input_buf(buf: InputBuf) -> bool {
    assert_eq!(std::mem::size_of::<InputBuf>(), std::mem::size_of::<INPUT>());
    unsafe {
        SendInput(
            1,
            &buf as *const _ as *const INPUT,
            std::mem::size_of::<InputBuf>() as i32,
        ) == 1
    }
}

/// 拡張キー(E0 プレフィクス付き scan)の VK。scan code を併記して注入するため、
/// この区別を付けないと矢印キー等がテンキーの 4/6/8/2 と同じ scan で届く
fn is_extended_vk(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x6F | 0x5B | 0x5C | 0xA3 | 0xA5)
}

fn inject_key(vk: u16, up: bool) -> bool {
    inject_key_ex(vk, up, is_extended_vk(vk))
}

fn inject_key_ex(vk: u16, up: bool, extended: bool) -> bool {
    // wScan を必ず付ける: 日本語 IME 等は scan code 無しのキーを無視/不安定に
    // 扱うことがある(「ー」等の OEM キーで顕著)。vk と scan の併用が最も互換性が高い
    extern "system" {
        fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    }
    let scan = unsafe { MapVirtualKeyW(vk as u32, 0 /*MAPVK_VK_TO_VSC*/) } as u32;
    // KEYBDINPUT の共用体先頭 u32 は「低16bit=wVk / 高16bit=wScan」
    let vk_scan = ((scan & 0xFFFF) << 16) | (vk as u32 & 0xFFFF);
    send_input_buf(InputBuf {
        itype: INPUT_KEYBOARD,
        _pad: 0,
        body: [
            vk_scan,
            (if up { KEYEVENTF_KEYUP } else { 0 }) | if extended { 0x0001 /*EXTENDEDKEY*/ } else { 0 },
            0,
            0,
            0,
            0,
        ],
        extra: 0,
    })
}

fn inject_mouse_move_rel(dx: i32, dy: i32) -> bool {
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [dx as u32, dy as u32, 0, MOUSEEVENTF_MOVE, 0, 0],
        extra: 0,
    })
}

/// 絶対位置移動(0..65535 座標、プライマリ画面)。MOUSEEVENTF_ABSOLUTE は
/// Windows のポインタ加速曲線を通らないため、Mac の速度感がそのまま再現される
fn inject_mouse_move_abs(x: i32, y: i32) -> bool {
    const ABSOLUTE: u32 = 0x8000;
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [x as u32, y as u32, 0, MOUSEEVENTF_MOVE | ABSOLUTE, 0, 0],
        extra: 0,
    })
}

fn inject_mouse_btn(btn: u8, down: bool) -> bool {
    const LEFTDOWN: u32 = 0x0002;
    const LEFTUP: u32 = 0x0004;
    const RIGHTDOWN: u32 = 0x0008;
    const RIGHTUP: u32 = 0x0010;
    const MIDDLEDOWN: u32 = 0x0020;
    const MIDDLEUP: u32 = 0x0040;
    let flags = match (btn, down) {
        (0, true) => LEFTDOWN,
        (0, false) => LEFTUP,
        (1, true) => RIGHTDOWN,
        (1, false) => RIGHTUP,
        (2, true) => MIDDLEDOWN,
        (2, false) => MIDDLEUP,
        _ => return true,
    };
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [0, 0, 0, flags, 0, 0],
        extra: 0,
    })
}

/// XButton1/2(ブラウザの戻る/進む)。idx: 0=戻る, 1=進む
fn inject_xbutton(idx: u8, down: bool) -> bool {
    const XDOWN: u32 = 0x0080;
    const XUP: u32 = 0x0100;
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [0, 0, (idx + 1) as u32, if down { XDOWN } else { XUP }, 0, 0],
        extra: 0,
    })
}

fn inject_scroll(dx: f64, dy: f64) -> bool {
    // WHEEL_DELTA=120 を 1 として 6 units(0.05 ノッチ)刻みで注入する。
    // プレシジョンタッチパッドと同じ高解像度スクロールで、主要アプリは
    // 120 未満の delta を正しく累積するため滑らかに動く(旧: 1.0 ノッチ未満は切り捨て)
    const WHEEL: u32 = 0x0800;
    const HWHEEL: u32 = 0x1000;
    let min_units = |v: f64| (v * 120.0).round().abs() >= 1.0;
    let mut ok = true;
    if min_units(dy) {
        let amount = (-dy * 120.0).round() as i32; // 下スクロール(Mac dy負)→Winは正
        ok &= send_input_buf(InputBuf {
            itype: INPUT_MOUSE,
            _pad: 0,
            body: [0, 0, amount as u32, WHEEL, 0, 0],
            extra: 0,
        });
    }
    if min_units(dx) {
        let amount = (dx * 120.0).round() as i32;
        ok &= send_input_buf(InputBuf {
            itype: INPUT_MOUSE,
            _pad: 0,
            body: [0, 0, amount as u32, HWHEEL, 0, 0],
            extra: 0,
        });
    }
    ok
}

// ---------- 修飾キー状態管理(Mac mods → Win VK) ----------
struct ModState {
    ctrl: bool,
    alt: bool,
    win: bool,
    shift: bool,
    /// 注入して押下中のキー((vk, 拡張))。離脱・切断時に全部 up を注入する
    /// (修飾だけ解放していた旧実装では、切替の瞬間に押していた矢印キー等が
    /// Windows 側で押下扱いのまま残った)
    pressed: Vec<(u16, bool)>,
}
impl ModState {
    fn new() -> Self {
        Self { ctrl: false, alt: false, win: false, shift: false, pressed: Vec::new() }
    }
    /// 通常キーの注入(押下状態を追跡する)
    fn key(&mut self, vk: u16, down: bool, extended: bool) -> bool {
        self.pressed.retain(|&(v, e)| !(v == vk && e == extended));
        if down {
            self.pressed.push((vk, extended));
        }
        inject_key_ex(vk, !down, extended)
    }
    /// 離脱時の後片付け: 押下中の通常キー・修飾・マウスボタン・Alt+Tab を全て離す
    fn release_everything(&mut self) {
        for (vk, ext) in std::mem::take(&mut self.pressed) {
            inject_key_ex(vk, true, ext);
        }
        self.release_all();
        for b in 0u8..=2 {
            if BTN_W[b as usize].swap(false, Ordering::Relaxed) {
                inject_mouse_btn(b, false);
            }
        }
        if ALT_TAB_ACTIVE.swap(false, Ordering::Relaxed) {
            inject_key(0x09, true);
            inject_key(VK_MENU, true);
        }
    }
    fn apply(&mut self, ctrl: bool, opt: bool, cmd: bool, shift: bool) {
        // Mac: cmd→Win Ctrl(既定)/ Alt(Cfg で切替可), option→もう一方, ctrl→Winキー, shift→Shift
        let (cmd_vk, opt_vk) = if CMD_ALT.load(Ordering::Relaxed) {
            (VK_MENU, VK_CONTROL)
        } else {
            (VK_CONTROL, VK_MENU)
        };
        let want = [(cmd_vk, cmd), (opt_vk, opt), (VK_LWIN, ctrl), (VK_SHIFT, shift)];
        let mut state = [
            (VK_CONTROL, &mut self.ctrl),
            (VK_MENU, &mut self.alt),
            (VK_LWIN, &mut self.win),
            (VK_SHIFT, &mut self.shift),
        ];
        for (vk, cur) in &mut state {
            let target = want.iter().find(|(v, _)| *v == *vk).map(|(_, t)| *t).unwrap_or(false);
            if **cur != target {
                inject_key(*vk, !target);
            }
            **cur = target;
        }
    }
    fn release_all(&mut self) {
        self.apply(false, false, false, false);
    }
    /// Mac cmd キーの押下状態(self.ctrl が cmd に対応)
    fn cmd_pressed(&self) -> bool {
        self.ctrl
    }
}

// ---------- Win32 直宣言(コンソール無し運用/二重起動防止) ----------
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(n_std_handle: u32) -> *mut core::ffi::c_void;
    fn SetStdHandle(n_std_handle: u32, handle: *mut core::ffi::c_void) -> i32;
    fn CreateMutexW(
        attrs: *mut core::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> *mut core::ffi::c_void;
}

/// GUI サブシステムでは stdout が無効な場合があり、そのままだと println! が
/// パニックするため NUL デバイスへ繋ぎ替える(リダイレクト起動時は何もしない)
fn ensure_stdout() {
    unsafe {
        const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (u32)-11
        const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF2; // (u32)-12
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if !out.is_null() && out as isize != -1 {
            return; // リダイレクト起動などで有効
        }
        if let Ok(nul) = std::fs::OpenOptions::new().write(true).open("NUL") {
            use std::os::windows::io::AsRawHandle;
            let h = nul.as_raw_handle() as *mut core::ffi::c_void;
            SetStdHandle(STD_OUTPUT_HANDLE, h);
            SetStdHandle(STD_ERROR_HANDLE, h);
            std::mem::forget(nul); // ハンドルはプロセス終了まで保持
        }
    }
}

/// 二重起動防止(5分毎の自動復帰タスクが既存インスタンスと並走しないように)
fn acquire_single_instance() -> bool {
    unsafe {
        let mut name: Vec<u16> = "Local\\Tsunagu-Instance".encode_utf16().collect();
        name.push(0);
        let h = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
        if windows_sys::Win32::Foundation::GetLastError() == 183 {
            // ERROR_ALREADY_EXISTS = 既に起動している(自動復帰タスクからの起動等)
            let _ = h;
            return false;
        }
        true // ミューテックスはプロセス終了まで保持(明示解放しない)
    }
}

/// コンソール付き起動(手動実行/SSH/ターミナル)を検出したら、DETACHED_PROCESS
/// な自分を再起動して即終了する。GUI サブシステムでもコンソールから起動すると
/// そのコンソールに所属し、「ターミナルを閉じたら接続が切れる」原因になる。
/// 起動経路がどうであれコンソールの生死に左右されない本体へ置き換える
/// (ミューテックス取得の前に行うため、再起動先との二重起動競合も起きない)
fn detach_if_console() {
    unsafe {
        if GetConsoleWindow().is_null() {
            return; // コンソール無し起動(schtasks/vbs)= そのまま本体として続行
        }
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return,
        };
        // コマンドラインを "exeパス" 引数… の形で組み立てる
        let mut cmd = String::new();
        cmd.push('"');
        cmd.push_str(&exe.to_string_lossy());
        cmd.push('"');
        for a in std::env::args().skip(1) {
            cmd.push(' ');
            cmd.push_str(&a);
        }
        let mut cmdw: Vec<u16> = cmd.encode_utf16().collect();
        cmdw.push(0);
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let ok = CreateProcessW(
            std::ptr::null(),
            cmdw.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            DETACHED_PROCESS,
            std::ptr::null(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        );
        if ok != 0 {
            // 子プロセスは自分のミューテックスを取得して常駐を引き継ぐ
            windows_sys::Win32::Foundation::CloseHandle(pi.hProcess);
            windows_sys::Win32::Foundation::CloseHandle(pi.hThread);
            println!("[info] コンソールから独立したプロセスへ引き継ぎました");
            exit(0);
        }
        // CreateProcess 失敗時はそのまま続行(コンソール依存は受容するが機能は継続)
        eprintln!("[warn] デタッチ再起動に失敗。コンソール付きで継続します");
    }
}

/// 表示用のリリースバージョン(ステータス窓等)
pub const VERSION_STR: &str = env!("CARGO_PKG_VERSION");
const BUILD_ID: &str = "win-20260926-143916-049c56c";

fn main() {
    if std::env::args().any(|a| a == "--preview-ui") { tray::preview(); return; }
    ensure_stdout();
    // コンソール付き起動なら DETACHED な自分へ置き換わって終了(常駐性の根保証)
    detach_if_console();
    if !acquire_single_instance() {
        return; // 既に起動している(トレイの既存インスタンスが稼働中)
    }
    println!("[info] tsunagu-win {BUILD_ID}");
    let args: Vec<String> = std::env::args().collect();
    // トークンは必須(旧既定値 "tsunagu-dev" での脆弱な稼働を廃止)。
    // 環境変数 > exe同階層の .env > ~/.config/tsunagu/env の順で解決する
    let token = match tsunagu_common::envutil::get("TSUNAGU_TOKEN") {
        Some(t) if !t.is_empty() => t,
        _ => {
            eprintln!(
                "[fatal] TSUNAGU_TOKEN が未設定です。tsunagu-win.exe と同じフォルダの .env に\
                 TSUNAGU_TOKEN=<Mac側と同じ値> を設定してください"
            );
            exit(1);
        }
    };
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);

    // 対話デスクトップへ接続(SSH 起動では失敗する。schtasks/スタートアップ起動を使う)
    unsafe {
        let desk = OpenInputDesktop(0, false, 0x01FF);
        if desk.is_null() {
            eprintln!("[fatal] OpenInputDesktop failed. 対話セッションで起動してください");
            exit(1);
        }
        if SetThreadDesktop(desk) == 0 {
            eprintln!("[fatal] SetThreadDesktop failed");
            CloseDesktop(desk);
            exit(1);
        }
    }

    let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    println!("[info] desktop attached. screen {w}x{h}. listening on :{port}");

    if args.iter().any(|a| a == "--debug-keys") {
        DEBUG_KEYS.store(true, Ordering::Relaxed);
    }
    // 接続先は優先度順に: --host 引数 > TSUNAGU_HOST(.env 可)> Tailscale の既定
    // (有線直結 Thunderbolt ブリッジ / USB-LAN 直結の際は .env で指定する)
    let host = args
        .iter()
        .position(|a| a == "--host")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .or_else(|| tsunagu_common::envutil::get("TSUNAGU_HOST"))
        .unwrap_or_else(|| "100.100.10.9".to_string());
    println!("[info] desktop attached. screen {w}x{h}. connecting to {host}:{port}");
    *crate::tray::HOST_NOW.lock().unwrap_or_else(|e| e.into_inner()) = host.clone();

    // 接続方向: 既定は Win=クライアント(本環境のAP隔離対策)。
    // TSUNAGU_ROLE=server(--listen)で Win=サーバ(Mac=クライアント)に反転できる
    // (通常ネットワークの配布先向け)
    let role_server = args.iter().any(|a| a == "--listen")
        || tsunagu_common::envutil::get("TSUNAGU_ROLE").as_deref() == Some("server");

    // 接続中スピーカーミュートの初期値(既定 ON=Mac のみ発音)
    if tsunagu_common::envutil::get("TSUNAGU_MUTE_SPK").as_deref() == Some("0") {
        SPK_MUTE_MODE.store(false, Ordering::Relaxed);
    }

    // タスクトレイ常駐(状態表示・バルーン通知・終了)。失敗しても本体は継続
    tray::start();

    // 音声転送(Windows→Mac)。クライアントモードの接続先へ送る
    // (サーバモードは TSUNAGU_AUDIO_HOST で明示指定した時のみ)
    if tsunagu_common::envutil::get("TSUNAGU_AUDIO").as_deref() != Some("0") {
        let audio_host = tsunagu_common::envutil::get("TSUNAGU_AUDIO_HOST")
            .or_else(|| if role_server { None } else { Some(host.clone()) });
        match audio_host {
            Some(h) => audio::start(h, token.clone()),
            None => println!("[audio] サーバモードで音声先未指定のため無効(TSUNAGU_AUDIO_HOST で指定可)"),
        }
    }

    let bulk_ep: &'static bulk::Endpoint = BULK.get_or_init(|| bulk::Endpoint {
        link: &BULK_LINK,
        token: token.clone(),
        dir: std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join("Downloads")
            .join("Tsunagu"),
        on_event: win_on_bulk,
        log: |s| println!("{s}"),
    });

    if role_server {
        println!("[info] server mode. screen {w}x{h}");
        let bind = tsunagu_common::envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        let ep = bulk_ep;
        std::thread::spawn(move || bulk::serve(ep, &bind, port + bulk::PORT_OFFSET, tsunagu_common::net::is_tailscale));
        server_loop(&token, port, w, h);
        return;
    }

    use std::net::ToSocketAddrs;
    let addr = (host.as_str(), port).to_socket_addrs().ok().and_then(|mut it| it.next());
    let addr = match addr {
        Some(a) => a,
        None => {
            eprintln!("[fatal] invalid host");
            exit(1);
        }
    };
    println!("[info] client mode: connecting to {host}:{port}");
    let mut bulk_addr = addr;
    bulk_addr.set_port(port + bulk::PORT_OFFSET);
    std::thread::spawn(move || bulk::connect_loop(bulk_ep, bulk_addr, || CONNECTED.load(Ordering::Relaxed)));
    client_loop(&addr, &token, w, h);
}

/// 接続モード(既定): 相手(Mac)へ接続し続ける。切断は指数バックオフで再接続
fn client_loop(addr: &std::net::SocketAddr, token: &str, w: i32, h: i32) {
    let mut backoff = 500u64;
    loop {
        match std::net::TcpStream::connect_timeout(addr, Duration::from_secs(3)) {
            Ok(s) => {
                println!("[conn] connected");
                if let Err(e) = client_session(s, token, w, h) {
                    println!("[disc] {e}");
                }
                backoff = 500;
            }
            Err(e) => println!("[conn] failed: {e}"),
        }
        std::thread::sleep(Duration::from_millis(backoff));
        backoff = (backoff * 2).min(3000);
    }
}

/// 待受モード(TSUNAGU_ROLE=server): 相手(Mac=クライアント)からの接続を受け入れる。
/// hello のトークン検証後に hello_ok(自画面 w/h 付き)を返す
fn server_loop(token: &str, port: u16, w: i32, h: i32) {
    use std::io::Read;
    let bind_ip = tsunagu_common::envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[fatal] listen {bind_ip}:{port} failed: {e}");
            exit(1);
        }
    };
    println!("[info] listening on {bind_ip}:{port}");
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                println!("[conn] accept error: {e}");
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        println!("[conn] accepted from {peer}");
        // Tailscale CGNAT(100.64.0.0/10)外は即拒否(0.0.0.0 待受時の装置的防御)
        if let std::net::IpAddr::V4(v4) = peer.ip() {
            let o = v4.octets();
            if !(o[0] == 100 && (64..=127).contains(&o[1])) {
                println!("[conn] rejected: {peer} は Tailscale 範囲外です");
                continue;
            }
        } else {
            println!("[conn] rejected: {peer} (IPv6)");
            continue;
        }
        stream.set_nodelay(true).ok();
        // hello を待つ(未認証のため 8MB の行長制限付き)
        let mut pre = match stream.try_clone() {
            Ok(s) => std::io::BufReader::new(s),
            Err(_) => continue,
        };
        let mut line = String::new();
        match (&mut pre).take(8 * 1024 * 1024 + 1).read_line(&mut line) {
            Ok(0) | Err(_) => {
                println!("[conn] closed before hello");
                continue;
            }
            Ok(_) => {}
        }
        if line.len() > 8 * 1024 * 1024 {
            println!("[conn] hello too large. dropped");
            continue;
        }
        let ok = match decode(&line) {
            Some(Msg::Hello { ver, name, token: t, .. }) if compatible(ver) && t == token => {
                println!("[hello] from {name}");
                true
            }
            _ => false,
        };
        if !ok {
            println!("[conn] invalid hello");
            continue;
        }
        // hello_ok(相手=Mac が画面サイズを得られるよう自画面 w/h を含める)
        let mut wr = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = wr
            .write_all(encode(&Msg::HelloOk { name: "desktop".into(), w, h }).as_bytes())
            .and_then(|_| wr.flush());
        drop(wr);
        CONNECTED.store(true, Ordering::Relaxed);
        println!("[conn] established");
        tray::notify("tsunagu", "接続しました");
        audio::speaker_connect_mute(SPK_MUTE_MODE.load(Ordering::Relaxed));
        let _ = session(stream, w, h);
        CONNECTED.store(false, Ordering::Relaxed);
        BULK_LINK.clear();
        audio::speaker_disconnect();
        println!("[conn] lost. waiting for reconnect...");
        tray::notify("tsunagu", "切断しました(待機中)");
    }
}

/// 接続モード(既定)のセッション: hello 送信 → hello_ok 受信 → 本体セッション
fn client_session(stream: TcpStream, token: &str, w: i32, h: i32) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let mut writer = stream.try_clone()?;
    // クライアントとして hello を送る(encode() が行末 \n を持つため write_all で送る)
    let mut hello_sent = false;
    for _ in 0..3 {
        let hello = encode(&Msg::Hello {
            ver: VERSION,
            name: "desktop".into(),
            token: token.to_string(),
            w,
            h,
        });
        if writer.write_all(hello.as_bytes()).and_then(|_| writer.flush()).is_ok() {
            hello_sent = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if !hello_sent {
        return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "hello send failed"));
    }
    // hello_ok を待つ(Mac が accept 後に応答しない場合に再接続ループへ戻れるよう期限を切る)
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut pre = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    pre.read_line(&mut line)?;
    match decode(line.trim()) {
        Some(Msg::HelloOk { name, w: mw, h: mh }) => {
            println!("[hello] ok from {name} (mac screen {mw}x{mh})");
        }
        _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid hello_ok")),
    }
    CONNECTED.store(true, Ordering::Relaxed);
    tray::notify("tsunagu", "接続しました");
    audio::speaker_connect_mute(SPK_MUTE_MODE.load(Ordering::Relaxed));
    let r = session(stream, w, h);
    CONNECTED.store(false, Ordering::Relaxed);
    BULK_LINK.clear();
    audio::speaker_disconnect();
    tray::notify("tsunagu", "切断しました(自動再接続中)");
    r
}

/// 認証済みストリームの本体処理(接続/待受 両モード共通)
fn session(stream: TcpStream, w: i32, h: i32) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    // 読み出しタイムアウト: Mac は3秒毎に ping を送るため 12秒無音は経路断。
    // タイムアウトで read がエラーを返し、再接続ループへ制御が戻る(半開対策)
    stream.set_read_timeout(Some(Duration::from_secs(12))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    // 送信の単一ライタ化: 受信ループとクリップ監視スレッドが同一ソケットへ並行
    // write すると行が混線し、Mac 側 decode で黙って捨てられる(pong 欠損→偽切断)。
    // Mac 側と同じ mpsc+単一スレッド構成へ集約する(レビュー Wave1 X2/P0-3)
    let (wtx, wrx) = std::sync::mpsc::channel::<String>();
    *WTX.lock().unwrap_or_else(|e| e.into_inner()) = Some(wtx.clone());
    let mut writer = stream.try_clone()?;
    std::thread::spawn(move || {
        while let Ok(line) = wrx.recv() {
            if writer.write_all(line.as_bytes()).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    });
    let reader = BufReader::new(stream);
    let mut mods = ModState::new();
    // マウス移動のサブピクセル残高。Mac のトラックパッドは 1px 未満の delta が
    // 連続するため、毎回 round すると遅い移動が消えてカクカクする。整数部のみ注入し
    // 端数は次イベントへ持ち越す。
    let mut accum = (0.0f64, 0.0f64);
    // 認証は session の前(client_session/server_loop)で完了している
    let mut hello_done = true;
    let mut last_return_notify = Instant::now() - Duration::from_secs(10);
    let running = Arc::new(AtomicBool::new(true));
    let running_w = running.clone();

    // (旧heartbeatスレッドは削除: ソケット生死は read のエラーで判定し、
    //  接続監視は Mac 側の ping/pong が担うため不要だった)

    // 接続時点のクリップボードは送らない(以後の変化だけを Mac へ戻る時に同期する)
    LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                mods.release_everything();
                running_w.store(false, Ordering::Relaxed);
                return Err(e);
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg = match decode(&line) {
            Some(m) => m,
            None => continue,
        };
        match msg {
            Msg::HelloOk { name, w: mw, h: mh } => {
                println!("[hello] (重複) ok from {name} (mac screen {mw}x{mh})");
            }
            Msg::Ping { ts } => {
                let _ = wtx.send(encode(&Msg::Pong { ts }));
            }
            Msg::Cfg { cmd_alt, spk_mute, side, clip } => {
                CMD_ALT.store(cmd_alt, Ordering::Relaxed);
                CLIP_SHARE_W.store(clip, Ordering::Relaxed);
                SIDE_W.store(side.min(7), Ordering::Relaxed);
                println!("[cfg] ⌘キー -> {}", if cmd_alt { "Alt" } else { "Ctrl" });
                if SPK_MUTE_MODE.swap(spk_mute, Ordering::Relaxed) != spk_mute {
                    println!("[cfg] 接続中スピーカーミュート -> {}", if spk_mute { "ON" } else { "OFF" });
                    audio::speaker_set_mode(spk_mute, true);
                }
            }
            Msg::Vol { op } => {
                // VK_VOLUME_UP(0xAF)/DOWN(0xAE)/MUTE(0xAD)。up/down は2回送って調整幅を稼ぐ
                const VK_VOL_UP: u16 = 0xAF;
                const VK_VOL_DOWN: u16 = 0xAE;
                const VK_VOL_MUTE: u16 = 0xAD;
                match op {
                    0 => {
                        for _ in 0..2 {
                            inject_key(VK_VOL_UP, false);
                            inject_key(VK_VOL_UP, true);
                        }
                    }
                    1 => {
                        for _ in 0..2 {
                            inject_key(VK_VOL_DOWN, false);
                            inject_key(VK_VOL_DOWN, true);
                        }
                    }
                    _ => {
                        inject_key(VK_VOL_MUTE, false);
                        inject_key(VK_VOL_MUTE, true);
                    }
                }
                println!("[vol] op={op}");
            }
            Msg::Stat { rtt } => {
                RTT_MS.store(rtt, Ordering::Relaxed);
            }
            Msg::Key { kc, down, ctrl, opt, cmd, shift, tr } => {
                if !hello_done {
                    continue;
                }
                if DEBUG_KEYS.load(Ordering::Relaxed) && down {
                    let ch = tsunagu_common::charmap::mac_kc_to_char(kc);
                    println!("[key] kc={kc} ch={ch:?} mods c={ctrl} o={opt} m={cmd} s={shift}");
                }
                // Mac JIS の 英数(102)/かな(104)キーは Windows 側 IME の開閉に変換する
                // (HIToolbox 実測: kVK_JIS_Eisu=102, kVK_JIS_Kana=104)
                if down {
                    match kc {
                        104 => {
                            ime_set_open(true);
                            if DEBUG_KEYS.load(Ordering::Relaxed) {
                                println!("[ime] kana(kc=104) -> IME on");
                            }
                            continue;
                        }
                        102 => {
                            ime_set_open(false);
                            if DEBUG_KEYS.load(Ordering::Relaxed) {
                                println!("[ime] eisu(kc=102) -> IME off");
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
                // Mac の cmd+Tab(ウィンドウ切替)は Windows の Alt+Tab へ変換する。
                // Alt は cmd が離されるまで保持し、離した瞬間に切替を確定させる。
                // 確定条件から prev_cmd 依存を外した: cmd+Tab down の mods.apply(false,...)
                // が self.cmd 相当を false に落とすため旧条件は恒偽で、VK_MENU up が
                // 誰にも注入されず Alt が押しっぱなしに残留する実績バグだった
                if ALT_TAB_ACTIVE.load(Ordering::Relaxed) && !cmd {
                    // cmd 離下 → Alt+Tab 確定
                    inject_key(0x09, true); // VK_TAB up
                    inject_key(VK_MENU, true);
                    ALT_TAB_ACTIVE.store(false, Ordering::Relaxed);
                    println!("[alttab] confirmed");
                }
                if kc == 48 && cmd && !opt && !ctrl && !tr {
                    if down {
                        // cmd 分の Ctrl 押下を抑制してから Alt+Tab を合成
                        mods.apply(false, opt, false, shift);
                        inject_key(VK_MENU, false);
                        inject_key(0x09, false);
                        ALT_TAB_ACTIVE.store(true, Ordering::Relaxed);
                    } else {
                        inject_key(0x09, true); // Tab up のみ(Alt は保持)
                    }
                    if DEBUG_KEYS.load(Ordering::Relaxed) {
                        println!("[alttab] cmd+tab -> alt+tab");
                    }
                    continue;
                }
                mods.apply(ctrl, opt, cmd, shift);
                if let Some(vk) = mac_kc_to_win_vk(kc) {
                    // テンキー Enter(76)は通常 Enter と同じ VK で拡張フラグだけが違う
                    let ext = kc == 76 || is_extended_vk(vk);
                    let ok = mods.key(vk, down, ext);
                    if DEBUG_KEYS.load(Ordering::Relaxed) && !ok {
                        println!("[key] INJECT FAILED kc={kc}");
                    }
                }
            }
            Msg::MouseMove { dx, dy } => {
                if !hello_done {
                    continue;
                }
                accum.0 += dx;
                accum.1 += dy;
                // 異常な残高(1e6超)は何かの暴発なので捨てる
                if accum.0.abs() > 1.0e6 || accum.1.abs() > 1.0e6 {
                    accum.0 = 0.0;
                    accum.1 = 0.0;
                }
                let (ix, iy) = (accum.0.trunc(), accum.1.trunc());
                if ix != 0.0 || iy != 0.0 {
                    accum.0 -= ix;
                    accum.1 -= iy;
                    inject_mouse_move_rel(ix as i32, iy as i32);
                    // 実際にカーソルが動いたときだけ左端到達を判定する
                    maybe_notify_return(&wtx, &mut last_return_notify, w, h, &mut mods);
                }
            }
            Msg::MouseAbs { nx, ny } => {
                if !hello_done {
                    continue;
                }
                let x = (nx.clamp(0.0, 1.0) * 65535.0).round() as i32;
                let y = (ny.clamp(0.0, 1.0) * 65535.0).round() as i32;
                inject_mouse_move_abs(x, y);
                if DEBUG_KEYS.load(Ordering::Relaxed) {
                    println!("[abs] -> ({x},{y})");
                }
                maybe_notify_return(&wtx, &mut last_return_notify, w, h, &mut mods);
            }
            Msg::MouseButton { btn, down } => {
                if !hello_done {
                    continue;
                }
                if btn >= 3 {
                    // XButton1/2(トラックパッドの戻る/進むスワイプ)
                    inject_xbutton(btn - 3, down);
                } else {
                    BTN_W[btn as usize].store(down, Ordering::Relaxed);
                    inject_mouse_btn(btn, down);
                }
            }
            Msg::Scroll { dx, dy } => {
                if !hello_done {
                    continue;
                }
                inject_scroll(dx, dy);
            }
            Msg::Leave => {
                // Mac が制御を取り戻した: 押しっぱなしを残さず、Windows 側で
                // コピーされた内容があれば Mac へ渡す
                mods.release_everything();
                sync_clipboard_to_mac();
            }
            Msg::Warp { nx, ny } => {
                if !hello_done {
                    continue;
                }
                let x = (nx.clamp(0.0, 1.0) * w as f64) as i32;
                let y = (ny.clamp(0.0, 1.0) * h as f64) as i32;
                unsafe { SetCursorPos(x, y) };
                last_return_notify = Instant::now();
            }
            Msg::Clip { text } => {
                if text.len() > CLIP_MAX_CHARS {
                    continue;
                }
                *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
                let ok = clipboard_write_text(&text)
                    || (std::thread::sleep(Duration::from_millis(150)), clipboard_write_text(&text)).1;
                if ok {
                    LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
                    println!("[clip] mac->win {} bytes", text.len());
                } else {
                    println!("[clip] mac->win write failed (busy clipboard)");
                }
            }
            Msg::Bye => {
                running_w.store(false, Ordering::Relaxed);
                break;
            }
            _ => {}
        }
    }
    mods.release_everything();
    running_w.store(false, Ordering::Relaxed);
    Ok(())
}

/// ファイル受信の開始: Downloads\Tsunagu へ新規作成し書き込みハンドルを返す。
/// サイズ上限 200MB。ファイル名はパス区切り・Windows 禁止文字・先頭 '.' を無害化
/// カーソルが Mac 側の境界(SIDE に応じた端)に達したら Mac へ復帰通知
/// (連打防止 0.7 秒クールダウン)。境界に沿った比率も送り、Mac 側の復帰位置に
/// 反映させる(境界の連続性)。side 0/1=縦比率、2/3=横比率を ny へ載せる
fn maybe_notify_return(
    wtx: &std::sync::mpsc::Sender<String>,
    last: &mut Instant,
    w: i32,
    h: i32,
    mods: &mut ModState,
) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    let side = SIDE_W.load(Ordering::Relaxed);
    let hit = match side {
        1 | 6 | 7 => p.x >= w - 1, // Mac は左(左上/左下含む)→ Win の右端で戻る
        2 => p.y >= h - 1,          // Mac は上にある → Win の下端で戻る
        3 => p.y <= 1,              // Mac は下にある → Win の上端で戻る
        _ => p.x <= 1,              // 既定: Mac は右(右上/右下含む)→ 左端で戻る
    };
    if hit && last.elapsed() >= Duration::from_millis(700) {
        let ny = match side {
            2 | 3 => {
                if w > 0 { (p.x as f64 / w as f64).clamp(0.0, 1.0) } else { 0.5 }
            }
            _ => {
                if h > 0 { (p.y as f64 / h as f64).clamp(0.0, 1.0) } else { 0.5 }
            }
        };
        let _ = wtx.send(encode(&Msg::Return { ny }));
        *last = Instant::now();
        // Mac へ制御を返すための後片付け: 押しっぱなしの修飾キーに加え、
        // (a) ドラッグ中のマウスボタンを離す(選択ドラッグの残留防止)
        // (b) Alt+Tab 変換が未確定なら確定する(スイッチャー残留防止)
        mods.release_everything();
    }
}
