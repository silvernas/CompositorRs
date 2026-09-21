//! CPU compositing of a `Document` into one premultiplied RGBA8 buffer.

use crate::core::blend::BlendMode;
use crate::core::document::Document;
use crate::core::layer::{Layer, LayerSampling, LayerTransform};
use crate::core::pixel_buffer::PixelBuffer;
use crate::rendering::{filters, gpu_effects};
use rayon::prelude::*;

/// Render the whole document at its native resolution.
pub fn composite_document(doc: &Document) -> PixelBuffer {
    let mut canvas = PixelBuffer::new(doc.width, doc.height);
    for &id in &doc.paint_order() {
        let layer = doc.layer(id).unwrap();
        if layer.is_group {
            continue;
        }
        if let Some(adj) = &layer.adjustment {
            filters::apply_adjustment(&mut canvas, adj);
            continue;
        }
        let Some(asset) = &layer.asset else { continue };
        let sampled = sample_asset(&asset.image, &layer.transform, doc.width, doc.height);
        let has_fx = layer
            .effects
            .as_ref()
            .map(|e| !e.visible().is_empty())
            .unwrap_or(false);
        let fx_buf = if has_fx {
            Some(gpu_effects::render(&sampled, layer))
        } else {
            None
        };
        let src = fx_buf.as_ref().unwrap_or(&sampled);
        let coverage = mask_coverage(layer, &sampled);
        blend_into(&mut canvas, src, &coverage, layer);
    }
    canvas
}

pub(crate) fn sample_asset(asset: &PixelBuffer, t: &LayerTransform, cw: u32, ch: u32) -> PixelBuffer {
    let mut out = PixelBuffer::new(cw, ch);
    let w = cw as usize;
    let aw = asset.width.max(1) as f32;
    let ah = asset.height.max(1) as f32;
    let sw = t.size.0.max(1.0);
    let sh = t.size.1.max(1.0);
    let cx = t.origin.0 + sw / 2.0;
    let cy = t.origin.1 + sh / 2.0;
    let (sin, cos) = t.radians().sin_cos();
    let (fx, fy) = (if t.flip_x { -1.0 } else { 1.0 }, if t.flip_y { -1.0 } else { 1.0 });
    let smooth = t.sampling != LayerSampling::Nearest;
    out.data.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let dyb = y as f32 - cy;
        for x in 0..w {
            let dx = x as f32 - cx;
            let a = (dx * cos + dyb * sin) * fx;
            let b = (-dx * sin + dyb * cos) * fy;
            let px = sample_texel(asset, (a / sw + 0.5) * aw, (b / sh + 0.5) * ah, smooth);
            row[x * 4..x * 4 + 4].copy_from_slice(&px);
        }
    });
    out
}

