//! Core document model: pure data with no windowing or GPU dependency.
//!
//! - `pixel_buffer`: premultiplied RGBA8 image type + unique layer ids.
//! - `blend`: layer blend-mode math (W3C compositing formulas).
//! - `layer`: layers, masks, effects, adjustments, transforms, shapes, text.
//! - `selection`: coverage-based selection geometry and operations.
//! - `document`: the canvas + layer list and tree/ordering helpers.
//! - `history`: snapshot-based undo/redo.

pub mod blend;
pub mod document;
pub mod history;
pub mod layer;
pub mod pixel_buffer;
pub mod selection;
