//! CPU layer-effects renderer: stroke, drop shadow, inner shadow, color overlay.
//!
//! Direct translation of the Swift `LayerEffectsRenderer` CPU path, operating
//! on document-sized premultiplied RGBA8 buffers. All blending is premultiplied
//! source-over, matching the rest of the pipeline.
//!
//! Compose order (mirrors the Swift `render`):
//!   1. drop shadow   (bottom)
//!   2. outside stroke
//!   3. source pixels
//!   4. color overlay
//!   5. inner shadow
//!   6. inside stroke (top)

use rayon::prelude::*;

use crate::core::layer::Layer;
use crate::core::pixel_buffer::PixelBuffer;

/// Render a layer's enabled effects around its sampled pixels.
///
/// `sampled` is the layer's pixels already sampled into a document-sized
/// premultiplied buffer. Returns a new buffer with effects baked in.
pub fn render_effects(sampled: &PixelBuffer, layer: &Layer) -> PixelBuffer {
    let Some(effects) = &layer.effects else {
        return sampled.clone();
    };
    let eff = effects.visible();
    if eff.is_empty() {
        return sampled.clone();
    }
    let (w, h) = (sampled.width as usize, sampled.height as usize);
    let mut fx = PixelBuffer::new(w as u32, h as u32);
    let shape = alpha_map(sampled);

    // 1. Drop shadow — shifted + blurred shape, filled at the very bottom.
    if let Some(shadow) = eff.shadow {
        let (dx, dy) = shadow.offset();
        let mut moved = shift(&shape, w, h, dx, dy);
        if shadow.blur > 0.0 {
            moved = gaussian_blur(&moved, w, h, shadow.blur / 2.0);
        }
        fill_over(&mut fx, shadow.color, shadow.opacity, &moved);
    }

    // 2. Outside stroke — dilated shape minus shape.
    if let Some(stroke) = eff.stroke.filter(|s| !s.inside) {
        let reach = stroke.size.round().max(1.0) as usize;
        let dilated = extreme(&shape, w, h, reach, false);
        let mut ring = vec![0.0; w * h];
        for i in 0..ring.len() {
            ring[i] = (dilated[i] - shape[i]).max(0.0);
        }
        fill_over(&mut fx, stroke.color, stroke.opacity, &ring);
    }

    // 3. Source pixels (premultiplied source-over).
    source_over_buffer(&mut fx, sampled);

    // 4. Color overlay — tint the whole shape.
    if let Some(ov) = eff.color_overlay {
        fill_over(&mut fx, ov.color, ov.opacity, &shape);
    }

    // 5. Inner shadow — shape outside the shifted/blurred shape, clipped.
    if let Some(shadow) = eff.inner_shadow {
        let (dx, dy) = shadow.offset();
        let mut moved = shift(&shape, w, h, dx, dy);
        if shadow.blur > 0.0 {
            moved = gaussian_blur(&moved, w, h, shadow.blur / 2.0);
        }
        let mut inside = vec![0.0; w * h];
        for i in 0..inside.len() {
            inside[i] = (shape[i] * (1.0 - moved[i])).clamp(0.0, 1.0);
        }
        fill_over(&mut fx, shadow.color, shadow.opacity, &inside);
    }

    // 6. Inside stroke — shape minus eroded shape.
    if let Some(stroke) = eff.stroke.filter(|s| s.inside) {
        let reach = stroke.size.round().max(1.0) as usize;
        let eroded = extreme(&shape, w, h, reach, true);
        let mut ring = vec![0.0; w * h];
        for i in 0..ring.len() {
            ring[i] = (shape[i] - eroded[i]).max(0.0);
        }
        fill_over(&mut fx, stroke.color, stroke.opacity, &ring);
    }

    fx
}

/// Bilinear sample a shifted copy of `src` (offset in pixels; boundary clamp).
fn shift(src: &[f32], w: usize, h: usize, dx: f32, dy: f32) -> Vec<f32> {
    let (fw, fh) = (w as f32, h as f32);
    let mut out = vec![0.0; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            let sx = x as f32 - dx;
            let sy = y as f32 - dy;
            let x0 = sx.floor().clamp(0.0, fw - 1.0) as usize;
            let y0 = sy.floor().clamp(0.0, fh - 1.0) as usize;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = (sx - x0 as f32).clamp(0.0, 1.0);
            let fy = (sy - y0 as f32).clamp(0.0, 1.0);
            let top = src[y0 * w + x0] + (src[y0 * w + x1] - src[y0 * w + x0]) * fx;
            let bot = src[y1 * w + x0] + (src[y1 * w + x1] - src[y1 * w + x0]) * fx;
            *v = top + (bot - top) * fy;
        }
    });
    out
}

/// Gaussian blur on a float coverage map. Three box passes approximate a
/// Gaussian (same trick as `filters::gaussian_blur`), O(n) per pass.
fn gaussian_blur(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let r = ((sigma * 0.75).round() as usize).max(1);
    let mut cur = src.to_vec();
    for _ in 0..3 {
        cur = box_blur_pass(&cur, w, h, r);
    }
    cur
}

