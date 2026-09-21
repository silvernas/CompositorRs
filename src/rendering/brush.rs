//! Software brush: soft circular stamps and strokes written directly into a
//! layer's premultiplied RGBA8 buffer, plus selection-aware fill/clear helpers.

use crate::core::pixel_buffer::PixelBuffer;
use crate::core::selection::Selection;

/// Straight (non-premultiplied) color 0..1 and brush geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushParams {
    /// Brush diameter in layer pixels.
    pub size: f32,
    /// 0 = fully soft edge, 1 = hard edge.
    pub hardness: f32,
    /// Stamp opacity 0..1.
    pub opacity: f32,
    /// Straight sRGB color, channels 0..1.
    pub color: [f32; 3],
}

impl Default for BrushParams {
    fn default() -> Self {
        BrushParams {
            size: 32.0,
            hardness: 0.6,
            opacity: 1.0,
            color: [0.1, 0.6, 0.9],
        }
    }
}

/// Paint one soft-edged stamp centered at `(cx, cy)` (layer pixel space),
/// compositing `color` over the buffer with source-over.
pub fn stamp(buf: &mut PixelBuffer, cx: f32, cy: f32, p: &BrushParams) {
    let radius = (p.size / 2.0).max(0.5);
    let inner = radius * (1.0 - p.hardness.clamp(0.0, 1.0));
    let x0 = ((cx - radius).floor() as i32).max(0);
    let y0 = ((cy - radius).floor() as i32).max(0);
    let x1 = ((cx + radius).ceil() as i32).min(buf.width as i32 - 1);
    let y1 = ((cy + radius).ceil() as i32).min(buf.height as i32 - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    let (w, _h) = (buf.width as usize, buf.height as usize);
    let (sr, sg, sb) = (p.color[0], p.color[1], p.color[2]);
    let op = p.opacity.clamp(0.0, 1.0);
    let inv_r = 1.0 / (radius - inner).max(1e-4);
    for y in y0..=y1 {
        let dy = (y as f32 + 0.5) - cy;
        for x in x0..=x1 {
            let dx = (x as f32 + 0.5) - cx;
            let d = (dx * dx + dy * dy).sqrt();
            let mut a = if d <= inner {
                1.0
            } else if d >= radius {
                0.0
            } else {
                let t = (d - inner) * inv_r;
                let t = t.clamp(0.0, 1.0);
                (1.0 - t) * (1.0 - t)
            };
            if a <= 0.0 {
                continue;
            }
            a *= op;
            let o = (y as usize * w + x as usize) * 4;
            let dst_a = buf.data[o + 3] as f32 / 255.0;
            let out_a = a + dst_a * (1.0 - a);
            if out_a <= 0.0 {
                continue;
            }
            let inv = 1.0 / out_a;
            buf.data[o] = ((sr * a + buf.data[o] as f32 / 255.0 * dst_a * (1.0 - a)) * inv * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            buf.data[o + 1] = ((sg * a + buf.data[o + 1] as f32 / 255.0 * dst_a * (1.0 - a)) * inv * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            buf.data[o + 2] = ((sb * a + buf.data[o + 2] as f32 / 255.0 * dst_a * (1.0 - a)) * inv * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            buf.data[o + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Paint a stroke from `from` to `to` by stamping every `spacing` fraction of
/// the brush diameter along the line.
pub fn stroke(buf: &mut PixelBuffer, from: (f32, f32), to: (f32, f32), p: &BrushParams, spacing: f32) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let dist = (dx * dx + dy * dy).sqrt();
    let step = (p.size * spacing.max(0.05)).max(1.0);
    if dist <= 0.0 {
        stamp(buf, from.0, from.1, p);
        return;
    }
    let n = (dist / step).ceil().max(1.0) as usize;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        stamp(buf, from.0 + dx * t, from.1 + dy * t, p);
    }
}

/// Erase (make fully transparent) every pixel whose selection coverage > 0.
/// The selection is treated as document-aligned over the buffer.
pub fn clear_selection(buf: &mut PixelBuffer, selection: &Selection) {
    let w = buf.width as usize;
    let h = buf.height as usize;
    let sw = selection.width as usize;
    let sh = selection.height as usize;
    let min_dim = (w * h).min(sw * sh);
    for i in 0..min_dim {
        if selection.coverage[i] > 0 {
            buf.data[i * 4..i * 4 + 4].fill(0);
        }
    }
}

/// Fill selected pixels with `color` (straight sRGB) at `opacity`.
pub fn fill_selection(buf: &mut PixelBuffer, selection: &Selection, color: [f32; 3], opacity: f32) {
    let w = buf.width as usize;
    let h = buf.height as usize;
    let sw = selection.width as usize;
    let sh = selection.height as usize;
    if w != sw || h != sh {
        return;
    }
    let op = opacity.clamp(0.0, 1.0);
    let (sr, sg, sb) = (color[0], color[1], color[2]);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let cov = selection.coverage[i] as f32 / 255.0;
            if cov <= 0.0 {
                continue;
            }
            let a = cov * op;
            let o = i * 4;
            let dst_a = buf.data[o + 3] as f32 / 255.0;
            let out_a = a + dst_a * (1.0 - a);
            if out_a <= 0.0 {
                continue;
            }
            let inv = 1.0 / out_a;
            buf.data[o] = ((sr * a + buf.data[o] as f32 / 255.0 * dst_a * (1.0 - a)) * inv * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            buf.data[o + 1] = ((sg * a + buf.data[o + 1] as f32 / 255.0 * dst_a * (1.0 - a)) * inv * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            buf.data[o + 2] = ((sb * a + buf.data[o + 2] as f32 / 255.0 * dst_a * (1.0 - a)) * inv * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            buf.data[o + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}
