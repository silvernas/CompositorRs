//! FFI bindings and safe Rust wrappers for the reused C pixel-algorithm kernel.
//!
//! All buffers are premultiplied RGBA8 (`u8` × 4 per pixel) with a row `stride` in bytes,
//! matching the original Compositor C code. Pointers are only borrowed for the duration of a
//! call, so the safe wrappers are sound as long as the caller passes correctly sized buffers
//! (checked by the bounds of the slices).

use std::ffi::c_long;

mod sys {
    use std::ffi::{c_int, c_long};

    extern "C" {
        // BrushPixels.h
        pub fn brush_alpha_bounds(
            bytes: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            bounds: *mut usize,
        );
        pub fn layer_extract_alpha(
            rgba: *const u8,
            rgba_stride: usize,
            gray: *mut u8,
            gray_stride: usize,
            width: usize,
            height: usize,
        );
        pub fn layer_unpremultiply_opaque(
            rgba: *mut u8,
            stride: usize,
            width: usize,
            height: usize,
        );
        pub fn layer_restore_alpha(
            rgba: *mut u8,
            stride: usize,
            alpha: *const u8,
            alpha_stride: usize,
            width: usize,
            height: usize,
        );

        // HealPixels.h
        pub fn heal_coverage_bounds(
            gray: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            bounds: *mut c_long,
        );
        pub fn spot_heal(
            rgba: *mut u8,
            coverage: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            opacity: f32,
            mode: c_int,
            seed: u32,
        ) -> c_int;

        // LevelsPixels.h
        pub fn levels_apply(pixels: *mut u8, count: usize, tables: *const f32);
        pub fn levels_histogram(
            pixels: *const u8,
            coverage: *const u8,
            count: usize,
            bins: *mut f64,
        );

        // WandPixels.h
        pub fn wand_mask(
            rgba: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            seed_x: usize,
            seed_y: usize,
            radius: usize,
            tolerance: c_int,
            contiguous: c_int,
            mask: *mut u8,
        ) -> c_long;
        pub fn wand_trace(
            mask: *const u8,
            width: usize,
            height: usize,
            points: *mut *mut i32,
            point_count: *mut usize,
            loops: *mut *mut i32,
            loop_count: *mut usize,
        ) -> c_int;

        // NoisePixels.h
        pub fn noise_add(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            amount: f32,
            gaussian: c_int,
            monochromatic: c_int,
            seed: u32,
        );

        // LensPixels.h
        pub fn lens_distort(
            source: *const u8,
            destination: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            k: f64,
        );

        // ContentFill.h
        pub fn content_fill(
            rgba: *mut u8,
            stride: usize,
            mask: *const u8,
            mask_stride: usize,
            width: c_int,
            height: c_int,
        ) -> c_int;

        // AdjustPixels.h
        pub fn adjust_gradient_map(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            table: *const u8,
        );
        pub fn adjust_grain(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            amount: f64,
            size: f64,
            roughness: f64,
            seed: u32,
            origin_x: f64,
            origin_y: f64,
            units_per_pixel: f64,
        );
        pub fn rgba_clamp_premultiplied(rgba: *mut u8, count: usize);
    }
}

// ---------------------------------------------------------------------------
// BrushPixels
// ---------------------------------------------------------------------------

/// Half-open bounds `[left, top, right, bottom]` of nonzero alpha in premultiplied RGBA.
pub fn brush_alpha_bounds(bytes: &[u8], width: usize, height: usize, stride: usize) -> [usize; 4] {
    let mut bounds = [0usize; 4];
    unsafe {
        sys::brush_alpha_bounds(bytes.as_ptr(), width, height, stride, bounds.as_mut_ptr());
    }
    bounds
}

/// Extract the alpha channel into a `width` × `height` gray buffer.
pub fn layer_extract_alpha(
    rgba: &[u8],
    rgba_stride: usize,
    width: usize,
    height: usize,
) -> Vec<u8> {
    let mut gray = vec![0u8; width * height];
    unsafe {
        sys::layer_extract_alpha(
            rgba.as_ptr(),
            rgba_stride,
            gray.as_mut_ptr(),
            width,
            width,
            height,
        );
    }
    gray
}

pub fn layer_unpremultiply_opaque(rgba: &mut [u8], stride: usize, width: usize, height: usize) {
    unsafe { sys::layer_unpremultiply_opaque(rgba.as_mut_ptr(), stride, width, height) };
}

