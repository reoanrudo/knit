// tsunagu-mac: Mac 側クライアント。CGEventTap で入力を横流しし、Windows へ送信する。
// 画面右端でカーソルが Mac→Windows 切替、Windows カーソル左端(または F13)で復帰。
#![allow(non_camel_case_types)]

mod audio;
mod gui;

use tsunagu_common::{bulk, envutil, secure};
use tsunagu_common::proto::{compatible, decode, encode, Msg, PORT, VERSION};
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
static HOTKEY_KC: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(KC_F13);

fn hotkey_kc() -> i64 {
    HOTKEY_KC.load(Ordering::Relaxed)
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
    fn CGEventGetIntegerValueField(event: CGEventRef, field: i32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGGetActiveDisplayList(max_displays: u32, active_displays: *mut u32, display_count: *mut u32) -> i32;
    fn CGDisplayRegisterReconfigurationCallback(
        callback: unsafe extern "C" fn(u32, u32, *mut core::ffi::c_void),
        user_info: *mut core::ffi::c_void,
    ) -> i32;
    fn CGWarpMouseCursorPosition(new: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connect: bool) -> i32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGSetLocalEventsSuppressionInterval(seconds: f64) -> i32;
    fn CGEventCreate(allocator: CFAllocatorRef) -> CGEventRef;
    fn CGEventCreateMouseEvent(
        source: CFAllocatorRef, mouse_type: u32, mouse_position: CGPoint, button: u64,
    ) -> CGEventRef;
    fn CGEventSetIntegerValueField(event: CGEventRef, field: i32, value: i64);
    fn CGEventPost(tap: i32, event: CGEventRef);
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
    fn objc_autoreleasePoolPush() -> *mut core::ffi::c_void;
    fn objc_autoreleasePoolPop(pool: *mut core::ffi::c_void);
}

/// バックグラウンドスレッドで ObjC の一時オブジェクトを扱う区間を囲む。
/// メインスレッド以外には autorelease pool が無く、nsstring 等が解放されずに
/// 溜まり続ける(120ms 周期の監視で確定的にリークしていた。レビュー B-P1-1)
fn with_pool<T>(f: impl FnOnce() -> T) -> T {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let r = f();
        objc_autoreleasePoolPop(pool);
        r
    }
}

/// パスワードマネージャ等が「記録・共有しないで」と印を付けたコピーか
/// (nspasteboard.org の慣行。1Password/Bitwarden/キーチェーン等が付ける)
unsafe fn pb_is_concealed(pb: ID) -> bool {
    let types = msg0(pb, sel_registerName(c"types".as_ptr()));
    if types.is_null() {
        return false;
    }
    ["org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType", "com.agilebits.onepassword"]
        .iter()
        .any(|t| {
            let f: unsafe extern "C" fn(ID, SEL, ID) -> u8 = std::mem::transmute(objc_msgSend as *const () as usize);
            f(types, sel_registerName(c"containsObject:".as_ptr()), nsstring(t)) != 0
        })
}

/// 最後に Windows と同期したクリップボードの changeCount
static LAST_SYNC_COUNT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(-1);

/// Windows へ入る時に Mac のクリップボードを渡す(Deskflow と同じ「画面を離れる時に
/// 同期」方式)。コピーのたびに送る旧方式は、Mac 内だけのコピペでも最大 200MB の
/// ファイルを流し、パスワード等も即座に相手へ渡っていた。
/// force=true はメニューの「今すぐ Windows へ送る」用: 変化チェックだけを飛ばし、
/// 秘匿除外・エコーバック防止はそのまま効く
pub(crate) fn sync_clipboard_to_win(force: bool) {
    if !CLIP_SHARE.load(Ordering::Relaxed) || !CONNECTED.load(Ordering::Relaxed) {
        // メニューからの明示操作なら、何も起きなかった理由を伝える
        if force {
            let why = if !CONNECTED.load(Ordering::Relaxed) { "未接続" } else { "クリップボード共有が OFF" };
            eprintln!("[clip] 送信できません({why})");
            notify("tsunagu", &format!("クリップボードを送れません({why})"));
        }
        return;
    }
    // 貼り付け元アプリの遅延提供データ読み出しでタップを止めないよう別スレッドで行う
    std::thread::spawn(move || with_pool(|| unsafe {
        let cnt = clipboard_change_count();
        if LAST_SYNC_COUNT.swap(cnt, Ordering::Relaxed) == cnt && !force {
            return;
        }
        if pb_is_concealed(general_pasteboard()) {
            eprintln!("[clip] 秘匿指定のコピー(パスワード等)のため送りません");
            return;
        }
        let text = mac_get_clipboard().filter(|t| !t.is_empty() && t.len() <= CLIP_MAX_BYTES);
        let Some(text) = text else {
            if let Some(files) = mac_clipboard_files() {
                let key = mac_files_key(&files);
                let dup = *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) == key;
                if !dup {
                    *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = key;
                    eprintln!("[file] クリップボードのファイル {} 件を渡します", files.len());
                    send_files_to_win(files, false);
                }
            } else if !force && now_ms().saturating_sub(LAST_IMG_RX_MS.load(Ordering::Relaxed)) < 1_000 {
                // 受信画像の載せ直後に来た同期: 送り返しの恐れがあるため見送る。
                // 明示送信(force)は意図が明確なためそのまま送る
                eprintln!("[clip] 画像受信直後のため同期を控えます");
                return;
            } else if let Some(dib) = mac_clipboard_image_dib() {
                match BULK_LINK.send(|w| bulk::send_image(w, &dib)) {
                    Ok(()) => {
                        eprintln!("[clip] mac->win image {}KB", dib.len() / 1024);
                        if force {
                            notify("tsunagu", "クリップボードの画像を Windows へ送りました");
                        }
                    }
                    Err(e) => eprintln!("[clip] mac->win image 送信失敗: {e}"),
                }
            }
            return;
        };
        // 自分が Windows から受信して書き込んだ内容は送り返さない(ループ防止)
        if LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(text.as_str()) {
            return;
        }
        eprintln!("[clip] mac->win {} bytes", text.len());
        send_msg(&Msg::Clip { text });
        if force {
            notify("tsunagu", "クリップボードを Windows へ送りました");
        }
    }));
}

/// 最後に Windows から受信して書き込んだテキスト(エコーバック送信防止)
static LAST_RECV_CLIP: Mutex<Option<String>> = Mutex::new(None);
/// 最後の画像受信時刻(ms)。受信画像のクリップボード載せ→changeCount 更新の
/// 間に切替同期が走ると画像を送り返してしまう(最大 64MB の無駄転送)ため、
/// 直近の受信では画像送信を 1 回控える(changeCount 保護の二重ガード)
static LAST_IMG_RX_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const CLIP_MAX_BYTES: usize = 1024 * 1024; // 1MB(Win側と同じ上限)

type ID = *mut core::ffi::c_void;
type SEL = *mut core::ffi::c_void;

// objc_msgSend は可変引数宣言のまま呼ぶと引数の渡りが壊れる(SIGSEGV実績あり)ため、
// 呼び出しシグネチャごとに transmute した固定シグネチャで呼ぶ(rust-objc 界の定番方式)
unsafe fn msg0(target: ID, sel: SEL) -> ID {
    let f: unsafe extern "C" fn(ID, SEL) -> ID = std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}
unsafe fn msg1_id(target: ID, sel: SEL, a: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID = std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, a)
}
unsafe fn msg1_cstr(target: ID, sel: SEL, p: *const core::ffi::c_char) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, *const core::ffi::c_char) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, p)
}
unsafe fn msg2_bool(target: ID, sel: SEL, a: ID, b: ID) -> u8 {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, a, b)
}
unsafe fn msg0_isize(target: ID, sel: SEL) -> isize {
    let f: unsafe extern "C" fn(ID, SEL) -> isize = std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}
