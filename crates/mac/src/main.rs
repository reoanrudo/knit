// tsunagu-mac: Mac 側クライアント。CGEventTap で入力を横流しし、Windows へ送信する。
// 画面右端でカーソルが Mac→Windows 切替、Windows カーソル左端(または F13)で復帰。
#![allow(non_camel_case_types)]

mod audio;
mod gui;

use tsunagu_common::envutil;
use tsunagu_common::proto::{decode, encode, Msg, PORT, VERSION};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ---------- CoreGraphics C API 直宣言 ----------
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGSize {
    pub w: f64,
    pub h: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

type CGEventRef = *mut core::ffi::c_void;
type CFMachPortRef = *mut core::ffi::c_void;
type CFRunLoopRef = *mut core::ffi::c_void;
type CFRunLoopSourceRef = *mut core::ffi::c_void;
type CFAllocatorRef = *mut core::ffi::c_void;
type CFStringRef = *mut core::ffi::c_void;
type CGEventMask = u64;
type CGEventFlags = u64;

// EventType 定数(SDK ヘッダ実測値)
const EVT_LEFT_DOWN: u32 = 1;
const EVT_LEFT_UP: u32 = 2;
const EVT_RIGHT_DOWN: u32 = 3;
const EVT_RIGHT_UP: u32 = 4;
const EVT_MOUSE_MOVED: u32 = 5;
const EVT_LEFT_DRAGGED: u32 = 6;
const EVT_RIGHT_DRAGGED: u32 = 7;
const EVT_KEY_DOWN: u32 = 10;
const EVT_KEY_UP: u32 = 11;
const EVT_FLAGS_CHANGED: u32 = 12;
const EVT_SCROLL_WHEEL: u32 = 22;
const EVT_OTHER_DOWN: u32 = 25;
const EVT_OTHER_UP: u32 = 26;
const EVT_OTHER_DRAGGED: u32 = 27;

// field 定数(SDK ヘッダ実測値)
const FIELD_DELTA_X: i32 = 4;
const FIELD_DELTA_Y: i32 = 5;
const FIELD_KEYCODE: i32 = 9;
const FIELD_SCROLL_A1: i32 = 96; // PointDeltaAxis1(縦, ピクセル)
const FIELD_SCROLL_A2: i32 = 97; // PointDeltaAxis2(横, ピクセル)

// flags(IOLLEvent.h 実測値)
const FLAG_SHIFT: CGEventFlags = 0x0002_0000;
const FLAG_CTRL: CGEventFlags = 0x0004_0000;
const FLAG_OPT: CGEventFlags = 0x0008_0000;
const FLAG_CMD: CGEventFlags = 0x0010_0000;
const FLAG_FN: CGEventFlags = 0x8000_0000; // kCGEventFlagMaskSecondaryFn

const KC_F13: i64 = 105;
/// 切替ホットキー(Mac keycode)。TSUNAGU_HOTKEY_KC で変更可。
/// MacBook 内蔵キーボードには F13 が無いため、例えば右Cmd(54)等に変えられる
static HOTKEY_KC: OnceLock<i64> = OnceLock::new();

fn hotkey_kc() -> i64 {
    HOTKEY_KC.get().copied().unwrap_or(KC_F13)
}

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: i32, place: i32, options: u32, events_of_interest: CGEventMask,
        callback: unsafe extern "C" fn(
            proxy: *mut core::ffi::c_void, event_type: u32, event: CGEventRef, user_info: *mut core::ffi::c_void,
        ) -> CGEventRef,
        user_info: *mut core::ffi::c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetType(event: CGEventRef) -> u32;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: i32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGGetActiveDisplayList(max_displays: u32, active_displays: *mut u32, display_count: *mut u32) -> i32;
    fn CGWarpMouseCursorPosition(new: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connect: bool) -> i32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGSetLocalEventsSuppressionInterval(seconds: f64) -> i32;
    fn CGEventCreate(allocator: CFAllocatorRef) -> CGEventRef;
    fn CFRelease(cf: *mut core::ffi::c_void);
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef, c_str: *const core::ffi::c_char, encoding: u32,
    ) -> CFStringRef;
    static kCFBooleanTrue: *const core::ffi::c_void;
    // Deskflow hideCursor/showCursor が使う非公開 CGS API(カーソル非表示の安定化)
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(
        cid: i32, target_cid: i32, key: CFStringRef, value: *const core::ffi::c_void,
    ) -> i32;
    fn CFMachPortCreateRunLoopSource(alloc: CFAllocatorRef, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
    fn CFRunLoopGetMain() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: CFStringRef;
}

// ---------- ObjC ランタイム直宣言(NSPasteboard 操作) ----------
// NSPasteboard は AppKit のクラスのため、リンクしてクラス登録を発生させる必要がある
#[link(name = "AppKit", kind = "framework")]
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    fn objc_getClass(name: *const core::ffi::c_char) -> *mut core::ffi::c_void;
    fn sel_registerName(name: *const core::ffi::c_char) -> *mut core::ffi::c_void;
    fn objc_msgSend(
        receiver: *mut core::ffi::c_void,
        sel: *mut core::ffi::c_void,
        ...
    ) -> *mut core::ffi::c_void;
    /// ファイル URL のみを読む指定キー(readObjectsForClasses:options: 用)
    static NSPasteboardURLReadingFileURLsOnlyKey: *mut core::ffi::c_void;
}

/// 最後に Windows から受信して書き込んだテキスト(エコーバック送信防止)
static LAST_RECV_CLIP: Mutex<Option<String>> = Mutex::new(None);
const CLIP_MAX_BYTES: usize = 1024 * 1024; // 1MB(Win側と同じ上限)

type ID = *mut core::ffi::c_void;
type SEL = *mut core::ffi::c_void;

// objc_msgSend は可変引数宣言のまま呼ぶと引数の渡りが壊れる(SIGSEGV実績あり)ため、
// 呼び出しシグネチャごとに transmute した固定シグネチャで呼ぶ(rust-objc 界の定番方式)
unsafe fn msg0(target: ID, sel: SEL) -> ID {
    let f: unsafe extern "C" fn(ID, SEL) -> ID = std::mem::transmute(objc_msgSend as usize);
    f(target, sel)
}
unsafe fn msg1_id(target: ID, sel: SEL, a: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID = std::mem::transmute(objc_msgSend as usize);
    f(target, sel, a)
}
unsafe fn msg1_cstr(target: ID, sel: SEL, p: *const core::ffi::c_char) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, *const core::ffi::c_char) -> ID =
        std::mem::transmute(objc_msgSend as usize);
    f(target, sel, p)
}
unsafe fn msg2_bool(target: ID, sel: SEL, a: ID, b: ID) -> u8 {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 =
        std::mem::transmute(objc_msgSend as usize);
    f(target, sel, a, b)
}
unsafe fn msg0_isize(target: ID, sel: SEL) -> isize {
    let f: unsafe extern "C" fn(ID, SEL) -> isize = std::mem::transmute(objc_msgSend as usize);
    f(target, sel)
}
unsafe fn msg0_cstr(target: ID, sel: SEL) -> *const core::ffi::c_char {
    let f: unsafe extern "C" fn(ID, SEL) -> *const core::ffi::c_char =
        std::mem::transmute(objc_msgSend as usize);
    f(target, sel)
}

unsafe fn nsstring(s: &str) -> ID {
    let mut buf = s.as_bytes().to_vec();
    buf.push(0);
    msg1_cstr(
        objc_getClass(c"NSString".as_ptr()),
        sel_registerName(c"stringWithUTF8String:".as_ptr()),
        buf.as_ptr() as *const core::ffi::c_char,
    )
}

unsafe fn general_pasteboard() -> ID {
    msg0(
        objc_getClass(c"NSPasteboard".as_ptr()),
        sel_registerName(c"generalPasteboard".as_ptr()),
    )
}

/// NSPasteboard へテキストを書き込む(Windows→Mac 受信時)
unsafe fn mac_set_clipboard(text: &str) -> bool {
    let pb = general_pasteboard();
    if pb.is_null() {
        eprintln!("[clip] set: pasteboard=null");
        return false;
    }
    msg0(pb, sel_registerName(c"clearContents".as_ptr()));
    let s = nsstring(text);
    let uti = nsstring("public.utf8-plain-text");
    let ok = msg2_bool(pb, sel_registerName(c"setString:forType:".as_ptr()), s, uti);
    if ok == 0 {
        eprintln!("[clip] set failed: str={} uti={}", !s.is_null(), !uti.is_null());
    }
    ok != 0
}

/// DIB(Windows 画像)に BMP ファイルヘッダを付与して BMP データへ変換する。
/// NSBitmapImageRep は BMP ファイル形式を受け付けるため
fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    if dib.len() < 40 {
        return Vec::new();
    }
    // BITMAPINFOHEADER: biSize は先頭4バイト(旧実装は誤って biWidth を読んでいた)
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let bpp = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize;
    let palette = if clr_used > 0 {
        clr_used * 4
    } else if bpp == 8 {
        1024
    } else {
        0
    };
    let off = 14 + header_size + palette;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(off as u32).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// BMP 画像を Mac のクリップボードへ TIFF として書き込む(Windows→Mac 画像同期)
unsafe fn mac_set_clipboard_image_bmp(bmp: &[u8]) -> bool {
    let pb = general_pasteboard();
    if pb.is_null() || bmp.is_empty() {
        return false;
    }
    // NSData dataWithBytes:length:
    let data = {
        let f: unsafe extern "C" fn(ID, SEL, *const u8, usize) -> ID =
            std::mem::transmute(objc_msgSend as usize);
        f(
            objc_getClass(c"NSData".as_ptr()),
            sel_registerName(c"dataWithBytes:length:".as_ptr()),
            bmp.as_ptr(),
            bmp.len(),
        )
    };
    if data.is_null() {
        eprintln!("[clip] image FAILED at NSData");
        return false;
    }
    // NSBitmapImageRep imageRepWithData:
    let rep = {
        let f: unsafe extern "C" fn(ID, SEL, ID) -> ID = std::mem::transmute(objc_msgSend as usize);
        f(
            objc_getClass(c"NSBitmapImageRep".as_ptr()),
            sel_registerName(c"imageRepWithData:".as_ptr()),
            data,
        )
    };
    if rep.is_null() {
        eprintln!("[clip] image FAILED at imageRepWithData (BMP 不整合の可能性)");
        return false;
    }
    // [rep TIFFRepresentation]
    let tiff = msg0(rep, sel_registerName(c"TIFFRepresentation".as_ptr()));
    if tiff.is_null() {
        eprintln!("[clip] image FAILED at TIFFRepresentation");
        return false;
    }
    msg0(pb, sel_registerName(c"clearContents".as_ptr()));
    let uti = nsstring("public.tiff");
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 = std::mem::transmute(objc_msgSend as usize);
    let ok = f(pb, sel_registerName(c"setData:forType:".as_ptr()), tiff, uti);
    if ok == 0 {
        eprintln!(
            "[clip] image FAILED at setData (data={} rep={} tiff={} bmp={}B)",
            !data.is_null(),
            !rep.is_null(),
            !tiff.is_null(),
            bmp.len()
        );
    }
    ok != 0
}

/// NSPasteboard からテキストを読む(Mac→Windows 送信時)
unsafe fn mac_get_clipboard() -> Option<String> {
    let pb = general_pasteboard();
    if pb.is_null() {
        return None;
    }
    let uti = nsstring("public.utf8-plain-text");
    let s = msg1_id(pb, sel_registerName(c"stringForType:".as_ptr()), uti);
    if s.is_null() {
        return None;
    }
    let utf8 = msg0_cstr(s, sel_registerName(c"UTF8String".as_ptr()));
    if utf8.is_null() {
        return None;
    }
    Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
}

fn clipboard_change_count() -> isize {
    unsafe { msg0_isize(general_pasteboard(), sel_registerName(c"changeCount".as_ptr())) }
}

