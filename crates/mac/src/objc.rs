use crate::*;

// ---------- ObjC ランタイム直宣言(NSPasteboard 操作) ----------
// NSPasteboard は AppKit のクラスのため、リンクしてクラス登録を発生させる必要がある
#[link(name = "AppKit", kind = "framework")]
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    pub(crate) fn objc_getClass(name: *const core::ffi::c_char) -> *mut core::ffi::c_void;
    pub(crate) fn sel_registerName(name: *const core::ffi::c_char) -> *mut core::ffi::c_void;
    pub(crate) fn objc_msgSend(
        receiver: *mut core::ffi::c_void,
        sel: *mut core::ffi::c_void,
        ...
    ) -> *mut core::ffi::c_void;
    /// ファイル URL のみを読む指定キー(readObjectsForClasses:options: 用)
    pub(crate) static NSPasteboardURLReadingFileURLsOnlyKey: *mut core::ffi::c_void;
    pub(crate) fn objc_autoreleasePoolPush() -> *mut core::ffi::c_void;
    pub(crate) fn objc_autoreleasePoolPop(pool: *mut core::ffi::c_void);
}

/// バックグラウンドスレッドで ObjC の一時オブジェクトを扱う区間を囲む。
/// メインスレッド以外には autorelease pool が無く、nsstring 等が解放されずに
/// 溜まり続ける(120ms 周期の監視で確定的にリークしていた。レビュー B-P1-1)
pub(crate) fn with_pool<T>(f: impl FnOnce() -> T) -> T {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let r = f();
        objc_autoreleasePoolPop(pool);
        r
    }
}

/// パスワードマネージャ等が「記録・共有しないで」と印を付けたコピーか
/// (nspasteboard.org の慣行。1Password/Bitwarden/キーチェーン等が付ける)
pub(crate) unsafe fn pb_is_concealed(pb: ID) -> bool {
    let types = msg0(pb, sel_registerName(c"types".as_ptr()));
    if types.is_null() {
        return false;
    }
    [
        "org.nspasteboard.ConcealedType",
        "org.nspasteboard.TransientType",
        "com.agilebits.onepassword",
    ]
    .iter()
    .any(|t| {
        let f: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
            std::mem::transmute(objc_msgSend as *const () as usize);
        f(
            types,
            sel_registerName(c"containsObject:".as_ptr()),
            nsstring(t),
        ) != 0
    })
}
/// Mac の最前面アプリ名(NSWorkspace。権限不要)
pub(crate) unsafe fn mac_frontmost_app_name() -> Option<String> {
    let ws = msg0(
        objc_getClass(c"NSWorkspace".as_ptr()),
        sel_registerName(c"sharedWorkspace".as_ptr()),
    );
    if ws.is_null() {
        return None;
    }
    let app = msg0(ws, sel_registerName(c"frontmostApplication".as_ptr()));
    if app.is_null() {
        return None;
    }
    let name = msg0(app, sel_registerName(c"localizedName".as_ptr()));
    if name.is_null() {
        return None;
    }
    let utf8 = msg0_cstr(name, sel_registerName(c"UTF8String".as_ptr()));
    if utf8.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned(),
    )
}
pub(crate) type ID = *mut core::ffi::c_void;
pub(crate) type SEL = *mut core::ffi::c_void;

// objc_msgSend は可変引数宣言のまま呼ぶと引数の渡りが壊れる(SIGSEGV実績あり)ため、
// 呼び出しシグネチャごとに transmute した固定シグネチャで呼ぶ(rust-objc 界の定番方式)
pub(crate) unsafe fn msg0(target: ID, sel: SEL) -> ID {
    let f: unsafe extern "C" fn(ID, SEL) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}
pub(crate) unsafe fn msg1_id(target: ID, sel: SEL, a: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, a)
}
pub(crate) unsafe fn msg1_cstr(target: ID, sel: SEL, p: *const core::ffi::c_char) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, *const core::ffi::c_char) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, p)
}
pub(crate) unsafe fn msg2_bool(target: ID, sel: SEL, a: ID, b: ID) -> u8 {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel, a, b)
}
pub(crate) unsafe fn msg0_isize(target: ID, sel: SEL) -> isize {
    let f: unsafe extern "C" fn(ID, SEL) -> isize =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}
