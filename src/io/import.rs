//! Opening images (PNG/JPEG/TIFF) into a `Document` with one pixel layer.

use std::path::Path;

use image::ImageReader;

use crate::core::document::Document;
use crate::core::layer::{Asset, Layer};
use crate::core::pixel_buffer::PixelBuffer;
use crate::rendering::downsample;

/// Decode a file into a premultiplied RGBA8 buffer.
///
/// Supports PNG/JPEG/TIFF (and whatever the `image` crate enables, e.g. BMP,
/// GIF, WebP, AVIF). HEIC/HEIF is decoded via the optional `heic` feature
/// (requires the system libheif library).
pub fn load_pixel_buffer(path: &Path) -> Result<PixelBuffer, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "heic" || ext == "heif" {
        #[cfg(feature = "heic")]
        {
            return load_heic(path);
        }
        #[cfg(not(feature = "heic"))]
        {
            return Err("HEIC 支持未启用：请使用 --features heic 重新构建（需要系统 libheif 库）".into());
        }
    }
    let img = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .to_rgba8();
    Ok(rgba_to_premultiplied(&img))
}

/// Decode an HEIC/HEIF image into a premultiplied RGBA8 buffer via libheif.
#[cfg(feature = "heic")]
fn load_heic(path: &Path) -> Result<PixelBuffer, String> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
    let lib = LibHeif::new();
    let ctx = HeifContext::read_from_file(path).map_err(|e| e.to_string())?;
    let handle = ctx.primary_image_handle().map_err(|e| e.to_string())?;
    let img = lib
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)
        .map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    let planes = img.planes();
    let rgb = planes
        .interleaved
        .ok_or_else(|| "HEIC 解码结果缺少交错 RGB 平面".to_string())?;
    let stride = rgb.stride;
    let mut out = PixelBuffer::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let o = y as usize * stride + x as usize * 3;
            let d = (y as usize * w as usize + x as usize) * 4;
            out.data[d] = rgb.data[o];
            out.data[d + 1] = rgb.data[o + 1];
            out.data[d + 2] = rgb.data[o + 2];
            out.data[d + 3] = 255;
        }
    }
    Ok(out)
}

/// Convert straight RGBA8 from the `image` crate into premultiplied RGBA8.
pub fn rgba_to_premultiplied(img: &image::RgbaImage) -> PixelBuffer {
    let (w, h) = img.dimensions();
    let mut buf = PixelBuffer::new(w, h);
    for (i, px) in img.pixels().enumerate() {
        let a = px.0[3] as f32 / 255.0;
        let mul = |v: u8| (v as f32 * a).round().clamp(0.0, 255.0) as u8;
        buf.data[i * 4] = mul(px.0[0]);
        buf.data[i * 4 + 1] = mul(px.0[1]);
        buf.data[i * 4 + 2] = mul(px.0[2]);
        buf.data[i * 4 + 3] = px.0[3];
    }
    buf
}

/// Open an image file as a new document with a single pixel layer.
pub fn open_document(path: &Path) -> Result<Document, String> {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into());
    let image = load_pixel_buffer(path)?;
    let thumbnail = downsample::downsample(&image, 96);
    let asset = Asset { image, thumbnail, name };
    let layer = Layer::from_asset(asset, (0.0, 0.0));
    let (w, h) = layer.size();
    let mut doc = Document::new(w as u32, h as u32);
    doc.insert_layer(layer, 0);
    Ok(doc)
}

/// A new blank document of the given size.
pub fn new_document(width: u32, height: u32) -> Document {
    let mut doc = Document::new(width, height);
    let layer = Layer::blank("Background".into(), (width, height));
    doc.insert_layer(layer, 0);
    doc
}
