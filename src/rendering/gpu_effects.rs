//! Layer-effects rendering pipeline with an optional GPU backend.
//!
//! The compositor calls [`render`] for any layer carrying enabled effects.
//! A wgpu compute backend can be plugged in behind the `gpu` feature; until
//! then (or whenever the GPU path fails), rendering transparently falls back
//! to the CPU renderer in [`super::effects`], so no-GPU environments always
//! work.

use crate::core::layer::Layer;
use crate::core::pixel_buffer::PixelBuffer;

/// Render the layer's effects into a document-sized premultiplied buffer.
pub fn render(sampled: &PixelBuffer, layer: &Layer) -> PixelBuffer {
    // Future: `if let Some(g) = try_gpu(sampled, layer) { return g; }`
    super::effects::render_effects(sampled, layer)
}

/// Attempt GPU rendering; returns `None` when no GPU backend is available,
/// signalling the caller to use the CPU fallback.
#[allow(dead_code)]
fn try_gpu(_sampled: &PixelBuffer, _layer: &Layer) -> Option<PixelBuffer> {
    // #[cfg(feature = "gpu")] { ... wgpu compute pass ... }
    None
}