pub(crate) unsafe fn msg0_cstr(target: ID, sel: SEL) -> *const core::ffi::c_char {
    let f: unsafe extern "C" fn(ID, SEL) -> *const core::ffi::c_char =
        std::mem::transmute(objc_msgSend as *const () as usize);
    f(target, sel)
}

pub(crate) unsafe fn nsstring(s: &str) -> ID {
    let mut buf = s.as_bytes().to_vec();
    buf.push(0);
    msg1_cstr(
        objc_getClass(c"NSString".as_ptr()),
        sel_registerName(c"stringWithUTF8String:".as_ptr()),
        buf.as_ptr() as *const core::ffi::c_char,
    )
}

pub(crate) unsafe fn general_pasteboard() -> ID {
    msg0(
        objc_getClass(c"NSPasteboard".as_ptr()),
        sel_registerName(c"generalPasteboard".as_ptr()),
    )
}

/// ドラッグ用ペーストボード(NSPasteboardNameDrag = "Apple CFPasteboard drag")。
/// Finder 等のファイルドラッグ中のみファイル URL が載る。セレクタは
/// pasteboardWithName:(クラスメソッド)。NSPasteboardNameDrag 定数の実体文字列を直接渡す
pub(crate) unsafe fn drag_pasteboard() -> ID {
    let name = nsstring("Apple CFPasteboard drag");
    msg1_id(
        objc_getClass(c"NSPasteboard".as_ptr()),
        sel_registerName(c"pasteboardWithName:".as_ptr()),
        name,
    )
}

/// NSPasteboard へテキストを書き込む(Windows→Mac 受信時)
pub(crate) unsafe fn mac_set_clipboard(text: &str) -> bool {
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
        eprintln!(
            "[clip] set failed: str={} uti={}",
            !s.is_null(),
            !uti.is_null()
        );
    }
    ok != 0
}

/// DIB(Windows 画像)に BMP ファイルヘッダを付与して BMP データへ変換する。
/// NSBitmapImageRep は BMP ファイル形式を受け付けるため
pub(crate) fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    if dib.len() < 40 {
        return Vec::new();
    }
    // BITMAPINFOHEADER: biSize は先頭4バイト(旧実装は誤って biWidth を読んでいた)
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let bpp = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let comp = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
    let clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize;
    let palette = if clr_used > 0 {
        clr_used * 4
    } else if bpp == 8 {
        1024
    } else {
        0
    };
    // BI_BITFIELDS(biCompression=3) が biSize=40 で来た場合だけ、ヘッダ直後に
    // 12 バイトのカラーマスクが付く(Windows のクリップボードが実際に出す形。
    // これを飛ばさないと画像全体が 3px ずれる。biSize>=52 はマスク込みのサイズ)
    let masks = if comp == 3 && header_size == 40 {
        12
    } else {
        0
    };
    let off = 14 + header_size + palette + masks;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(off as u32).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// DIB(Windows 画像)を PNG へ変換する(Android のクリップボードへ渡す時の形式。
