//! Document selection as grayscale coverage at document resolution.
//!
//! The Swift project stores the selection as a `CGPath`; for a CPU raster
//! editor it is far simpler and faster to keep it as an 8-bit coverage mask
//! (white = selected) at the document's pixel resolution. `None` on the
//! document means *no selection* ("touch everything"); a `Selection` whose
//! coverage is all zero is an explicit empty selection ("touch nothing").

/// How a new outline combines with the existing selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionMode {
    Replace,
    Add,
    Subtract,
    Intersect,
}

/// Grayscale selection coverage. `coverage[y*width + x]` is 0..=255 (selected).
#[derive(Clone, PartialEq, Debug)]
pub struct Selection {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
    pub antialiased: bool,
    /// How far the edge fades, in document pixels. 0 is a hard edge.
    pub feather: f32,
}

impl Selection {
    /// An explicit empty selection (nothing selected).
    pub fn empty(width: u32, height: u32) -> Self {
        Selection {
            width,
            height,
            coverage: vec![0u8; width as usize * height as usize],
            antialiased: true,
            feather: 0.0,
        }
    }

    /// A selection of the whole canvas.
    pub fn all(width: u32, height: u32) -> Self {
        Selection {
            width,
            height,
            coverage: vec![255u8; width as usize * height as usize],
            antialiased: true,
            feather: 0.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.coverage.iter().all(|&v| v == 0)
    }

    /// The integer bounding box of selected pixels (min/max inclusive), or `None`.
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let w = self.width as usize;
        let h = self.height as usize;
        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        let mut any = false;
        for y in 0..h {
            for x in 0..w {
                if self.coverage[y * w + x] > 0 {
                    any = true;
                    if x < min_x {
                        min_x = x;
                    }
                    if x > max_x {
                        max_x = x;
                    }
                    if y < min_y {
                        min_y = y;
                    }
                    if y > max_y {
                        max_y = y;
                    }
                }
            }
        }
        if !any {
            None
        } else {
            Some((min_x as u32, min_y as u32, max_x as u32, max_y as u32))
        }
    }

    /// Combine `self` with `other` (both must match dimensions) under `mode`.
    pub fn combine(&self, other: &Selection, mode: SelectionMode) -> Selection {
        debug_assert_eq!(self.width, other.width);
        debug_assert_eq!(self.height, other.height);
        let cov: Vec<u8> = self
            .coverage
            .iter()
            .zip(other.coverage.iter())
            .map(|(&a, &b)| match mode {
                SelectionMode::Replace => b,
                SelectionMode::Add => a.max(b),
                SelectionMode::Subtract => a.saturating_sub(b),
                SelectionMode::Intersect => a.min(b),
            })
            .collect();
        Selection {
            width: self.width,
            height: self.height,
            coverage: cov,
            antialiased: self.antialiased || other.antialiased,
            feather: 0.0,
        }
    }

    /// Invert within the canvas: selected ↔ unselected.
    pub fn invert(&self) -> Selection {
        let cov: Vec<u8> = self.coverage.iter().map(|&v| 255 - v).collect();
        Selection {
            width: self.width,
            height: self.height,
            coverage: cov,
            antialiased: self.antialiased,
            feather: self.feather,
        }
    }

    /// Grow the selection by `amount` pixels (morphological dilation).
    pub fn expand(&self, amount: i32) -> Selection {
        if amount <= 0 {
            return self.clone();
        }
        let cov = dilate(&self.coverage, self.width as usize, self.height as usize, amount as usize);
        Selection {
            width: self.width,
            height: self.height,
            coverage: cov,
            antialiased: self.antialiased,
            feather: self.feather,
        }
    }

    /// Shrink the selection by `amount` pixels (morphological erosion).
    pub fn contract(&self, amount: i32) -> Selection {
        if amount <= 0 {
            return self.clone();
        }
        let cov = erode(&self.coverage, self.width as usize, self.height as usize, amount as usize);
        Selection {
            width: self.width,
            height: self.height,
            coverage: cov,
            antialiased: self.antialiased,
            feather: self.feather,
        }
    }

    /// Soften the edge by `amount` pixels (Gaussian blur of the coverage).
    pub fn feather_by(&self, amount: f32) -> Selection {
        if amount <= 0.0 {
            return self.clone();
        }
        let sigma = amount / 2.0;
        let cov = gaussian_blur(&self.coverage, self.width as usize, self.height as usize, sigma);
        Selection {
            width: self.width,
            height: self.height,
            coverage: cov,
            antialiased: self.antialiased,
            feather: (self.feather.hypot(amount)).min(250.0),
        }
    }

    /// Sample coverage at a document pixel (0..=255), clamped to the canvas.
    pub fn sample(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0;
        }
        self.coverage[y as usize * self.width as usize + x as usize]
    }
}

/// Separable sliding-window max (dilation). Mirrors Swift's `extreme(..., smallest:false)`.
pub fn dilate(source: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    extreme(source, width, height, radius, false)
}

/// Separable sliding-window min (erosion). Mirrors Swift's `extreme(..., smallest:true)`.
pub fn erode(source: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    extreme(source, width, height, radius, true)
}

