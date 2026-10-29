// NV12 は隙間のない偶数幅・偶数高さの平面として受け取る。
pub(crate) fn nv12_to_bgra(src: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
        return None;
    }
    let width = usize::try_from(w).ok()?;
    let height = usize::try_from(h).ok()?;
    let pixels = width.checked_mul(height)?;
    if src.len() != pixels.checked_add(pixels / 2)? {
        return None;
    }
    let mut out = vec![0; pixels.checked_mul(4)?];
    for y in 0..height {
        for x in 0..width {
            let c = i32::from(src[y * width + x]) - 16;
            let uv = pixels + (y / 2) * width + (x / 2) * 2;
            let d = i32::from(src[uv]) - 128;
            let e = i32::from(src[uv + 1]) - 128;
            let clip = |n: i32| ((n + 128) >> 8).clamp(0, 255) as u8;
            let i = (y * width + x) * 4;
            out[i..i + 4].copy_from_slice(&[
                clip(298 * c + 516 * d),
                clip(298 * c - 100 * d - 208 * e),
                clip(298 * c + 409 * e),
                255,
            ]);
        }
    }
    Some(out)
}

pub(crate) fn aspect_fit(
    w: u32,
    h: u32,
    screen_w: u32,
    screen_h: u32,
) -> Option<(u32, u32, u32, u32)> {
    if w == 0 || h == 0 || screen_w == 0 || screen_h == 0 {
        return None;
    }
    let (dw, dh) = if u64::from(screen_w) * u64::from(h) <= u64::from(screen_h) * u64::from(w) {
        (
            screen_w,
            (u64::from(screen_w) * u64::from(h) / u64::from(w)) as u32,
        )
    } else {
        (
            (u64::from(screen_h) * u64::from(w) / u64::from(h)) as u32,
            screen_h,
        )
    };
    Some(((screen_w - dw) / 2, (screen_h - dh) / 2, dw, dh))
}

pub(crate) fn select_monitor(primary: &[bool], requested: Option<usize>) -> Option<usize> {
    match requested {
        Some(i) => (i < primary.len()).then_some(i),
        None => primary
            .iter()
            .position(|&p| !p)
            .or_else(|| (!primary.is_empty()).then_some(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_converts_limited_range_black_white_and_color() {
        assert_eq!(
            nv12_to_bgra(&[16, 235, 16, 235, 128, 128], 2, 2),
            Some(vec![
                0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255
            ])
        );
        assert_eq!(
            nv12_to_bgra(&[81, 81, 81, 81, 90, 240], 2, 2),
            Some(vec![0, 0, 255, 255].repeat(4))
        );
        assert_eq!(nv12_to_bgra(&[16; 5], 2, 2), None);
        assert_eq!(nv12_to_bgra(&[], 1, 1), None);
    }

    #[test]
    fn aspect_fit_centers_image_without_cropping() {
        assert_eq!(
            aspect_fit(1920, 1080, 1280, 1024),
            Some((0, 152, 1280, 720))
        );
        assert_eq!(
            aspect_fit(1280, 1024, 1920, 1080),
            Some((285, 0, 1350, 1080))
        );
        assert_eq!(aspect_fit(0, 1080, 1920, 1080), None);
    }

    #[test]
    fn monitor_selection_prefers_requested_then_non_primary() {
        assert_eq!(select_monitor(&[true, false, false], None), Some(1));
        assert_eq!(select_monitor(&[true], None), Some(0));
        assert_eq!(select_monitor(&[true, false], Some(0)), Some(0));
        assert_eq!(select_monitor(&[true, false], Some(2)), None);
        assert_eq!(select_monitor(&[], None), None);
    }
}