/// 端末側のアプリは image/png の扱いが最も確実なため)。
/// BI_RGB(0)と BI_BITFIELDS(3、Mac のクリップボードが実出する形式。標準の
/// BGRX 並びとみなす)の 24/32bpp に対応
pub(crate) fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let w = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]);
    let h_raw = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
    let bpp = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let comp = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
    if (comp != 0 && comp != 3) || (bpp != 24 && bpp != 32) || w <= 0 || h_raw == 0 {
        return None;
    }
    let top_down = h_raw < 0;
    let (w, h) = (w as usize, h_raw.unsigned_abs() as usize);
    // BI_BITFIELDS(biCompression=3) が biSize=40 の時だけ直後に 12 バイトの
    // カラーマスクが付く(dib_to_bmp と同じ判断)
    let masks = if comp == 3 && header_size == 40 { 12 } else { 0 };
    let clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize;
    let palette = if clr_used > 0 {
        clr_used * 4
    } else if bpp == 8 {
        1024
    } else {
        0
    };
    let off = header_size + palette + masks;
    let stride = (w * bpp / 8 + 3) & !3usize;
    let px = dib.get(off..)?;
    if px.len() < stride * h {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        // DIB は下から上(top-down のみ上から下)
        let src_row = if top_down { row } else { h - 1 - row };
        let line = &px[src_row * stride..src_row * stride + w * bpp / 8];
        match bpp {
            32 => {
                for p in line.as_chunks::<4>().0 {
                    rgba.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
                }
            }
            _ => {
                for p in line.as_chunks::<3>().0 {
                    rgba.extend_from_slice(&[p[2], p[1], p[0], 0xFF]);
                }
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }
    Some(out)
}

/// BMP 画像を Mac のクリップボードへ TIFF として書き込む(Windows→Mac 画像同期)
pub(crate) unsafe fn mac_set_clipboard_image_bmp(bmp: &[u8]) -> bool {
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
        let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
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
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> u8 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let ok = f(
        pb,
        sel_registerName(c"setData:forType:".as_ptr()),
        tiff,
        uti,
    );
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
pub(crate) unsafe fn mac_get_clipboard() -> Option<String> {
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
    Some(
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned(),
    )
}

pub(crate) fn clipboard_change_count() -> isize {
    unsafe {
        msg0_isize(
            general_pasteboard(),
            sel_registerName(c"changeCount".as_ptr()),
        )
    }
}

/// NSPasteboard にファイル参照(Finder の ⌘C 等)があるか調べ、パス群を返す。
/// readObjectsForClasses:options:(NSURL + FileURLsOnly=YES)で読むことで
/// NSFilenamesPboardType / public.file-url / alias のどの載せ方でも拾う
pub(crate) unsafe fn pb_files(pb: ID) -> Option<Vec<std::path::PathBuf>> {
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
    // ここで切り詰めない: 65 件以上を黙って 64 件にすると静かなデータ欠落に
    // なる。本当の件数を返し、掴みドラッグは offer_drag_to_win が件数超過を
    // 拒否して通知し、⌘C 経路は bulk::collect が上限(512 件・容量制限)を審査する
    let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let mut out = Vec::new();
    for i in 0..n {
        let url = at(
            urls,
            sel_registerName(c"objectAtIndex:".as_ptr()),
            i as usize,
        );
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
        let s = std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned();
        if !s.is_empty() {
            out.push(std::path::PathBuf::from(s));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// general クリップボードのファイル参照(⌘C 検出用の既存経路)
pub(crate) unsafe fn mac_clipboard_files() -> Option<Vec<std::path::PathBuf>> {
    pb_files(general_pasteboard())
}

/// NSPasteboard へファイル参照を書き込む(Finder の ⌘C 相当)。
/// Windows からのファイル受信完了時に呼ぶ。writeObjects: が file URL を
/// 載せるため、Finder への ⌘V がそのまま動く
pub(crate) unsafe fn mac_clipboard_write_files(paths: &[std::path::PathBuf]) -> bool {
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
/// Mac のクリップボード画像(PNG/TIFF/JPEG)を Windows の CF_DIB 形式にする。
/// NSBitmapImageRep で BMP に書き出し、先頭 14 バイトのファイルヘッダを外すと DIB になる
/// (Windows 側に画像デコーダを持たずに済む)
pub(crate) unsafe fn mac_clipboard_image_dib() -> Option<Vec<u8>> {
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
    let props = msg0(
        objc_getClass(c"NSDictionary".as_ptr()),
        sel_registerName(c"dictionary".as_ptr()),
    );
    let repr: unsafe extern "C" fn(ID, SEL, usize, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    // NSBitmapImageFileTypeBMP = 1
    let bmp = repr(
        rep,
        sel_registerName(c"representationUsingType:properties:".as_ptr()),
        1,
        props,
    );
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
