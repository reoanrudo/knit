// icongen: アプリケーションアイコン素材を生成する開発ツール(手動実行)。
//   cargo run -p sd-mac --bin icongen
// 出力:
//   assets/AppIcon.iconset/*.png  (icns化は scripts/gen-icons.sh の iconutil が行う)
//   win-dist/app.ico              (Windows exe 埋込 + トレイ用)
// デザイン: 青のグラデーション角丸スクリーンに白モニタ+双方向矢印(⇄)。
// メニューバーのテンプレートアイコンは gui.rs が実行時に同モチーフで描画する
#![allow(non_camel_case_types)]

type CGContextRef = *mut core::ffi::c_void;
type CGColorSpaceRef = *mut core::ffi::c_void;
type CGImageRef = *mut core::ffi::c_void;
type CGGradientRef = *mut core::ffi::c_void;
type CFStringRef = *mut core::ffi::c_void;
type CFURLRef = *mut core::ffi::c_void;
type CGImageDestinationRef = *mut core::ffi::c_void;
type CFAllocatorRef = *mut core::ffi::c_void;
type CGColorRef = *mut core::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGAffineTransform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;
    fn CGBitmapContextCreate(
        data: *mut u8, width: usize, height: usize, bits_per_component: usize,
        bytes_per_row: usize, space: CGColorSpaceRef, bitmap_info: u32,
    ) -> CGContextRef;
    fn CGContextRelease(ctx: CGContextRef);
    fn CGBitmapContextCreateImage(ctx: CGContextRef) -> CGImageRef;
    fn CGContextSetRGBFillColor(ctx: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetRGBStrokeColor(ctx: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetLineWidth(ctx: CGContextRef, w: f64);
    fn CGContextSetLineCap(ctx: CGContextRef, cap: u32); // 0=butt 1=round 2=square
    fn CGContextSetLineJoin(ctx: CGContextRef, j: u32);
    fn CGContextBeginPath(ctx: CGContextRef);
    fn CGContextMoveToPoint(ctx: CGContextRef, x: f64, y: f64);
    fn CGContextAddLineToPoint(ctx: CGContextRef, x: f64, y: f64);
    fn CGContextAddArcToPoint(ctx: CGContextRef, x1: f64, y1: f64, x2: f64, y2: f64, r: f64);
    fn CGContextClosePath(ctx: CGContextRef);
    fn CGContextFillPath(ctx: CGContextRef);
    fn CGContextStrokePath(ctx: CGContextRef);
    fn CGGradientCreateWithColorComponents(
        space: CGColorSpaceRef, components: *const f64, locations: *const f64, count: usize,
    ) -> CGGradientRef;
    fn CGContextDrawLinearGradient(
        ctx: CGContextRef, gradient: CGGradientRef, start: CGPoint, end: CGPoint, options: u32,
    );
    fn CGContextSetShadowWithColor(ctx: CGContextRef, offset: CGPoint, blur: f64, color: CGColorRef);
    fn CGColorCreateSRGB(r: f64, g: f64, b: f64, a: f64) -> CGColorRef;
    fn CGContextSaveGState(ctx: CGContextRef);
    fn CGContextRestoreGState(ctx: CGContextRef);
    fn CGContextConcatCTM(ctx: CGContextRef, transform: CGAffineTransform);
    fn CGContextClip(ctx: CGContextRef);
    fn CGContextAddArc(
        ctx: CGContextRef, x: f64, y: f64, radius: f64, start_angle: f64, end_angle: f64,
        clockwise: i32,
    );
    fn CGImageRelease(img: CGImageRef);
    fn CFURLCreateFromFileSystemRepresentation(
        alloc: CFAllocatorRef, path: *const u8, len: isize, is_directory: bool,
    ) -> CFURLRef;
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef, c_str: *const core::ffi::c_char, encoding: u32,
    ) -> CFStringRef;
    fn CGImageDestinationCreateWithURL(
        url: CFURLRef, img_type: CFStringRef, count: usize, options: *mut core::ffi::c_void,
    ) -> CGImageDestinationRef;
    fn CGImageDestinationAddImage(
        dest: CGImageDestinationRef, image: CGImageRef, props: *mut core::ffi::c_void,
    );
    fn CGImageDestinationFinalize(dest: CGImageDestinationRef) -> bool;
    fn CFRelease(cf: *mut core::ffi::c_void);
}

/// 角丸矩形パスを追加(4隅の arcTo)
unsafe fn add_rounded_rect(ctx: CGContextRef, x: f64, y: f64, w: f64, h: f64, r: f64) {
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, x + r, y);
    CGContextAddArcToPoint(ctx, x + w, y, x + w, y + h, r);
    CGContextAddArcToPoint(ctx, x + w, y + h, x, y + h, r);
    CGContextAddArcToPoint(ctx, x, y + h, x, y, r);
    CGContextAddArcToPoint(ctx, x, y, x + w, y, r);
    CGContextClosePath(ctx);
}