unsafe fn msg0_cstr(target: ID, sel: SEL) -> *const core::ffi::c_char {
    let f: unsafe extern "C" fn(ID, SEL) -> *const core::ffi::c_char =
        std::mem::transmute(objc_msgSend as *const () as usize);
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

/// ドラッグ用ペーストボード(NSPasteboardNameDrag = "Apple CFPasteboard drag")。
/// Finder 等のファイルドラッグ中のみファイル URL が載る。セレクタは
/// pasteboardWithName:(クラスメソッド)。NSPasteboardNameDrag 定数の実体文字列を直接渡す
unsafe fn drag_pasteboard() -> ID {
    let name = nsstring("Apple CFPasteboard drag");
    msg1_id(
        objc_getClass(c"NSPasteboard".as_ptr()),
        sel_registerName(c"pasteboardWithName:".as_ptr()),
        name,
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
            std::mem::transmute(objc_msgSend as *const () as usize);
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
        let f: unsafe extern "C" fn(ID, SEL, ID) -> ID = std::mem::transmute(objc_msgSend as *const () as usize);
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
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 = std::mem::transmute(objc_msgSend as *const () as usize);
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
unsafe fn pb_files(pb: ID) -> Option<Vec<std::path::PathBuf>> {
    if pb.is_null() {
        return None;
    }
    let url_cls = objc_getClass(c"NSURL".as_ptr());
    if url_cls.is_null() {
        return None;
    }
    let classes = {
        let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            objc_getClass(c"NSArray".as_ptr()),
            sel_registerName(c"arrayWithObject:".as_ptr()),
            url_cls,
        )
    };
    let options = {
        let yes = {
            let f: unsafe extern "C" fn(ID, SEL, u8) -> ID =
                std::mem::transmute(objc_msgSend as *const () as usize);
            f(
                objc_getClass(c"NSNumber".as_ptr()),
                sel_registerName(c"numberWithBool:".as_ptr()),
                1,
            )
        };
        let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            objc_getClass(c"NSDictionary".as_ptr()),
            sel_registerName(c"dictionaryWithObject:forKey:".as_ptr()),
            yes,
            NSPasteboardURLReadingFileURLsOnlyKey,
        )
    };
    let urls = {
        let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
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
    // 65 件目以降は無通知で欠けるため打ち切りをログへ残す
    if n > 64 {
        eprintln!("[clip] ファイル参照 {n} 件のうち先頭 64 件のみ扱います");
    }
    let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
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

/// general クリップボードのファイル参照(⌘C 検出用の既存経路)
unsafe fn mac_clipboard_files() -> Option<Vec<std::path::PathBuf>> {
    pb_files(general_pasteboard())
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
        std::mem::transmute(objc_msgSend as *const () as usize);
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
        std::mem::transmute(objc_msgSend as *const () as usize);
    let arr = make_arr(
        objc_getClass(c"NSArray".as_ptr()),
        sel_registerName(c"arrayWithObjects:count:".as_ptr()),
        urls.as_ptr(),
        urls.len(),
    );
    let write: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    write(pb, sel_registerName(c"writeObjects:".as_ptr()), arr) != 0
}

/// ファイル群の指紋(パス+合計サイズ)。形式は common::files::key に統一
fn mac_files_key(paths: &[std::path::PathBuf]) -> String {
    let v: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    bulk::files_key(&v)
}

/// ファイル群を Windows へ送る(FileBegin → FileChunk… → FileEnd)。
/// GUI メニュー(NSOpenPanel)と Finder の ⌘C 検出の両方から呼ぶ。別スレッド実行。
/// drop=true は「掴んだまま境界越え」: FileDropBegin で始め FileDropEnd で終わり、
/// Windows 側はクリップボードではなく OLE ドラッグとして扱う
pub fn send_files_to_win(paths: Vec<std::path::PathBuf>, drop: bool) {
    if FILE_TX_BUSY.swap(true, Ordering::Relaxed) {
        eprintln!("[file] 送信中のため要求を無視しました");
        return;
    }
    std::thread::spawn(move || {
        let total = bulk::total_size(&paths);
        // 空ファイルのみの選択(total=0)は正当な送信のため、件数 0 だけを拒否する
        if paths.is_empty() || total > bulk::MAX_TOTAL {
            eprintln!("[file] 送信拒否: {} 件 / 合計 {total} bytes", paths.len());
            notify(
                "tsunagu",
                &format!("ファイルを送信できません(合計 {}MB。上限 200MB)", total / 1024 / 1024),
            );
            FILE_TX_BUSY.store(false, Ordering::Relaxed);
            return;
        }
        eprintln!("[file] 送信開始: {} 件 / 合計 {}KB{}", paths.len(), total / 1024, if drop { "(掴みドラッグ)" } else { "" });
        let t0 = std::time::Instant::now();
        // 進捗は 10% 刻みでログへ(巨大転送中に固まって見えるのを防ぐ)。
        // クロージャは send で消費されるため、再試行側にも同じ形を書く
        macro_rules! send_with_progress {
            () => { BULK_LINK.send(|w| {
                let mut last_step = 0u64;
                bulk::send_files_with_progress(w, &paths, drop, |sent, total| {
                    if total > 0 {
                        let step = sent * 10 / total.max(1);
                        if step > last_step {
                            last_step = step;
                            eprintln!("[file] 転送 {}%({}/{})", step * 10, sent, total);
                        }
                    }
                })
            }) };
        }
        // 未接続(NotConnected)は本線再接続直後の bulk 張り直しの窓(最大約 5 秒)で起きる。
        // ユーザー操作がログ 1 行で失われるのを防ぐため、少し待って 1 回だけやり直す
        let mut r = send_with_progress!();
        if r.as_ref().is_err_and(|e| e.kind() == std::io::ErrorKind::NotConnected) {
            eprintln!("[file] bulk 経路の再接続を待って再試行します");
            std::thread::sleep(Duration::from_millis(2500));
            r = send_with_progress!();
        }
        match r {
            Ok(n) => {
                let secs = t0.elapsed().as_secs_f64().max(0.001);
                eprintln!(
                    "[file] 送信完了({n} 件, {:.1}MB/s)",
                    total as f64 / 1024.0 / 1024.0 / secs
                );
                if drop {
                    notify("tsunagu", &format!("{n} 件のファイルを Windows へ掴んで渡しました"));
                } else {
                    notify("tsunagu", &format!("{n} 件のファイルを Windows へ送信しました(Ctrl+V で貼り付け)"));
                }
            }
            Err(e) => {
                eprintln!("[file] 送信失敗: {e}");
                notify("tsunagu", "Windows へファイルを送れませんでした(ファイル転送経路が未接続)");
            }
        }
        FILE_TX_BUSY.store(false, Ordering::Relaxed);
    });
}

/// Mac のクリップボード画像(PNG/TIFF/JPEG)を Windows の CF_DIB 形式にする。
/// NSBitmapImageRep で BMP に書き出し、先頭 14 バイトのファイルヘッダを外すと DIB になる
/// (Windows 側に画像デコーダを持たずに済む)
unsafe fn mac_clipboard_image_dib() -> Option<Vec<u8>> {
    let pb = general_pasteboard();
    let data = ["public.png", "public.tiff", "public.jpeg"]
        .iter()
        .map(|t| msg1_id(pb, sel_registerName(c"dataForType:".as_ptr()), nsstring(t)))
        .find(|d| !d.is_null())?;
    let rep = msg1_id(
        objc_getClass(c"NSBitmapImageRep".as_ptr()),
        sel_registerName(c"imageRepWithData:".as_ptr()),
        data,
    );
    if rep.is_null() {
        return None;
    }
    let props = msg0(objc_getClass(c"NSDictionary".as_ptr()), sel_registerName(c"dictionary".as_ptr()));
    let repr: unsafe extern "C" fn(ID, SEL, usize, ID) -> ID = std::mem::transmute(objc_msgSend as *const () as usize);
    // NSBitmapImageFileTypeBMP = 1
    let bmp = repr(rep, sel_registerName(c"representationUsingType:properties:".as_ptr()), 1, props);
    if bmp.is_null() {
        return None;
    }
    let len = msg0_isize(bmp, sel_registerName(c"length".as_ptr())) as usize;
    let ptr = msg0(bmp, sel_registerName(c"bytes".as_ptr())) as *const u8;
    if ptr.is_null() || len <= 14 || len > bulk::MAX_IMAGE {
        return None;
    }
    Some(std::slice::from_raw_parts(ptr, len)[14..].to_vec())
}

/// 大容量経路の受信完了(Windows からのファイル・画像)
fn mac_on_bulk(e: bulk::Event) {
    with_pool(|| match e {
        bulk::Event::Files { paths, .. } => {
            let n = paths.len();
            let ok = unsafe { mac_clipboard_write_files(&paths) };
            // 自分が載せたファイルを Windows へ送り返さない
            *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = mac_files_key(&paths);
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            push_recent_rx(&paths);
            if ok {
                eprintln!("[file] win->mac 受信: {n} 件(⌘V で貼り付け可)");
                notify("tsunagu", &format!("ファイルを受信: {n} 件(⌘V で貼り付け可)"));
            } else {
                eprintln!("[file] win->mac 受信: {n} 件(クリップボード載せ失敗)");
            }
        }
        bulk::Event::Image(dib) => {
            if !CLIP_SHARE.load(Ordering::Relaxed) {
                return;
            }
            LAST_IMG_RX_MS.store(now_ms(), Ordering::Relaxed);
            let ok = unsafe { mac_set_clipboard_image_bmp(&dib_to_bmp(&dib)) };
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            eprintln!("[clip] win->mac image {}KB {}", dib.len() / 1024, if ok { "ok" } else { "FAILED" });
        }
    })
}

/// Mac 流ショートカットの翻訳対応表。(kc, 修飾) → 翻訳先 (kc, ctrl, opt, cmd, shift)。
/// 翻訳先の修飾は「既定マップ(cmd→Ctrl / opt→Alt / ctrl→Win)」の意味で並べる
/// (CMD_ALT 交換は tap 側の送信時に行う)。対応をテストで固定するため表へ切り出した。
/// fn+F11 は FN フラグを見るためここに含めない
pub(crate) fn mac_shortcut_translation(
    kc: u16,
    ctrl: bool,
    opt: bool,
    cmd: bool,
    shift: bool,
) -> Option<(u16, bool, bool, bool, bool)> {
    if cmd && ctrl && !opt {
        // ⌘Ctrl+Q = 画面ロック(Win+L)
        return (kc == 12).then_some((37, true, false, false, false));
    }
    if cmd && opt && !ctrl {
        // ⌘⌥Esc = タスクマネージャ(Ctrl+Shift+Esc)
        return (kc == 53).then_some((53, false, false, true, true));
    }
    if cmd && !ctrl && !opt {
        // 戻り値: (翻訳先 kc, ctrl, opt, cmd, shift)
        let t = match kc {
            // ⌘←→ = 行頭/行末(Home/End)。Shift は透過(行選択)
            123 => Some((115, false, false, false, shift)),
            124 => Some((119, false, false, false, shift)),
            // ⌘↑↓ = 文書先頭/末尾(Ctrl+Home/End)。Shift 透過
            126 => Some((115, false, false, true, shift)),
            125 => Some((119, false, false, true, shift)),
            // ⌘M/⌘H = 最小化(Win+Down)
            43 | 4 => Some((125, true, false, false, false)),
            // ⌘] / ⌘[ = 次タブ / 前タブ(Ctrl(+Shift)+Tab)。⌘⇧[ も「前タブ」
            30 => Some((48, false, false, true, false)),
            33 => Some((48, false, false, true, true)),
            // ⌘⇧4 / ⌘⇧3 = スクリーンショット(Win+Shift+S)
            21 | 18 if shift => Some((1, true, false, false, true)),
            // ⌘⇧5 = 画面録画(Win+Alt+R)
            23 if shift => Some((15, false, true, false, false)),
            // ⌘Q = ウィンドウを閉じる(Alt+F4)
            12 => Some((118, false, true, false, false)),
            // ⌘G / ⌘⇧G = 次を検索 / 前を検索(F3 / Shift+F3)
            32 => Some((99, false, false, false, shift)),
            // ⌘. = キャンセル → Escape
            47 => Some((53, false, false, false, false)),
            // ⌘Space = IME/言語切替(Win+Space)
            49 => Some((49, true, false, false, false)),
            _ => None,
        };
        return t;
    }
    if opt && !cmd && !ctrl {
        // ⌥←→ = 単語移動(Ctrl+←→)。Shift は透過(単語選択)
        let t = match kc {
            123 => Some((123, false, false, true, shift)),
            124 => Some((124, false, false, true, shift)),
            _ => None,
        };
        return t;
    }
    None
}

/// ゲームモード(Windows 側がカーソルの閉じ込め等を検知して要求)。true の間は
/// 絶対位置ではなく相対移動で送る(FPS・3D ソフトの視点回転のため)
static GAME_REL: AtomicBool = AtomicBool::new(false);
/// 画面ロックの連動(TSUNAGU_LOCK_SYNC=0 で無効)
static LOCK_SYNC: AtomicBool = AtomicBool::new(true);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

/// Secure Input(パスワード欄等でキー入力の横取りを OS が止める状態)の原因アプリ名。
/// この間はキーボードを Windows へ送れないため、切替時に知らせる(Deskflow と同じ配慮)
fn secure_input_app() -> Option<String> {
    if unsafe { IsSecureEventInputEnabled() } == 0 {
        return None;
    }
    let out = std::process::Command::new("ioreg").args(["-l", "-w", "0", "-d", "1"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let pid = text
        .split("kCGSSessionSecureInputPID\"=")
        .nth(1)
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .filter(|s| !s.is_empty());
    let name = pid.and_then(|pid| {
        let o = std::process::Command::new("ps").args(["-p", pid, "-o", "comm="]).output().ok()?;
        let n = String::from_utf8_lossy(&o.stdout).trim().rsplit('/').next()?.to_string();
        (!n.is_empty()).then_some(n)
    });
    Some(name.unwrap_or_else(|| "不明なアプリ".into()))
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGSessionCopyCurrentDictionary() -> *const core::ffi::c_void;
    fn CFDictionaryGetValue(d: *const core::ffi::c_void, key: *const core::ffi::c_void) -> *const core::ffi::c_void;
    fn CFBooleanGetValue(b: *const core::ffi::c_void) -> u8;
}

/// Mac の画面がロックされているか
fn screen_locked() -> bool {
    unsafe {
        let d = CGSessionCopyCurrentDictionary();
        if d.is_null() {
            return false;
        }
        let key = CFStringCreateWithCString(std::ptr::null_mut(), c"CGSSessionScreenIsLocked".as_ptr(), 0x0800_0100);
        let v = if key.is_null() { std::ptr::null() } else { CFDictionaryGetValue(d, key as *const _) };
        let locked = !v.is_null() && CFBooleanGetValue(v) != 0;
        if !key.is_null() {
            CFRelease(key as *mut _);
        }
        CFRelease(d as *mut _);
        locked
    }
}

/// 本線がいま繋がっている Windows のアドレス(大容量経路の接続先・経路診断に使う)
static PEER_IP: Mutex<Option<std::net::IpAddr>> = Mutex::new(None);

/// 接続経路の短い表示(メニューバー用)。LAN 内なら "LAN 直"、100.x なら "Tailscale"
pub fn route_label() -> &'static str {
    match *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(ip) if tsunagu_common::net::is_tailscale(ip) => "Tailscale",
        Some(_) => "LAN 直",
        None => "",
    }
}

/// Tailscale の経路状態(0=不明/Tailscale 外, 1=直結, 2=中継(DERP))。
/// 中継は遅延が数倍になるため、切り替わった時に知らせる
pub static TS_PATH: AtomicU8 = AtomicU8::new(0);

/// 本線を確実に切る(スロットから外すだけでは受信スレッドが読み出しを待ち続ける)
fn drop_stream(reason: &str) {
    let taken = STREAM_SLOT.get().and_then(|s| s.lock().unwrap_or_else(|e| e.into_inner()).take());
    if let Some(s) = taken {
        eprintln!("[conn] {reason}。接続を張り直します");
        s.shutdown();
    }
}

/// `tailscale status --json` から相手への経路が直結か中継かを調べる
fn tailscale_path(peer: std::net::IpAddr) -> Option<u8> {
    let out = ["tailscale", "/Applications/Tailscale.app/Contents/MacOS/Tailscale"]
        .iter()
        .find_map(|bin| std::process::Command::new(bin).args(["status", "--json"]).output().ok())
        .filter(|o| o.status.success())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let ip = peer.to_string();
    v["Peer"].as_object()?.values().find_map(|p| {
        let has = p["TailscaleIPs"].as_array()?.iter().any(|x| x.as_str() == Some(ip.as_str()));
        has.then(|| if p["CurAddr"].as_str().unwrap_or("").is_empty() { 2 } else { 1 })
    })
}

/// 大容量経路(ファイル・画像)。本線とは別の TCP 接続
static BULK_LINK: bulk::Link = bulk::Link::new();
static BULK: OnceLock<bulk::Endpoint> = OnceLock::new();

/// macOS の通知センターへ表示(接続/切断のユーザー可視化)。
/// osascript 経由で追加権限なしで出せる。失敗しても本体には影響しない。
/// osascript 起動に数百msかかるため別スレッドで発火し、accept/受信スレッドを
/// ブロックしない(レビュー Wave1 A-M9/D-F3)
fn notify(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    // osascript の文字列リテラルで特別な意味を持つ文字を先に無効化する(流用時に
    // ファイル名等が入っても構文エラーで通知だけ落ちる、という事故を防ぐ)
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "'");
    std::thread::spawn(move || {
        let out = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"{}\"",
                    esc(&body),
                    esc(&title)
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
/// 最近 Windows から受信したファイル(メニューの「最新の受信を開く」用。新しい順に最大 10 件)
pub(crate) static RECENT_RX: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

/// 受信履歴へ追加(新しい順で先頭に挿入し、10 件で打ち切る。バッチ内の元順は保つ)
pub(crate) fn push_recent_rx(paths: &[std::path::PathBuf]) {
    let mut r = RECENT_RX.lock().unwrap_or_else(|e| e.into_inner());
    for p in paths.iter().rev() {
        r.insert(0, p.clone());
    }
    r.truncate(10);
}
static TX: OnceLock<Sender<String>> = OnceLock::new();
static STREAM_SLOT: OnceLock<Arc<Mutex<Option<secure::Writer>>>> = OnceLock::new();
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
/// Mac 流ショートカット翻訳(TSUNAGU_MAC_KEYS=0 で無効)。タップ内で毎イベント
/// 設定を引かないよう起動時にキャッシュする
static MAC_KEYS: AtomicBool = AtomicBool::new(true);
/// 2本指横スワイプ→戻る/進む(TSUNAGU_SWIPE_NAV=0 で横ホイールのまま)
static SWIPE_NAV: AtomicBool = AtomicBool::new(true);
/// 現在押下中のマウスボタン(0=左,1=右,2=中)。切替時の持ち込み再送に使う
static BTN_DOWN: [AtomicBool; 3] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];

/// ファイル掴みドラッグ越境: ドラッグペーストボードにファイルがあり左ボタン押下中
/// (= Finder 等のファイルを掴んでいる)true。境界切替の許可と切替時の送信に使う
static DRAG_FILE: AtomicBool = AtomicBool::new(false);
/// 掴んでいるファイルのパス群(DRAG_FILE=true の間だけ有効)
static DRAG_FILES: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());
/// 合成 LeftMouseUp(kCGEventSourceUserData=41)に刻む識別マジック。
/// 掴み切替直後の Mac 側ドラッグ完結用投稿であり、Win へ転送してはならない
const SYNTH_UP_MAGIC: i64 = 0x54554e41475550; // "TSUNAGUP" 的な一意値

/// SIDE(0=右/1=左/2=上/3=下/4=右上/5=右下/6=左上/7=左下)を文字列表現で
pub fn side_name() -> &'static str {
    match SIDE.load(Ordering::Relaxed) {
        1 => "左",
        2 => "上",
        3 => "下",
        4 => "右上",
        5 => "右下",
        6 => "左上",
        7 => "左下",
        _ => "右",
    }
}

/// SIDE を「境界方向(0=右/1=左/2=上/3=下)」へ正規化(斜めは水平境界に寄せる)
fn side_dir() -> u8 {
    match SIDE.load(Ordering::Relaxed) {
        1 | 6 | 7 => 1,
        2 => 2,
        3 => 3,
        _ => 0,
    }
}

/// 接続する境界の範囲(境界に沿った位置の割合 0..1)。Mac の「ディスプレイ配置」
/// と同じ発想: Windows 画面が Mac の端のどの範囲に接しているか。
/// 斜め(4-7)は半分、それ以外は全域。配置エディタのドラッグで更に細かく決まる
pub static LAY_RANGE: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 1.0));