/// NSPasteboard にファイル参照(Finder の ⌘C 等)があるか調べ、パス群を返す。
/// readObjectsForClasses:options:(NSURL + FileURLsOnly=YES)で読むことで
/// NSFilenamesPboardType / public.file-url / alias のどの載せ方でも拾う
unsafe fn mac_clipboard_files() -> Option<Vec<std::path::PathBuf>> {
    let pb = general_pasteboard();
    if pb.is_null() {
        return None;
    }
    let url_cls = objc_getClass(c"NSURL".as_ptr());
    if url_cls.is_null() {
        return None;
    }
    let classes = {
        let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(objc_msgSend as usize);
        f(
            objc_getClass(c"NSArray".as_ptr()),
            sel_registerName(c"arrayWithObject:".as_ptr()),
            url_cls,
        )
    };
    let options = {
        let yes = {
            let f: unsafe extern "C" fn(ID, SEL, u8) -> ID =
                std::mem::transmute(objc_msgSend as usize);
            f(
                objc_getClass(c"NSNumber".as_ptr()),
                sel_registerName(c"numberWithBool:".as_ptr()),
                1,
            )
        };
        let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(objc_msgSend as usize);
        f(
            objc_getClass(c"NSDictionary".as_ptr()),
            sel_registerName(c"dictionaryWithObject:forKey:".as_ptr()),
            yes,
            NSPasteboardURLReadingFileURLsOnlyKey,
        )
    };
    let urls = {
        let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(objc_msgSend as usize);
        f(
            pb,
            sel_registerName(c"readObjectsForClasses:options:".as_ptr()),
            classes,
            options,
        )
    };
    if urls.is_null() {
        return None;
    }
    let n = msg0_isize(urls, sel_registerName(c"count".as_ptr()));
    if n <= 0 {
        return None;
    }
    let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
        std::mem::transmute(objc_msgSend as usize);
    let mut out = Vec::new();
    for i in 0..n.min(64) {
        let url = at(urls, sel_registerName(c"objectAtIndex:".as_ptr()), i as usize);
        if url.is_null() {
            continue;
        }
        let path = msg0(url, sel_registerName(c"path".as_ptr()));
        if path.is_null() {
            continue;
        }
        let utf8 = msg0_cstr(path, sel_registerName(c"UTF8String".as_ptr()));
        if utf8.is_null() {
            continue;
        }
        let s = std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned();
        if !s.is_empty() {
            out.push(std::path::PathBuf::from(s));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// NSPasteboard へファイル参照を書き込む(Finder の ⌘C 相当)。
/// Windows からのファイル受信完了時に呼ぶ。writeObjects: が file URL を
/// 載せるため、Finder への ⌘V がそのまま動く
unsafe fn mac_clipboard_write_files(paths: &[std::path::PathBuf]) -> bool {
    let pb = general_pasteboard();
    if pb.is_null() || paths.is_empty() {
        return false;
    }
    let url_cls = objc_getClass(c"NSURL".as_ptr());
    if url_cls.is_null() {
        return false;
    }
    let make_url: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(objc_msgSend as usize);
    let mut urls = Vec::new();
    for p in paths {
        let s = nsstring(&p.to_string_lossy());
        if s.is_null() {
            continue;
        }
        let url = make_url(url_cls, sel_registerName(c"fileURLWithPath:".as_ptr()), s);
        if !url.is_null() {
            urls.push(url);
        }
    }
    if urls.is_empty() {
        return false;
    }
    msg0(pb, sel_registerName(c"clearContents".as_ptr()));
    let make_arr: unsafe extern "C" fn(ID, SEL, *const ID, usize) -> ID =
        std::mem::transmute(objc_msgSend as usize);
    let arr = make_arr(
        objc_getClass(c"NSArray".as_ptr()),
        sel_registerName(c"arrayWithObjects:count:".as_ptr()),
        urls.as_ptr(),
        urls.len(),
    );
    let write: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
        std::mem::transmute(objc_msgSend as usize);
    write(pb, sel_registerName(c"writeObjects:".as_ptr()), arr) != 0
}

/// ファイル群の指紋(パス+合計サイズ)。同一コピーの再検出・エコーバック判定に使う
fn mac_files_key(paths: &[std::path::PathBuf]) -> String {
    let sizes: u64 = paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
        .sum();
    format!(
        "{}|{sizes}",
        paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\u{1}")
    )
}

/// Windows からのファイル受信を ~/Downloads/Tsunagu へ新規作成する。
/// 同名との衝突は "名前 (n).拡張子" として回避。1ファイル 200MB 上限
fn mac_file_begin(name: &str, size: u64) -> Option<(std::fs::File, std::path::PathBuf)> {
    const MAX_FILE: u64 = 200 * 1024 * 1024;
    if size == 0 || size > MAX_FILE {
        eprintln!("[file] win->mac skip(name={name:?} size={size})");
        return None;
    }
    let bad = name.is_empty()
        || name.len() > 255
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || name.starts_with('.');
    let base = if bad { "file".to_string() } else { name.to_string() };
    let dir = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .map(|h| h.join("Downloads/Tsunagu"))
        .filter(|d| std::fs::create_dir_all(d).is_ok())?;
    let p = std::path::Path::new(&base);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 0..1000u32 {
        let cand = if i == 0 {
            dir.join(&base)
        } else {
            dir.join(format!("{stem} ({i}){ext}"))
        };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(f) => return Some((f, cand)),
            Err(_) => continue,
        }
    }
    None
}

/// ファイル群を Windows へ送る(FileBegin → FileChunk… → FileEnd)。
/// GUI メニュー(NSOpenPanel)と Finder の ⌘C 検出の両方から呼ぶ。別スレッド実行
pub fn send_files_to_win(paths: Vec<std::path::PathBuf>) {
    if FILE_TX_BUSY.swap(true, Ordering::Relaxed) {
        eprintln!("[file] 送信中のため要求を無視しました");
        return;
    }
    std::thread::spawn(move || {
        const MAX_TOTAL: u64 = 200 * 1024 * 1024; // Windows 側の受信上限と同じ
        const CHUNK: usize = 3 * 1024 * 1024;
        let mut total = 0u64;
        for p in &paths {
            if let Ok(m) = std::fs::metadata(p) {
                if m.is_file() {
                    total += m.len();
                }
            }
        }
        if total == 0 || total > MAX_TOTAL {
            eprintln!("[file] 送信拒否: {} 件 / 合計 {total} bytes", paths.len());
            notify(
                "tsunagu",
                &format!("ファイルを送信できません(合計 {}MB。上限 200MB)", total / 1024 / 1024),
            );
            FILE_TX_BUSY.store(false, Ordering::Relaxed);
            return;
        }
        if !CONNECTED.load(Ordering::Relaxed) {
            eprintln!("[file] 未接続のため送信しません");
            notify("tsunagu", "Windows 未接続のためファイルを送信できません");
            FILE_TX_BUSY.store(false, Ordering::Relaxed);
            return;
        }
        eprintln!("[file] 送信開始: {} 件 / 合計 {}KB", paths.len(), total / 1024);
        use std::io::Read;
        let mut buf = vec![0u8; CHUNK];
        for p in &paths {
            let Ok(meta) = std::fs::metadata(p) else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let size = meta.len();
            let Ok(mut f) = std::fs::File::open(p) else {
                eprintln!("[file] open 失敗: {}", p.display());
                continue;
            };
            send_msg(&Msg::FileBegin { name: name.clone(), size });
            let mut remain = size;
            while remain > 0 {
                let want = remain.min(CHUNK as u64) as usize;
                match f.read(&mut buf[..want]) {
                    Ok(0) => break,
                    Ok(n) => {
                        send_msg(&Msg::FileChunk { data: tsunagu_common::b64::encode(&buf[..n]) });
                        remain -= n as u64;
                    }
                    Err(_) => break,
                }
            }
            send_msg(&Msg::FileEnd);
            eprintln!("[file] 送信: {name} ({size} bytes)");
        }
        send_msg(&Msg::FileBatchEnd);
        eprintln!("[file] 送信完了({} 件)。Windows 側は Ctrl+V で貼り付けられます", paths.len());
        notify("tsunagu", &format!("{} 件のファイルを Windows へ送信しました", paths.len()));
        FILE_TX_BUSY.store(false, Ordering::Relaxed);
    });
}

/// macOS の通知センターへ表示(接続/切断のユーザー可視化)。
/// osascript 経由で追加権限なしで出せる。失敗しても本体には影響しない。
/// osascript 起動に数百msかかるため別スレッドで発火し、accept/受信スレッドを
/// ブロックしない(レビュー Wave1 A-M9/D-F3)
fn notify(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        let out = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"{}\"",
                    body.replace('"', "'"),
                    title.replace('"', "'")
                ),
            ])
            .output();
        let _ = out;
    });
}

// ---------- 共有状態 ----------
static WIN_MODE: AtomicBool = AtomicBool::new(false);
static CONNECTED: AtomicBool = AtomicBool::new(false);
/// ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)。トグル時に Windows へ Cfg で同期
static CMD_ALT: AtomicBool = AtomicBool::new(false);
/// 接続中の Windows スピーカーミュート(true=Mac のみ発音。既定 ON)。
/// トグル時に Windows へ Cfg で同期(TSUNAGU_MUTE_SPK=0 で初期無効化)
static SPK_MUTE: AtomicBool = AtomicBool::new(true);
/// スクロール方向の反転(既定 false=Windows 標準の指の動きに合わせてある)
/// スクロール方向の手動上書き(true=Windows 標準。false 既定=Mac の設定に合わせる)
static SCROLL_FLIP: AtomicBool = AtomicBool::new(false);
/// macOS の「自然スクロール」設定(起動時に取得。true=トラックパッドのコンテンツ追従)
static NATURAL_SCROLL: AtomicBool = AtomicBool::new(true);

/// macOS のスクロール方向設定を読む(失敗時は出荷既定の自然スクロール扱い)。
/// ユーザーが Mac で使っている向きへ Windows 側も自動で合わせるために使う
fn detect_natural_scroll() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "com.apple.swipescrolledirection"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() != "0")
        .unwrap_or(true)
}
/// Windows との RTT(ms)。ping/pong 往復で測定(メニュー状態行の表示用)
static RTT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// ファイル送信中(多重送信の抑制)
static FILE_TX_BUSY: AtomicBool = AtomicBool::new(false);
/// 直近に送ったクリップボードファイルの指紋(同じ ⌘C の再送防止)
static LAST_SENT_FILES: Mutex<String> = Mutex::new(String::new());
static TX: OnceLock<Sender<String>> = OnceLock::new();
static STREAM_SLOT: OnceLock<Arc<Mutex<Option<TcpStream>>>> = OnceLock::new();
static SCREEN_W: OnceLock<f64> = OnceLock::new();
static SCREEN_H: OnceLock<f64> = OnceLock::new();
static TAP_PORT: OnceLock<usize> = OnceLock::new();
static DIAG_MOVE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_KEY_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_SEND_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DIAG_WARP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_MODE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_SCROLL_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_ABS_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIAG_SELF_HEAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TAP_REARM_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// ウォッチドッグ用: 最終タップ受信時刻と最終 abs 送信時刻(ms)。
/// 「ユーザーが操作中なのに Windows へ届いていない」状態を検知する
static LAST_EVENT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_ABS_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 境界ダブルタップ切替(Deskflow switchDoubleTap 相当)。
/// TSUNAGU_EDGE_TAPS(既定2)= 境界に連続で2回当てた時だけ切替。1回の到達では
/// 切替しないため、境界付近での日常作業と Windows への移動が分離される。
/// GUI から実行中に切り替え可能なため AtomicU32(初期値は起動時に store)
static EDGE_TAPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(2);
static EDGE_AT_EDGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static EDGE_LAST_HIT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
// ---- Deskflow 標準オプション(画面位置 links / switchDelay / switchDoubleTap /
// ---- switchCorners / clipboardSharing)----
/// Windows 画面の位置(0=右/1=左/2=上/3=下)。Deskflow links 相当
pub static SIDE: AtomicU8 = AtomicU8::new(0);
/// 端に N ms 滞ってから切替(switchDelay。0=無効でダブルタップ/即時判定)
pub static SWITCH_DELAY_MS: AtomicU64 = AtomicU64::new(0);
/// ダブルタップの判定窓 ms(switchDoubleTap)
pub static DOUBLE_TAP_MS: AtomicU64 = AtomicU64::new(700);
/// 四隅の切替禁止サイズ px(switchCornerSize。0=無効)
pub static CORNER_PX: AtomicU64 = AtomicU64::new(0);
/// クリップボード共有(clipboardSharing)
pub static CLIP_SHARE: AtomicBool = AtomicBool::new(true);
/// スクロール互換モード(TSUNAGU_SCROLL_COMPAT=1 / 設定窓): 120 未満の
/// ホイール量を無視する古い設計のアプリ向けに 1 ノッチ(120)単位で送る。
/// 既定 OFF=高解像度(0.05 ノッチ刻み)で滑らかに
pub static SCROLL_COMPAT: AtomicBool = AtomicBool::new(false);
/// 端到達の開始時刻(switchDelay の滞在計測用)
static EDGE_STAY_SINCE_MS: AtomicU64 = AtomicU64::new(0);
/// 横スワイプ(戻る/進む)の状態: (累積 dx, 最終イベント時刻, 最終発火時刻)
static SWIPE_ACC: std::sync::Mutex<(f64, u64, u64)> = std::sync::Mutex::new((0.0, 0, 0));
/// ドラッグ中切替(TSUNAGU_DRAG_SWITCH=1): 押したまま境界を越えられる
pub static DRAG_SWITCH: AtomicBool = AtomicBool::new(false);
/// Ctrl+クリック=右クリック翻訳(TSUNAGU_CTRL_CLICK=1 で有効。既定 OFF:
/// 2本指クリックで右クリックできるため不要であり、修飾フラグの混入で
/// 意図しない右クリックメニューが出る事故の温床になった)
pub static CTRL_CLICK: AtomicBool = AtomicBool::new(false);
/// 現在押下中のマウスボタン(0=左,1=右,2=中)。切替時の持ち込み再送に使う
static BTN_DOWN: [AtomicBool; 3] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];

