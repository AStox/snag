use image::{Rgba, RgbaImage, imageops};

use crate::error::Result;

pub const CROP_RADIUS: i32 = 900;
/// Full-display PNG sent to the model is downscaled to this width. Crop stays native.
pub const FULL_MAX_WIDTH: u32 = 1280;

pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    Ok(img)
}

pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::{ColorType, ImageEncoder};
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, FilterType::NoFilter).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        ColorType::Rgba8,
    )?;
    Ok(buf)
}

fn blend(dst: &mut Rgba<u8>, src: Rgba<u8>) {
    let a = src[3] as f32 / 255.0;
    if a <= 0.0 {
        return;
    }
    for i in 0..3 {
        dst[i] = ((src[i] as f32) * a + (dst[i] as f32) * (1.0 - a)) as u8;
    }
    dst[3] = 255;
}

pub fn mark_cursor(img: &mut RgbaImage, cx: i32, cy: i32) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let ring_r = 26i32;
    let ring_t = 3i32;
    let dot_r = 5i32;
    let ring = Rgba([232, 96, 78, 230]);
    let halo = Rgba([255, 255, 255, 90]);
    let fill = Rgba([232, 96, 78, 40]);
    let dot = Rgba([232, 96, 78, 255]);

    let max_r = ring_r + ring_t + 4;
    for y in (cy - max_r).max(0)..(cy + max_r).min(h) {
        for x in (cx - max_r).max(0)..(cx + max_r).min(w) {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            let px = img.get_pixel_mut(x as u32, y as u32);
            if d2 <= (ring_r - 2) * (ring_r - 2) {
                blend(px, fill);
            }
            let outer = ring_r + ring_t;
            let inner = ring_r - 1;
            if d2 <= (outer + 2) * (outer + 2) && d2 >= (inner - 2) * (inner - 2) {
                blend(px, halo);
            }
            if d2 <= outer * outer && d2 >= inner * inner {
                blend(px, ring);
            }
            if d2 <= dot_r * dot_r {
                blend(px, dot);
            }
        }
    }
}

pub fn crop_around(img: &RgbaImage, cx: i32, cy: i32) -> RgbaImage {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let x0 = (cx - CROP_RADIUS).max(0);
    let y0 = (cy - CROP_RADIUS).max(0);
    let x1 = (cx + CROP_RADIUS).min(w);
    let y1 = (cy + CROP_RADIUS).min(h);
    let cw = (x1 - x0).max(1) as u32;
    let ch = (y1 - y0).max(1) as u32;
    imageops::crop_imm(img, x0 as u32, y0 as u32, cw, ch).to_image()
}

pub fn mark_and_pack(png: &[u8], cx: u32, cy: u32) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut img = decode_png(png)?;
    let cx = cx.min(img.width().saturating_sub(1)) as i32;
    let cy = cy.min(img.height().saturating_sub(1)) as i32;
    mark_cursor(&mut img, cx, cy);
    let crop = crop_around(&img, cx, cy);
    Ok((encode_png(&img)?, encode_png(&crop)?))
}

pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> RgbaImage {
    RgbaImage::from_raw(width, height, rgba)
        .unwrap_or_else(|| RgbaImage::new(width.max(1), height.max(1)))
}

/// Nearest-neighbor downsample of packed RGBA. Avoids allocating a 5K buffer then resizing.
pub fn downsample_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_width: u32,
) -> (u32, u32, Vec<u8>) {
    let width = width.max(1);
    let height = height.max(1);
    if width <= max_width {
        return (width, height, rgba.to_vec());
    }
    let scale = max_width as f32 / width as f32;
    let tw = max_width;
    let th = ((height as f32) * scale).round().max(1.0) as u32;
    let mut out = vec![0u8; tw as usize * th as usize * 4];
    for y in 0..th {
        let sy = ((y as f32 / scale) as u32).min(height - 1);
        for x in 0..tw {
            let sx = ((x as f32 / scale) as u32).min(width - 1);
            let si = ((sy * width + sx) * 4) as usize;
            let di = ((y * tw + x) * 4) as usize;
            if si + 4 <= rgba.len() && di + 4 <= out.len() {
                out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
            }
        }
    }
    (tw, th, out)
}

pub fn scale_cursor(px: i32, py: i32, src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> (i32, i32) {
    if src_w == 0 || src_h == 0 {
        return (0, 0);
    }
    let nx = (px as f32 * dst_w as f32 / src_w as f32).round() as i32;
    let ny = (py as f32 * dst_h as f32 / src_h as f32).round() as i32;
    (
        nx.clamp(0, dst_w.saturating_sub(1) as i32),
        ny.clamp(0, dst_h.saturating_sub(1) as i32),
    )
}