/// SIDE を設定し、Windows へ Cfg で同期する
pub fn set_side(v: u8) {
    let v = v.min(7);
    SIDE.store(v, Ordering::Relaxed);
    *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = match v {
        4 | 6 => (0.0, 0.5),
        5 | 7 => (0.5, 1.0),
        _ => (0.0, 1.0),
    };
    send_cfg();
    eprintln!("[cfg] Windows の位置 -> {}", side_name());
}

/// Windows と共有する設定を一括送信する(接続確立時と各設定の変更時)
pub fn send_cfg() {
    send_msg(&Msg::Cfg {
        cmd_alt: CMD_ALT.load(Ordering::Relaxed),
        spk_mute: SPK_MUTE.load(Ordering::Relaxed),
        side: SIDE.load(Ordering::Relaxed),
        clip: CLIP_SHARE.load(Ordering::Relaxed),
    });
}
static LAST_PONG_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 単調時計の ms。壁時計(SystemTime)は NTP 補正やスリープ復帰で飛び、
/// ダブルタップ判定・復帰ガード・pong 監視を誤動作させるため使わない。
/// 0 を「未設定」の意味で使う箇所があるため 1 秒のオフセットを足す
fn now_ms() -> u64 {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1_000
}

/// 復帰直後は右端判定を一定時間無効化する(再突入チャタリング防止)
static EDGE_GUARD_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 自前管理のカーソル位置(delta 積算)。タップ内での毎イベント CGEventCreate は
/// 負荷としてカクつきに効くため、積算+間欠同期(Deskflow の m_xCursor 方式)にする。
static CUR_POS: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// 接続相手(Windows)の画面サイズ(px)。hello で受信しスケール自動算出に使う
pub(crate) static WIN_SCREEN: Mutex<(f64, f64)> = Mutex::new((1920.0, 1080.0));
/// WIN モード中の Windows 仮想カーソル位置(px)。絶対位置送信モードで使う
static WIN_CUR: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// 前回 Windows モードを出た位置(0..1)。次回の切替はそこへ戻る(Deskflow 標準の体験)
static LAST_WIN_POS: Mutex<(f64, f64)> = Mutex::new((-1.0, -1.0));
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
/// 画面構成。ディスプレイの抜き差し・配置変更の通知で作り直す(Deskflow と同じく
/// CGDisplayRegisterReconfigurationCallback を使う。旧実装は起動時に一度だけ確定させ、
/// モニターを抜き差しすると境界がずれたままだった)
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geo {
    /// メイン画面の大きさ(絶対位置モードの速度換算・配置エディタ用)
    pub main_w: f64,
    pub main_h: f64,
    /// 全ディスプレイを合わせた領域の端(グローバル座標)
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    /// 各辺で Windows に接する出口ディスプレイの、辺に沿った範囲
    /// (左右の辺は y の範囲、上下の辺は x の範囲)
    exit: [(f64, f64); 4],
}