unsafe fn stroke_line(ctx: CGContextRef, x1: f64, y1: f64, x2: f64, y2: f64) {
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, x1, y1);
    CGContextAddLineToPoint(ctx, x2, y2);
    CGContextStrokePath(ctx);
}

/// 矢印(軸+2本の頭)。太さ・色は呼び出し前に設定しておく
unsafe fn stroke_arrow(ctx: CGContextRef, x1: f64, y1: f64, x2: f64, y2: f64, head: f64) {
    stroke_line(ctx, x1, y1, x2, y2);
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    // 進行方向 45° の2本を先端から引く(先端が(x2,y2))
    let bx1 = x2 - head * (ux * 0.7071 + uy * 0.7071);
    let by1 = y2 - head * (uy * 0.7071 - ux * 0.7071);
    let bx2 = x2 - head * (ux * 0.7071 - uy * 0.7071);
    let by2 = y2 - head * (uy * 0.7071 + ux * 0.7071);
    stroke_line(ctx, bx1, by1, x2, y2);
    stroke_line(ctx, bx2, by2, x2, y2);
}

/// サイズ size×size のアプリアイコンを RGBA バッファへ描く。
/// デザイン「キーキャップ・シーム」: アイコン全体を1つのキーキャップとし、
/// 天面を左右に割る。左半球(グラファイト)に ⌘、右半球(ブルー)に ⊞ を刻印し、
/// 中央の光るシーム(境界)を大きな白カーソルが横断する。
/// 「入力(キー)」「Mac/Windows」「境界を越える」を1枚で表す
unsafe fn draw_app_icon(size: usize) -> Vec<u8> {
    let s = size as f64;
    let k = s / 1024.0; // 1024基準の設計座標→実サイズ倍率
    let bytes_per_row = size * 4;
    let mut data = vec![0u8; bytes_per_row * size];
    let space = CGColorSpaceCreateDeviceRGB();
    // BGRA (PremultipliedFirst | ByteOrder32Little)
    let ctx = CGBitmapContextCreate(
        data.as_mut_ptr(), size, size, 8, bytes_per_row, space, 2 | (2 << 12),
    );
    assert!(!ctx.is_null(), "CGBitmapContextCreate failed");
    let vgrad = |c0: [f64; 3], c1: [f64; 3]| -> CGGradientRef {
        let comps: [f64; 8] = [c0[0], c0[1], c0[2], 1.0, c1[0], c1[1], c1[2], 1.0];
        CGGradientCreateWithColorComponents(space, comps.as_ptr(), std::ptr::null(), 2)
    };

    // --- キーキャップ側面(ベース) ---
    add_rounded_rect(ctx, 0.0, 0.0, s, s, 230.0 * k);
    CGContextClip(ctx);
    let wall = vgrad([0.09, 0.11, 0.16], [0.16, 0.19, 0.27]); // 暗→やや明
    CGContextDrawLinearGradient(
        ctx, wall, CGPoint { x: 0.0, y: 0.0 }, CGPoint { x: 0.0, y: s }, 0,
    );

    // --- キーキャップ天面(左右に割る) ---
    let (fx, fy, fw, fh, fr) = (110.0 * k, 96.0 * k, 804.0 * k, 808.0 * k, 168.0 * k);
    CGContextSaveGState(ctx);
    add_rounded_rect(ctx, fx, fy, fw, fh, fr);
    CGContextClip(ctx);
    // 左半球 = Mac グラファイト
    let mac_g = vgrad([0.30, 0.335, 0.42], [0.145, 0.165, 0.22]);
    CGContextDrawLinearGradient(
        ctx, mac_g, CGPoint { x: 0.0, y: fy + fh }, CGPoint { x: 0.0, y: fy }, 0,
    );
    CGContextRestoreGState(ctx);
    // ↑ 両グラデーションを重ねると左が潰れるため、改めて右半分だけ描き直す
    CGContextSaveGState(ctx);
    add_rounded_rect(ctx, fx, fy, fw, fh, fr);
    CGContextClip(ctx);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, fx + fw / 2.0, fy);
    CGContextAddLineToPoint(ctx, fx + fw, fy);
    CGContextAddLineToPoint(ctx, fx + fw, fy + fh);
    CGContextAddLineToPoint(ctx, fx + fw / 2.0, fy + fh);
    CGContextClosePath(ctx);
    CGContextClip(ctx);
    let win_g2 = vgrad([0.26, 0.52, 0.97], [0.10, 0.30, 0.80]);
    CGContextDrawLinearGradient(
        ctx, win_g2, CGPoint { x: 0.0, y: fy + fh }, CGPoint { x: 0.0, y: fy }, 0,
    );

    // --- 刻印: 左に ⌘(角丸枠+四隅の輪)、右に ⊞(2x2) ---
    let emboss = |draw: &dyn Fn()| {
        // 安っぽい浮き出し防止に「下にずらした暗い複製」で彫刻感
        CGContextSaveGState(ctx);
        CGContextConcatCTM(ctx, CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: -10.0 * k });
        CGContextSetRGBStrokeColor(ctx, 0.0, 0.0, 0.0, 0.22);
        CGContextSetRGBFillColor(ctx, 0.0, 0.0, 0.0, 0.22);
        draw();
        CGContextRestoreGState(ctx);
        CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.93);
        CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 0.93);
        draw();
    };
    // ⌘: 角丸矩形の輪 + 四隅の円環(同一ストロークで重ねると⌘に見える)
    let cmd_stroke_w = 30.0 * k;
    let (cx0, cy0, cs) = (196.0 * k, 470.0 * k, 216.0 * k); // 左半球中心(300,578)
    emboss(&|| {
        CGContextSetLineWidth(ctx, cmd_stroke_w);
        CGContextSetLineJoin(ctx, 1);
        add_rounded_rect(ctx, cx0, cy0, cs, cs, 46.0 * k);
        CGContextStrokePath(ctx);
        for (px_, py_) in [
            (cx0, cy0), (cx0 + cs, cy0), (cx0, cy0 + cs), (cx0 + cs, cy0 + cs),
        ] {
            // 円環を角そのものに重ね輪郭と接続させる(本物の⌘に近づける)
            CGContextBeginPath(ctx);
            CGContextAddArc(ctx, px_, py_, 54.0 * k, 0.0, std::f64::consts::TAU, 1);
            CGContextStrokePath(ctx);
        }
    });
    // ⊞: 2x2 の角丸四角
    let (nx, ny, nc, ng) = (648.0 * k, 506.0 * k, 122.0 * k, 34.0 * k); // 右半球中心(724,578)
    emboss(&|| {
        for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            add_rounded_rect(
                ctx, nx + i * (nc + ng), ny + j * (nc + ng), nc, nc, 26.0 * k,
            );
            CGContextFillPath(ctx);
        }
    });

    // --- シーム(境界)の光 ---
    CGContextSaveGState(ctx);
    add_rounded_rect(ctx, fx, fy, fw, fh, fr);
    CGContextClip(ctx);
    CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.14);
    CGContextSetLineWidth(ctx, 34.0 * k);
    stroke_line(ctx, fx + fw / 2.0, fy + 6.0 * k, fx + fw / 2.0, fy + fh - 6.0 * k);
    CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.55);
    CGContextSetLineWidth(ctx, 7.0 * k);
    stroke_line(ctx, fx + fw / 2.0, fy + 6.0 * k, fx + fw / 2.0, fy + fh - 6.0 * k);
    // 天面のハイライト(上面 35% の白ベール)
    let sheen: [f64; 8] = [1.0, 1.0, 1.0, 0.10, 1.0, 1.0, 1.0, 0.0];
    let sheen_g = CGGradientCreateWithColorComponents(space, sheen.as_ptr(), std::ptr::null(), 2);
    CGContextDrawLinearGradient(
        ctx, sheen_g, CGPoint { x: 0.0, y: fy + fh }, CGPoint { x: 0.0, y: fy + fh * 0.62 }, 0,
    );
    CGContextRestoreGState(ctx);

    // --- 大きな白カーソル(シームを横断、約55%) ---
    // ポインタ形状(設計座標、原点=左下)
    let arrow: [(f64, f64); 7] = [
        (0.0, 562.0),   // 先端
        (0.0, 68.7),
        (125.5, 187.6),
        (203.2, 0.0),
        (278.0, 29.9),
        (200.4, 209.0),
        (355.7, 223.7),
    ];
    let (ox, oy) = (368.0 * k, 180.0 * k);
    let (ccx, ccy) = (ox + 178.0 * k, oy + 281.0 * k); // カーソル外接箱の中心
    let theta = -14.0_f64.to_radians();
    let (cos_t, sin_t) = (theta.cos(), theta.sin());
    let rot = CGAffineTransform {
        a: cos_t, b: sin_t, c: -sin_t, d: cos_t,
        tx: ccx - cos_t * ccx + sin_t * ccy,
        ty: ccy - sin_t * ccx - cos_t * ccy,
    };
    CGContextSaveGState(ctx);
    CGContextConcatCTM(ctx, rot);
    let shadow = CGColorCreateSRGB(0.02, 0.04, 0.12, 0.40);
    CGContextSetShadowWithColor(ctx, CGPoint { x: 0.0, y: -30.0 * k }, 46.0 * k, shadow);
    let path = |close: bool| {
        CGContextBeginPath(ctx);
        let (x0, y0) = arrow[0];
        CGContextMoveToPoint(ctx, ox + x0 * k, oy + y0 * k);
        for (x, y) in &arrow[1..] {
            CGContextAddLineToPoint(ctx, ox + x * k, oy + y * k);
        }
        if close {
            CGContextClosePath(ctx);
        }
    };
    path(true);
    CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 1.0);
    CGContextFillPath(ctx);
    CGContextSetShadowWithColor(ctx, CGPoint { x: 0.0, y: 0.0 }, 0.0, std::ptr::null_mut());
    path(true);
    CGContextSetRGBStrokeColor(ctx, 0.55, 0.62, 0.85, 0.30);
    CGContextSetLineWidth(ctx, 4.0 * k);
    CGContextStrokePath(ctx);
    CGContextRestoreGState(ctx);

    CGContextRelease(ctx);
    for g in [wall, mac_g, win_g2, sheen_g] {
        CFRelease(g);
    }
    CFRelease(space);
    CFRelease(shadow);
    data
}