/// One separable box blur pass on f32 data (horizontal then vertical),
/// using prefix sums with clamped window edges.
fn box_blur_pass(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let mut tmp = vec![0.0; w * h];
    let mut prefix = vec![0.0; w + 1];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let dst = &mut tmp[y * w..(y + 1) * w];
        for x in 0..w {
            prefix[x + 1] = prefix[x] + row[x];
        }
        for x in 0..w {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius).min(w - 1);
            let sum = prefix[hi + 1] - prefix[lo];
            let count = (hi - lo + 1) as f32;
            dst[x] = sum / count;
        }
    }
    let mut out = vec![0.0; w * h];
    let mut prefix = vec![0.0; h + 1];
    for x in 0..w {
        for y in 0..h {
            prefix[y + 1] = prefix[y] + tmp[y * w + x];
        }
        for y in 0..h {
            let lo = y.saturating_sub(radius);
            let hi = (y + radius).min(h - 1);
            let sum = prefix[hi + 1] - prefix[lo];
            let count = (hi - lo + 1) as f32;
            out[y * w + x] = sum / count;
        }
    }
    out
}

/// Morphological dilate (`smallest=false`) or erode (`smallest=true`) with a
/// square structuring element of `reach` pixels. Sliding-window extrema via a
/// monotonic queue, O(n) per row/column, rows run in parallel.
fn extreme(src: &[f32], w: usize, h: usize, reach: usize, smallest: bool) -> Vec<f32> {
    let mut tmp = vec![0.0; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        row_extreme(&src[y * w..(y + 1) * w], w, reach, smallest, row);
    });
    // Vertical pass over columns.
    let mut out = vec![0.0; w * h];
    let mut col_buf = vec![0.0; h];
    let mut col_res = vec![0.0; h];
    for x in 0..w {
        for y in 0..h {
            col_buf[y] = tmp[y * w + x];
        }
        row_extreme(&col_buf, h, reach, smallest, &mut col_res);
        for y in 0..h {
            out[y * w + x] = col_res[y];
        }
    }
    out
}

/// Sliding-window min/max over one 1-D row with clamped edges.
fn row_extreme(src: &[f32], n: usize, reach: usize, smallest: bool, out: &mut [f32]) {
    // Extend the row by clamping so the window has constant width 2r+1.
    let win = reach * 2 + 1;
    let ext_len = n + win - 1;
    let mut ext = vec![0.0; ext_len];
    for i in 0..ext_len {
        let si = (i as isize - reach as isize).clamp(0, n as isize - 1) as usize;
        ext[i] = src[si];
    }
    let mut dq: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for i in 0..ext_len {
        // Maintain monotonic deque over `ext`.
        if smallest {
            while let Some(&b) = dq.back() {
                if ext[i] <= ext[b] {
                    dq.pop_back();
                } else {
                    break;
                }
            }
        } else {
            while let Some(&b) = dq.back() {
                if ext[i] >= ext[b] {
                    dq.pop_back();
                } else {
                    break;
                }
            }
        }
        dq.push_back(i);
        while let Some(&f) = dq.front() {
            if f + win <= i {
                dq.pop_front();
            } else {
                break;
            }
        }
        if i + 1 >= win {
            let idx = i + 1 - win;
            out[idx] = ext[*dq.front().unwrap()];
        }
    }
}

/// Extract per-pixel alpha (0..1) from a premultiplied buffer.
fn alpha_map(buf: &PixelBuffer) -> Vec<f32> {
    buf.data
        .chunks_exact(4)
        .map(|p| p[3] as f32 / 255.0)
        .collect()
}

/// Premultiplied source-over: `out = src + dst * (1 - src.a)`.
fn source_over_buffer(dst: &mut PixelBuffer, src: &PixelBuffer) {
    for (i, px) in dst.data.chunks_exact_mut(4).enumerate() {
        let sa = src.data[i * 4 + 3] as f32 / 255.0;
        if sa <= 0.0 {
            continue;
        }
        let keep = 1.0 - sa;
        for c in 0..3 {
            px[c] = (src.data[i * 4 + c] as f32 + px[c] as f32 * keep)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
        let da = px[3] as f32 / 255.0;
        px[3] = ((sa + da * keep) * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

/// Fill a flat color (× opacity × coverage) over the buffer, source-over.
fn fill_over(dst: &mut PixelBuffer, color: (f32, f32, f32), opacity: f32, cov: &[f32]) {
    let a = opacity.clamp(0.0, 1.0);
    let (cr, cg, cb) = color;
    for (i, px) in dst.data.chunks_exact_mut(4).enumerate() {
        let k = (cov[i] * a).clamp(0.0, 1.0);
        if k <= 0.0 {
            continue;
        }
        let (sr, sg, sb) = (cr * k, cg * k, cb * k);
        let keep = 1.0 - k;
        let da = px[3] as f32 / 255.0;
        px[0] = (sr + px[0] as f32 / 255.0 * keep).round().clamp(0.0, 255.0) as u8;
        px[1] = (sg + px[1] as f32 / 255.0 * keep).round().clamp(0.0, 255.0) as u8;
        px[2] = (sb + px[2] as f32 / 255.0 * keep).round().clamp(0.0, 255.0) as u8;
        px[3] = ((k + da * keep) * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}