/// 現在の SIDE(0=右/1=左/2=上/3=下)を文字列表現で
pub fn side_name() -> &'static str {
    match SIDE.load(Ordering::Relaxed) {
        1 => "左",
        2 => "上",
        3 => "下",
        _ => "右",
    }
}

/// SIDE を設定し、Windows へ Cfg で同期する
pub fn set_side(v: u8) {
    SIDE.store(v.min(3), Ordering::Relaxed);
    send_msg(&Msg::Cfg {
        cmd_alt: CMD_ALT.load(Ordering::Relaxed),
        spk_mute: SPK_MUTE.load(Ordering::Relaxed),
        side: v.min(3),
    });
    eprintln!("[cfg] Windows の位置 -> {}", side_name());
}
static LAST_PONG_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 復帰直後は右端判定を一定時間無効化する(再突入チャタリング防止)
static EDGE_GUARD_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 自前管理のカーソル位置(delta 積算)。タップ内での毎イベント CGEventCreate は
/// 負荷としてカクつきに効くため、積算+間欠同期(Deskflow の m_xCursor 方式)にする。
static CUR_POS: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// 接続相手(Windows)の画面サイズ(px)。hello で受信しスケール自動算出に使う
static WIN_SCREEN: Mutex<(f64, f64)> = Mutex::new((1920.0, 1080.0));
/// WIN モード中の Windows 仮想カーソル位置(px)。絶対位置送信モードで使う
static WIN_CUR: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// 前回送信した絶対位置(量子化変化検出用)
static LAST_ABS_SENT: Mutex<(f64, f64)> = Mutex::new((-1.0, -1.0));
/// 前回 Windows モードを出た位置(0..1)。次回の切替はそこへ戻る(Deskflow 標準の体験)
static LAST_WIN_POS: Mutex<(f64, f64)> = Mutex::new((0.05, 0.5));
/// 絶対位置送信モード(既定ON。TSUNAGU_MOUSE_MODE=rel で旧・相対移動に戻す)
static MOUSE_ABS_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// 切替方式: false=境界+ホットキー(既定)/ true=ホットキー(F13)のみで切替、
/// 切替後は境界を超えても戻らないロック状態になる(TSUNAGU_SWITCH_MODE=hotkey)
static HOTKEY_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CUR_SYNC_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// ライブカーソル位置を取得(CFRelease まで面倒を見る)
unsafe fn live_cursor() -> Option<CGPoint> {
    let probe = CGEventCreate(std::ptr::null_mut());
    if probe.is_null() {
        return None;
    }
    let loc = CGEventGetLocation(probe);
    CFRelease(probe);
    Some(loc)
}
/// WIN モード中のカーソル固定位置(右端内側, y)。漏れ移動を warp で巻き戻す基準。
static LOCK_POS: Mutex<Option<(f64, f64)>> = Mutex::new(None);
/// スクロール変換の累積残高(dx, dy)[ノッチ]。除数を大きくしても細かい動きを失わないための仕組み。
static SCROLL_ACC: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// スクロール速度除数(ピクセル→ノッチ変換。大きいほど遅い)。設定ウィンドウの
/// スライダーからも可変(TSUNAGU_SCROLL_DIV は初期値)。f64 を AtomicU64 ビットで保持
static SCROLL_DIV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(60.0f64.to_bits());

/// 現在のスクロール除数を f64 で読む
pub fn scroll_div() -> f64 {
    f64::from_bits(SCROLL_DIV.load(Ordering::Relaxed))
}

/// スクロール除数を設定(20..240 にクランプ)。設定ウィンドウから呼ばれる
pub fn set_scroll_div(v: f64) {
    let clamped = v.clamp(20.0, 240.0);
    SCROLL_DIV.store(clamped.to_bits(), Ordering::Relaxed);
}
/// マウス移動の倍率(Mac の加速済み delta に Windows の加速が重なる調整用)。
/// カーソル速度倍率(abs 座標系)。設定ウィンドウのスライダーから可変。
/// f64 を AtomicU64 ビットで保持(スクロール除数と同じ方式)
static MOUSE_SCALE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1.0f64.to_bits());

/// 現在のカーソル速度倍率
pub fn mouse_scale() -> f64 {
    f64::from_bits(MOUSE_SCALE.load(Ordering::Relaxed))
}

/// カーソル速度倍率を設定(0.2..3.0 にクランプ)
pub fn set_mouse_scale(v: f64) {
    MOUSE_SCALE.store(v.clamp(0.2, 3.0).to_bits(), Ordering::Relaxed);
}
/// 境界切替の判定閾値(px、境界からの距離)。設定窓スライダーで可変。
/// f64 を AtomicU64 ビットで保持
static EDGE_PX: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(2.0f64.to_bits());

/// 現在の境界判定閾値(px)
pub fn edge_px() -> f64 {
    f64::from_bits(EDGE_PX.load(Ordering::Relaxed))
}

/// 境界判定閾値を設定(0..50px にクランプ)。大きいほど切替が敏感になる
pub fn set_edge_px(v: f64) {
    EDGE_PX.store(v.clamp(0.0, 50.0).to_bits(), Ordering::Relaxed);
}
/// 全ディスプレイ領域(union)の右端。右にサブモニターがある環境では
/// メイン画面右端で切替すると Mac 内のモニター間移動ができなくなるため、
/// 仮想画面全体の右端で判定する
static UNION_MAX_X: OnceLock<f64> = OnceLock::new();
/// 出口(union 右端を持つディスプレイ)のグローバル y 範囲。
/// 切替・復帰の高さ対応を MacBook 基準ではなく出口モニター基準で正確に行う
static EDGE_DISP_Y: OnceLock<(f64, f64)> = OnceLock::new();
/// カーソル非表示状態の管理(hide/show の対称性を保証し、復帰時に必ず表示する)
static CURSOR_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Windows モード開始: カーソル移動とマウス入力の関連を切断し、
/// Mac カーソルを画面右端の固定位置へ置く(Synergy/Deskflow 方式)
/// Deskflow hideCursor/showCursor 内の「SetsCursorInBackground」プロパティ設定。
/// バックグラウンド接続でもカーソル表示状態を維持し、非表示がランダムに解除されるのを防ぐ
unsafe fn set_cursor_in_background() {
    let key = CFStringCreateWithCString(
        std::ptr::null_mut(),
        c"SetsCursorInBackground".as_ptr(),
        0, // kCFStringEncodingMacRoman
    );
    if !key.is_null() {
        let cid = _CGSDefaultConnection();
        CGSSetConnectionProperty(cid, cid, key, kCFBooleanTrue);
        CFRelease(key);
    }
}

fn enter_win_mode_cursor_lock() {
    // 持ち越していたスクロール残量を切替時に捨てる(切替直後の意図しないスクロール防止)
    *SCROLL_ACC.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
    // Deskflow leave() 相当: hideCursor(プロパティ付き) → suppression間隔最小化 → 関連切断 → warp固定
    unsafe {
        set_cursor_in_background();
        let d = CGMainDisplayID();
        // hide が多重に積もると show が追いつかずカーソルが消えたままになるため
        // フラグで 1 回だけ隠す
        if !CURSOR_HIDDEN.swap(true, Ordering::Relaxed) {
            CGDisplayHideCursor(d);
        }
        CGSetLocalEventsSuppressionInterval(0.0001);
        CGAssociateMouseAndMouseCursorPosition(false);
        // 関連切断は非同期で効き始めるため、切替直後の漏れ移動が数ピクセル出る。
        // 固定位置を右端内側に warp しておき、以降の漏れは都度巻き戻す(境界の同時移動対策)
        let mut lock_y = 400.0;
        let ev = CGEventCreate(std::ptr::null_mut());
        if !ev.is_null() {
            let loc = CGEventGetLocation(ev);
            lock_y = loc.y;
            CFRelease(ev);
        }
        // SIDE の境界端に固定(SCREEN_W はメイン画面幅なので、右サブモニターが
        // ある環境で右端固定すると隠れカーソルが MacBook 側へ飛んでしまう)
        let side = SIDE.load(Ordering::Relaxed);
        let main_h = SCREEN_H.get().copied().unwrap_or(1329.0);
        let edge_x = UNION_MAX_X.get().copied().unwrap_or_else(|| {
            SCREEN_W.get().copied().unwrap_or(2056.0)
        });
        let ev2 = CGEventCreate(std::ptr::null_mut());
        let mut lock_x2 = 0.0f64;
        if !ev2.is_null() {
            let loc2 = CGEventGetLocation(ev2);
            lock_x2 = loc2.x;
            CFRelease(ev2);
        }
        let (lock_x, lock_y) = match side {
            1 => (2.0, lock_y),                     // 左端
            2 => (lock_x2, 2.0),                    // 上端
            3 => (lock_x2, main_h - 2.0),           // 下端
            _ => (edge_x - 2.0, lock_y),            // 右端(既定)
        };
        CGWarpMouseCursorPosition(CGPoint { x: lock_x, y: lock_y });
        // タップが握った位置を CUR_POS にも反映(積算の起点を正しくする)
        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (lock_x, lock_y);
        *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) = Some((lock_x, lock_y));
    }
}

/// Windows モード終了: 関連を復元し、右端の内側へカーソルを戻す。
/// ny は Windows 側カーソルの高さ(0..1)。与えられた場合は同じ高さへ戻す(境界連続性)。
fn leave_win_mode_cursor_unlock(ny: Option<f64>) {
    // Deskflow enter() 相当: 関連復元 → showCursor(プロパティ付き) → suppression解除 → 位置復帰
    unsafe {
        // 次回の切替で同じ場所へ戻れるよう、Windows 画面内の現在地を記憶する
        {
            let wc = *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
            let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
            if ww > 0.0 && wh > 0.0 && wc.0 >= 0.0 {
                *LAST_WIN_POS.lock().unwrap_or_else(|e| e.into_inner()) =
                    ((wc.0 / ww).clamp(0.05, 0.95), (wc.1 / wh).clamp(0.05, 0.95));
            }
        }
        // 時間ガードを 400ms に増強し、復帰直後の再突入を防ぐ(距離ガードの代替)
        EDGE_GUARD_UNTIL_MS.store(now_ms() + 400, Ordering::Relaxed);
        // 注意: ここでライブ位置を同期すると「復帰ワープ前」の境界位置
        // (2309)を掴んでしまい、ガード明けに即再突入する原因になる。
        // CUR_POS はこの後のワープ先で上書きするため、ここでは同期しない
        
        *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        CGAssociateMouseAndMouseCursorPosition(true);
        set_cursor_in_background();
        let d = CGMainDisplayID();
        // カーソルの再表示漏れ(透明のまま戻るバグ)を防ぐため、フラグが立って
        // いるときは show を複数回呼んで確実に表示する(呼び過ぎても無害)
        if CURSOR_HIDDEN.swap(false, Ordering::Relaxed) {
            for _ in 0..3 {
                CGDisplayShowCursor(d);
            }
        }
        CGSetLocalEventsSuppressionInterval(0.0); // Deskflow setZeroSuppressionInterval
        // 復帰位置: ダブルタップ切替が有効な間は出た境界のすぐ内側(60px)へ戻す。
        // 1回の到達では切替しなくなったため境界近くでも再突入せず、境界を
        // 跨いで戻ってくる連続的な体験になる。
        // 1回切替(TSUNAGU_EDGE_TAPS=1)では従来どおり MacBook 側へ退けて
        // 誤再突入を防ぐ
        // SIDE(Windows の位置)に応じた復帰座標: 出てきた境界のすぐ内側へ。
        // ny は Windows 側カーソルの「境界に沿った比率」(side 0/1=縦、2/3=横)
        let main_w = SCREEN_W.get().copied().unwrap_or(2056.0);
        let main_h = SCREEN_H.get().copied().unwrap_or(1329.0);
        let edge_x = UNION_MAX_X.get().copied().unwrap_or(main_w);
        let taps = EDGE_TAPS.load(Ordering::Relaxed);
        let side = SIDE.load(Ordering::Relaxed);
        let inset: f64 = if taps >= 2 { 60.0 } else { 150.0 };
        let (ymin, ymax) = EDGE_DISP_Y
            .get()
            .copied()
            .unwrap_or((0.0, main_h));
        let along_y = |r: Option<f64>| -> f64 {
            match r {
                Some(n) => (ymin + n.clamp(0.0, 1.0) * (ymax - ymin)).clamp(ymin + 20.0, (ymax - 20.0).max(ymin + 20.0)),
                None => (ymin + ymax) / 2.0,
            }
        };
        let along_x = |r: Option<f64>| -> f64 {
            match r {
                Some(n) => (n.clamp(0.0, 1.0) * main_w).clamp(20.0, main_w - 20.0),
                None => main_w / 2.0,
            }
        };
        let (x, y) = match side {
            1 => (inset.max(20.0), along_y(ny)), // 左端から戻る
            2 => (along_x(ny), inset.max(20.0)), // 上端から戻る
            3 => (along_x(ny), (main_h - inset).min(main_h - 20.0)), // 下端から戻る
            _ => {
                // 右端(既定): 1回切替時は MacBook 側へ十分退ける
                let x = if taps >= 2 {
                    edge_x - inset
                } else {
                    (main_w - 100.0).min(edge_x - inset)
                };
                (x, along_y(ny))
            }
        };
        CGWarpMouseCursorPosition(CGPoint { x, y });
        // ワープ先を CUR_POS へ反映(同期をワープ前に取ると境界値が残り
        // 復帰直後に必ず再突入して戻れなくなる)
        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (x, y);
        eprintln!("[return] -> mac ({x:.0},{y:.0})");
    }
}