fn rgba_to_png(data: &mut [u8], width: usize, height: usize) -> CGImageRef {
    unsafe {
        let space = CGColorSpaceCreateDeviceRGB();
        let ctx = CGBitmapContextCreate(
            data.as_mut_ptr() as *mut u8, width, height, 8, width * 4, space, 2 | (2 << 12),
        );
        let img = CGBitmapContextCreateImage(ctx);
        CGContextRelease(ctx);
        CFRelease(space);
        img
    }
}

fn write_png(path: &std::path::Path, data: &mut [u8], width: usize, height: usize) {
    unsafe {
        let img = rgba_to_png(data, width, height);
        assert!(!img.is_null());
        let mut p = path.as_os_str().to_string_lossy().into_owned().into_bytes();
        p.push(0);
        let url = CFURLCreateFromFileSystemRepresentation(
            std::ptr::null_mut(), p.as_ptr(), (p.len() - 1) as isize, false,
        );
        let png_type = CFStringCreateWithCString(
            std::ptr::null_mut(), c"public.png".as_ptr(), 0x0800_0100, // kCFStringEncodingUTF8
        );
        let dest = CGImageDestinationCreateWithURL(url, png_type, 1, std::ptr::null_mut());
        assert!(!dest.is_null(), "CGImageDestinationCreateWithURL failed: {path:?}");
        CGImageDestinationAddImage(dest, img, std::ptr::null_mut());
        assert!(CGImageDestinationFinalize(dest), "PNG finalize failed: {path:?}");
        CFRelease(dest);
        CFRelease(url);
        CFRelease(png_type);
        CGImageRelease(img);
    }
}

