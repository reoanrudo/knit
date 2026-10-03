// icongen: アプリケーションアイコン素材を生成する開発ツール(手動実行)。
//   cargo run -p knit-mac --bin icongen
// 出力:
//   assets/AppIcon.iconset/*.png  (icns化は scripts/gen-icons.sh の iconutil が行う)
//   win-dist/app.ico              (Windows exe 埋込 + トレイ用)
// デザイン: インディゴの面に、二つの連続するリボン。
// メニューバーのテンプレートアイコンは gui.rs が実行時に同モチーフで描画する
#![allow(non_camel_case_types)]
#![allow(clippy::duplicated_attributes)] // 複数フレームワークの #[link] 並記(実害なし)

type CGContextRef = *mut core::ffi::c_void;
type CGColorSpaceRef = *mut core::ffi::c_void;
type CGImageRef = *mut core::ffi::c_void;
type CGGradientRef = *mut core::ffi::c_void;
type CFStringRef = *mut core::ffi::c_void;
type CFURLRef = *mut core::ffi::c_void;
type CGImageDestinationRef = *mut core::ffi::c_void;
type CFAllocatorRef = *mut core::ffi::c_void;

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
        data: *mut u8,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: CGColorSpaceRef,
        bitmap_info: u32,
    ) -> CGContextRef;
    fn CGContextRelease(ctx: CGContextRef);
    fn CGBitmapContextCreateImage(ctx: CGContextRef) -> CGImageRef;

    fn CGContextSetRGBStrokeColor(ctx: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetLineWidth(ctx: CGContextRef, w: f64);
    fn CGContextSetLineCap(ctx: CGContextRef, cap: u32); // 0=butt 1=round 2=square
    fn CGContextSetLineJoin(ctx: CGContextRef, j: u32);
    fn CGContextBeginPath(ctx: CGContextRef);
    fn CGContextMoveToPoint(ctx: CGContextRef, x: f64, y: f64);

    fn CGContextAddArcToPoint(ctx: CGContextRef, x1: f64, y1: f64, x2: f64, y2: f64, r: f64);
    fn CGContextClosePath(ctx: CGContextRef);
    fn CGContextAddCurveToPoint(
        ctx: CGContextRef,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
    );

    fn CGContextStrokePath(ctx: CGContextRef);
    fn CGGradientCreateWithColorComponents(
        space: CGColorSpaceRef,
        components: *const f64,
        locations: *const f64,
        count: usize,
    ) -> CGGradientRef;
    fn CGContextDrawLinearGradient(
        ctx: CGContextRef,
        gradient: CGGradientRef,
        start: CGPoint,
        end: CGPoint,
        options: u32,
    );

    fn CGContextConcatCTM(ctx: CGContextRef, transform: CGAffineTransform);
    fn CGContextClip(ctx: CGContextRef);

    fn CGImageRelease(img: CGImageRef);
    fn CFURLCreateFromFileSystemRepresentation(
        alloc: CFAllocatorRef,
        path: *const u8,
        len: isize,
        is_directory: bool,
    ) -> CFURLRef;
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const core::ffi::c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CGImageDestinationCreateWithURL(
        url: CFURLRef,
        img_type: CFStringRef,
        count: usize,
        options: *mut core::ffi::c_void,
    ) -> CGImageDestinationRef;
    fn CGImageDestinationAddImage(
        dest: CGImageDestinationRef,
        image: CGImageRef,
        props: *mut core::ffi::c_void,
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

/// 二つの流れが連続するリボン。小サイズでも輪郭が残る単純な曲線を使う。
unsafe fn draw_app_icon(size: usize) -> Vec<u8> {
    let s = size as f64;
    let mut data = vec![0u8; size * size * 4];
    let space = CGColorSpaceCreateDeviceRGB();
    let ctx = CGBitmapContextCreate(
        data.as_mut_ptr(),
        size,
        size,
        8,
        size * 4,
        space,
        2 | (2 << 12),
    );
    assert!(!ctx.is_null());
    add_rounded_rect(ctx, s * 0.04, s * 0.04, s * 0.92, s * 0.92, s * 0.23);
    CGContextClip(ctx);
    let colors = [0.055, 0.065, 0.15, 1.0, 0.22, 0.20, 0.44, 1.0];
    let gradient = CGGradientCreateWithColorComponents(space, colors.as_ptr(), std::ptr::null(), 2);
    CGContextDrawLinearGradient(
        ctx,
        gradient,
        CGPoint { x: s, y: 0.0 },
        CGPoint { x: 0.0, y: s },
        0,
    );
    CGContextConcatCTM(
        ctx,
        CGAffineTransform {
            a: s / 128.0,
            b: 0.0,
            c: 0.0,
            d: s / 128.0,
            tx: 0.0,
            ty: 0.0,
        },
    );
    CGContextSetLineWidth(ctx, 11.5);
    CGContextSetLineCap(ctx, 1);
    CGContextSetLineJoin(ctx, 1);
    // 奥のループを先に描き、交差部分には背景色の隙間を設ける。
    CGContextSetRGBStrokeColor(ctx, 0.55, 0.64, 1.0, 1.0);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 61.0, 57.0);
    CGContextAddCurveToPoint(ctx, 36.0, 25.0, 17.0, 38.0, 28.0, 59.0);
    CGContextAddCurveToPoint(ctx, 34.0, 70.0, 44.0, 77.0, 60.0, 82.0);
    CGContextStrokePath(ctx);
    CGContextSetRGBStrokeColor(ctx, 0.11, 0.12, 0.25, 1.0);
    CGContextSetLineWidth(ctx, 19.0);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 51.0, 47.0);
    CGContextAddCurveToPoint(ctx, 73.0, 81.0, 84.0, 97.0, 99.0, 83.0);
    CGContextStrokePath(ctx);
    CGContextSetLineWidth(ctx, 11.5);
    CGContextSetRGBStrokeColor(ctx, 0.94, 0.96, 1.0, 1.0);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 49.0, 44.0);
    CGContextAddCurveToPoint(ctx, 69.0, 66.0, 79.0, 103.0, 99.0, 83.0);
    CGContextAddCurveToPoint(ctx, 116.0, 64.0, 83.0, 46.0, 69.0, 43.0);
    CGContextStrokePath(ctx);
    CGContextRelease(ctx);
    CFRelease(gradient);
    CFRelease(space);
    data
}

fn rgba_to_png(data: &mut [u8], width: usize, height: usize) -> CGImageRef {
    unsafe {
        let space = CGColorSpaceCreateDeviceRGB();
        let ctx = CGBitmapContextCreate(
            data.as_mut_ptr(),
            width,
            height,
            8,
            width * 4,
            space,
            2 | (2 << 12),
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
            std::ptr::null_mut(),
            p.as_ptr(),
            (p.len() - 1) as isize,
            false,
        );
        let png_type = CFStringCreateWithCString(
            std::ptr::null_mut(),
            c"public.png".as_ptr(),
            0x0800_0100, // kCFStringEncodingUTF8
        );
        let dest = CGImageDestinationCreateWithURL(url, png_type, 1, std::ptr::null_mut());
        assert!(
            !dest.is_null(),
            "CGImageDestinationCreateWithURL failed: {path:?}"
        );
        CGImageDestinationAddImage(dest, img, std::ptr::null_mut());
        assert!(
            CGImageDestinationFinalize(dest),
            "PNG finalize failed: {path:?}"
        );
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
            write_png(
                &iconset.join(name),
                &mut data,
                *size as usize,
                *size as usize,
            );
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
