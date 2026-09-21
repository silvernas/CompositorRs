//! Premultiplied RGBA8 pixel buffer — the universal image representation.
//!
//! Every buffer in Compositor is stored premultiplied (the color channels are
//! already multiplied by alpha) with 8 bits per channel, 4 bytes per pixel,
//! row-major with a tight `width * 4` stride. This matches the reused C pixel
//! algorithms exactly, so buffers can be passed to FFI without conversion.

use std::sync::atomic::{AtomicU64, Ordering};

/// A rectangular buffer of premultiplied RGBA8 pixels.
///
/// `data` has length `width * height * 4`. Channel order is R, G, B, A.
/// Because the data is premultiplied, `(r, g, b)` are each `<= a` (in 0..=255).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelBuffer {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl PixelBuffer {
    /// A new zeroed (fully transparent) buffer.
    pub fn new(width: u32, height: u32) -> Self {
        let data = vec![0u8; width as usize * height as usize * 4];
        Self { width, height, data }
    }

    /// A solid opaque buffer of the given straight (non-premultiplied) color.
    pub fn solid(width: u32, height: u32, r: u8, g: u8, b: u8) -> Self {
        let mut buf = Self::new(width, height);
        for px in buf.data.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = 255;
        }
        buf
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Byte length of one row.
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    /// Approximate memory footprint in bytes (the retained cost for history).
    pub fn byte_size(&self) -> usize {
        self.data.len()
    }

    /// Unpremultiply a single pixel in place (used before separable blend math).
    #[inline]
    pub fn unpremultiply_pixel(px: &[u8; 4]) -> [f32; 4] {
        let a = px[3] as f32 / 255.0;
        if a <= 0.0 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        [
            px[0] as f32 / 255.0 / a,
            px[1] as f32 / 255.0 / a,
            px[2] as f32 / 255.0 / a,
            a,
        ]
    }

    /// Premultiply a straight-color pixel back into RGBA8 storage.
    #[inline]
    pub fn premultiply_pixel(c: [f32; 3], a: f32) -> [u8; 4] {
        let a = a.clamp(0.0, 1.0);
        [
            (c[0] * a * 255.0).round().clamp(0.0, 255.0) as u8,
            (c[1] * a * 255.0).round().clamp(0.0, 255.0) as u8,
            (c[2] * a * 255.0).round().clamp(0.0, 255.0) as u8,
            (a * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }
}

/// A stable, process-unique identifier for a layer (mirrors Swift's `UUID`).
///
/// Implemented with a random seed plus a monotonic counter so it needs no
/// external crate and stays unique across copy/duplicate operations.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LayerId(pub u128);

impl LayerId {
    /// Generate a fresh unique id.
    pub fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        static SEED: AtomicU64 = AtomicU64::new(0);
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let seed = match SEED.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => {
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0x9E3779B97F4A7C15);
                // Mix to a non-trivial seed.
                let mixed = nanos.wrapping_mul(0x2545F4914F6CDD1D).wrapping_add(0x123456789ABCDEF0);
                SEED.store(mixed, Ordering::SeqCst);
                mixed
            }
            Err(existing) => existing,
        };
        let low = COUNTER.fetch_add(1, Ordering::SeqCst);
        LayerId(((seed as u128) << 64) | (low as u128))
    }
}

impl Default for LayerId {
    fn default() -> Self {
        Self::new()
    }
}
