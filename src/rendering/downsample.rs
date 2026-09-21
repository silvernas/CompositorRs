//! Area-average downsampling for zoomed-out display and thumbnails.

use crate::core::pixel_buffer::PixelBuffer;
use rayon::prelude::*;

/// Downscale `source` so its larger dimension is at most `max_dim`, or clone if smaller.
/// Averages every covered source pixel (valid for premultiplied data).
pub fn downsample(source: &PixelBuffer, max_dim: u32) -> PixelBuffer {
    let scale = (max_dim as f32 / source.width.max(source.height).max(1) as f32).min(1.0);
    if scale >= 1.0 {
        return source.clone();
    }
    let dw = (source.width as f32 * scale).round().max(1.0) as usize;
    let dh = (source.height as f32 * scale).round().max(1.0) as usize;
    let sw = source.width as usize;
    let sh = source.height as usize;
    let mut out = vec![0u8; dw * dh * 4];
    out.par_chunks_mut(dw * 4).enumerate().for_each(|(y, row)| {
        let y0 = (y * sh / dh).max(0);
        let y1 = (((y + 1) * sh) / dh).max(y0 + 1).min(sh);
        for x in 0..dw {
            let x0 = x * sw / dw;
            let x1 = (((x + 1) * sw) / dw).max(x0 + 1).min(sw);
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for sy in y0..y1 {
                let row_src = &source.data[sy * sw * 4..];
                for sx in x0..x1 {
                    let o = sx * 4;
                    for c in 0..4 {
                        acc[c] += row_src[o + c] as u32;
                    }
                    n += 1;
                }
            }
            for c in 0..4 {
                row[x * 4 + c] = (acc[c] / n).min(255) as u8;
            }
        }
    });
    PixelBuffer { width: dw as u32, height: dh as u32, data: out }
}