/// PNG バイト群をマルチサイズ .ico へ(Vista+ の PNG 埋込形式)
fn write_ico(path: &std::path::Path, pngs: &[(u32, Vec<u8>)]) {
    use std::io::Write;
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type=icon
    out.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6u32 + 16 * pngs.len() as u32;
    for (size, bytes) in pngs {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        out.push(dim); // width (0=256)
        out.push(dim); // height
        out.push(0); // colors
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bpp
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += bytes.len() as u32;
    }
    for (_, bytes) in pngs {
        out.extend_from_slice(bytes);
    }
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&out).unwrap();
}

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let iconset = root.join("assets/AppIcon.iconset");
    std::fs::create_dir_all(&iconset).unwrap();

    // icns 用 PNG 一式(iconutil が要求する命名)
    let sizes: &[(u32, &str)] = &[
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ];
    unsafe {
        for (size, name) in sizes {
            let mut data = draw_app_icon(*size as usize);
            write_png(&iconset.join(name), &mut data, *size as usize, *size as usize);
        }
    }
    println!("[icongen] PNG x{} を {iconset:?} へ出力", sizes.len());

    // Windows 用 .ico(16/32/48/256 の PNG 埋込)
    let mut pngs = Vec::new();
    for size in [16u32, 32, 48, 256] {
        let mut data = unsafe { draw_app_icon(size as usize) };
        let tmp = root.join(format!("assets/.tmp_{size}.png"));
        write_png(&tmp, &mut data, size as usize, size as usize);
        pngs.push((size, std::fs::read(&tmp).unwrap()));
        let _ = std::fs::remove_file(&tmp);
    }
    write_ico(&root.join("win-dist/app.ico"), &pngs);
    println!("[icongen] win-dist/app.ico を出力");
    println!("[icongen] 次: scripts/gen-icons.sh で icns 化+app.ico をコミット");
}
