//! Saving the composited result (PNG/JPEG) from a premultiplied buffer.

use std::path::Path;

use crate::core::pixel_buffer::PixelBuffer;

pub fn save_png(path: &Path, buf: &PixelBuffer) -> Result<(), String> {
    let img = to_straight_image(buf);
    img.save(path).map_err(|e| e.to_string())
}

pub fn save_jpeg(path: &Path, buf: &PixelBuffer, quality: u8) -> Result<(), String> {
    let img = to_straight_image(buf);
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut out = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    rgb.write_with_encoder(encoder).map_err(|e| e.to_string())?;
    Ok(())
}

/// Convert premultiplied RGBA8 to straight RGBA8 for encoding.
fn to_straight_image(buf: &PixelBuffer) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(buf.width, buf.height);
    for (i, px) in img.pixels_mut().enumerate() {
        let a = buf.data[i * 4 + 3] as f32 / 255.0;
        let depremul = |v: u8| -> u8 {
            if a <= 0.0 {
                0
            } else {
                (v as f32 / a).round().clamp(0.0, 255.0) as u8
            }
        };
        px.0 = [
            depremul(buf.data[i * 4]),
            depremul(buf.data[i * 4 + 1]),
            depremul(buf.data[i * 4 + 2]),
            buf.data[i * 4 + 3],
        ];
    }
    img
}
