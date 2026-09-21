//! Layer blend modes — the separable and non-separable compositing math.
//!
//! Mirrors `LayerBlendMode` from the Swift project and implements the
//! W3C Compositing and Blending Level 1 formulas so the result matches
//! Photoshop. Notably `Color Burn` and `Color Dodge` are implemented here
//! correctly (the Swift project works around Core Graphics getting them
//! wrong); we just do the math directly.
//!
//! All math operates on *straight* (unpremultiplied) colors; callers in the
//! compositor pass premultiplied RGBA8, which `composite` unpremultiplies,
//! blends, and re-premultiplies.

use crate::core::pixel_buffer::PixelBuffer;

/// The blend modes Compositor supports, in the same menu order as the Swift app.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    SoftLight,
    Darken,
    Lighten,
    Difference,
    ColorDodge,
    ColorBurn,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// The order shown in the blend-mode dropdown (matches `LayerBlendMode.allCases`).
    pub const ALL: [BlendMode; 14] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::Difference => "Difference",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
        }
    }

    pub fn from_label(s: &str) -> Option<BlendMode> {
        BlendMode::ALL.iter().find(|m| m.label() == s).copied()
    }
}

/// Separable blend: one channel at a time. `cb` = backdrop, `cs` = source (both 0..1).
fn blend_separable(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => {
            if cb <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::ColorDodge => {
            if cb == 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        // Non-separable modes fall back to normal in this per-channel path.
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => cs,
    }
}

/// Convert RGB (0..1) to HSL-ish components: hue (0..1), saturation, luminance.
fn rgb_to_hsl(c: [f32; 3]) -> (f32, f32, f32) {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let h = if d == 0.0 {
        0.0
    } else if max == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0) / 6.0
    } else if max == c[1] {
        ((c[2] - c[0]) / d + 2.0) / 6.0
    } else {
        ((c[0] - c[1]) / d + 4.0) / 6.0
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |t: f32| -> f32 {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0)]
}

/// Non-separable blend modes operate on the color as a whole.
fn blend_nonseparable(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::Hue => {
            let (_, s, l) = rgb_to_hsl(cb);
            let (h, _, _) = rgb_to_hsl(cs);
            hsl_to_rgb(h, s, l)
        }
        BlendMode::Saturation => {
            let (h, _, l) = rgb_to_hsl(cb);
            let (_, s, _) = rgb_to_hsl(cs);
            hsl_to_rgb(h, s, l)
        }
        BlendMode::Color => {
            let (h, s, _) = rgb_to_hsl(cs);
            let (_, _, l) = rgb_to_hsl(cb);
            hsl_to_rgb(h, s, l)
        }
        BlendMode::Luminosity => {
            let (h, s, _) = rgb_to_hsl(cb);
            let (_, _, l) = rgb_to_hsl(cs);
            hsl_to_rgb(h, s, l)
        }
        // Separable modes never reach here.
        _ => cs,
    }
}

/// Composite one premultiplied source pixel over a premultiplied backdrop pixel
/// using the given blend mode, returning a premultiplied result.
///
/// Implements the W3C general blend + composite formula:
/// `Co = αs·(1−αb)·Cs + αs·αb·B(Cb,Cs) + (1−αs)·αb·Cb`, `αo = αs + αb·(1−αs)`.
pub fn composite(src: [u8; 4], backdrop: [u8; 4], mode: BlendMode) -> [u8; 4] {
    let src_alpha = src[3] as f32 / 255.0;
    let backdrop_alpha = backdrop[3] as f32 / 255.0;
    if src_alpha <= 0.0 {
        return backdrop;
    }
    if backdrop_alpha <= 0.0 {
        return src;
    }

    let cs = PixelBuffer::unpremultiply_pixel(&src); // straight source
    let cb = PixelBuffer::unpremultiply_pixel(&backdrop); // straight backdrop

    let blended = if matches!(
        mode,
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity
    ) {
        blend_nonseparable(mode, [cb[0], cb[1], cb[2]], [cs[0], cs[1], cs[2]])
    } else {
        [
            blend_separable(mode, cb[0], cs[0]),
            blend_separable(mode, cb[1], cs[1]),
            blend_separable(mode, cb[2], cs[2]),
        ]
    };

    let ao = (src_alpha + backdrop_alpha * (1.0 - src_alpha)).clamp(0.0, 1.0);
    let co = |i: usize| -> f32 {
        src_alpha * (1.0 - backdrop_alpha) * cs[i]
            + src_alpha * backdrop_alpha * blended[i]
            + (1.0 - src_alpha) * backdrop_alpha * cb[i]
    };
    let color = [co(0), co(1), co(2)];
    PixelBuffer::premultiply_pixel(color, ao)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_over_opaque() {
        // Opaque red over opaque blue should stay red.
        let r = composite([255, 0, 0, 255], [0, 0, 255, 255], BlendMode::Normal);
        assert_eq!(r, [255, 0, 0, 255]);
    }

    #[test]
    fn multiply_darkens() {
        // Multiply of white over gray keeps gray; multiply of black over anything is black.
        let r = composite([0, 0, 0, 255], [200, 200, 200, 255], BlendMode::Multiply);
        assert_eq!(r[3], 255);
        assert!(r[0] <= 5 && r[1] <= 5 && r[2] <= 5);
    }

    #[test]
    fn transparent_source_unchanged() {
        let bg = [10, 20, 30, 40];
        assert_eq!(composite([0, 0, 0, 0], bg, BlendMode::Normal), bg);
    }

    #[test]
    fn semi_transparent_source_over_opaque_backdrop() {
        // 50% red over opaque blue → purple-ish (0.5, 0, 0.5, 1) premultiplied.
        let r = composite([128, 0, 0, 128], [0, 0, 255, 255], BlendMode::Normal);
        assert_eq!(r[3], 255);
        assert!((r[0] as i32 - 128).abs() <= 2, "red = {}", r[0]);
        assert!(r[1] <= 2, "green = {}", r[1]);
        assert!((r[2] as i32 - 128).abs() <= 2, "blue = {}", r[2]);
    }

    #[test]
    fn color_burn_black_source_stays_black() {
        // Color Burn with black source yields black (matches Photoshop).
        let r = composite([0, 0, 0, 255], [180, 180, 180, 255], BlendMode::ColorBurn);
        assert!(r[0] <= 5 && r[1] <= 5 && r[2] <= 5);
    }
}
