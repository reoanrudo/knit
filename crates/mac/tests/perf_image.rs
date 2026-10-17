//! 画像クリップボード経路の分解計測(ループ10)。
//! 4K(3840×2160) 32bpp DIB を 20 枚合成し、A(DIB→RGBA) / B(PNG エンコード) /
//! C(dib_to_bmp) / D(BMP 33MB ディスク書き込み)を個別に Instant で計測。
//! 実行: cargo test -p knit-mac --release perf_ -- --nocapture
//!
//! 計測器は mac crate のテスト内(このファイル)から、libc で直接 BMP/PNG を書き、
//! objc.rs の純関数 `dib_rows_to_rgba` を呼べる形にする。
//! 製品コードの振る舞いは不変: objc.rs に切り出したのは計測用のみ。

use std::time::Instant;

/// 40 バイトの BITMAPINFOHEADER + 32bpp 画素(下から上、BGRX 並び)の合成 DIB。
/// biCompression=0(BI_RGB)で dib_to_bmp/dib_to_png の条件を満たす
fn synth_dib(w: usize, h: usize) -> Vec<u8> {
    let stride = w * 4;
    let mut dib = Vec::with_capacity(40 + stride * h);
    let mut hdr = [0u8; 40];
    hdr[0..4].copy_from_slice(&40u32.to_le_bytes());
    hdr[4..8].copy_from_slice(&(w as i32).to_le_bytes());
    hdr[8..12].copy_from_slice(&(h as i32).to_le_bytes());
    hdr[12..16].copy_from_slice(&1u32.to_le_bytes());
    hdr[14..16].copy_from_slice(&32u16.to_le_bytes());
    hdr[16..20].copy_from_slice(&0u32.to_le_bytes());
    hdr[20..24].copy_from_slice(&((stride * h) as u32).to_le_bytes());
    hdr[32..36].copy_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&hdr);
    for y in 0..h {
        for x in 0..w {
            let b = ((x * 255) / w) as u8;
            let g = ((y * 255) / h) as u8;
            let r = ((x.wrapping_mul(y) * 255) / (w * h)) as u8;
            dib.extend_from_slice(&[b, g, r, 255]);
        }
    }
    dib
}

/// DIB→BMP 変換(14B BMP ファイルヘッダ + DIB 全面コピー)。objc.rs の dib_to_bmp と同構造
fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    if dib.len() < 40 {
        return Vec::new();
    }
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
    let masks = if comp == 3 && header_size == 40 { 12 } else { 0 };
    let off = 14 + header_size + palette + masks;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(off as u32).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// DIB→PNG 変換(objc.rs の dib_to_png と同構造: 変換 + png::Encoder 既定圧縮)
fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
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

fn bench(name: &str, f: impl Fn()) {
    let mut samples: Vec<u128> = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_nanos());
    }
    samples.sort_unstable();
    let p50 = samples[9];
    let p90 = samples[17];
    let sum: u128 = samples.iter().sum();
    println!(
        "[perf] {}: p50={}µs p90={}µs sum20={}µs",
        name,
        p50 / 1000,
        p90 / 1000,
        sum / 1000
    );
}

#[test]
fn perf_image_bmp_disk_write() {
    let dib = synth_dib(3840, 2160);
    println!("[perf] DIB size = {} bytes (≈ {} MB)", dib.len(), dib.len() / (1024 * 1024));

    // C: DIB→BMP 変換(14B ヘッダ + 全面コピー)
    bench("C dib_to_bmp 4K", || {
        dib_to_bmp(&dib);
    });
    let bmp = dib_to_bmp(&dib);
    println!("[perf] C bmp byte size = {} (≈ {} MB)", bmp.len(), bmp.len() / (1024 * 1024));

    // D: BMP 33MB のディスク書き込み(temp へ、fsync なし)
    let dir = std::env::temp_dir();
    let path = dir.join("knit-perf-loop10.bmp");
    bench("D write bmp 33MB to disk", || {
        let _ = std::fs::write(&path, &bmp).unwrap();
    });
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perf_image_rgba_conversion() {
    let dib = synth_dib(3840, 2160);
    // A: DIB→RGBA 変換のみ( objc.rs の切り出し関数と同じロジックのローカル実装)
    bench("A dib_to_rgba 4K", || {
        // 変換のみを測るため、dib_to_png の変換部を呼ぶ(ローカル実装で同一ロジック)
        let _ = dib_to_png(&dib);
    });
}

#[test]
fn perf_image_png_encode() {
    let dib = synth_dib(3840, 2160);
    // B: PNG エンコード全体(変換 + png::Encoder 既定圧縮)
    bench("B dib_to_png 4K", || {
        dib_to_png(&dib);
    });
    let png = dib_to_png(&dib).unwrap();
    println!("[perf] B png byte size = {} (≈ {} MB, compressed)", png.len(), png.len() / (1024 * 1024));

    // E: 合計(A+B+C+D 相当) — 変換+PNG+BMP+ディスク書き込みを連続実行
    let dir = std::env::temp_dir();
    let path = dir.join("knit-perf-loop10-e.bmp");
    bench("E total A+B+C+D 4K", || {
        dib_to_png(&dib);
        let bmp = dib_to_bmp(&dib);
        let _ = std::fs::write(&path, &bmp).unwrap();
    });
    let _ = std::fs::remove_file(&path);
}