const GEO_DEFAULT: Geo = Geo {
    main_w: 2056.0,
    main_h: 1329.0,
    min_x: 0.0,
    max_x: 2056.0,
    min_y: 0.0,
    max_y: 1329.0,
    exit: [(0.0, 1329.0), (0.0, 1329.0), (0.0, 2056.0), (0.0, 2056.0)],
};

static GEO: Mutex<Option<Geo>> = Mutex::new(None);

pub(crate) fn geo() -> Geo {
    GEO.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(GEO_DEFAULT)
}

impl Geo {
    /// dir(0=右/1=左/2=上/3=下)の境界までの距離。左右に別モニターがある環境でも
    /// 「全体の端」で測るため、Mac 内のモニター間移動では切り替わらない
    fn gap(&self, dir: u8, x: f64, y: f64) -> f64 {
        match dir {
            1 => x - self.min_x,
            2 => y - self.min_y,
            3 => self.max_y - y,
            _ => self.max_x - x,
        }
    }
    fn exit_span(&self, dir: u8) -> (f64, f64) {
        self.exit[dir.min(3) as usize]
    }
    /// 境界に沿った位置の比率(0..1)。左右の辺は y、上下の辺は x で測る
    fn along_ratio(&self, dir: u8, x: f64, y: f64) -> f64 {
        let (lo, hi) = self.exit_span(dir);
        let v = if dir >= 2 { x } else { y };
        if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.5 }
    }
    /// 比率から境界に沿った座標へ(端から 20px は避ける)
    fn along_pos(&self, dir: u8, r: Option<f64>) -> f64 {
        let (lo, hi) = self.exit_span(dir);
        match r {
            Some(n) => (lo + n.clamp(0.0, 1.0) * (hi - lo)).clamp(lo + 20.0, (hi - 20.0).max(lo + 20.0)),
            None => (lo + hi) / 2.0,
        }
    }
    /// 境界から inset だけ内側の、境界に沿った比率 r の点
    fn inside_point(&self, dir: u8, inset: f64, r: Option<f64>) -> (f64, f64) {
        let a = self.along_pos(dir, r);
        match dir {
            1 => (self.min_x + inset, a),
            2 => (a, self.min_y + inset),
            3 => (a, self.max_y - inset),
            _ => (self.max_x - inset, a),
        }
    }
}

fn compute_geo() -> Geo {
    unsafe {
        let mb = CGDisplayBounds(CGMainDisplayID());
        let mut g = Geo {
            main_w: mb.size.w,
            main_h: mb.size.h,
            min_x: mb.origin.x,
            max_x: mb.origin.x + mb.size.w,
            min_y: mb.origin.y,
            max_y: mb.origin.y + mb.size.h,
            exit: [
                (mb.origin.y, mb.origin.y + mb.size.h),
                (mb.origin.y, mb.origin.y + mb.size.h),
                (mb.origin.x, mb.origin.x + mb.size.w),
                (mb.origin.x, mb.origin.x + mb.size.w),
            ],
        };
        let mut ids = [0u32; 16];
        let mut n = 0u32;
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..(n as usize).min(16)] {
                let b = CGDisplayBounds(*id);
                let (l, r, t, btm) = (b.origin.x, b.origin.x + b.size.w, b.origin.y, b.origin.y + b.size.h);
                // 同じ辺を複数のディスプレイが共有する(縦に並べた外部モニター等)場合は
                // 出口の範囲を合算する
                let widen = |e: &mut (f64, f64), lo: f64, hi: f64| *e = (e.0.min(lo), e.1.max(hi));
                if r > g.max_x {
                    g.max_x = r;
                    g.exit[0] = (t, btm);
                } else if r == g.max_x {
                    widen(&mut g.exit[0], t, btm);
                }
                if l < g.min_x {
                    g.min_x = l;
                    g.exit[1] = (t, btm);
                } else if l == g.min_x {
                    widen(&mut g.exit[1], t, btm);
                }
                if t < g.min_y {
                    g.min_y = t;
                    g.exit[2] = (l, r);
                } else if t == g.min_y {
                    widen(&mut g.exit[2], l, r);
                }
                if btm > g.max_y {
                    g.max_y = btm;
                    g.exit[3] = (l, r);
                } else if btm == g.max_y {
                    widen(&mut g.exit[3], l, r);
                }
            }
        }
        g
    }
}

fn refresh_geo() {
    let g = compute_geo();
    *GEO.lock().unwrap_or_else(|e| e.into_inner()) = Some(g);
    eprintln!(
        "[screen] main {}x{} / 全体 x={:.0}..{:.0} y={:.0}..{:.0}",
        g.main_w, g.main_h, g.min_x, g.max_x, g.min_y, g.max_y
    );
}