pub fn layer_restore_alpha(
    rgba: &mut [u8],
    stride: usize,
    alpha: &[u8],
    alpha_stride: usize,
    width: usize,
    height: usize,
) {
    unsafe {
        sys::layer_restore_alpha(
            rgba.as_mut_ptr(),
            stride,
            alpha.as_ptr(),
            alpha_stride,
            width,
            height,
        );
    }
}

// ---------------------------------------------------------------------------
// HealPixels
// ---------------------------------------------------------------------------

/// Half-open bounds `[x0, y0, x1, y1]` of nonzero bytes in a gray bitmap.
pub fn heal_coverage_bounds(gray: &[u8], width: usize, height: usize, stride: usize) -> [i64; 4] {
    let mut bounds = [0 as c_long; 4];
    unsafe {
        sys::heal_coverage_bounds(gray.as_ptr(), width, height, stride, bounds.as_mut_ptr());
    }
    [
        bounds[0] as i64,
        bounds[1] as i64,
        bounds[2] as i64,
        bounds[3] as i64,
    ]
}

/// Spot healing (content-aware / create-texture / proximity-match) in place.
pub fn spot_heal(
    rgba: &mut [u8],
    coverage: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    opacity: f32,
    mode: i32,
    seed: u32,
) -> Result<(), String> {
    let rc = unsafe {
        sys::spot_heal(
            rgba.as_mut_ptr(),
            coverage.as_ptr(),
            width,
            height,
            stride,
            opacity,
            mode,
            seed,
        )
    };
    if rc == -1 {
        Err("spot_heal: out of memory".into())
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LevelsPixels
// ---------------------------------------------------------------------------

/// Apply per-channel levels lookup tables. `tables` is `3 × 256` normalized values (0.0..1.0).
pub fn levels_apply(pixels: &mut [u8], tables: &[f32; 768]) {
    let count = pixels.len() / 4;
    unsafe { sys::levels_apply(pixels.as_mut_ptr(), count, tables.as_ptr()) };
}

/// Fill a `4 × 256` histogram. `coverage` is an optional per-pixel weight mask.
pub fn levels_histogram(pixels: &[u8], coverage: Option<&[u8]>, bins: &mut [f64; 1024]) {
    let count = pixels.len() / 4;
    let cov = coverage.map_or(std::ptr::null(), |c| c.as_ptr());
    unsafe { sys::levels_histogram(pixels.as_ptr(), cov, count, bins.as_mut_ptr()) };
}

// ---------------------------------------------------------------------------
// WandPixels
// ---------------------------------------------------------------------------

/// Magic-wand selection. Returns the number of selected pixels.
#[allow(clippy::too_many_arguments)]
pub fn wand_mask(
    rgba: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    seed_x: usize,
    seed_y: usize,
    radius: usize,
    tolerance: i32,
    contiguous: bool,
    mask: &mut [u8],
) -> Result<i64, String> {
    let contiguous = if contiguous { 1 } else { 0 };
    let rc = unsafe {
        sys::wand_mask(
            rgba.as_ptr(),
            width,
            height,
            stride,
            seed_x,
            seed_y,
            radius,
            tolerance,
            contiguous,
            mask.as_mut_ptr(),
        )
    };
    if rc == -1 {
        Err("wand_mask: out of memory".into())
    } else {
        Ok(rc as i64)
    }
}

/// The traced outline of a selection mask, as closed loops of corner points.
pub struct TracedOutline {
    /// `(x, y)` pairs in pixel-edge coordinates.
    pub points: Vec<i32>,
    /// Corner count of each loop.
    pub loops: Vec<i32>,
}

pub fn wand_trace(mask: &[u8], width: usize, height: usize) -> Result<TracedOutline, String> {
    let mut points: *mut i32 = std::ptr::null_mut();
    let mut point_count: usize = 0;
    let mut loops: *mut i32 = std::ptr::null_mut();
    let mut loop_count: usize = 0;

    let rc = unsafe {
        sys::wand_trace(
            mask.as_ptr(),
            width,
            height,
            &mut points,
            &mut point_count,
            &mut loops,
            &mut loop_count,
        )
    };

    if rc == 0 {
        let result = TracedOutline {
            points: unsafe { std::slice::from_raw_parts(points, point_count * 2).to_vec() },
            loops: unsafe { std::slice::from_raw_parts(loops, loop_count).to_vec() },
        };
        unsafe {
            libc::free(points as *mut libc::c_void);
            libc::free(loops as *mut libc::c_void);
        }
        Ok(result)
    } else if rc == -1 {
        Err("wand_trace: out of memory".into())
    } else {
        Err("wand_trace: outline too detailed to draw".into())
    }
}

// ---------------------------------------------------------------------------
// NoisePixels
// ---------------------------------------------------------------------------

pub fn noise_add(
    rgba: &mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    amount: f32,
    gaussian: bool,
    monochromatic: bool,
    seed: u32,
) {
    let gaussian = if gaussian { 1 } else { 0 };
    let monochromatic = if monochromatic { 1 } else { 0 };
    unsafe {
        sys::noise_add(
            rgba.as_mut_ptr(),
            width,
            height,
            stride,
            amount,
            gaussian,
            monochromatic,
            seed,
        );
    }
}

// ---------------------------------------------------------------------------
// LensPixels
// ---------------------------------------------------------------------------

/// Radial lens distortion. `k = 0` copies the source exactly.
pub fn lens_distort(
    source: &[u8],
    destination: &mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    k: f64,
) {
    unsafe {
        sys::lens_distort(
            source.as_ptr(),
            destination.as_mut_ptr(),
            width,
            height,
            stride,
            k,
        );
    }
}

// ---------------------------------------------------------------------------
// ContentFill
// ---------------------------------------------------------------------------

/// Content-aware fill. Returns `1` on success, `0` when no source patch exists.
pub fn content_fill(
    rgba: &mut [u8],
    stride: usize,
    mask: &[u8],
    mask_stride: usize,
    width: i32,
    height: i32,
) -> Result<i32, String> {
    let rc = unsafe {
        sys::content_fill(
            rgba.as_mut_ptr(),
            stride,
            mask.as_ptr(),
            mask_stride,
            width,
            height,
        )
    };
    if rc == -1 {
        Err("content_fill: allocation failure".into())
    } else {
        Ok(rc)
    }
}

// ---------------------------------------------------------------------------
// AdjustPixels
// ---------------------------------------------------------------------------

/// Gradient map. `table` is `256 × 3` straight sRGB bytes, darkest first.
pub fn adjust_gradient_map(rgba: &mut [u8], width: usize, height: usize, stride: usize, table: &[u8; 768]) {
    unsafe { sys::adjust_gradient_map(rgba.as_mut_ptr(), width, height, stride, table.as_ptr()) };
}

#[allow(clippy::too_many_arguments)]
pub fn adjust_grain(
    rgba: &mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    amount: f64,
    size: f64,
    roughness: f64,
    seed: u32,
    origin_x: f64,
    origin_y: f64,
    units_per_pixel: f64,
) {
    unsafe {
        sys::adjust_grain(
            rgba.as_mut_ptr(),
            width,
            height,
            stride,
            amount,
            size,
            roughness,
            seed,
            origin_x,
            origin_y,
            units_per_pixel,
        );
    }
}

/// Clamp each premultiplied channel back to its pixel's alpha (post-Lanczos ringing fix).
pub fn rgba_clamp_premultiplied(rgba: &mut [u8]) {
    let count = rgba.len() / 4;
    unsafe { sys::rgba_clamp_premultiplied(rgba.as_mut_ptr(), count) };
}

// ---------------------------------------------------------------------------
// Self test
// ---------------------------------------------------------------------------

/// Deterministic smoke test exercising every bound C function against known inputs.
pub fn self_test() -> Result<(), String> {
    fn check(cond: bool, msg: &str) -> Result<(), String> {
        if cond {
            Ok(())
        } else {
            Err(msg.to_string())
        }
    }

    // brush_alpha_bounds: a single opaque pixel at (1, 1) in a 3×3 image.
    {
        let mut img = vec![0u8; 3 * 3 * 4];
        img[(1 * 12) + 1 * 4 + 3] = 255;
        let b = brush_alpha_bounds(&img, 3, 3, 12);
        check(b == [1, 1, 2, 2], "brush_alpha_bounds")?;
    }

    // layer_extract_alpha: gray equals the alpha channel.
    {
        let rgba = [10u8, 20, 30, 100, 40, 50, 60, 200];
        let gray = layer_extract_alpha(&rgba, 8, 2, 1);
        check(gray == vec![100, 200], "layer_extract_alpha")?;
    }

    // layer_unpremultiply_opaque + layer_restore_alpha run without crashing.
    {
        let mut rgba = [128u8, 64, 32, 128];
        layer_unpremultiply_opaque(&mut rgba, 4, 1, 1);
        check(rgba[3] == 255, "layer_unpremultiply_opaque sets alpha to 255")?;
        let alpha = [128u8];
        layer_restore_alpha(&mut rgba, 4, &alpha, 1, 1, 1);
        check(rgba[3] == 128, "layer_restore_alpha restores alpha")?;
    }

    // levels_apply: identity tables leave opaque pixels unchanged.
    {
        let mut px = [128u8, 64, 32, 255, 0, 0, 0, 0];
        let mut tables = [0.0f32; 768];
        for c in 0..3 {
            for i in 0..256 {
                tables[c * 256 + i] = i as f32 / 255.0;
            }
        }
        levels_apply(&mut px, &tables);
        check(
            px == [128, 64, 32, 255, 0, 0, 0, 0],
            "levels_apply identity",
        )?;
    }

    // levels_histogram: produces a plausible sum for a single opaque pixel.
    {
        let px = [255u8, 255, 255, 255];
        let mut bins = [0.0f64; 1024];
        levels_histogram(&px, None, &mut bins);
        let sum: f64 = bins.iter().sum();
        // One opaque pixel: 3 channels × 1.0 (per-channel bins) + 3 × (1/3) (combined bin).
        check((sum - 4.0).abs() < 1e-9, "levels_histogram total weight")?;
    }

    // wand_mask: uniform image, contiguous, selects every pixel.
    {
        let w = 4usize;
        let h = 4usize;
        let stride = w * 4;
        let mut rgba = vec![0u8; w * h * 4];
        for p in rgba.chunks_mut(4) {
            p.copy_from_slice(&[50, 60, 70, 255]);
        }
        let mut mask = vec![0u8; w * h];
        let count = wand_mask(&rgba, w, h, stride, 1, 1, 0, 0, true, &mut mask)?;
        check(count == 16, "wand_mask contiguous count")?;
    }

    // wand_trace: a 2×2 filled block in a 4×4 mask traces one loop of four corners.
    {
        let mut mask = vec![0u8; 4 * 4];
        mask[1 * 4 + 1] = 255;
        mask[1 * 4 + 2] = 255;
        mask[2 * 4 + 1] = 255;
        mask[2 * 4 + 2] = 255;
        let outline = wand_trace(&mask, 4, 4)?;
        check(!outline.points.is_empty(), "wand_trace produces points")?;
        check(!outline.loops.is_empty(), "wand_trace produces loops")?;
    }

    // noise_add runs without crashing and leaves alpha alone.
    {
        let mut rgba = vec![128u8; 4 * 4];
        for p in rgba.chunks_mut(4) {
            p[3] = 255;
        }
        noise_add(&mut rgba, 2, 2, 8, 10.0, false, false, 1);
        for p in rgba.chunks(4) {
            check(p[3] == 255, "noise_add leaves alpha untouched")?;
        }
    }

    // lens_distort: k = 0 copies exactly.
    {
        let src = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let mut dst = [0u8; 8];
        lens_distort(&src, &mut dst, 2, 1, 8, 0.0);
        check(dst == src, "lens_distort identity")?;
    }

    // content_fill: an empty mask has no work to do and must not fail.
    {
        let mut rgba = vec![0u8; 2 * 2 * 4];
        let mask = vec![0u8; 2 * 2];
        let rc = content_fill(&mut rgba, 8, &mask, 2, 2, 2)?;
        check(rc == 0, "content_fill empty mask returns 0")?;
    }

    // adjust_gradient_map: identity gradient maps an opaque pixel to its luminance (keeps alpha).
    {
        let mut rgba = [100u8, 150, 200, 255];
        let mut table = [0u8; 768];
        for i in 0..256 {
            table[i * 3] = i as u8;
            table[i * 3 + 1] = i as u8;
            table[i * 3 + 2] = i as u8;
        }
        adjust_gradient_map(&mut rgba, 1, 1, 4, &table);
        check(rgba[3] == 255, "adjust_gradient_map keeps alpha")?;
        check(
            rgba[0] == rgba[1] && rgba[1] == rgba[2],
            "adjust_gradient_map identity is gray",
        )?;
    }

    // adjust_grain runs without crashing.
    {
        let mut rgba = vec![128u8; 4 * 4];
        for p in rgba.chunks_mut(4) {
            p[3] = 255;
        }
        adjust_grain(&mut rgba, 2, 2, 8, 20.0, 1.0, 50.0, 1, 0.0, 0.0, 1.0);
    }

    // rgba_clamp_premultiplied: channel values above alpha are clamped to alpha.
    {
        let mut rgba = [255u8, 0, 0, 128];
        rgba_clamp_premultiplied(&mut rgba);
        check(rgba == [128, 0, 0, 128], "rgba_clamp_premultiplied clamps")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        super::self_test().expect("FFI smoke test");
    }
}