fn send_msg(msg: &Msg) {
    DIAG_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
    if let Some(tx) = TX.get() {
        let line = encode(msg);
        let _ = tx.send(line);
    }
}

/// ホットキー/メニューバーGUI からの手動トグル(F13 とメニューの共通経路)。
/// 切替状態遷移はここを含む既存6経路のまま(一元化はモジュール分割 Phase5 で実施)
fn do_toggle(reason: &str) {
    if !CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    let next = !WIN_MODE.load(Ordering::Relaxed);
    WIN_MODE.store(next, Ordering::Relaxed);
    if next {
        DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    eprintln!("[mode] {} ({reason})", if next { "WINDOWS" } else { "MAC" });
    if next {
        enter_win_mode_cursor_lock();
    } else {
        // 戻り先の高さは Windows 側カーソルの現在高さに合わせる
        let ny = {
            let wc = *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
            let (_ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
            if wh > 0.0 { (wc.1 / wh).clamp(0.0, 1.0) } else { 0.5 }
        };
        leave_win_mode_cursor_unlock(Some(ny));
    }
}

// ---------- イベントタップコールバック ----------
unsafe extern "C" fn tap_callback(
    _proxy: *mut core::ffi::c_void,
    event_type: u32,
    event: CGEventRef,
    _user_info: *mut core::ffi::c_void,
) -> CGEventRef {
    // Deskflow 同様、タイムアウトで無効化されたら再び有効化する(でないと抑制が静かに止まる)
    if event_type == 0xFFFFFFFE || event_type == 0xFFFFFFFD {
        if let Some(&tap) = TAP_PORT.get() {
            CGEventTapEnable(tap as CFMachPortRef, true);
        }
        return std::ptr::null_mut();
    }
    let win_mode = WIN_MODE.load(Ordering::Relaxed);
    let connected = CONNECTED.load(Ordering::Relaxed);

    // ホットキー(F13 既定 / TSUNAGU_HOTKEY_KC)= 手動トグル(常に有効、握る)。
    // 修飾キー(右Cmd=54 等)は flagsChanged として届くため、flags の該当ビットで
    // 押下/解放を判別し、押下側でのみトグルする(up・解放側は握るだけ)。
    // 旧実装は KEY_DOWN しかトグルせず(=修飾キー指定が機能しない)、かつ up 把握の
    // 分岐が外側条件により到達不能なデッドコードだった(レビュー Wave1-X5)
    if matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED) {
        let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
        // 音量キー(F10/11/12 相当: 74=ミュート/73=下/72=上)は Mac の音量を変えず
        // Windows 側の音量として転送する(実体は Vol メッセージ+イベント握りつぶし)。
        // FN フラグ付き(=本体の音量キー操作)のみ。F11 全画面など F キーとしての
        // 使用は Windows へ素通しさせ、誤転送(意図しない音量变化)を防ぐ
        if event_type == EVT_KEY_DOWN
            && (72..=74).contains(&kc)
            && CGEventGetFlags(event) & FLAG_FN != 0
            && win_mode
        {
            let op = match kc {
                72 => 0u8, // VolumeUp
                73 => 1,   // VolumeDown
                _ => 2,    // Mute
            };
            send_msg(&Msg::Vol { op });
            return std::ptr::null_mut(); // Mac 側の音量変更を抑制
        }
        if kc == hotkey_kc() {
            let pressed = match event_type {
                EVT_KEY_DOWN => true,
                EVT_KEY_UP => false,
                _ => {
                    let flags = CGEventGetFlags(event);
                    match kc {
                        54 | 55 => flags & FLAG_CMD != 0,
                        58 | 61 => flags & FLAG_OPT != 0,
                        59 | 62 => flags & FLAG_CTRL != 0,
                        56 | 60 => flags & FLAG_SHIFT != 0,
                        _ => true,
                    }
                }
            };
            if pressed && connected {
                do_toggle("hotkey");
            }
            return std::ptr::null_mut(); // トグル専用キーのため down/up 両方握る
        }
    }

    if matches!(event_type, EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED) {
        DIAG_MOVE_COUNT.fetch_add(1, Ordering::Relaxed);
        LAST_EVENT_MS.store(now_ms(), Ordering::Relaxed);
    }
    if matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED) {
        DIAG_KEY_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    // マウスボタンの押下状態はモードに関係なく追跡する(ドラッグ中切替の
    // 持ち込み判定に使う。Mac モードの素通し経路でも更新が必要)
    match event_type {
        EVT_LEFT_DOWN => BTN_DOWN[0].store(true, Ordering::Relaxed),
        EVT_LEFT_UP => BTN_DOWN[0].store(false, Ordering::Relaxed),
        EVT_RIGHT_DOWN => BTN_DOWN[1].store(true, Ordering::Relaxed),
        EVT_RIGHT_UP => BTN_DOWN[1].store(false, Ordering::Relaxed),
        EVT_OTHER_DOWN => BTN_DOWN[2].store(true, Ordering::Relaxed),
        EVT_OTHER_UP => BTN_DOWN[2].store(false, Ordering::Relaxed),
        _ => {}
    }

    if !win_mode {
        // Mac モード: 右端到達で Windows モードへ。
        // Deskflow onMouseMove 準拠: イベント位置はキュー滞留で数フレーム遅れるため、
        // CGEventCreate(NULL) のライブカーソル位置で判定する(境界の応答性の鍵)
        let drag_ok = DRAG_SWITCH.load(Ordering::Relaxed);
        if (matches!(event_type, EVT_MOUSE_MOVED)
            || (drag_ok
                && matches!(event_type, EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED)))
            && connected
            && !HOTKEY_ONLY.load(Ordering::Relaxed) // hotkey モードでは境界切替しない(ロック)
            && now_ms() >= EDGE_GUARD_UNTIL_MS.load(Ordering::Relaxed)
        {
            if let Some(w) = SCREEN_W.get() {
                // delta 積算でカーソル位置を追跡(Deskflow の m_xCursor 方式)。
                // 32イベントに1回ライブ位置へ同期しドリフトを補正する
                let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
                let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
                let n = CUR_SYNC_N.fetch_add(1, Ordering::Relaxed);
                let mut pos = CUR_POS.lock().unwrap_or_else(|e| e.into_inner());
                pos.0 += dx;
                pos.1 += dy;
                if n % 16 == 0 {
                    if let Some(loc) = live_cursor() {
                        *pos = (loc.x, loc.y);
                    }
                }
                let (px, py) = *pos;
                drop(pos);
                let edge = edge_px();
                // 仮想画面全体の右端(右サブモニターがある環境ではそちらの右端)
                let main_w = w;
                let main_h = SCREEN_H.get().copied().unwrap_or(1329.0);
                let edge_x = UNION_MAX_X.get().copied().unwrap_or(*w);
                let side = SIDE.load(Ordering::Relaxed);
                // SIDE に応じた「切替境界までの距離」(小さいほど端に近い)。
                // Deskflow の links(left/right/up/down)相当。左/上は主画面原点(0)基点
                let gap = |x: f64, y: f64| -> f64 {
                    match side {
                        1 => x,            // 左端
                        2 => y,            // 上端
                        3 => main_h - y,   // 下端
                        _ => edge_x - x,   // 右端(既定)
                    }
                };
                // 境界から十分内側へ戻ったらヒット状態をリセット(次の到達を1回目として数える)
                if gap(px, py) > edge + 8.0 {
                    EDGE_AT_EDGE.store(false, Ordering::Relaxed);
                    EDGE_STAY_SINCE_MS.store(0, Ordering::Relaxed);
                }
                if gap(px, py) <= edge {
                    // switchCorners(+cornerSize): 四隅 N px 内では切替しない(誤爆防止)
                    let corner = CORNER_PX.load(Ordering::Relaxed) as f64;
                    if corner > 0.0
                        && (px < corner || px > edge_x - corner)
                        && (py < corner || py > main_h - corner)
                    {
                        return event;
                    }
                    // 二段階判定: 積算値が閾値を超えても、実カーソル(ライブ位置)が
                    // 境界付近でなければ発火しない。ドリフトが残っていても
                    // MacBook 中央などでの誤発火を構造的に防ぐ
                    let Some(loc) = live_cursor() else { return event };
                    if gap(loc.x, loc.y) > edge + 40.0 {
                        // 積算ドリフト検出: 実位置で CUR_POS を補正して通過
                        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (loc.x, loc.y);
                        return event;
                    }
                    // switchDelay: 端に N ms 滞ってから切替(0=無効)。
                    // 滞在判定は「端に到達し続けている」間のみ継続する
                    let delay = SWITCH_DELAY_MS.load(Ordering::Relaxed);
                    if delay > 0 {
                        let now = now_ms();
                        let since = EDGE_STAY_SINCE_MS.load(Ordering::Relaxed);
                        if since == 0 {
                            EDGE_STAY_SINCE_MS.store(now, Ordering::Relaxed);
                            return event; // 滞在計測を開始(まだ切替ない)
                        }
                        if now.saturating_sub(since) < delay {
                            return event; // まだ規定時間に達していない
                        }
                        EDGE_STAY_SINCE_MS.store(0, Ordering::Relaxed);
                    } else if !EDGE_AT_EDGE.swap(true, Ordering::Relaxed) {
                        // switchDoubleTap: 閾値を「下から跨いだ瞬間」だけをヒットと数え、
                        // 判定窓(DOUBLE_TAP_MS)以内の 2回目のヒットでのみ切替する。
                        // カーソルが境界に張り付いたまま出す delta は継続扱いで数えない
                        let taps = EDGE_TAPS.load(Ordering::Relaxed);
                        let now = now_ms();
                        let win_ms = DOUBLE_TAP_MS.load(Ordering::Relaxed).max(100);
                        let prev = EDGE_LAST_HIT_MS.swap(now, Ordering::Relaxed);
                        let fire = taps <= 1 || (prev > 0 && now.saturating_sub(prev) <= win_ms);
                        if !fire {
                            // 1回目: 境界から少し内側へ弾き返す。壁に当たった感触で
                            // 「もう一度押すと通る」ことを体感させる(本質の可視化)
                            eprintln!("[edge] 1回目の到達(跳ね返し)");
                            let (bx, by) = match side {
                                1 => (15.0, loc.y),
                                2 => (loc.x, 15.0),
                                3 => (loc.x, main_h - 15.0),
                                _ => (edge_x - 15.0, loc.y),
                            };
                            CGWarpMouseCursorPosition(CGPoint { x: bx, y: by });
                            *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (bx, by);
                            EDGE_AT_EDGE.store(false, Ordering::Relaxed);
                            return event;
                        }
                        EDGE_LAST_HIT_MS.store(0, Ordering::Relaxed);
                    }
                    WIN_MODE.store(true, Ordering::Relaxed);
                    DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
                    eprintln!("[mode] WINDOWS (edge) at ({:.0},{:.0})", loc.x, loc.y);

                    // ドラッグ中の切替: 既定は全ボタンを離して持ち込まない(誤ドラッグ防止。
                    // レビュー Wave1 C-S13)。TSUNAGU_DRAG_SWITCH=1 では逆に押下中の
                    // ボタンを Windows 側で押し直す=「掴んだまま境界を越える」体験
                    if event_type != EVT_MOUSE_MOVED {
                        if DRAG_SWITCH.load(Ordering::Relaxed) {
                            for b in 0u8..=2 {
                                if BTN_DOWN[b as usize].load(Ordering::Relaxed) {
                                    send_msg(&Msg::MouseButton { btn: b, down: true });
                                }
                            }
                        } else {
                            for b in 0u8..=2 {
                                send_msg(&Msg::MouseButton { btn: b, down: false });
                            }
                        }
                    }
                    // 前回 Windows に出た位置があればそこへ戻し、なければ境界の対応高さへ。
                    // 高さは出口ディスプレイの y 範囲で正規化(反転なし: 画面上端同士が対応)
                    let (mut nx, mut ny) = *LAST_WIN_POS.lock().unwrap_or_else(|e| e.into_inner());
                    if nx < 0.0 {
                        nx = 0.05;
                        let (ymin, ymax) = EDGE_DISP_Y
                            .get()
                            .copied()
                            .unwrap_or((0.0, SCREEN_H.get().copied().unwrap_or(1329.0)));
                        ny = if ymax > ymin {
                            ((loc.y - ymin) / (ymax - ymin)).clamp(0.0, 1.0)
                        } else {
                            0.5
                        };
                    }
                    send_msg(&Msg::Warp { nx, ny });
                    eprintln!("[warp] -> win ({:.2},{:.2})", nx, ny);
                    // 絶対位置モードの仮想カーソルを Warp 先で初期化
                    {
                        let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                        *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner()) = (nx * ww, ny * wh);
                        *LAST_ABS_SENT.lock().unwrap_or_else(|e| e.into_inner()) = (-1.0, -1.0);
                    }
                    enter_win_mode_cursor_lock();
                    return std::ptr::null_mut();
                }
            }
        }
        return event; // 素通し
    }

    // Windows モード: 全イベントを握って転送
    let flags = CGEventGetFlags(event);
    let (ctrl, opt, cmd, shift) = (
        flags & FLAG_CTRL != 0,
        flags & FLAG_OPT != 0,
        flags & FLAG_CMD != 0,
        flags & FLAG_SHIFT != 0,
    );
    match event_type {
        EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED => {
            let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
            let down = if event_type == EVT_FLAGS_CHANGED {
                // flagsChanged は「その修飾が押された」イベントのみ来る(離す時は flags から消える)
                // 押下状態は flags から判定
                match kc {
                    55 => cmd,
                    56 | 60 => shift,
                    58 | 61 => opt,
                    59 | 62 => ctrl,
                    _ => false,
                }
            } else {
                event_type == EVT_KEY_DOWN
            };
            if down && (kc == 104 || kc == 102) {
                eprintln!("[ime] kc={kc} ({}) 転送", if kc == 104 { "かな" } else { "英数" });
            }
            // ---- Mac 流ショートカットの Windows 翻訳(指癖をそのまま通す) ----
            // 元キーは握りつぶし、翻訳先の Key を送る。修飾の対応:
            //   cmd→Win Ctrl / opt→Win Alt / ctrl→Win キー(既定マップ)
            // 注意: flagsChanged(mod キー単体)は翻訳しない。
            // 常時有効(マスト機能)。TSUNAGU_MAC_KEYS=0 でのみオフ
            if event_type != EVT_FLAGS_CHANGED
                && envutil::get("TSUNAGU_MAC_KEYS").as_deref() != Some("0")
            {
                // 翻訳先の修飾は「既定マップ(cmd→Ctrl / opt→Alt)」で解釈させる。
                // CMD_ALT=true でも翻訳の意味が変わらないよう、cmd/opt を差し替える
                let swap = crate::CMD_ALT.load(Ordering::Relaxed);
                let send = |kc2: u16, d: bool, c: bool, o: bool, m: bool, sh: bool| {
                    let (c2, o2, m2) = if swap { (c, m, o) } else { (c, o, m) };
                    send_msg(&Msg::Key { kc: kc2, down: d, ctrl: c2, opt: o2, cmd: m2, shift: sh, tr: true });
                };
                // fn+F11(Mac のデスクトップ表示)= Win+D
                if kc == 103 && flags & FLAG_FN != 0 {
                    send(2, down, true, false, false, false); // D + ctrl フラグ(Win キー)
                    return std::ptr::null_mut();
                }
                let translated = if cmd && ctrl && !opt {
                    // ⌘Ctrl+Q = 画面ロック(Win+L)
                    match kc {
                        12 => { send(37, down, true, false, false, false); true }
                        _ => false,
                    }
                } else if cmd && opt && !ctrl {
                    // ⌘⌥Esc = タスクマネージャ(Ctrl+Shift+Esc)
                    match kc {
                        53 => { send(53, down, false, false, true, true); true }
                        _ => false,
                    }
                } else if cmd && !ctrl && !opt {
                    match kc {
                        // ⌘←→ = 行頭/行末(Windows の Home/End)。Shift は透過(行選択)
                        123 => { send(115, down, false, false, false, shift); true }
                        124 => { send(119, down, false, false, false, shift); true }
                        // ⌘↑↓ = 文書先頭/末尾(Ctrl+Home/End)。Shift 透過(文書選択)
                        126 => { send(115, down, false, false, true, shift); true }
                        125 => { send(119, down, false, false, true, shift); true }
                        // ⌘M/⌘H = 最小化(Win+Down = ctrl フラグ)
                        43 | 4 => { send(125, down, true, false, false, false); true }
                        // ⌘] / ⌘[ = ブラウザの次/前タブ(Ctrl(+Shift)+Tab)
                        // Mac と同じく ⌘⇧[ も「前タブ」(shift 状態は見ない)
                        30 => { send(48, down, false, false, true, false); true }
                        33 => { send(48, down, false, false, true, true); true }
                        // ⌘⇧4 / ⌘⇧3 = スクリーンショット(Win+Shift+S の切取り)
                        21 | 18 if shift => { send(1, down, true, false, false, true); true }
                        // ⌘⇧5 = 画面録画(Win+Alt+R)
                        23 if shift => { send(15, down, false, true, false, false); true }
                        // ⌘Q = ウィンドウを閉じる(Alt+F4 = opt フラグ+F4)
                        12 => { send(118, down, false, true, false, false); true }
                        // ⌘G / ⌘⇧G = 次を検索 / 前を検索(F3 / Shift+F3)
                        // G の Mac keycode=32、F3=99
                        32 if !shift => { send(99, down, false, false, false, false); true }
                        32 => { send(99, down, false, false, false, true); true }
                        // ⌘⇧N = シークレット/新規ウィンドウ系は Ctrl+Shift+N で自然に動く
                        // ⌘. (kc47? . は47) = キャンセル→Escape 相当(Windows でも Esc)
                        47 => { send(53, down, false, false, false, false); true }
                        // ⌘Space = IME/言語切替(Win+Space = ctrl フラグ)
                        49 => { send(49, down, true, false, false, false); true }
                        _ => false,
                    }
                } else if opt && !cmd && !ctrl {
                    match kc {
                        // ⌥←→ = 単語移動(Windows では Ctrl+←→ = cmd フラグ)。
                        // Shift は透過(⌥⇧←→ = 単語単位の選択)
                        123 => { send(123, down, false, false, true, shift); true }
                        124 => { send(124, down, false, false, true, shift); true }
                        _ => false,
                    }
                } else {
                    false
                };
                if translated {
                    return std::ptr::null_mut(); // 元キーは送らない
                }
            }
            send_msg(&Msg::Key { kc, down, ctrl, opt, cmd, shift, tr: false });
        }
        EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED => {
            let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
            let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
            if dx != 0.0 || dy != 0.0 {
                let sc = mouse_scale();
                if MOUSE_ABS_MODE.load(Ordering::Relaxed) {
                    // 絶対位置モード: Mac の加速済み delta に Windows 側の加速が
                    // 二重に乗るのを防ぎつつ、画面比率で見た目の移動距離を揃える
                    let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                    let (mw, mh) = (
                        SCREEN_W.get().copied().unwrap_or(2056.0),
                        SCREEN_H.get().copied().unwrap_or(1329.0),
                    );
                    let (sx, sy) = (ww / mw, wh / mh); // 方向別スケール(改善B)
                    // 重要: WIN_CUR のガードをこのブロック内で必ず解放してから
                    // leave_win_mode_cursor_unlock を呼ぶ(内部で WIN_CUR を再ロック
                    // するため、保持したまま呼ぶと自己デッドロックでタップが固まる)
                    let (nx, ny, at_left) = {
                        let mut wc = WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
                        wc.0 = (wc.0 + dx * sc * sx).clamp(0.0, ww - 2.0);
                        wc.1 = (wc.1 + dy * sc * sy).clamp(0.0, wh - 2.0);
                        (
                            wc.0 / ww,
                            wc.1 / wh,
                            !HOTKEY_ONLY.load(Ordering::Relaxed)
                                && event_type == EVT_MOUSE_MOVED
                                && wc.0 <= 2.0,
                        )
                    };
                    // 毎イベント送信(量子化スキップは低速時にステップ感が出るため廃止)
                    *LAST_ABS_SENT.lock().unwrap_or_else(|e| e.into_inner()) = (nx, ny);
                    LAST_ABS_MS.store(now_ms(), Ordering::Relaxed);
                    DIAG_ABS_COUNT.fetch_add(1, Ordering::Relaxed);
                    send_msg(&Msg::MouseAbs { nx, ny });
                    // 左端到達はMac内完結で即復帰(Win往復のRTT分を削減)
                    // ドラッグ中は意図しない復帰をしない(ボタン操作中の境界越えのため)
                    if at_left {
                        WIN_MODE.store(false, Ordering::Relaxed);
                        DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
                        eprintln!("[mode] MAC (abs-left)");
                        leave_win_mode_cursor_unlock(Some(ny));
                    }
                } else {
                    // 相対移動モード(従来互換)
                    send_msg(&Msg::MouseMove { dx: dx * sc, dy: dy * sc });
                }
            }
            // カーソル固定の巻き戻しは 200ms 監視スレッドに集約した
            // (タップ内で毎イベント CGEventCreate すると負荷でカクつくため)
        }
        EVT_LEFT_DOWN | EVT_LEFT_UP => {
            // Mac 流「Ctrl+クリック=右クリック」は TSUNAGU_CTRL_CLICK=1 のみ
            // (既定 OFF。右クリックは 2本指クリックが本体操作)
            let btn = if ctrl && CTRL_CLICK.load(Ordering::Relaxed) { 1u8 } else { 0 };
            let d = event_type == EVT_LEFT_DOWN;
            BTN_DOWN[0].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn, down: d });
        }
        EVT_RIGHT_DOWN | EVT_RIGHT_UP => {
            let d = event_type == EVT_RIGHT_DOWN;
            BTN_DOWN[1].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 1, down: d });
        }
        EVT_OTHER_DOWN | EVT_OTHER_UP => {
            let d = event_type == EVT_OTHER_DOWN;
            BTN_DOWN[2].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 2, down: d });
        }
        EVT_SCROLL_WHEEL => {
            let dy = CGEventGetIntegerValueField(event, FIELD_SCROLL_A1) as f64;
            let dx = CGEventGetIntegerValueField(event, FIELD_SCROLL_A2) as f64;
            // 2本指の横スワイプ →「戻る/進む」。Mac の体感(1スワイプ=1ページ)を
            // 忠実に再現する: ジェスチャは「300ms イベントが途切れるまで」を一続きと
            // みなし、その間の発火は 1 回だけ(指を離した後の慣性 delta が届いても
            // 再発火しない=2段階戻りの防止)。閾値 60px・横優勢(|dx|*2>|dy|)のみ。
            // TSUNAGU_SWIPE_NAV=0 で従来の横ホイールへ戻せる
            let swipe_nav = envutil::get("TSUNAGU_SWIPE_NAV").as_deref() != Some("0");
            if swipe_nav && win_mode && dx != 0.0 && dx.abs() * 2.0 > dy.abs() {
                let now = now_ms();
                let mut acc = SWIPE_ACC.lock().unwrap_or_else(|e| e.into_inner());
                // 前回のイベントから 300ms 以上空いていたら新しいジェスチャ
                // (=累積と発火済みフラグの両方を引き直す)
                if now.saturating_sub(acc.1) > 300 {
                    if acc.2 != 0 {
                        eprintln!("[swipe] gesture 追加分={:.0}(発火済みのため不採用)", acc.0);
                    } else if acc.0.abs() > 8.0 {
                        eprintln!("[swipe] gesture total={:.0}(未達)", acc.0);
                    }
                    *acc = (0.0, now, 0);
                }
                acc.0 += dx;
                acc.1 = now;
                // acc.2 != 0 = このジェスチャで発火済み。以後の累積は破棄扱い
                if acc.2 == 0 && acc.0.abs() >= 60.0 {
                    // Mac の操作感: 指を右へスワイプ(ページを左へめくる)=戻る。
                    // dx>0=指右 → XButton1(戻る)、dx<0=指左 → XButton2(進む)
                    let btn = if acc.0 > 0.0 { 3u8 } else { 4 }; // 3=戻る, 4=進む
                    send_msg(&Msg::MouseButton { btn, down: true });
                    send_msg(&Msg::MouseButton { btn, down: false });
                    eprintln!("[swipe] {} 送信(total={:.0})", if btn == 3 { "戻る" } else { "進む" }, acc.0);
                    *acc = (0.0, now, now); // 発火済みマーク(ジェスチャ完結まで保持)
                }
                // 横優勢ジェスチャはここで完結(縦の揺れも無視し二重発火を防ぐ)
                return std::ptr::null_mut();
            }
            if dx != 0.0 || dy != 0.0 {
                // ピクセル delta → ノッチ単位へ累積変換。0.05ノッチ(=6 wheel units)刻みで
                // 送る=Windows のプレシジョンタッチパッドと同じ高解像度スクロール。
                // 0.25刻み(30 units)は低速スクロールがカクつくため細かくした。
                // 除数を大きくすると遅くなる(設定ウィンドウのスライダーで可変)。端数は持ち越し
                // 互換モードは 1 ノッチ(120)単位に量子化(旧来のホイール相当)。
                // 既定は 0.05 ノッチ(=6 wheel units)の高解像度
                let q: f64 = if SCROLL_COMPAT.load(Ordering::Relaxed) { 1.0 } else { 0.05 };
                let div = scroll_div();
                // 方向: 既定は Mac の操作感に合わせる(自然スクロール設定を起動時に
                // 取得)。SCROLL_FLIP=true は「Windows 標準」への手動上書き。
                // 実測: 自然スクロール環境で Mac と同じ向きになるのは -1 側
                let aligned = if NATURAL_SCROLL.load(Ordering::Relaxed) { -1.0 } else { 1.0 };
                let sgn = if SCROLL_FLIP.load(Ordering::Relaxed) { -aligned } else { aligned };
                let mut acc = SCROLL_ACC.lock().unwrap_or_else(|e| e.into_inner());
                acc.0 += sgn * dx / div;
                acc.1 += sgn * dy / div;
                // 異常な残高(1e6超)は何かの暴発なので捨てる
                if acc.0.abs() > 1.0e6 || acc.1.abs() > 1.0e6 {
                    *acc = (0.0, 0.0);
                }
                let (ix, iy) = ((acc.0 / q).trunc() * q, (acc.1 / q).trunc() * q);
                if ix != 0.0 || iy != 0.0 {
                    acc.0 -= ix;
                    acc.1 -= iy;
                    DIAG_SCROLL_COUNT.fetch_add(1, Ordering::Relaxed);
                    send_msg(&Msg::Scroll { dx: ix, dy: iy });
                }
            }
        }
        _ => {}
    }
    std::ptr::null_mut() // 握りつぶす
}