/// Two-pass (horizontal then vertical) max/min filter using a monotonic deque,
/// so cost does not grow with `radius` (as in `LayerEffects.extreme`).
fn extreme(source: &[u8], width: usize, height: usize, radius: usize, smallest: bool) -> Vec<u8> {
    if width == 0 || height == 0 || source.len() != width * height {
        return vec![0u8; source.len()];
    }
    let radius = radius.max(1);
    let mut pass = vec![0u8; source.len()];
    let mut result = vec![0u8; source.len()];
    let mut queue = vec![0usize; width.max(height)];

    // Horizontal pass.
    for line in 0..height {
        let base = line * width;
        let mut head = 0;
        let mut tail = 0;
        let mut next = 0;
        for center in 0..width {
            while next <= (center + radius).min(width - 1) {
                let value = source[base + next];
                while tail > head {
                    let prev = source[base + queue[tail - 1]];
                    if smallest {
                        if prev <= value {
                            break;
                        }
                    } else if prev >= value {
                        break;
                    }
                    tail -= 1;
                }
                queue[tail] = next;
                tail += 1;
                next += 1;
            }
            while head < tail && queue[head] < center.saturating_sub(radius) {
                head += 1;
            }
            let outside = center < radius || center + radius >= width;
            result[base + center] = if smallest && outside {
                0
            } else {
                source[base + queue[head]]
            };
        }
    }

    // Vertical pass → pass buffer.
    for line in 0..width {
        let base = line;
        // Monotonic deque (head/tail indexes into `queue`) scanning `result` top-down.
        let mut head = 0;
        let mut tail = 0;
        let mut next = 0;
        for center in 0..height {
            while next <= (center + radius).min(height - 1) {
                let value = result[base + next * width];
                while tail > head {
                    let prev = result[base + queue[tail - 1] * width];
                    if smallest {
                        if prev <= value {
                            break;
                        }
                    } else if prev >= value {
                        break;
                    }
                    tail -= 1;
                }
                queue[tail] = next;
                tail += 1;
                next += 1;
            }
            while head < tail && queue[head] < center.saturating_sub(radius) {
                head += 1;
            }
            let idx = base + center * width;
            let outside = center < radius || center + radius >= height;
            pass[idx] = if smallest && outside {
                0
            } else {
                result[base + queue[head] * width]
            };
        }
    }
    pass
}

/// Separable Gaussian blur on an 8-bit buffer (used for feathering).
fn gaussian_blur(source: &[u8], width: usize, height: usize, sigma: f32) -> Vec<u8> {
    if width == 0 || height == 0 {
        return vec![0u8; source.len()];
    }
    let sigma = sigma.max(0.01);
    // Kernel radius ~ 3 sigmas, capped for performance.
    let r = (3.0 * sigma).ceil() as usize;
    let r = r.max(1).min(64);
    let mut kernel = vec![0.0f32; 2 * r + 1];
    let mut sum = 0.0;
    for i in 0..=2 * r {
        let x = i as f32 - r as f32;
        let v = (-0.5 * (x / sigma) * (x / sigma)).exp();
        kernel[i] = v;
        sum += v;
    }
    for v in &mut kernel {
        *v /= sum;
    }

    let mut temp = vec![0f32; source.len()];
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0.0;
            for k in 0..=2 * r {
                let xx = clampi(x as isize + k as isize - r as isize, width as isize);
                acc += source[y * width + xx as usize] as f32 * kernel[k];
            }
            temp[y * width + x] = acc;
        }
    }
    let mut out = vec![0u8; source.len()];
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0.0;
            for k in 0..=2 * r {
                let yy = clampi(y as isize + k as isize - r as isize, height as isize);
                acc += temp[yy as usize * width + x] * kernel[k];
            }
            out[y * width + x] = acc.clamp(0.0, 255.0).round() as u8;
        }
    }
    out
}

#[inline]
fn clampi(v: isize, max: isize) -> isize {
    if v < 0 {
        0
    } else if v >= max {
        max - 1
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dilate_grows() {
        let mut sel = Selection::empty(9, 9);
        sel.coverage[4 * 9 + 4] = 255; // single center pixel
        let grown = sel.expand(1);
        // Center plus its 4-neighbors should now be selected.
        assert_eq!(grown.sample(4, 4), 255);
        assert_eq!(grown.sample(3, 4), 255);
        assert_eq!(grown.sample(5, 4), 255);
        assert_eq!(grown.sample(4, 3), 255);
        assert_eq!(grown.sample(4, 5), 255);
        assert_eq!(grown.sample(0, 0), 0);
    }

    #[test]
    fn erode_shrinks() {
        let mut sel = Selection::all(9, 9);
        let shrunk = sel.contract(1);
        // The border row/col should now be empty.
        assert_eq!(shrunk.sample(0, 0), 0);
        assert_eq!(shrunk.sample(4, 4), 255);
    }

    #[test]
    fn add_combines() {
        let mut a = Selection::empty(5, 5);
        a.coverage[0] = 255;
        let mut b = Selection::empty(5, 5);
        b.coverage[1] = 255;
        let c = a.combine(&b, SelectionMode::Add);
        assert_eq!(c.sample(0, 0), 255);
        assert_eq!(c.sample(1, 0), 255);
    }
}
