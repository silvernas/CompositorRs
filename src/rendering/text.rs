//! Text rendering for the text tool.
//!
//! Rasterizes a string into a tightly-bounded premultiplied RGBA8 buffer using
//! `ab_glyph`. The system font is looked up once and cached; CJK-capable fonts
//! are preferred so Chinese text renders correctly on Windows.

use std::sync::OnceLock;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};

use crate::core::pixel_buffer::PixelBuffer;

static FONT: OnceLock<Option<FontArc>> = OnceLock::new();

/// Candidate Windows system fonts, CJK-capable first.
const FONT_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\msyh.ttc",    // 微软雅黑
    "C:\\Windows\\Fonts\\msyhbd.ttc",  // 微软雅黑 Bold
    "C:\\Windows\\Fonts\\simhei.ttf",  // 黑体
    "C:\\Windows\\Fonts\\arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
];

fn font() -> Option<&'static FontArc> {
    FONT.get_or_init(|| {
        for path in FONT_CANDIDATES {
            if let Ok(data) = std::fs::read(path) {
                // TTC collections need the first face; try_from_vec reads face 0.
                if let Ok(f) = FontArc::try_from_vec(data) {
                    return Some(f);
                }
            }
        }
        None
    })
    .as_ref()
}

/// Render `text` at `px` points using `color` (sRGB, straight).
///
/// Returns the buffer plus its size in pixels. The origin is the top-left of
/// the glyph box; descenders may extend below `height` (kept inside).
pub fn render_text(text: &str, px: f32, color: [u8; 3]) -> Result<(PixelBuffer, u32, u32), String> {
    let font = font().ok_or_else(|| "未找到可用系统字体（arial/微软雅黑等）".to_string())?;
    let px = px.clamp(4.0, 1000.0);
    let scaled = font.as_scaled(PxScale::from(px));

    // Collect glyphs with pen advance.
    let mut glyphs = Vec::new();
    let mut pen_x = 0.0f32;
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        let glyph = gid.with_scale_and_position(PxScale::from(px), ab_glyph::point(pen_x, 0.0));
        pen_x += scaled.h_advance(gid);
        glyphs.push(glyph);
    }
    if glyphs.is_empty() || pen_x <= 0.0 {
        return Err("没有可渲染的文本".into());
    }

    // Tight bounds over the outlined glyphs.
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for g in &glyphs {
        if let Some(out) = font.outline_glyph(g.clone()) {
            let b = out.px_bounds();
            min_x = min_x.min(b.min.x);
            min_y = min_y.min(b.min.y);
            max_x = max_x.max(b.max.x);
            max_y = max_y.max(b.max.y);
        }
    }
    if !(min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite()) {
        return Err("字形边界计算失败".into());
    }
    let pad = 2i32;
    let w = (max_x.ceil() as i32 - min_x.floor() as i32 + pad * 2).max(1) as u32;
    let h = (max_y.ceil() as i32 - min_y.floor() as i32 + pad * 2).max(1) as u32;
    let (w, h) = (w.min(8192), h.min(8192));

    // Rasterize with premultiplied source-over compositing.
    let mut buf = vec![0u8; w as usize * h as usize * 4];
    let ox = min_x.floor() as i32 - pad;
    let oy = min_y.floor() as i32 - pad;
    let (cr, cg, cb) = (color[0] as f32, color[1] as f32, color[2] as f32);
    for g in &glyphs {
        let Some(out) = font.outline_glyph(g.clone()) else { continue };
        let b = out.px_bounds();
        out.draw(|x, y, cov| {
            let xx = x as i32 + b.min.x as i32 - ox;
            let yy = y as i32 + b.min.y as i32 - oy;
            if xx < 0 || yy < 0 || xx >= w as i32 || yy >= h as i32 {
                return;
            }
            let sa = cov as f32;
            if sa <= 0.0 {
                return;
            }
            let keep = 1.0 - sa;
            let i = (yy as usize * w as usize + xx as usize) * 4;
            buf[i] = (cr * sa + buf[i] as f32 * keep).round().clamp(0.0, 255.0) as u8;
            buf[i + 1] = (cg * sa + buf[i + 1] as f32 * keep).round().clamp(0.0, 255.0) as u8;
            buf[i + 2] = (cb * sa + buf[i + 2] as f32 * keep).round().clamp(0.0, 255.0) as u8;
            let da = buf[i + 3] as f32 / 255.0;
            buf[i + 3] = ((sa + da * keep) * 255.0).round().clamp(0.0, 255.0) as u8;
        });
    }

    let mut pb = PixelBuffer::new(w, h);
    pb.data = buf;
    Ok((pb, w, h))
}
