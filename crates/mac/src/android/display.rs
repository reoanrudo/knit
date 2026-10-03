use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy)]
pub(crate) struct PhysicalSize {
    pub width_mm: f64,
    pub height_mm: f64,
}

static SIZES: OnceLock<Mutex<HashMap<String, PhysicalSize>>> = OnceLock::new();

pub(crate) fn remember(id: &str, size: Option<PhysicalSize>) {
    let mut sizes = SIZES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(size) = size {
        sizes.insert(id.to_owned(), size);
    } else {
        sizes.remove(id);
    }
}

pub(crate) fn get(id: &str) -> Option<PhysicalSize> {
    SIZES
        .get()?
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .copied()
}

// wm densityはUIの倍率。液晶の寸法にはDisplayDeviceInfoの物理DPIを使う。
pub(crate) fn parse(out: &str) -> Option<PhysicalSize> {
    let line = out
        .lines()
        .find(|line| line.contains("DisplayDeviceInfo{") && line.contains("type INTERNAL"))?;
    let parts = line.split_whitespace().collect::<Vec<_>>();
    let pixels = parts.windows(3).find_map(|p| {
        if p[1] != "x" {
            return None;
        }
        Some((
            p[0].parse::<f64>().ok()?,
            p[2].trim_end_matches(',').parse::<f64>().ok()?,
        ))
    })?;
    let dpi = line
        .split("density ")
        .nth(1)?
        .split(',')
        .nth(1)?
        .split(" dpi")
        .next()?;
    let (x, y) = dpi.trim().split_once(" x ")?;
    let (x, y) = (x.parse::<f64>().ok()?, y.parse::<f64>().ok()?);
    if ![x, y]
        .iter()
        .all(|d| d.is_finite() && (50.0..=1200.0).contains(d))
    {
        return None;
    }
    let size = PhysicalSize {
        width_mm: pixels.0 / x * 25.4,
        height_mm: pixels.1 / y * 25.4,
    };
    let diagonal = size.width_mm.hypot(size.height_mm);
    (size.width_mm > 0.0 && size.height_mm > 0.0 && (70.0..=1500.0).contains(&diagonal))
        .then_some(size)
}

// 入力のピクセル数は変更せず、配置図だけMacのpoints/mmへ換算する。
pub(crate) fn layout_size(
    pixels: (f64, f64),
    physical: Option<PhysicalSize>,
    mac: (f64, f64),
    mac_mm: (f64, f64),
) -> (f64, f64) {
    let diagonal = pixels.0.max(1.0).hypot(pixels.1.max(1.0));
    let display_diagonal = match physical {
        Some(size) if mac_mm.0 > 0.0 && mac_mm.1 > 0.0 => {
            size.width_mm.hypot(size.height_mm) * mac.0.hypot(mac.1) / mac_mm.0.hypot(mac_mm.1)
        }
        // 寸法を取得できない端末はMacより小さい概算表示にする。
        _ => mac.0.hypot(mac.1) * 0.8,
    };
    let factor = display_diagonal / diagonal;
    (pixels.0.max(1.0) * factor, pixels.1.max(1.0) * factor)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = "DisplayDeviceInfo{\"内蔵\": uniqueId=\"local:1\", 2032 x 3048, density 400, 294.257 x 294.145 dpi, type INTERNAL}";

    #[test]
    fn physical_dpi_is_distinct_from_ui_density() {
        let size = parse(SAMPLE).unwrap();
        assert!((size.width_mm - 175.4).abs() < 0.2);
        assert!((size.height_mm - 263.2).abs() < 0.2);
        assert!(parse(&SAMPLE.replace("294.257", "NaN")).is_none());
        assert!(parse(&SAMPLE.replace("294.257", "0")).is_none());
        assert!(parse("density 400").is_none());
    }

    #[test]
    fn smaller_high_resolution_tablet_stays_smaller_than_mac() {
        let mac = (2056.0, 1329.0);
        let mm = (344.7, 222.8);
        let tablet = layout_size((3048.0, 2032.0), parse(SAMPLE), mac, mm);
        assert!(tablet.0 < mac.0 && tablet.1 < mac.1);
        assert!((tablet.0 / mac.0 - 0.764).abs() < 0.01);
        let scaled = layout_size((1524.0, 1016.0), parse(SAMPLE), mac, mm);
        assert!((scaled.0 - tablet.0).abs() < 0.001);
        let portrait = layout_size((2032.0, 3048.0), parse(SAMPLE), mac, mm);
        assert!((portrait.0 - tablet.1).abs() < 0.001);
    }

    #[test]
    fn missing_metrics_use_a_bounded_estimate() {
        let size = layout_size((10000.0, 6000.0), None, (2000.0, 1400.0), (0.0, 0.0));
        assert!(size.0 < 2000.0 && size.1 < 1400.0);
    }
}