fn sample_texel(asset: &PixelBuffer, x: f32, y: f32, smooth: bool) -> [u8; 4] {
    let w = asset.width as usize;
    let h = asset.height as usize;
    let at = |i: usize, j: usize, c: usize| asset.data[(j * w + i) * 4 + c] as f32;
    if smooth {
        let x0 = x.floor().clamp(0.0, asset.width as f32 - 1.0) as usize;
        let y0 = y.floor().clamp(0.0, asset.height as f32 - 1.0) as usize;
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = ((x - x0 as f32).clamp(0.0, 1.0), (y - y0 as f32).clamp(0.0, 1.0));
        let mut out = [0u8; 4];
        for c in 0..4 {
            let top = at(x0, y0, c) + (at(x1, y0, c) - at(x0, y0, c)) * fx;
            let bot = at(x0, y1, c) + (at(x1, y1, c) - at(x0, y1, c)) * fx;
            out[c] = (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8;
        }
        out
    } else {
        let xi = x.round().clamp(0.0, asset.width as f32 - 1.0) as usize;
        let yi = y.round().clamp(0.0, asset.height as f32 - 1.0) as usize;
        let o = (yi * w + xi) * 4;
        [asset.data[o], asset.data[o + 1], asset.data[o + 2], asset.data[o + 3]]
    }
}

/// Per-pixel coverage in 0..1: sampled alpha × enabled mask (alpha-only otherwise).
pub(crate) fn mask_coverage(layer: &Layer, sampled: &PixelBuffer) -> Vec<f32> {
    let w = sampled.width as usize;
    let h = sampled.height as usize;
    let mut cov = vec![0f32; w * h];
    let Some(m) = &layer.mask else {
        for y in 0..h {
            for x in 0..w {
                cov[y * w + x] = sampled.data[(y * w + x) * 4 + 3] as f32 / 255.0;
            }
        }
        return cov;
    };
    if !m.is_enabled {
        for y in 0..h {
            for x in 0..w {
                cov[y * w + x] = sampled.data[(y * w + x) * 4 + 3] as f32 / 255.0;
            }
        }
        return cov;
    }
    let mask_img = &m.asset.image;
    let t = layer.transform;
    let (sw, sh) = (t.size.0.max(1.0), t.size.1.max(1.0));
    let (aw, ah) = (mask_img.width.max(1) as f32, mask_img.height.max(1) as f32);
    let (cx, cy) = (t.origin.0 + sw / 2.0, t.origin.1 + sh / 2.0);
    let (sin, cos) = t.radians().sin_cos();
    let (fx, fy) = (if t.flip_x { -1.0 } else { 1.0 }, if t.flip_y { -1.0 } else { 1.0 });
    let (mw, mh) = (mask_img.width as usize, mask_img.height as usize);
    for y in 0..h {
        let dy = y as f32 - cy;
        for x in 0..w {
            let dx = x as f32 - cx;
            let a = (dx * cos + dy * sin) * fx;
            let b = (-dx * sin + dy * cos) * fy;
            let xi = (((a / sw + 0.5) * aw).floor() as usize).min(mw - 1);
            let yi = (((b / sh + 0.5) * ah).floor() as usize).min(mh - 1);
            let alpha = mask_img.data[(yi * mw + xi) * 4 + 3] as f32 / 255.0;
            let base = sampled.data[(y * w + x) * 4 + 3] as f32 / 255.0;
            cov[y * w + x] = base * alpha;
        }
    }
    cov
}

/// Composite `sampled` over `dst` per pixel, applying mask coverage and opacity.
pub(crate) fn blend_into(dst: &mut PixelBuffer, sampled: &PixelBuffer, cov: &[f32], layer: &Layer) {
    let (w, _h) = (sampled.width as usize, sampled.height as usize);
    let opacity = layer.opacity.clamp(0.0, 1.0);
    let mode = layer.blend_mode;
    dst.data
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let i = y * w + x;
                let k = (cov[i] * opacity).clamp(0.0, 1.0);
                if k <= 0.0 {
                    continue;
                }
                let o = i * 4;
                let src = [
                    (sampled.data[o] as f32 * k).round() as u8,
                    (sampled.data[o + 1] as f32 * k).round() as u8,
                    (sampled.data[o + 2] as f32 * k).round() as u8,
                    (sampled.data[o + 3] as f32 * k).round() as u8,
                ];
                if mode == BlendMode::Normal && row[x * 4 + 3] == 255 {
                    // Fast path: opaque backdrop, normal blend → source replaces.
                    if src[3] >= 250 {
                        row[x * 4..x * 4 + 4].copy_from_slice(&[src[0], src[1], src[2], src[3]]);
                        continue;
                    }
                }
                let backdrop = [row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]];
                let out = crate::core::blend::composite(src, backdrop, mode);
                row[x * 4..x * 4 + 4].copy_from_slice(&out);
            }
        });
}

/// Rasterize `layer` into `dst` at document size (sampling, mask, opacity, blend).
/// Layers without a pixel asset are no-ops.
pub(crate) fn rasterize_layer_into(dst: &mut PixelBuffer, layer: &Layer) {
    let Some(asset) = &layer.asset else { return };
    let sampled = sample_asset(&asset.image, &layer.transform, dst.width, dst.height);
    let cov = mask_coverage(layer, &sampled);
    blend_into(dst, &sampled, &cov, layer);
}
