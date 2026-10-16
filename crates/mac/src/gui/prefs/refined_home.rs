//! 配置図専用のOS別ホーム画面イメージ。実端末のキャプチャは取得しない。
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Home {
    Mac,
    Windows,
    Android,
}

fn image_bytes(home: Home) -> &'static [u8] {
    match home {
        Home::Mac => include_bytes!("../../../../../assets/settings-home/mac-home.png"),
        Home::Windows => include_bytes!("../../../../../assets/settings-home/windows-home.png"),
        Home::Android => include_bytes!("../../../../../assets/settings-home/android-home.png"),
    }
}

fn cover_rect(destination: NSRect, width: f64, height: f64) -> NSRect {
    if width <= 0.0 || height <= 0.0 {
        return destination;
    }
    let scale = (destination.w / width).max(destination.h / height);
    let w = width * scale;
    let h = height * scale;
    NSRect {
        x: destination.x + (destination.w - w) / 2.0,
        y: destination.y + (destination.h - h) / 2.0,
        w,
        h,
    }
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: *const core::ffi::c_void, bytes: *const u8, length: isize) -> ID;
    fn CGImageSourceCreateWithData(data: ID, options: ID) -> ID;
    fn CGImageSourceCreateImageAtIndex(source: ID, index: usize, options: ID) -> ID;
    fn CGImageGetWidth(image: ID) -> usize;
    fn CGImageGetHeight(image: ID) -> usize;
    fn CGContextDrawImage(ctx: ID, rect: NSRect, image: ID);
    fn CGContextSaveGState(ctx: ID);
    fn CGContextRestoreGState(ctx: ID);
    fn CGContextClipToRect(ctx: ID, rect: NSRect);
}

unsafe fn image(home: Home) -> ID {
    // 各画像は一度だけデコードし、プロセス終了まで保持する。設定ファイルIOなし。
    static IMAGES: [std::sync::OnceLock<usize>; 3] = [const { std::sync::OnceLock::new() }; 3];
    let index = match home {
        Home::Mac => 0,
        Home::Windows => 1,
        Home::Android => 2,
    };
    *IMAGES[index].get_or_init(|| {
        let bytes = image_bytes(home);
        let data = CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize);
        if data.is_null() {
            return 0;
        }
        let source = CGImageSourceCreateWithData(data, std::ptr::null_mut());
        CFRelease(data);
        if source.is_null() {
            return 0;
        }
        let image = CGImageSourceCreateImageAtIndex(source, 0, std::ptr::null_mut());
        CFRelease(source);
        image as usize
    }) as ID
}

pub(super) unsafe fn draw(ctx: ID, destination: NSRect, home: Home) -> bool {
    let image = image(home);
    if image.is_null() {
        return false;
    }
    let rect = cover_rect(
        destination,
        CGImageGetWidth(image) as f64,
        CGImageGetHeight(image) as f64,
    );
    CGContextSaveGState(ctx);
    CGContextClipToRect(ctx, destination);
    CGContextDrawImage(ctx, rect, image);
    CGContextRestoreGState(ctx);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_home_images_are_decodable_and_platform_specific() {
        let mut images = Vec::new();
        for home in [Home::Mac, Home::Windows, Home::Android] {
            let decoder = png::Decoder::new(std::io::Cursor::new(image_bytes(home)));
            let mut reader = decoder.read_info().expect("bundled home must decode");
            let info = reader.info();
            assert!(info.width >= 600 && info.height >= 600);
            let mut buffer = vec![0; reader.output_buffer_size()];
            reader
                .next_frame(&mut buffer)
                .expect("complete image must decode");
            images.push(image_bytes(home));
        }
        assert_ne!(images[0], images[1]);
        assert_ne!(images[1], images[2]);
    }
    #[test]
    fn images_cover_screen_without_distorting_aspect() {
        let d = NSRect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 200.0,
        };
        let result = cover_rect(d, 1600.0, 1000.0);
        assert!((result.w / result.h - 1.6).abs() < 1e-12);
        assert!(result.w >= d.w && result.h >= d.h);
        assert!((result.x + result.w / 2.0 - 60.0).abs() < 1e-12);
        assert!((result.y + result.h / 2.0 - 120.0).abs() < 1e-12);
    }
}
