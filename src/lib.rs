//! Compositor — a native Windows image editor, rewritten in Rust.
//!
//! - `ffi`: safe wrappers around the reused C pixel-algorithm kernel.
//! - `core`: the pure-data document model (layers, masks, blend modes, selection, history).
//! - `rendering`: CPU compositor, downsampling and adjustment filters.
//! - `io`: image import/export.
//! - `ui`: egui canvas and panels.
//! - `app`: the eframe application shell.

pub mod app;
pub mod core;
pub mod ffi;
pub mod io;
pub mod rendering;
pub mod ui;