/// ディスプレイ構成の変更通知(メイン RunLoop 上で呼ばれる)。変更完了時だけ作り直す
unsafe extern "C" fn display_reconfigured(_display: u32, flags: u32, _user: *mut core::ffi::c_void) {
    const BEGIN: u32 = 1; // kCGDisplayBeginConfigurationFlag
    if flags & BEGIN == 0 {
        refresh_geo();
    }
}

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
    sync_clipboard_to_win(false);
    std::thread::spawn(|| {
        static LAST: AtomicU64 = AtomicU64::new(0);
        if let Some(app) = secure_input_app() {
            eprintln!("[secure] Secure Input 有効(原因: {app})。キーボードは Windows へ届きません");
            let now = now_ms();
            if now.saturating_sub(LAST.swap(now, Ordering::Relaxed)) > 60_000 {
                notify(
                    "tsunagu",
                    &format!("「{app}」がパスワード入力等の保護を有効にしているため、キーボードを Windows へ送れません。そのアプリの入力欄から離れてください"),
                );
            }
        }
    });
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
        // 全ディスプレイを合わせた領域の、Windows 側の辺に固定する(メイン画面の端に
        // 固定すると、その先にサブモニターがある環境で隠れカーソルが別画面へ飛ぶ)
        let dir = side_dir();
        let g = geo();
        let lock_x2 = live_cursor().map(|p| p.x).unwrap_or((g.min_x + g.max_x) / 2.0);
        let (lock_x, lock_y) = match dir {
            1 => (g.min_x + 2.0, lock_y),
            2 => (lock_x2, g.min_y + 2.0),
            3 => (lock_x2, g.max_y - 2.0),
            _ => (g.max_x - 2.0, lock_y),
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
    // Windows が自力で検知できない離脱(ホットキー/Mac 内完結の左端/ウォッチドッグ)でも
    // 押下中のキー・ボタンが Windows に残らないよう、必ず後片付けを依頼する
    send_msg(&Msg::Leave);
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
        let g = geo();
        let dir = side_dir();
        let taps = EDGE_TAPS.load(Ordering::Relaxed);
        let inset: f64 = if taps >= 2 { 60.0 } else { 150.0 };
        let (mut x, y) = g.inside_point(dir, inset, ny);
        if dir == 0 && taps < 2 {
            // 1回切替では MacBook 側へ十分退けて誤再突入を防ぐ
            x = x.min(g.main_w - 100.0);
        }
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
        // 本体キーボードの音量(F10-12)とメディア(F7-F9)キーは fn フラグ付きで届く。
        // Windows 側の音量・メディア操作として転送し、Mac 側の操作は握る。
        // fn 無しは F キーとしての使用のため素通り(誤転送防止)
        if event_type == EVT_KEY_DOWN
            && win_mode
            && CGEventGetFlags(event) & FLAG_FN != 0
        {
            let op = match kc {
                72 => Some(0u8), // F12 音量 up
                73 => Some(1),   // F11 音量 down
                74 => Some(2),   // F10 ミュート
                100 => Some(3),  // F7 前の曲へ
                101 => Some(4),  // F8 再生・一時停止
                103 => Some(5),  // F9 次の曲へ
                _ => None,
            };
            if let Some(op) = op {
                send_msg(&Msg::Vol { op });
                return std::ptr::null_mut(); // Mac 側の音量・再生変更を抑制
            }
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
            // キーリピートは down が連続で届くため、押下エッジ(未押下→押下)だけで
            // トグルする。押しっぱなしでの高速トグル暴発を防ぐ
            static HOTKEY_DOWN: AtomicBool = AtomicBool::new(false);
            if pressed && !HOTKEY_DOWN.swap(true, Ordering::Relaxed) && connected {
                do_toggle("hotkey");
            }
            if !pressed {
                HOTKEY_DOWN.store(false, Ordering::Relaxed);
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
        // ファイル掴み中(ドラッグペーストボードにファイル+左ボタン押下)は設定に
        // 依らず常に「掴んだまま境界越え」を許可する(掴んでいる意図が明確なため)
        let drag_ok = DRAG_SWITCH.load(Ordering::Relaxed) || DRAG_FILE.load(Ordering::Relaxed);
        if (matches!(event_type, EVT_MOUSE_MOVED)
            || (drag_ok
                && matches!(event_type, EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED)))
            && connected
            && !HOTKEY_ONLY.load(Ordering::Relaxed) // hotkey モードでは境界切替しない(ロック)
            && now_ms() >= EDGE_GUARD_UNTIL_MS.load(Ordering::Relaxed)
        {
            {
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
                let g = geo();
                let dir = side_dir();
                let (lay_lo, lay_hi) = *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner());
                // SIDE に応じた「切替境界までの距離」(小さいほど端に近い)。
                // Deskflow の links 相当+斜め(4-7)は境界の半分(上/下)でのみ接続
                let gap = |x: f64, y: f64| -> f64 {
                    let base = g.gap(dir, x, y);
                    if base > edge + 40.0 {
                        return base; // 境界から遠い=範囲判定不要
                    }
                    // 境界付近でのみ接続範囲(配置エディタ/斜め配置)の制限を適用する。
                    // 範囲は出口ディスプレイの辺に沿った比率で測る
                    let f = g.along_ratio(dir, x, y);
                    if f < lay_lo || f > lay_hi {
                        return f64::MAX; // Windows 画面が接している範囲外
                    }
                    base
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
                        && (px < g.min_x + corner || px > g.max_x - corner)
                        && (py < g.min_y + corner || py > g.max_y - corner)
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
                            // 配置番号(0-7)ではなく方向で判定する(旧実装は左上/左下配置で
                            // 右端へ弾き返していた)
                            let (bx, by) = match dir {
                                1 => (g.min_x + 15.0, loc.y),
                                2 => (loc.x, g.min_y + 15.0),
                                3 => (loc.x, g.max_y - 15.0),
                                _ => (g.max_x - 15.0, loc.y),
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
                    // ボタンを Windows 側で押し直す=「掴んだまま境界を越える」体験。
                    // 掴みドラッグ中は EVT_MOUSE_MOVED 由来の切替でも持ち込む
                    // (押下直後の軽い移動は MOVED として届くことがある=実測。
                    // 持ち込み漏れは Win 側のフォールバックを誘発する)
                    if event_type != EVT_MOUSE_MOVED || DRAG_FILE.load(Ordering::Relaxed) {
                        if drag_ok {
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
                    // ファイル掴み切替: 掴んだファイルを Windows へ流し、Mac 側の
                    // ドラッグは合成 LeftMouseUp で完結させる(Finder の宙吊り防止)。
                    // UP は tap コールバック内で post できないため別スレッド投稿。
                    // 投稿イベントは自分の HID タップを再通過する(定番の再帰問題)ため
                    // kCGEventSourceUserData にマジックを刻み、tap 側で識別して
                    // 「Mac へ素通し・Windows へは転送しない」処理をする(転送すると
                    // 押したままのユーザー意図に反して Win 側が離した扱いになる)
                    if DRAG_FILE.swap(false, Ordering::Relaxed) {
                        let files = std::mem::take(&mut *DRAG_FILES.lock().unwrap_or_else(|e| e.into_inner()));
                        if !files.is_empty() {
                            // ⌘C ポーリング経由の再送を指紋で抜く(通常は載らないが保険)
                            let key = mac_files_key(&files);
                            *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = key;
                            eprintln!("[file] 掴みドラッグ切替: {} 件を転送します", files.len());
                            send_files_to_win(files, true);
                        }
                        std::thread::spawn(|| {
                            std::thread::sleep(Duration::from_millis(60));
                            unsafe {
                                let pos = live_cursor().unwrap_or(CGPoint { x: 0.0, y: 0.0 });
                                let e = CGEventCreateMouseEvent(
                                    std::ptr::null_mut(),
                                    3, // kCGEventLeftMouseUp
                                    pos,
                                    0, // kCGMouseButtonLeft
                                );
                                if !e.is_null() {
                                    CGEventSetIntegerValueField(e as CGEventRef, 41, SYNTH_UP_MAGIC);
                                    CGEventPost(0 /* kCGHIDEventTap */, e);
                                    CFRelease(e as *mut core::ffi::c_void);
                                }
                            }
                        });
                    }
                    // 前回 Windows に出た位置があればそこへ戻し、なければ境界の対応高さへ。
                    // 高さは出口ディスプレイの y 範囲で正規化(反転なし: 画面上端同士が対応)
                    let (mut nx, mut ny) = *LAST_WIN_POS.lock().unwrap_or_else(|e| e.into_inner());
                    if nx < 0.0 {
                        // 初回: 越えた境界の対応位置から入る(Windows 側の反対の辺)
                        let r = g.along_ratio(dir, loc.x, loc.y);
                        (nx, ny) = match dir {
                            1 => (0.95, r),
                            2 => (r, 0.95),
                            3 => (r, 0.05),
                            _ => (0.05, r),
                        };
                    }
                    send_msg(&Msg::Warp { nx, ny });
                    eprintln!("[warp] -> win ({:.2},{:.2})", nx, ny);
                    // 絶対位置モードの仮想カーソルを Warp 先で初期化
                    {
                        let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                        *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner()) = (nx * ww, ny * wh);
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
                    54 | 55 => cmd,
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
            // Caps Lock は Mac では押すたびに flagsChanged が 1 回だけ来る(押下/解放の
            // 区別がない)。Windows はキーの押し離しでトグルするため 1 回を down+up に展開する
            if event_type == EVT_FLAGS_CHANGED && kc == 57 {
                for d in [true, false] {
                    send_msg(&Msg::Key { kc, down: d, ctrl, opt, cmd, shift, tr: false });
                }
                return std::ptr::null_mut();
            }
            // ---- Mac 流ショートカットの Windows 翻訳(指癖をそのまま通す) ----
            // 元キーは握りつぶし、翻訳先の Key を送る。修飾の対応:
            //   cmd→Win Ctrl / opt→Win Alt / ctrl→Win キー(既定マップ)
            // 注意: flagsChanged(mod キー単体)は翻訳しない。
            // 常時有効(マスト機能)。TSUNAGU_MAC_KEYS=0 でのみオフ
            if event_type != EVT_FLAGS_CHANGED && MAC_KEYS.load(Ordering::Relaxed) {
                // 翻訳先の修飾は「既定マップ(cmd→Ctrl / opt→Alt)」で解釈させる。
                // CMD_ALT=true でも翻訳の意味が変わらないよう、cmd/opt を差し替える
                let swap = crate::CMD_ALT.load(Ordering::Relaxed);
                let send = |kc2: u16, d: bool, c: bool, o: bool, m: bool, sh: bool| {
                    let (c2, o2, m2) = if swap { (c, m, o) } else { (c, o, m) };
                    send_msg(&Msg::Key { kc: kc2, down: d, ctrl: c2, opt: o2, cmd: m2, shift: sh, tr: true });
                };
                // fn+F11(Mac のデスクトップ表示)= Win+D(FN フラグは表の外)
                if kc == 103 && flags & FLAG_FN != 0 {
                    send(2, down, true, false, false, false); // D + ctrl フラグ(Win キー)
                    return std::ptr::null_mut();
                }
                if let Some((kc2, c, o, m, s)) = mac_shortcut_translation(kc, ctrl, opt, cmd, shift) {
                    send(kc2, down, c, o, m, s);
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
                if MOUSE_ABS_MODE.load(Ordering::Relaxed) && !GAME_REL.load(Ordering::Relaxed) {
                    // 絶対位置モード: Mac の加速済み delta に Windows 側の加速が
                    // 二重に乗るのを防ぎつつ、画面比率で見た目の移動距離を揃える
                    let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                    let (mw, mh) = { let g = geo(); (g.main_w, g.main_h) };
                    let (sx, sy) = (ww / mw, wh / mh); // 方向別スケール(改善B)
                    // 重要: WIN_CUR のガードをこのブロック内で必ず解放してから
                    // leave_win_mode_cursor_unlock を呼ぶ(内部で WIN_CUR を再ロック
                    // するため、保持したまま呼ぶと自己デッドロックでタップが固まる)
                    let (nx, ny, at_left) = {
                        let mut wc = WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
                        wc.0 = (wc.0 + dx * sc * sx).clamp(0.0, ww - 2.0);
                        wc.1 = (wc.1 + dy * sc * sy).clamp(0.0, wh - 2.0);
                        // Mac 側へ戻る辺は Windows の位置で決まる(右配置なら Windows の左端、
                        // 左配置なら右端、上配置なら下端、下配置なら上端)。旧実装は常に
                        // 左端で判定し、上下配置では Windows の左端に触れるだけで戻っていた
                        let at_edge = match side_dir() {
                            1 => wc.0 >= ww - 3.0,
                            2 => wc.1 >= wh - 3.0,
                            3 => wc.1 <= 2.0,
                            _ => wc.0 <= 2.0,
                        };
                        (
                            wc.0 / ww,
                            wc.1 / wh,
                            !HOTKEY_ONLY.load(Ordering::Relaxed) && event_type == EVT_MOUSE_MOVED && at_edge,
                        )
                    };
                    // 毎イベント送信(量子化スキップは低速時にステップ感が出るため廃止)
                    LAST_ABS_MS.store(now_ms(), Ordering::Relaxed);
                    DIAG_ABS_COUNT.fetch_add(1, Ordering::Relaxed);
                    send_msg(&Msg::MouseAbs { nx, ny });
                    // 左端到達はMac内完結で即復帰(Win往復のRTT分を削減)
                    // ドラッグ中は意図しない復帰をしない(ボタン操作中の境界越えのため)
                    if at_left {
                        WIN_MODE.store(false, Ordering::Relaxed);
                        DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
                        eprintln!("[mode] MAC (abs-edge)");
                        // 境界に沿った比率: 左右の辺は縦位置、上下の辺は横位置
                        leave_win_mode_cursor_unlock(Some(if side_dir() >= 2 { nx } else { ny }));
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
            // 自分が投稿した合成 LeftMouseUp(掴み切替直後の Mac 側ドラッグ完結用)。
            // Mac へ素通しし Win へは転送しない(ユーザーはまだ押している)
            if event_type == EVT_LEFT_UP
                && CGEventGetIntegerValueField(event, 41 /* kCGEventSourceUserData */)
                    == SYNTH_UP_MAGIC
            {
                BTN_DOWN[0].store(false, Ordering::Relaxed);
                return event;
            }
            let d = event_type == EVT_LEFT_DOWN;
            BTN_DOWN[0].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 0, down: d });
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
            let swipe_nav = SWIPE_NAV.load(Ordering::Relaxed);
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
const BUILD_ID: &str = "build-20260926-235319-51f0769";

fn main() {
    eprintln!("[info] tsunagu-mac {BUILD_ID}");
    let args: Vec<String> = std::env::args().collect();
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-setup") { gui::setup::probe_invitation(); return; }
    if args.iter().any(|a| a == "--preview-setup") { let _=gui::setup::first_run(true); return; }
    if args.iter().any(|a| a == "--preview-ui") {
        gui::UI_PREVIEW.store(true, Ordering::Relaxed);
        if gui::start() {
            gui::SHOW_AT_START.store(true, Ordering::Relaxed);
            unsafe { gui::run_app() };
        }
        return;
    }
    gui::restore_preferences();

    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);
    // 既存のenvを優先。新規利用者だけOS保護の接続キーと初回導入を使用する。
    let no_gui = args.iter().any(|a| a == "--no-gui")
        || envutil::get("TSUNAGU_NO_GUI").is_some_and(|v|v=="1");
    let mut registered_now = false;
    // トークンは双方向の共有鍵。ハンドシェイクの成否が oracle になるため短い
    // トークンは LAN 内の総当たりで破られる。128bit 相当(32 文字)を下限に
    let token = if let Some(t) = envutil::get("TSUNAGU_TOKEN").filter(|t| t.len() >= 32) { t } else if envutil::get("TSUNAGU_TOKEN").is_some_and(|t| !t.is_empty()) {
        eprintln!("[fatal] TSUNAGU_TOKEN が短すぎます(32 文字未満)。scripts/gen-token.sh で生成してください");
        std::process::exit(1);
    } else {
        match tsunagu_common::credentials::load() {
            Ok(Some(t))=>t,
            Ok(None) if !no_gui=>match gui::setup::first_run(false){Some(t)=>{registered_now=true;t},None=>return},
            Ok(None)=>{eprintln!("[setup] 接続キー未設定。GUIで初回登録を完了してください。");return;},
            Err(_)=>{if !no_gui{gui::setup::error("保存した接続キーを読み取れません。キーチェーンのアクセス許可を確認してください。");}eprintln!("[setup] credential store unavailable");return;},
        }
    };

    if !no_gui && !gui::setup::ensure_permission() { return; }
    refresh_geo();
    let (screen_w, screen_h) = { let g = geo(); (g.main_w, g.main_h) };
    unsafe { CGDisplayRegisterReconfigurationCallback(display_reconfigured, std::ptr::null_mut()) };
    unsafe {
        if let Some(loc) = live_cursor() {
            *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (loc.x, loc.y);
        }
    }
    if let Some(d) = envutil::get("TSUNAGU_SCROLL_DIV").and_then(|v| v.parse::<f64>().ok()) {
        if d > 0.0 {
            set_scroll_div(d);
        }
    }
    if let Some(m) = envutil::get("TSUNAGU_MOUSE_SCALE").and_then(|v| v.parse::<f64>().ok()) {
        if m > 0.0 {
            set_mouse_scale(m);
        }
    }
    if let Some(e) = envutil::get("TSUNAGU_EDGE_PX").and_then(|v| v.parse::<f64>().ok()) {
        if e >= 0.0 && e < 100.0 {
            set_edge_px(e);
        }
    }
    if let Some(m) = envutil::get("TSUNAGU_MOUSE_MODE") {
        if m.eq_ignore_ascii_case("rel") {
            MOUSE_ABS_MODE.store(false, Ordering::Relaxed);
        }
    }
    if let Some(m) = envutil::get("TSUNAGU_SWITCH_MODE") {
        if m.eq_ignore_ascii_case("hotkey") {
            HOTKEY_ONLY.store(true, Ordering::Relaxed);
        }
    }
    if let Some(t) = envutil::get("TSUNAGU_EDGE_TAPS").and_then(|v| v.parse::<u32>().ok()) {
        if t >= 1 && t <= 3 {
            EDGE_TAPS.store(t, Ordering::Relaxed);
        }
    }
    if let Some(k) = envutil::get("TSUNAGU_HOTKEY_KC").and_then(|v| v.parse::<i64>().ok()) {
        if (1..=127).contains(&k) {
            HOTKEY_KC.store(k, Ordering::Relaxed);
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
        Some("upright") => SIDE.store(4, Ordering::Relaxed),
        Some("lowright") | Some("downright") => SIDE.store(5, Ordering::Relaxed),
        Some("upleft") => SIDE.store(6, Ordering::Relaxed),
        Some("lowleft") | Some("downleft") => SIDE.store(7, Ordering::Relaxed),
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
    if envutil::get("TSUNAGU_MAC_KEYS").as_deref() == Some("0") {
        MAC_KEYS.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_SWIPE_NAV").as_deref() == Some("0") {
        SWIPE_NAV.store(false, Ordering::Relaxed);
    }
    if envutil::get("TSUNAGU_LOCK_SYNC").as_deref() == Some("0") {
        LOCK_SYNC.store(false, Ordering::Relaxed);
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
        "[info] screen {screen_w}x{screen_h}. listening on :{port} (server mode). scroll_div={} mouse_scale={} edge_px={} clip_max={}KB mouse_mode={} switch_mode={} hotkey_kc={} edge_taps={}",
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
    let slot: Arc<Mutex<Option<secure::Writer>>> = Arc::new(Mutex::new(None));
    let _ = STREAM_SLOT.set(slot.clone());

    // 単一の送信スレッド(チャネル→ストリーム差し替え方式)
    std::thread::spawn(move || {
        use std::io::Write;
        let mut ping_at = std::time::Instant::now();
        // スリープ復帰の検知: 単調時計はスリープ中に進まないため、壁時計との差が
        // 開いたら眠っていたと分かる。眠っている間に相手側の接続は切れているのが
        // 普通で、単調時計基準の pong 監視では気づけないため、即座に張り直す
        let mut wall = std::time::SystemTime::now();
        let mut mono = std::time::Instant::now();
        loop {
            let (wall_now, mono_now) = (std::time::SystemTime::now(), std::time::Instant::now());
            let slept = wall_now
                .duration_since(wall)
                .unwrap_or_default()
                .saturating_sub(mono_now.duration_since(mono));
            (wall, mono) = (wall_now, mono_now);
            if slept > Duration::from_secs(3) {
                drop_stream(&format!("スリープ復帰を検知({}秒)", slept.as_secs()));
                BULK_LINK.clear();
            }
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
                            if let Some(s) = guard.take() {
                                s.shutdown();
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
            if ping_at.elapsed() >= Duration::from_secs(3) {
                ping_at = std::time::Instant::now();
                // 10 秒 pong が無ければ実質切断扱いでストリームを外す
                // (TCP が生きていても相手プロセスが固まった場合を拾う)
                if now_ms().saturating_sub(LAST_PONG_MS.load(Ordering::Relaxed)) > 10_000 {
                    drop_stream("pong が 10 秒途絶");
                    continue;
                }
                let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
                if let Some(s) = guard.as_mut() {
                    if s.write_all(encode(&Msg::Ping { ts: now_ms() }).as_bytes()).and_then(|_| s.flush()).is_err() {
                        if let Some(s) = guard.take() {
                            s.shutdown();
                        }
                    }
                }
            }
        }
    });

    // 画面ロックの連動(1 秒毎)。Mac がロックされたら Windows 操作中でも制御を
    // Mac へ戻し(ロック中の入力を Windows へ流さない)、Windows もロックする
    std::thread::spawn(|| {
        let mut was = false;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = screen_locked();
            if now && !was {
                eprintln!("[lock] Mac がロックされました");
                if WIN_MODE.swap(false, Ordering::Relaxed) {
                    leave_win_mode_cursor_unlock(None);
                }
                if LOCK_SYNC.load(Ordering::Relaxed) && CONNECTED.load(Ordering::Relaxed) {
                    send_msg(&Msg::Lock);
                }
            }
            was = now;
        }
    });

    // Tailscale 経路の診断(30 秒毎)。直結から中継(DERP)へ落ちると遅延が数倍になり
    // 「カクつき」の原因になるため、切り替わった時だけ通知する
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(30));
        let peer = *PEER_IP.lock().unwrap_or_else(|e| e.into_inner());
        let Some(peer) = peer.filter(|p| CONNECTED.load(Ordering::Relaxed) && tsunagu_common::net::is_tailscale(*p)) else {
            continue;
        };
        let Some(now) = tailscale_path(peer) else { continue };
        let before = TS_PATH.swap(now, Ordering::Relaxed);
        if before != now {
            eprintln!("[net] Tailscale 経路: {}", if now == 1 { "直結" } else { "中継(DERP)" });
            if now == 2 {
                notify("tsunagu", "Windows との通信が中継経由になりました(遅延が増えます)。同じネットワークか有線直結を推奨します");
            }
        }
    });

    // 音声受信・再生(Windows→Mac。独立ポート 24901。TSUNAGU_AUDIO=0 で無効)
    if envutil::get("TSUNAGU_AUDIO").as_deref() != Some("0") {
        // 音声は本線ポートからの差分 +1(24900→24901)
        audio::start(token.clone(), port + 1);
    }

    // 接続方向: 既定は Mac=サーバ(本環境のAP隔離対策)。TSUNAGU_ROLE=client +
    // TSUNAGU_HOST(または --host)で Mac=クライアント(通常ネットワークの配布先向け。
    // その場合は Windows 側を TSUNAGU_ROLE=server で待ち受ける)
    let client_role = envutil::get("TSUNAGU_ROLE").as_deref() == Some("client");
    let bulk_ep: &'static bulk::Endpoint = BULK.get_or_init(|| bulk::Endpoint {
        link: &BULK_LINK,
        token: token.clone(),
        dir: std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join("Downloads/Tsunagu"),
        on_event: mac_on_bulk,
        log: |s| eprintln!("{s}"),
    });
    if client_role {
        let host = args
            .iter()
            .position(|a| a == "--host")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .or_else(|| envutil::get("TSUNAGU_HOST"));
        eprintln!("[info] client mode: connecting to {}", host.as_deref().unwrap_or("LAN から自動検出"));
        std::thread::spawn(move || {
            bulk::connect_loop(
                bulk_ep,
                || {
                    let ip = *PEER_IP.lock().unwrap_or_else(|e| e.into_inner());
                    ip.map(|ip| std::net::SocketAddr::new(ip, port + bulk::PORT_OFFSET))
                },
                || CONNECTED.load(Ordering::Relaxed),
            )
        });
        std::thread::spawn(move || client_thread(host, port, token, screen_w, screen_h));
    } else {
        let bind = envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        // LAN 自動発見への応答(ブロードキャストを受けるため常に 0.0.0.0 で待つ)
        let tk = token.clone();
        std::thread::spawn(move || {
            if let Err(e) = tsunagu_common::discover::respond("0.0.0.0", port + tsunagu_common::discover::PORT_OFFSET, &tk, tsunagu_common::net::is_allowed) {
                eprintln!("[disc] 発見応答の待受に失敗: {e}(自動発見が使えません)");
            }
        });
        std::thread::spawn(move || {
            bulk::serve(bulk_ep, &bind, port + bulk::PORT_OFFSET, tsunagu_common::net::is_allowed)
        });
        std::thread::spawn(move || server_thread(port, token, screen_w, screen_h));
    }

    // 診断モード: 1秒ごとにモード/受信・送信カウント/実カーソル位置を記録
    if args.iter().any(|a| a == "--diag") {
        DIAG_ENABLED.store(true, Ordering::Relaxed);
        // 検証用の切替指示(--diag 起動時のみ): 一時ディレクトリへ "toggle" と書くと
        // 画面を切り替える。クリップボードは画面を移る時にだけ同期するため、自動検証で
        // 「移る」操作を起こす手段が要る(verify.sh が使う)。/tmp 共有領域だとローカルの
        // 他ユーザーから切替できるため temp_dir()=$TMPDIR(ユーザー固有)へ置く
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(200));
            let p = std::env::temp_dir().join("tsunagu-cmd");
            if let Ok(cmd) = std::fs::read_to_string(&p) {
                let _ = std::fs::remove_file(&p);
                if cmd.trim() == "toggle" {
                    do_toggle("verify");
                }
            }
        });
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

    // 起動時点のクリップボードは送らない(以後の変化だけを切替時に同期する)
    LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);

    // ファイル掴み検出(Mac→Windows の掴みドラッグ越境): ドラッグ用ペーストボードに
    // ファイル参照が載っており左ボタン押下中 = Finder 等でファイルを掴んでいる。
    // 「押下開始時点からの changeCount 変化」を条件にする(ドラッグ開始で初めて
    // ファイルが載るため)。押下前に載っていた残骸(キャンセル済みドラッグ等は
    // クリアされないことがある=実測)での誤検出を構造的に防ぐ
    std::thread::spawn(|| {
        // None=非押下。Some(cnt)=押下中の基準値(押下開始時点の changeCount)
        let mut baseline: Option<isize> = None;
        loop {
            std::thread::sleep(Duration::from_millis(120));
            if !BTN_DOWN[0].load(Ordering::Relaxed) {
                if DRAG_FILE.swap(false, Ordering::Relaxed) {
                    eprintln!("[file] 掴み終了(ボタンを離した)");
                }
                baseline = None;
                continue;
            }
            if DRAG_FILE.load(Ordering::Relaxed) {
                continue; // 掴み確立済み(切替は tap 側が担う)
            }
            with_pool(|| unsafe {
                let pb = drag_pasteboard();
                if pb.is_null() {
                    return;
                }
                let cnt = msg0_isize(pb, sel_registerName(c"changeCount".as_ptr()));
                match baseline {
                    None => {
                        // 押下開始直後: 現在値を基準に取る(残骸では検出しない)
                        baseline = Some(cnt);
                    }
                    Some(b) if cnt != b => {
                        baseline = Some(cnt);
                        if let Some(files) = pb_files(pb) {
                            DRAG_FILES.lock().unwrap_or_else(|e| e.into_inner()).clone_from(&files);
                            DRAG_FILE.store(true, Ordering::Relaxed);
                            eprintln!("[file] ファイル掴み検出: {} 件", files.len());
                        }
                    }
                    _ => {}
                }
            });
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
            if WIN_MODE.load(Ordering::Relaxed)
                && MOUSE_ABS_MODE.load(Ordering::Relaxed)
                && !GAME_REL.load(Ordering::Relaxed)
            {
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
        if registered_now || args.iter().any(|a| a == "--show-prefs") {
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
fn session_receive_loop(reader: &mut std::io::BufReader<secure::Reader>) {
    use std::io::BufRead;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            // 切断理由(EOF 以外)を残す: pong 途絶の張替えか read エラーかの区別が
            // 「なぜ切れたか」の追跡に必要。不正行(decode 失敗)は既存どおり無視
            Err(e) => {
                eprintln!("[conn] read error: {e}(kind={:?})", e.kind());
                break;
            }
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
                                with_pool(|| unsafe { mac_set_clipboard(&text) });
                                LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
                                eprintln!("[clip] win->mac {} bytes", text.len());
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
                        Msg::Ping { ts } => send_msg(&Msg::Pong { ts }),
                        Msg::Rel { on } => {
                            if GAME_REL.swap(on, Ordering::Relaxed) != on {
                                eprintln!("[game] ゲームモード -> {}", if on { "ON(相対移動)" } else { "OFF(絶対位置)" });
                            }
                        }
                        Msg::Screen { w, h } if w > 0 && h > 0 => {
                            *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = (w as f64, h as f64);
                            eprintln!("[info] win screen changed {w}x{h}");
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
    // 旧セッションの RTT が再接続直後に「前の接続の値」として表示されるのを防ぐ
    RTT_MS.store(0, Ordering::Relaxed);
    BULK_LINK.clear();
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
    // 待受アドレス: 既定は全インターフェース(LAN 直を受け入れる)。
    // 防御は is_allowed(接続元絞り)+ Noise ハンドシェイクが担う
    let bind_ip = envutil::get("TSUNAGU_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[fatal] listen {bind_ip}:{port} failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("[info] server mode. listening on {bind_ip}:{port}");
    let mut accept_errs: u32 = 0;
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(x) => {
                accept_errs = 0;
                x
            }
            Err(e) => {
                // fd 枯渇等で失敗が続くと 500ms 毎の洪水になるため、最初と
                // その後 20 回毎(≒10 秒)だけ出す
                accept_errs += 1;
                if accept_errs == 1 || accept_errs % 20 == 0 {
                    eprintln!("[conn] accept error: {e}(連続 {accept_errs} 回目)");
                }
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        eprintln!("[conn] accepted from {peer}");
        // 接続元の制限(LAN・有線直結・Tailscale のみ)。認証は暗号化ハンドシェイクで行う
        if !tsunagu_common::net::is_allowed(peer.ip()) {
            eprintln!("[conn] rejected: {peer} は許可範囲外です(TSUNAGU_ALLOW_ANY=1 で許可)");
            continue;
        }
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(Duration::from_secs(12))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
        let (r, w) = match secure::accept(stream, &token, b"tsunagu-main") {
            Ok(x) => x,
            Err(e) => {
                eprintln!("[conn] 暗号化ハンドシェイク失敗 ({peer}): {e}(トークン不一致の可能性)");
                continue;
            }
        };
        // hello を待つ(検証して hello_ok を返す)
        let mut reader = std::io::BufReader::new(r);
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
            Some(Msg::Hello { ver, w, h, .. }) if compatible(ver) => {
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
        LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
        {
            let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(w);
        }
        *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) = Some(peer.ip());
        // hello_ok 送信は送信スレッド経由で確実に
        send_msg(&Msg::HelloOk { name: "macbook".into(), w: screen_w as i32, h: screen_h as i32 });
        // 現在の ⌘キー設定を同期(切断中に切り替えていた場合の整合)
        send_cfg();
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
    s: TcpStream,
    token: &str,
    screen_w: f64,
    screen_h: f64,
) -> Result<(), String> {
    use std::io::{BufRead, Read, Write};
    s.set_read_timeout(Some(Duration::from_secs(12))).ok();
    eprintln!("[conn] connected");
    s.set_nodelay(true).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let (r, mut hw) = secure::connect(s, token, b"tsunagu-main")
        .map_err(|e| format!("暗号化ハンドシェイク失敗: {e}(トークン不一致の可能性)"))?;
    // hello(自画面サイズを相手へ伝える。相手は hello_ok で自画面を返す)
    let hello = encode(&Msg::Hello {
        ver: VERSION,
        name: "macbook".into(),
        token: String::new(),
        w: screen_w as i32,
        h: screen_h as i32,
    });
    hw.write_all(hello.as_bytes())
        .and_then(|_| hw.flush())
        .map_err(|_| "hello send failed".to_string())?;
    // hello_ok を待つ(行長制限付き)
    let mut reader = std::io::BufReader::new(r);
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
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    {
        let mut guard = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(hw);
    }
    CONNECTED.store(true, Ordering::Relaxed);
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    // 現在の ⌘キー設定を同期(クライアントモードの確立時)
    send_cfg();
    eprintln!("[conn] established");
    notify("tsunagu", "Windows に接続しました");
    session_receive_loop(&mut reader);
    on_disconnect();
    Ok(())
}

/// 接続モード(TSUNAGU_ROLE=client): Windows(サーバ)へ接続し続ける
fn client_thread(host: Option<String>, port: u16, token: String, screen_w: f64, screen_h: f64) {
    let mut backoff = 500u64;
    loop {
        let addrs = tsunagu_common::connect::resolve(host.as_deref(), port, &token);
        if addrs.is_empty() {
            // 候補が空なら first_reachable を呼ばない(Windows 側と同じ: 3.5 秒の空待ち防止)
            eprintln!("[conn] 接続先が見つかりません");
            std::thread::sleep(Duration::from_millis(backoff));
            backoff = (backoff * 2).min(3000);
            continue;
        }
        let t0 = std::time::Instant::now();
        match tsunagu_common::connect::first_reachable(&addrs, Duration::from_secs(3)) {
            Some((s, a)) => {
                eprintln!("[conn] connected ({a}) in {}ms", t0.elapsed().as_millis());
                *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) = Some(a.ip());
                match client_attempt(s, &token, screen_w, screen_h) {
                    Ok(()) => backoff = 500,
                    Err(e) => eprintln!("[conn] {e}"),
                }
            }
            None => eprintln!("[conn] どの接続先にも繋がりません: {:?}", host),
        }
        std::thread::sleep(Duration::from_millis(backoff));
        backoff = (backoff * 2).min(3000);
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::mac_shortcut_translation as tr;

    /// 翻訳対応の固定(タップ実装と表の乖離を防ぐ)。kc は Mac keycode
    #[test]
    fn shortcut_table_matches_spec() {
        // ⌘← = Home(Shift 透過: ⌘⇧← は Shift+Home)
        assert_eq!(tr(123, false, false, true, false), Some((115, false, false, false, false)));
        assert_eq!(tr(123, false, false, true, true), Some((115, false, false, false, true)));
        // ⌘→ = End、⌘↑ = Ctrl+Home、⌘↓ = Ctrl+End
        assert_eq!(tr(124, false, false, true, false), Some((119, false, false, false, false)));
        assert_eq!(tr(126, false, false, true, false), Some((115, false, false, true, false)));
        assert_eq!(tr(125, false, false, true, false), Some((119, false, false, true, false)));
        // ⌘M / ⌘H = Win+Down(最小化)
        assert_eq!(tr(43, false, false, true, false), Some((125, true, false, false, false)));
        assert_eq!(tr(4, false, false, true, false), Some((125, true, false, false, false)));
        // ⌘] = Ctrl+Tab / ⌘[ = Ctrl+Shift+Tab(⌘⇧[ も前タブ)
        assert_eq!(tr(30, false, false, true, false), Some((48, false, false, true, false)));
        assert_eq!(tr(33, false, false, true, false), Some((48, false, false, true, true)));
        assert_eq!(tr(33, false, false, true, true), Some((48, false, false, true, true)));
        // ⌘⇧4 / ⌘⇧3 = Win+Shift+S。⇧無しの ⌘4 は素の F4 相当へ翻訳しない
        assert_eq!(tr(21, false, false, true, true), Some((1, true, false, false, true)));
        assert_eq!(tr(18, false, false, true, true), Some((1, true, false, false, true)));
        assert_eq!(tr(21, false, false, true, false), None);
        // ⌘⇧5 = Win+Alt+R
        assert_eq!(tr(23, false, false, true, true), Some((15, false, true, false, false)));
        // ⌘Q = Alt+F4
        assert_eq!(tr(12, false, false, true, false), Some((118, false, true, false, false)));
        // ⌘G = F3 / ⌘⇧G = Shift+F3
        assert_eq!(tr(32, false, false, true, false), Some((99, false, false, false, false)));
        assert_eq!(tr(32, false, false, true, true), Some((99, false, false, false, true)));
        // ⌘. = Esc
        assert_eq!(tr(47, false, false, true, false), Some((53, false, false, false, false)));
        // ⌘Space = Win+Space
        assert_eq!(tr(49, false, false, true, false), Some((49, true, false, false, false)));
        // ⌘⌥Esc = Ctrl+Shift+Esc
        assert_eq!(tr(53, false, true, true, false), Some((53, false, false, true, true)));
        // ⌘Ctrl+Q = Win+L(⌘Q より優先)
        assert_eq!(tr(12, true, false, true, false), Some((37, true, false, false, false)));
        // ⌥← = Ctrl+←(単語移動)
        assert_eq!(tr(123, false, true, false, false), Some((123, false, false, true, false)));
        // 翻訳対象外: 素の A、⌘A(そのまま渡る)、⌥A
        assert_eq!(tr(0, false, false, false, false), None);
        assert_eq!(tr(0, false, false, true, false), None);
        assert_eq!(tr(0, false, true, false, false), None);
    }
}

#[cfg(test)]
mod recent_tests {
    use super::{push_recent_rx, RECENT_RX};
    use std::path::PathBuf;

    /// 履歴は新しい順・10 件で打ち切り・バッチ内の元順は保存
    #[test]
    fn recent_rx_keeps_newest_first_and_caps_at_10() {
        let mk = |i: usize| PathBuf::from(format!("/tmp/tsunagu-recent-{i}.txt"));
        {
            let mut r = RECENT_RX.lock().unwrap_or_else(|e| e.into_inner());
            r.clear();
        }
        push_recent_rx(&[mk(0), mk(1)]); // 古いバッチ(2 件)
        for i in 2..12 {
            push_recent_rx(&[mk(i)]);
        }
        {
            let r = RECENT_RX.lock().unwrap_or_else(|e| e.into_inner());
            assert_eq!(r.len(), 10, "10 件で打ち切る: {:?}", r);
            assert_eq!(r[0], mk(11), "最新が先頭");
            assert_eq!(r[9], mk(2), "最古は 2(0/1 は押し出される");
        }
        // バッチ内の元順(バッチ先頭が先頭に来る)。guard は push 前に必ず解放する
        // (push_recent_rx が同じ Mutex を取り、保持したまま呼ぶと自己デッドロック)
        {
            RECENT_RX.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
        push_recent_rx(&[mk(20), mk(21)]);
        let r = RECENT_RX.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(r[0], mk(20));
        assert_eq!(r[1], mk(21));
    }
}

#[cfg(test)]
mod geo_tests {
    use super::Geo;

    /// MacBook(0..2056) の左に 1920 幅、右に 2560 幅のモニターがある構成
    fn three_screens() -> Geo {
        Geo {
            main_w: 2056.0,
            main_h: 1329.0,
            min_x: -1920.0,
            max_x: 4616.0,
            min_y: -200.0,
            max_y: 1329.0,
            exit: [(-200.0, 1240.0), (0.0, 1080.0), (-1920.0, 0.0), (0.0, 2056.0)],
        }
    }

    #[test]
    fn edges_are_measured_on_the_whole_desktop() {
        let g = three_screens();
        // MacBook の左端(x=0)は左モニターへの通り道なので、左配置でも切替境界ではない
        assert!(g.gap(1, 0.0, 500.0) > 1000.0);
        assert_eq!(g.gap(1, -1920.0, 500.0), 0.0);
        // MacBook の右端も右モニターへの通り道
        assert!(g.gap(0, 2056.0, 500.0) > 1000.0);
        assert_eq!(g.gap(0, 4616.0, 500.0), 0.0);
    }

    #[test]
    fn return_point_is_inside_the_exit_display() {
        let g = three_screens();
        let (x, y) = g.inside_point(1, 60.0, Some(0.5));
        assert_eq!(x, -1860.0);
        assert_eq!(y, 540.0);
        let (x, y) = g.inside_point(0, 60.0, Some(0.0));
        assert_eq!(x, 4556.0);
        assert_eq!(y, -180.0); // 端から 20px は避ける
        assert_eq!(g.along_ratio(0, 4616.0, 520.0), 0.5);
        // 上下の辺は横位置で測る
        assert_eq!(g.along_ratio(3, 1028.0, 1329.0), 0.5);
    }
}