/// 表示用のリリースバージョン(設定ウィンドウ等)
pub const VERSION_STR: &str = env!("CARGO_PKG_VERSION");
const BUILD_ID: &str = "build-20260926-024255-9be0636";

fn main() {
    eprintln!("[info] tsunagu-mac {BUILD_ID}");
    let args: Vec<String> = std::env::args().collect();
    let _host = args
        .iter()
        .position(|a| a == "--host")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "100.84.0.2".to_string());
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);
    // トークンは必須(旧既定値 "tsunagu-dev" での脆弱な稼働を廃止。
    // 環境変数 > exe同階層.env > ~/.config/tsunagu/env の順で解決する)
    let token = match envutil::get("TSUNAGU_TOKEN") {
        Some(t) if !t.is_empty() => t,
        _ => {
            eprintln!(
                "[fatal] TSUNAGU_TOKEN が未設定です。`scripts/gen-token.sh` を実行するか、\
                 ~/.config/tsunagu/env に TSUNAGU_TOKEN=<ランダム値> を設定してください"
            );
            std::process::exit(1);
        }
    };

    let test_mode = args.iter().any(|a| a == "--test");

    let (screen_w, screen_h) = unsafe {
        let d = CGMainDisplayID();
        let b = CGDisplayBounds(d);
        (b.size.w, b.size.h)
    };
    // 全アクティブディスプレイの bounds 和集合の右端(仮想画面の右端)
    let (union_max_x, edge_disp_y) = unsafe {
        let mut ids = [0u32; 16];
        let mut n = 0u32;
        let mut max_x = screen_w;
        let mut disp_y = (0.0, screen_h);
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..n as usize] {
                let b = CGDisplayBounds(*id);
                let right = b.origin.x + b.size.w;
                if right > max_x {
                    max_x = right;
                    disp_y = (b.origin.y, b.origin.y + b.size.h);
                }
            }
        }
        (max_x, disp_y)
    };
    let _ = UNION_MAX_X.set(union_max_x);
    let _ = EDGE_DISP_Y.set(edge_disp_y);
    let _ = SCREEN_W.set(screen_w);
    let _ = SCREEN_H.set(screen_h);
    unsafe {
        if let Some(loc) = live_cursor() {
            *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (loc.x, loc.y);
        }
    }
    if let Some(d) = std::env::var("TSUNAGU_SCROLL_DIV").ok().and_then(|v| v.parse::<f64>().ok()) {
        if d > 0.0 {
            set_scroll_div(d);
        }
    }
    if let Some(m) = std::env::var("TSUNAGU_MOUSE_SCALE").ok().and_then(|v| v.parse::<f64>().ok()) {
        if m > 0.0 {
            set_mouse_scale(m);
        }
    }
    if let Some(e) = std::env::var("TSUNAGU_EDGE_PX").ok().and_then(|v| v.parse::<f64>().ok()) {
        if e >= 0.0 && e < 100.0 {
            set_edge_px(e);
        }
    }
    if let Some(m) = std::env::var("TSUNAGU_MOUSE_MODE").ok() {
        if m.eq_ignore_ascii_case("rel") {
            MOUSE_ABS_MODE.store(false, Ordering::Relaxed);
        }
    }
    if let Some(m) = std::env::var("TSUNAGU_SWITCH_MODE").ok() {
        if m.eq_ignore_ascii_case("hotkey") {
            HOTKEY_ONLY.store(true, Ordering::Relaxed);
        }
    }
    if let Some(t) = envutil::get("TSUNAGU_EDGE_TAPS").and_then(|v| v.parse::<u32>().ok()) {
        if t >= 1 && t <= 3 {
            EDGE_TAPS.store(t, Ordering::Relaxed);
        }
    }
    if let Some(k) = std::env::var("TSUNAGU_HOTKEY_KC").ok().and_then(|v| v.parse::<i64>().ok()) {
        if (1..=127).contains(&k) {
            let _ = HOTKEY_KC.set(k);
        }
    }
    // メニューで切替可能な設定の初期値(.env 経由でも指定できる)
    NATURAL_SCROLL.store(detect_natural_scroll(), Ordering::Relaxed);
    eprintln!(
        "[info] macOS scroll: {} / tsunagu 方向: {}",
        if NATURAL_SCROLL.load(Ordering::Relaxed) { "自然スクロール" } else { "標準(非自然)" },
        if envutil::get("TSUNAGU_SCROLL_FLIP").as_deref() == Some("1") {
            "Windows 標準(手動上書き)"
        } else {
            "Mac に合わせる"
        },
    );
    if envutil::get("TSUNAGU_SCROLL_FLIP").as_deref() == Some("1") {
        SCROLL_FLIP.store(true, Ordering::Relaxed);
    }
    // Deskflow 標準オプション(画面位置/切替/隅/クリップボード)
    match envutil::get("TSUNAGU_SIDE").as_deref() {
        Some("left") => SIDE.store(1, Ordering::Relaxed),
        Some("up") => SIDE.store(2, Ordering::Relaxed),
        Some("down") => SIDE.store(3, Ordering::Relaxed),
        _ => {}
    }
    if let Some(v) = envutil::get("TSUNAGU_SWITCH_DELAY").and_then(|v| v.parse::<u64>().ok()) {
        SWITCH_DELAY_MS.store(v.min(5000), Ordering::Relaxed);
    }
    if let Some(v) = envutil::get("TSUNAGU_DOUBLE_TAP_MS").and_then(|v| v.parse::<u64>().ok()) {
        DOUBLE_TAP_MS.store(v.clamp(100, 3000), Ordering::Relaxed);
    }
    if let Some(v) = envutil::get("TSUNAGU_CORNER_PX").and_then(|v| v.parse::<u64>().ok()) {
        CORNER_PX.store(v.min(500), Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_CLIP").as_deref() == Some("0") {
        CLIP_SHARE.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_DRAG_SWITCH").as_deref() == Some("1") {
        DRAG_SWITCH.store(true, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_CTRL_CLICK").as_deref() == Some("1") {
        CTRL_CLICK.store(true, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_SCROLL_COMPAT").as_deref() == Some("1") {
        SCROLL_COMPAT.store(true, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_CMD_ALT").as_deref() == Some("1") {
        CMD_ALT.store(true, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_MUTE_SPK").as_deref() == Some("0") {
        SPK_MUTE.store(false, Ordering::Relaxed);
    }
    eprintln!(
        "[info] screen {screen_w}x{screen_h} union_max_x={union_max_x:.0}. listening on :{port} (server mode). scroll_div={} mouse_scale={} edge_px={} clip_max={}KB mouse_mode={} switch_mode={} hotkey_kc={} edge_taps={}",
        scroll_div(),
        mouse_scale(),
        edge_px(),
        CLIP_MAX_BYTES / 1024,
        if MOUSE_ABS_MODE.load(Ordering::Relaxed) { "abs" } else { "rel" },
        if HOTKEY_ONLY.load(Ordering::Relaxed) { "hotkey(ロック)" } else { "edge" },
        hotkey_kc(),
        EDGE_TAPS.load(Ordering::Relaxed)
    );

    // 送信チャネル + 書き込みストリームスロット(接続が変わるたび差し替え)
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let _ = TX.set(tx);
    let slot: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
    let _ = STREAM_SLOT.set(slot.clone());

    // 単一の送信スレッド(チャネル→ストリーム差し替え方式)
    std::thread::spawn(move || {
        use std::io::Write;
        let mut ping_at = std::time::Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    // 送信の束ね(coalescing): 高頻度のマウス移動は 1 行 1 write+flush だと
                    // 小パケット連打になり、WiFi の揺らぎで束になって到着=カクつきの原因。
                    // キューに滞留中の行をまとめて 1 回の write にする(順序は保存され、
                    // Windows 側は行ごとに注入するため見た目の滑らかさが向上する)
                    // マウス移動(mouse_abs)は束ねない: 束ねると複数の目標位置が
                    // 同一フレームに到達して中間が描画されず、カクつきの原因になる。
                    // 移動は「最新位置の即時配送」が滑らかさの本体
                    let is_move = line.starts_with("{\"t\":\"mouse_abs\"");
                    let mut buf = line;
                    let mut total = buf.len();
                    if !is_move {
                        for _ in 0..32 {
                            if total > 256 * 1024 {
                                break; // 巨大行(ファイル chunk 等)の連結は程々に
                            }
                            match rx.try_recv() {
                                Ok(next) => {
                                    total += next.len();
                                    buf.push_str(&next);
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(s) = guard.as_mut() {
                        // encode() が行末 \n を持つため writeln! だと二重改行で
                        // ワイヤが \n\n になる(受信側の空行パースが倍増する)。write_all で送る
                        if s.write_all(buf.as_bytes()).and_then(|_| s.flush()).is_err() {
                            *guard = None; // 書けなくなったら外す(接続ループが検知)
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
            if ping_at.elapsed() >= Duration::from_secs(3) {
                ping_at = std::time::Instant::now();
                // 15 秒 pong が無ければ実質切断扱いでストリームを外す
                // (TCP が生きていても相手プロセスが固まった場合を拾う)
                if now_ms().saturating_sub(LAST_PONG_MS.load(Ordering::Relaxed)) > 10_000 {
                    eprintln!("[conn] pong timeout. dropping stream");
                    let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
                    *guard = None;
                    continue;
                }
                let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
                if let Some(s) = guard.as_mut() {
                    if s.write_all(encode(&Msg::Ping { ts: now_ms() }).as_bytes()).and_then(|_| s.flush()).is_err() {
                        *guard = None;
                    }
                }
            }
        }
    });

    // 音声受信・再生(Windows→Mac。独立ポート 24901。TSUNAGU_AUDIO=0 で無効)
    if envutil::get("TSUNAGU_AUDIO").as_deref() != Some("0") {
        audio::start(token.clone());
    }

    // 接続方向: 既定は Mac=サーバ(本環境のAP隔離対策)。TSUNAGU_ROLE=client +
    // TSUNAGU_HOST(または --host)で Mac=クライアント(通常ネットワークの配布先向け。
    // その場合は Windows 側を TSUNAGU_ROLE=server で待ち受ける)
    let client_role = envutil::get("TSUNAGU_ROLE").as_deref() == Some("client");
    if client_role {
        let host = args
            .iter()
            .position(|a| a == "--host")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .or_else(|| envutil::get("TSUNAGU_HOST"))
            .unwrap_or_else(|| {
                eprintln!(
                    "[fatal] TSUNAGU_ROLE=client には TSUNAGU_HOST=<Windows側IP> \
                     (または --host <IP>)の指定が必要です"
                );
                std::process::exit(1);
            });
        eprintln!("[info] client mode: connecting to {host}:{port}");
        std::thread::spawn(move || client_thread(host, port, token, screen_w, screen_h));
    } else {
        std::thread::spawn(move || server_thread(port, token, screen_w, screen_h));
    }

    // 実機E2E: notepadへ入力して Ctrl+S → ファイル名 → Enter で保存
    if args.iter().any(|a| a == "--test2") {
        std::thread::spawn(|| {
            for _ in 0..100 {
                if CONNECTED.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !CONNECTED.load(Ordering::Relaxed) {
                eprintln!("[test2] NOT CONNECTED");
                return;
            }
            std::thread::sleep(Duration::from_millis(800));
            WIN_MODE.store(true, Ordering::Relaxed);
            send_msg(&Msg::Minimize { title: "Windows Terminal".into() });
            send_msg(&Msg::Minimize { title: "terminal".into() });
            std::thread::sleep(Duration::from_millis(500));
            send_msg(&Msg::Focus { title: "メモ帳".into() });
            std::thread::sleep(Duration::from_millis(800));
            eprintln!("[test2] typing into notepad...");
            let type_str = |pairs: &[(u16, bool)]| {
                for &(kc, shift) in pairs {
                    send_msg(&Msg::Key { kc, down: true, ctrl: false, opt: false, cmd: false, shift, tr: false });
                    send_msg(&Msg::Key { kc, down: false, ctrl: false, opt: false, cmd: false, shift, tr: false });
                    std::thread::sleep(Duration::from_millis(25));
                }
            };
            // "tsunagu e2e ok" (Mac keycode)
            let body: Vec<(u16, bool)> = "tsunagu e2e ok".chars().filter_map(|c| {
                let kc = match c {
                    'a' => 0, 'b' => 11, 'c' => 8, 'd' => 2, 'e' => 14, 'f' => 3, 'g' => 5,
                    'h' => 4, 'i' => 34, 'j' => 38, 'k' => 40, 'l' => 37, 'm' => 46, 'n' => 45,
                    'o' => 31, 'p' => 35, 'q' => 12, 'r' => 15, 's' => 1, 't' => 17, 'u' => 32,
                    'v' => 9, 'w' => 13, 'x' => 7, 'y' => 16, 'z' => 6, ' ' => 49,
                    _ => return None,
                };
                Some((kc, false))
            }).collect();
            type_str(&body);
            std::thread::sleep(Duration::from_millis(300));
            // Cmd+S -> Win Ctrl+S(保存ダイアログ)
            send_msg(&Msg::Key { kc: 1, down: true, ctrl: false, opt: false, cmd: true, shift: false, tr: false });
            send_msg(&Msg::Key { kc: 1, down: false, ctrl: false, opt: false, cmd: true, shift: false, tr: false });
            std::thread::sleep(Duration::from_millis(800));
            // ファイル名欄: Cmd+A(全選択)して上書き
            send_msg(&Msg::Key { kc: 0, down: true, ctrl: false, opt: false, cmd: true, shift: false, tr: false });
            send_msg(&Msg::Key { kc: 0, down: false, ctrl: false, opt: false, cmd: true, shift: false, tr: false });
            std::thread::sleep(Duration::from_millis(200));
            // "e2eok.txt"
            let name: Vec<(u16, bool)> = "e2eok.txt".chars().filter_map(|c| {
                let kc = match c {
                    'a' => 0, 'e' => 14, 'k' => 40, 'o' => 31, 't' => 17, 'x' => 7,
                    '.' => 47, '2' => 19,
                    _ => return None,
                };
                Some((kc, false))
            }).collect();
            type_str(&name);
            std::thread::sleep(Duration::from_millis(200));
            // Enter(36)
            send_msg(&Msg::Key { kc: 36, down: true, ctrl: false, opt: false, cmd: false, shift: false, tr: false });
            send_msg(&Msg::Key { kc: 36, down: false, ctrl: false, opt: false, cmd: false, shift: false, tr: false });
            std::thread::sleep(Duration::from_millis(500));
            eprintln!("[test2] done (typed + saved)");
            WIN_MODE.store(false, Ordering::Relaxed);
        });
    }

    // テストモード: 接続確立後にキー列を自動送信(E2E検証用)
    if test_mode {
        std::thread::spawn(|| {
            for _ in 0..100 {
                if CONNECTED.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !CONNECTED.load(Ordering::Relaxed) {
                eprintln!("[test] NOT CONNECTED");
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
            WIN_MODE.store(true, Ordering::Relaxed);
            eprintln!("[test] sending key sequence...");
            // "SDEOK" の Mac keycode 列
            for kc in [1u16, 2, 14, 31, 40] {
                send_msg(&Msg::Key { kc, down: true, ctrl: false, opt: false, cmd: false, shift: false, tr: false });
                send_msg(&Msg::Key { kc, down: false, ctrl: false, opt: false, cmd: false, shift: false, tr: false });
                std::thread::sleep(Duration::from_millis(50));
            }
            send_msg(&Msg::MouseMove { dx: 120.0, dy: 60.0 });
            send_msg(&Msg::Scroll { dx: 0.0, dy: 1.0 });
            std::thread::sleep(Duration::from_millis(300));
            eprintln!("[test] sent all");
            WIN_MODE.store(false, Ordering::Relaxed);
        });
    }

    // 診断モード: 1秒ごとにモード/受信・送信カウント/実カーソル位置を記録
    if args.iter().any(|a| a == "--diag") {
        DIAG_ENABLED.store(true, Ordering::Relaxed);
        std::thread::spawn(|| {
            let mut last_cursor = (0.0f64, 0.0f64);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let (mode, mv, kd, sd, wp, mc, sc, ab, heal) = (
                    WIN_MODE.load(Ordering::Relaxed),
                    DIAG_MOVE_COUNT.load(Ordering::Relaxed),
                    DIAG_KEY_COUNT.load(Ordering::Relaxed),
                    DIAG_SEND_COUNT.load(Ordering::Relaxed),
                    DIAG_WARP_COUNT.load(Ordering::Relaxed),
                    DIAG_MODE_COUNT.load(Ordering::Relaxed),
                    DIAG_SCROLL_COUNT.load(Ordering::Relaxed),
                    DIAG_ABS_COUNT.load(Ordering::Relaxed),
                    DIAG_SELF_HEAL.load(Ordering::Relaxed),
                );
                unsafe {
                    let ev = CGEventCreate(std::ptr::null_mut());
                    let p = if ev.is_null() { CGPoint { x: 0.0, y: 0.0 } } else { CGEventGetLocation(ev) };
                    let moved = (p.x - last_cursor.0).abs() + (p.y - last_cursor.1).abs() > 1.0;
                    eprintln!(
                        "[diag] mode={} moves={mv} keys={kd} sent={sd} scrolls={sc} abs={ab} warp_fixed={wp} switches={mc} self_heal={heal} cursor=({:.0},{:.0}) moving={}",
                        if mode { "WIN" } else { "MAC" }, p.x, p.y, moved
                    );
                    last_cursor = (p.x, p.y);
                }
            }
        });
    }

    // クリップボード監視(Mac→Windows 方向): changeCount の変化でテキストを送る
    std::thread::spawn(|| {
        let mut last_count = clipboard_change_count();
        loop {
            std::thread::sleep(Duration::from_millis(200));
            // 未接続の間は基準を更新しない(切断中のコピーも再接続後に送る)
            if !CONNECTED.load(Ordering::Relaxed) {
                continue;
            }
            let cnt = clipboard_change_count();
            if cnt == last_count {
                continue;
            }
            last_count = cnt;
            // clipboardSharing=false の間は Mac→Win 方向へ送らない
            if !CLIP_SHARE.load(Ordering::Relaxed) {
                continue;
            }
            // テキストが無い/空のときは Finder の ⌘C(ファイル参照)を試す:
            // Mac で ⌘C → 画面端で切替 → Windows で Ctrl+V のファイル渡し
            let text = unsafe { mac_get_clipboard() }
                .filter(|t| !t.is_empty() && t.len() <= CLIP_MAX_BYTES);
            let Some(text) = text else {
                if let Some(files) = (unsafe { mac_clipboard_files() }) {
                    // 同じ選択の再 ⌘C で何度も流れないよう指紋で抜く
                    let key = mac_files_key(&files);
                    let dup = *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) == key;
                    if !dup {
                        *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = key;
                        eprintln!("[file] クリップボードのファイル {} 件を検出", files.len());
                        send_files_to_win(files);
                    }
                }
                continue;
            };
            // 自分が Windows から受信して書き込んだ内容は送り返さない(ループ防止)
            if LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(text.as_str()) {
                continue;
            }
            eprintln!("[clip] mac->win {} bytes", text.len());
            send_msg(&Msg::Clip { text });
        }
    });

    // WIN モード中のカーソル固定監視(改善ループ4):
    // イベントタップ経由の巻き戻しは移動イベントが来た時しか働かない。
    // 慣性や関連切断の効き遅れでカーソルが動いたままになる場合に備え、
    // 常時 200ms ごとに固定位置へ巻き戻す(境界の同時移動抑止の最終防衛)
    std::thread::spawn(|| {
        let mut fixes: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(150));
            // 自己修復: WIN モードでないのにカーソルが隠れたままの異常状態
            // (将来の同種バグや予期しない経路)を検知し、表示を復元する
            if !WIN_MODE.load(Ordering::Relaxed) && CURSOR_HIDDEN.load(Ordering::Relaxed) {
                unsafe {
                    let d = CGMainDisplayID();
                    for _ in 0..3 {
                        CGDisplayShowCursor(d);
                    }
                    CGAssociateMouseAndMouseCursorPosition(true);
                }
                CURSOR_HIDDEN.store(false, Ordering::Relaxed);
                DIAG_SELF_HEAL.fetch_add(1, Ordering::Relaxed);
                eprintln!("[cursor] self-heal: 復帰漏れを修復しました");
            }
            // タップ健全性: システムがタイムアウトでタップを無効化した際、
            // 無効化通知を取り逃しても 1 秒毎の冪等な再 enable で必ず復帰させる
            if let Some(&tap) = TAP_PORT.get() {
                if TAP_REARM_N.fetch_add(1, Ordering::Relaxed) % 7 == 0 {
                    // 150ms×7 ≒ 1秒毎
                    unsafe { CGEventTapEnable(tap as CFMachPortRef, true) };
                }
            }
            // ウォッチドッグ: WIN モード中にユーザーがマウスを動かしているのに
            // (直近2秒以内にタップ受信) Windows への転送が5秒止まっている状態は
            // 異常。強制的に Mac へ復帰させ、操作不能な状態に陥らないようにする。
            // LAST_ABS_MS は絶対位置モードしか更新しないため、条件にモードを含めないと
            // 相対モード(rel)で必ず誤発火する(rel が5秒で強制復帰されていた実績バグ)
            if WIN_MODE.load(Ordering::Relaxed) && MOUSE_ABS_MODE.load(Ordering::Relaxed) {
                let now = now_ms();
                let last_ev = LAST_EVENT_MS.load(Ordering::Relaxed);
                let last_abs = LAST_ABS_MS.load(Ordering::Relaxed);
                if last_ev > 0 && now.saturating_sub(last_ev) < 2_000 && now.saturating_sub(last_abs) > 5_000 {
                    WIN_MODE.store(false, Ordering::Relaxed);
                    eprintln!("[watchdog] WIN中に転送停止を検知。強制復帰します");
                    leave_win_mode_cursor_unlock(None);
                    continue;
                }
            }
            if !WIN_MODE.load(Ordering::Relaxed) {
                continue;
            }
            let Some((lx, ly)) = *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) else { continue };
            unsafe {
                let Some(loc) = live_cursor() else { continue };
                if (loc.x - lx).abs() > 1.0 || (loc.y - ly).abs() > 1.0 {
                    CGWarpMouseCursorPosition(CGPoint { x: lx, y: ly });
                    fixes += 1;
                    DIAG_WARP_COUNT.store(fixes, Ordering::Relaxed);
                }
            }
        }
    });

    // イベントタップ(メインスレッドで RunLoop)
    let mask: CGEventMask = (1 << EVT_LEFT_DOWN)
        | (1 << EVT_LEFT_UP)
        | (1 << EVT_RIGHT_DOWN)
        | (1 << EVT_RIGHT_UP)
        | (1 << EVT_MOUSE_MOVED)
        | (1 << EVT_LEFT_DRAGGED)
        | (1 << EVT_RIGHT_DRAGGED)
        | (1 << EVT_OTHER_DRAGGED)
        | (1 << EVT_KEY_DOWN)
        | (1 << EVT_KEY_UP)
        | (1 << EVT_FLAGS_CHANGED)
        | (1 << EVT_SCROLL_WHEEL)
        | (1 << EVT_OTHER_DOWN)
        | (1 << EVT_OTHER_UP);

    let tap = unsafe {
        CGEventTapCreate(
            0, // kCGHIDEventTap(ヘッダ実測: 0=HID, 1=Session, 2=Annotated。Deskflow は HID)
            0, // kCGHeadInsertEventTap
            0, // kCGEventTapOptionDefault = 0(抑制可/フィルタ)
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    };
    if tap.is_null() {
        eprintln!("[fatal] CGEventTapCreate failed(アクセシビリティ権限を確認)");
        std::process::exit(1);
    }
    let _ = TAP_PORT.set(tap as usize);
    unsafe {
        let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
        let rl = CFRunLoopGetMain();
        CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    eprintln!("[info] tap active. カーソルを画面右端へ動かすと Windows モード / F13・メニューでトグル");
    // メニューバー GUI(既定ON。--no-gui / TSUNAGU_NO_GUI=1 で CUI のみ)。
    // AppKit が使えない環境(ssh 由来のセッション等)では start() が失敗し、
    // 従来どおり CFRunLoop で継続する(タップはメインRunLoop共通モードのため共存可)
    let no_gui = args.iter().any(|a| a == "--no-gui")
        || envutil::get("TSUNAGU_NO_GUI").is_some_and(|v| v == "1");
    if !no_gui && gui::start() {
        eprintln!("[gui] メニューバー常駐を開始しました");
        // --show-prefs: 起動直後に設定ウィンドウを開く(スクリーンショット検証用)。
        // 実際の生成は NSApp.run 後のタイマー初回で行う
        if args.iter().any(|a| a == "--show-prefs") {
            gui::SHOW_AT_START.store(true, Ordering::Relaxed);
        }
        unsafe { gui::run_app() }; // NSApp.run(戻らない。終了はメニューから)
    } else {
        unsafe { CFRunLoopRun() };
    }
}

// ---------- 接続セッション(サーバ/クライアント両モード共通) ----------

/// 1行の長さ上限(画像base64 5MB 上限に対し余裕を持たせる。
/// 未認証helloを含む巨大行によるメモリ消費(DoS)対策)
const MAX_LINE: u64 = 8 * 1024 * 1024;

/// 認証済み接続の受信ループ(Return / Pong / Clip / Bye)
fn session_receive_loop(reader: &mut std::io::BufReader<TcpStream>) {
    use std::io::BufRead;
    // ファイル受信の途中状態(FileBegin → FileChunk… → FileEnd)。
    // 受信ループはシングルスレッドのためローカル変数で持つ
    let mut recv_file: Option<std::fs::File> = None;
    let mut recv_remain: u64 = 0;
    let mut recv_name = String::new();
    let mut recv_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() as u64 > MAX_LINE => {
                eprintln!("[conn] line too large. dropping connection");
                break;
            }
            Ok(_) => {
                if let Some(msg) = decode(&line) {
                    match msg {
                        Msg::Return { ny } => {
                            if HOTKEY_ONLY.load(Ordering::Relaxed) {
                                // hotkey モードでは Windows 側の左端到達を無視し、
                                // F13 で戻すまで Windows のまま(ロック状態)
                                continue;
                            }
                            // abs-left 復帰後に遅延到着した Return で二重に leave
                            // され再ワープされるのを防ぐ(既に Mac の場合は無視)
                            if !WIN_MODE.load(Ordering::Relaxed) {
                                continue;
                            }
                            WIN_MODE.store(false, Ordering::Relaxed);
                            eprintln!("[mode] MAC (return)");
                            leave_win_mode_cursor_unlock(Some(ny));
                        }
                        Msg::ClipData { kind, data } => {
                            if !CLIP_SHARE.load(Ordering::Relaxed) {
                                continue;
                            }
                            // 受信側にも上限を課す(送信側制限のみに依存しない)
                            if data.len() > 8 * 1024 * 1024 {
                                eprintln!("[clip] win->mac image too large. skipped");
                                continue;
                            }
                            if kind == "image/dib" {
                                if let Some(bytes) = tsunagu_common::b64::decode(&data) {
                                    let bmp = dib_to_bmp(&bytes);
                                    let ok = unsafe { mac_set_clipboard_image_bmp(&bmp) };
                                    eprintln!(
                                        "[clip] win->mac image {}KB {}",
                                        bytes.len() / 1024,
                                        if ok { "ok" } else { "FAILED" }
                                    );
                                }
                            }
                        }
                        Msg::Clip { text } => {
                            if !CLIP_SHARE.load(Ordering::Relaxed) {
                                continue;
                            }
                            if text.len() <= CLIP_MAX_BYTES {
                                // Windows の CRLF は Mac 向けに LF へ正規化
                                let text = if text.contains("\r\n") {
                                    text.replace("\r\n", "\n")
                                } else {
                                    text
                                };
                                *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(text.clone());
                                unsafe { mac_set_clipboard(&text) };
                                eprintln!("[clip] win->mac {} bytes", text.len());
                            }
                        }
                        Msg::FileBegin { name, size } => {
                            recv_file = None;
                            recv_remain = 0;
                            if let Some((f, path)) = mac_file_begin(&name, size) {
                                recv_name = path.to_string_lossy().into_owned();
                                recv_paths.push(path);
                                recv_file = Some(f);
                                recv_remain = size;
                            }
                        }
                        Msg::FileChunk { data } => {
                            if recv_file.is_none() {
                                continue;
                            }
                            let mut ok = false;
                            if let Some(bytes) = tsunagu_common::b64::decode(&data) {
                                if !bytes.is_empty() && bytes.len() as u64 <= recv_remain {
                                    use std::io::Write as _;
                                    let written = recv_file
                                        .as_mut()
                                        .map(|f| f.write_all(&bytes).is_ok())
                                        .unwrap_or(false);
                                    if written {
                                        recv_remain -= bytes.len() as u64;
                                        ok = true;
                                    }
                                }
                            }
                            if !ok {
                                eprintln!("[file] chunk 失敗。このファイルを破棄します");
                                recv_file = None;
                                recv_remain = 0;
                            }
                        }
                        Msg::FileEnd => {
                            recv_file = None; // File の drop で閉じる
                            recv_remain = 0;
                            recv_name.clear();
                        }
                        Msg::FileBatchEnd => {
                            // 一括送信の終了: 受け取った全ファイルをクリップボードへ
                            if !recv_paths.is_empty() {
                                let key = mac_files_key(&recv_paths);
                                let n = recv_paths.len();
                                let ok = unsafe { mac_clipboard_write_files(&recv_paths) };
                                // 自分が載せたファイルを監視スレッドが Windows へ
                                // 送り返さないよう指紋を登録(ループ防止)
                                *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = key;
                                if ok {
                                    eprintln!("[file] win->mac 受信: {n} 件(⌘V で貼り付け可)");
                                    notify("tsunagu", &format!("ファイルを受信: {n} 件(⌘V で貼り付け可)"));
                                } else {
                                    eprintln!("[file] win->mac 受信: {n} 件(クリップボード載せ失敗)");
                                }
                                recv_paths.clear();
                            }
                        }
                        Msg::Pong { ts } => {
                            let now = now_ms();
                            LAST_PONG_MS.store(now, Ordering::Relaxed);
                            // ping/pong の往復時間を RTT として保持し、Windows 側の
                            // ステータス窓表示にも回す(接続品質の見える化)
                            let rtt = now.saturating_sub(ts).min(60_000);
                            RTT_MS.store(rtt, Ordering::Relaxed);
                            send_msg(&Msg::Stat { rtt });
                        }
                        Msg::Bye => break,
                        _ => {}
                    }
                }
            }
        }
    }
}

/// セッション終了の共通後処理(スロット解除・切断通知・WIN 中なら正規 leave)
fn on_disconnect() {
    {
        let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
    CONNECTED.store(false, Ordering::Relaxed);
    // WIN モード中の切断は正規の leave 経由で復帰させる(カーソル表示・
    // EDGE_GUARD・CUR_POS 整合を自己修復スレッドの「たまたま」に任せない)
    if WIN_MODE.swap(false, Ordering::Relaxed) {
        eprintln!("[return] -> mac (disconnect)");
        leave_win_mode_cursor_unlock(None);
    }
    eprintln!("[conn] lost. waiting for reconnect...");
    notify("tsunagu", "切断しました(自動再接続中)");
}

/// 待受モード(既定): Windows からの接続を受け入れる
/// (本環境では Mac 発コネクションが不通なため、Windows 発に限定した設計)
fn server_thread(port: u16, token: String, screen_w: f64, screen_h: f64) {
    use std::io::{BufRead, Read};
    // 待受アドレス: 既定は Tailscale IF を想定した制限なし設定だが、
    // restart-mac.sh が TSUNAGU_BIND=$(tailscale ip -4) を渡すため、
    // 通常運用では Tailscale インタフェース以外で listen しない
    let bind_ip = envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[fatal] listen {bind_ip}:{port} failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("[info] server mode. listening on {bind_ip}:{port}");
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("[conn] accept error: {e}");
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        eprintln!("[conn] accepted from {peer}");
        // ピア許可: Tailscale の CGNAT 範囲(100.64.0.0/10)以外は即切断する。
        // 0.0.0.0 で listen してしまった場合の WiFi/LAN 露出に対する装置的防御
        let oct = match peer.ip() {
            std::net::IpAddr::V4(v4) => v4.octets(),
            std::net::IpAddr::V6(_) => [0, 0, 0, 0],
        };
        if !(oct[0] == 100 && (64..=127).contains(&oct[1])) {
            eprintln!("[conn] rejected: {peer} は Tailscale 範囲外です");
            continue;
        }
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(Duration::from_secs(12))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
        // hello を待つ(検証して hello_ok を返す)
        let mut reader = std::io::BufReader::new(match stream.try_clone() {
            Ok(s) => s,
            Err(_) => continue,
        });
        let mut line = String::new();
        // 最初の行=hello(この時点では未認証のため、take で読み込み段階から制限する)
        match (&mut reader).take(MAX_LINE + 1).read_line(&mut line) {
            Ok(0) | Err(_) => {
                eprintln!("[conn] closed before hello");
                continue;
            }
            Ok(_) if line.len() as u64 > MAX_LINE => {
                eprintln!("[conn] hello too large. dropped");
                continue;
            }
            Ok(_) => {}
        }
        let ok = match decode(&line) {
            Some(Msg::Hello { ver, name, token: t, w, h }) if ver == VERSION && t == token => {
                let _ = name;
                // 相手画面サイズを受信(速度一致の自動スケール算出に使用)
                if w > 0 && h > 0 {
                    *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = (w as f64, h as f64);
                    eprintln!("[info] win screen {w}x{h}");
                }
                true
            }
            _ => false,
        };
        if !ok {
            eprintln!("[conn] invalid hello");
            continue;
        }
        {
            let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(stream);
        }
        // hello_ok 送信は送信スレッド経由で確実に
        send_msg(&Msg::HelloOk { name: "macbook".into(), w: screen_w as i32, h: screen_h as i32 });
        // 現在の ⌘キー設定を同期(切断中に切り替えていた場合の整合)
        send_msg(&Msg::Cfg {
            cmd_alt: CMD_ALT.load(Ordering::Relaxed),
            spk_mute: SPK_MUTE.load(Ordering::Relaxed),
            side: SIDE.load(Ordering::Relaxed),
        });
        CONNECTED.store(true, Ordering::Relaxed);
        LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
        eprintln!("[conn] established");
        notify("tsunagu", "Windows に接続しました");
        session_receive_loop(&mut reader);
        on_disconnect();
    }
}

/// 接続モードの 1 セッション分(ハンドシェイク+本体)。Result はリトライ理由
fn client_attempt(
    addr: &std::net::SocketAddr,
    token: &str,
    screen_w: f64,
    screen_h: f64,
) -> Result<(), String> {
    use std::io::{BufRead, Read, Write};
    let s = std::net::TcpStream::connect_timeout(addr, Duration::from_secs(3))
        .map_err(|e| format!("connect failed: {e}"))?;
    eprintln!("[conn] connected");
    s.set_nodelay(true).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    // hello(自画面サイズを相手へ伝える。相手は hello_ok で自画面を返す)
    let mut hw = s.try_clone().map_err(|e| format!("clone failed: {e}"))?;
    let hello = encode(&Msg::Hello {
        ver: VERSION,
        name: "macbook".into(),
        token: token.to_string(),
        w: screen_w as i32,
        h: screen_h as i32,
    });
    hw.write_all(hello.as_bytes())
        .and_then(|_| hw.flush())
        .map_err(|_| "hello send failed".to_string())?;
    // hello_ok を待つ(行長制限付き)
    let sr = s.try_clone().map_err(|e| format!("clone failed: {e}"))?;
    let mut reader = std::io::BufReader::new(sr);
    let mut line = String::new();
    (&mut reader)
        .take(MAX_LINE + 1)
        .read_line(&mut line)
        .map_err(|_| "hello_ok read failed".to_string())?;
    if line.len() as u64 > MAX_LINE {
        return Err("hello_ok too large".into());
    }
    match decode(&line) {
        Some(Msg::HelloOk { w: mw, h: mh, .. }) if mw > 0 && mh > 0 => {
            *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = (mw as f64, mh as f64);
            eprintln!("[info] win screen {mw}x{mh}");
        }
        _ => return Err("invalid hello_ok".into()),
    }
    {
        let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(s);
    }
    CONNECTED.store(true, Ordering::Relaxed);
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    // 現在の ⌘キー設定を同期(クライアントモードの確立時)
    send_msg(&Msg::Cfg {
        cmd_alt: CMD_ALT.load(Ordering::Relaxed),
        spk_mute: SPK_MUTE.load(Ordering::Relaxed),
        side: SIDE.load(Ordering::Relaxed),
    });
    eprintln!("[conn] established");
    notify("tsunagu", "Windows に接続しました");
    session_receive_loop(&mut reader);
    on_disconnect();
    Ok(())
}

/// 接続モード(TSUNAGU_ROLE=client): Windows(サーバ)へ接続し続ける
fn client_thread(host: String, port: u16, token: String, screen_w: f64, screen_h: f64) {
    use std::net::ToSocketAddrs;
    let Some(addr) = (host.as_str(), port).to_socket_addrs().ok().and_then(|mut it| it.next())
    else {
        eprintln!("[fatal] invalid host: {host}");
        std::process::exit(1);
    };
    let mut backoff = 500u64;
    loop {
        if let Err(e) = client_attempt(&addr, &token, screen_w, screen_h) {
            eprintln!("[conn] {e}");
        } else {
            backoff = 500;
        }
        std::thread::sleep(Duration::from_millis(backoff));
        backoff = (backoff * 2).min(3000);
    }
}
