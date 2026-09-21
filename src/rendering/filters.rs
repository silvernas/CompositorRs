//! Non-destructive adjustment application (levels, curves, exposure, hue,
//! gradient map, grain). Levels/curves/exposure run through per-channel lookup
//! tables; the heavy C kernels (gradient map, grain) are reused via FFI.

use crate::core::layer::{AdjustmentKind, HueSaturationSettings, LayerAdjustment};
use crate::core::pixel_buffer::PixelBuffer;
use crate::core::selection::Selection;
use crate::ffi;

/// Apply an adjustment layer's settings to a buffer in place.
pub fn apply_adjustment(buf: &mut PixelBuffer, adj: &LayerAdjustment) {
    match adj.kind {
        AdjustmentKind::Levels => {
            let mut tables = [0f32; 768];
            for c in 0..3 {
                for i in 0..256 {
                    tables[c * 256 + i] = adj.levels.apply(i as f32 / 255.0, c);
                }
            }
            ffi::levels_apply(&mut buf.data, &tables);
        }
        AdjustmentKind::Curves => {
            let mut tables = [0f32; 768];
            for c in 0..4 {
                for i in 0..256 {
                    tables[c.min(2) * 256 + i] = adj.curves.value(i as f32, c) / 255.0;
                }
            }
            ffi::levels_apply(&mut buf.data, &tables);
        }
        AdjustmentKind::Exposure => {
            let t = adj.exposure.table();
            let mut tables = [0f32; 768];
            for c in 0..3 {
                tables[c * 256..(c + 1) * 256].copy_from_slice(&t);
            }
            ffi::levels_apply(&mut buf.data, &tables);
        }
        AdjustmentKind::Hsv => apply_hsv(buf, &adj.hsv),
        AdjustmentKind::GradientMap => {
            let (dark, light) = adj.gradient_map.ends();
            let mut table = [0u8; 768];
            for i in 0..256 {
                let t = i as f32 / 255.0;
                let lerp = |a: f32, b: f32| a + (b - a) * t;
                table[i * 3] = (lerp(dark.red, light.red) * 255.0).round().clamp(0.0, 255.0) as u8;
                table[i * 3 + 1] = (lerp(dark.green, light.green) * 255.0).round().clamp(0.0, 255.0) as u8;
                table[i * 3 + 2] = (lerp(dark.blue, light.blue) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            let (w, h, stride) = (buf.width as usize, buf.height as usize, buf.stride());
            ffi::adjust_gradient_map(&mut buf.data, w, h, stride, &table);
        }
        AdjustmentKind::Grain => {
            let g = &adj.grain;
            let (w, h, stride) = (buf.width as usize, buf.height as usize, buf.stride());
            ffi::adjust_grain(
                &mut buf.data,
                w,
                h,
                stride,
                g.amount as f64,
                g.size as f64,
                g.roughness as f64,
                g.seed,
                0.0,
                0.0,
                1.0,
            );
        }
    }
}

fn apply_hsv(buf: &mut PixelBuffer, h: &crate::core::layer::HueSaturationSettings) {
    if h.is_identity() {
        return;
    }
    let (dh, ds, dl) = (h.hue / 360.0, h.saturation / 100.0, h.lightness / 100.0);
    for px in buf.data.chunks_exact_mut(4) {
        let a = px[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        let inv = 1.0 / a;
        let (r, g, b) = (px[0] as f32 * inv / 255.0, px[1] as f32 * inv / 255.0, px[2] as f32 * inv / 255.0);
        let (mut hue, mut sat, mut lum) = rgb_to_hsl(r, g, b);
        hue = (hue + dh).rem_euclid(1.0);
        sat = (sat + ds).clamp(0.0, 1.0);
        lum = (lum + dl).clamp(0.0, 1.0);
        let (nr, ng, nb) = hsl_to_rgb(hue, sat, lum);
        px[0] = (nr * a * 255.0).round().clamp(0.0, 255.0) as u8;
        px[1] = (ng * a * 255.0).round().clamp(0.0, 255.0) as u8;
        px[2] = (nb * a * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

/// Destructive hue/saturation applied to a pixel layer's buffer in place
/// (PS "Image ▸ Adjustments ▸ Hue/Saturation").
///
/// When `selection` covers only part of the buffer, pixels are blended by the
/// coverage value so feathered edges fade smoothly. `colorize` tints the image
/// to a single hue while keeping the original lightness.
pub fn apply_hue_saturation(
    buf: &mut PixelBuffer,
    h: &HueSaturationSettings,
    selection: Option<&Selection>,
) {
    if h.is_identity() {
        return;
    }
    let (dh, ds, dl) = (h.hue / 360.0, h.saturation / 100.0, h.lightness / 100.0);
    let sel = selection.filter(|s| s.width == buf.width && s.height == buf.height);
    for (i, px) in buf.data.chunks_exact_mut(4).enumerate() {
        let a = px[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        let mix = sel.map(|s| s.coverage[i] as f32 / 255.0).unwrap_or(1.0);
        if mix <= 0.0 {
            continue;
        }
        let inv = 1.0 / a;
        let (r, g, b) = (
            px[0] as f32 * inv / 255.0,
            px[1] as f32 * inv / 255.0,
            px[2] as f32 * inv / 255.0,
        );
        let (hue, sat, lum) = rgb_to_hsl(r, g, b);
        let (nr, ng, nb) = if h.colorize {
            // Keep the original lightness; hue comes from the slider and the
            // saturation slider drives how colourful the tint is (0 = gray).
            let s = (0.25 + ds).clamp(0.0, 1.0);
            let l = (lum + dl).clamp(0.0, 1.0);
            hsl_to_rgb(dh.rem_euclid(1.0), s, l)
        } else {
            let (hh, ss, ll) = (
                (hue + dh).rem_euclid(1.0),
                (sat + ds).clamp(0.0, 1.0),
                (lum + dl).clamp(0.0, 1.0),
            );
            hsl_to_rgb(hh, ss, ll)
        };
        // Blend by selection coverage; write back premultiplied.
        let base = (nr * a * 255.0, ng * a * 255.0, nb * a * 255.0);
        px[0] = (px[0] as f32 * (1.0 - mix) + base.0 * mix)
            .round()
            .clamp(0.0, 255.0) as u8;
        px[1] = (px[1] as f32 * (1.0 - mix) + base.1 * mix)
            .round()
            .clamp(0.0, 255.0) as u8;
        px[2] = (px[2] as f32 * (1.0 - mix) + base.2 * mix)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
}

/// Destructive Gaussian blur applied to a pixel layer's buffer in place.
///
/// Approximates a Gaussian kernel with three separable box-blur passes
/// (each O(w×h) via running sums). Works on premultiplied RGBA8 directly.
pub fn gaussian_blur(buf: &mut PixelBuffer, radius: f32) {
    let r = radius.round().clamp(0.0, 500.0) as usize;
    if r < 1 {
        return;
    }
    let (w, h) = (buf.width as usize, buf.height as usize);
    if w < 1 || h < 1 {
        return;
    }
    for _ in 0..3 {
        box_blur_pass(buf, r);
    }
}

fn box_blur_pass(buf: &mut PixelBuffer, radius: usize) {
    let (w, h) = (buf.width as usize, buf.height as usize);
    let row = w * 4;
    let mut tmp = vec![0u8; buf.data.len()];

    // Horizontal pass: src -> tmp.
    let mut prefix = vec![0i64; w + 1];
    for y in 0..h {
        let src = &buf.data[y * row..(y + 1) * row];
        let dst = &mut tmp[y * row..(y + 1) * row];
        for c in 0..4 {
            prefix[0] = 0;
            for x in 0..w {
                prefix[x + 1] = prefix[x] + src[x * 4 + c] as i64;
            }
            for x in 0..w {
                let lo = x.saturating_sub(radius);
                let hi = (x + radius).min(w - 1);
                let sum = prefix[hi + 1] - prefix[lo];
                let count = (hi - lo + 1) as f32;
                dst[x * 4 + c] = (sum as f32 / count).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    // Vertical pass: tmp -> buf.
    let mut prefix = vec![0i64; h + 1];
    for x in 0..w {
        for c in 0..4 {
            prefix[0] = 0;
            for y in 0..h {
                prefix[y + 1] = prefix[y] + tmp[y * row + x * 4 + c] as i64;
            }
            for y in 0..h {
                let lo = y.saturating_sub(radius);
                let hi = (y + radius).min(h - 1);
                let sum = prefix[hi + 1] - prefix[lo];
                let count = (hi - lo + 1) as f32;
                buf.data[y * row + x * 4 + c] =
                    (sum as f32 / count).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |t: f32| -> f32 {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0))
}
