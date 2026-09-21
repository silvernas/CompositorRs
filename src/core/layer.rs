//! Layer, mask, effects, adjustments, transform and the asset type.
//!
//! This is the direct Rust translation of the Swift `ImageLayer` / `LayerMask`
//! / `LayerEffects` / `LayerAdjustment` model. Everything here is plain data with
//! no dependency on any windowing or GPU system, so it can be cloned for history
//! snapshots and (de)serialized for project files.

use crate::core::blend::BlendMode;
use crate::core::pixel_buffer::{LayerId, PixelBuffer};

/// How a layer is resampled when drawn at a different scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerSampling {
    Nearest,
    Smooth,
    High,
}

impl Default for LayerSampling {
    fn default() -> Self {
        LayerSampling::High
    }
}

/// A layer's placement on the document, in document pixels (y grows downward).
///
/// Mirrors `LayerTransform`: an unrotated box plus an optional rotation (clockwise,
/// degrees) and axis flips. `origin` is the top-left of the unrotated box.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LayerTransform {
    pub origin: (f32, f32),
    pub size: (f32, f32),
    pub rotation: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub sampling: LayerSampling,
}

impl Default for LayerTransform {
    fn default() -> Self {
        LayerTransform {
            origin: (0.0, 0.0),
            size: (1.0, 1.0),
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            sampling: LayerSampling::High,
        }
    }
}

impl LayerTransform {
    /// A transform whose box is exactly `size` at `origin` (1:1 with pixel grid).
    pub fn from_size(origin: (f32, f32), size: (u32, u32)) -> Self {
        LayerTransform {
            origin,
            size: (size.0 as f32, size.1 as f32),
            ..Default::default()
        }
    }

    pub fn center(&self) -> (f32, f32) {
        (
            self.origin.0 + self.size.0 / 2.0,
            self.origin.1 + self.size.1 / 2.0,
        )
    }

    pub fn radians(&self) -> f32 {
        self.rotation.to_radians()
    }

    /// Whether the transform holds finite, in-range values (mirrors `isValid`).
    pub fn is_valid(&self) -> bool {
        let ok = |v: f32| v.is_finite();
        ok(self.origin.0)
            && ok(self.origin.1)
            && ok(self.size.0)
            && ok(self.size.1)
            && ok(self.rotation)
            && self.size.0 >= 1.0
            && self.size.0 <= 300_000.0
            && self.size.1 >= 1.0
            && self.size.1 <= 300_000.0
            && self.origin.0.abs() <= 1_000_000.0
            && self.origin.1.abs() <= 1_000_000.0
    }
}

/// An imported (or generated) raster plus a downscaled thumbnail and a name.
///
/// Mirrors Swift's `ImportedImage`. The thumbnail is used by the UI; the model
/// keeps it so history snapshots and project serialization stay faithful.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Asset {
    pub image: PixelBuffer,
    pub thumbnail: PixelBuffer,
    pub name: String,
}

impl Asset {
    pub fn new(image: PixelBuffer, name: String) -> Self {
        // Cheap placeholder thumbnail; the IO layer can replace it with a real one.
        let thumbnail = downscale_thumbnail(&image);
        Self {
            image,
            thumbnail,
            name,
        }
    }

    pub fn solid_reveal() -> Self {
        Self::new(PixelBuffer::solid(1, 1, 255, 255, 255), "Layer Mask".into())
    }

    pub fn solid_hide() -> Self {
        Self::new(PixelBuffer::solid(1, 1, 0, 0, 0), "Layer Mask".into())
    }
}

/// A 1×1 or small grayscale thumbnail placeholder. Real downscaling (e.g. with
/// the `image` crate) is wired up in the IO layer; this keeps the model building.
fn downscale_thumbnail(image: &PixelBuffer) -> PixelBuffer {
    let factor = (96.0 / image.width.max(image.height).max(1) as f32).min(1.0);
    let w = (image.width as f32 * factor).max(1.0) as u32;
    let h = (image.height as f32 * factor).max(1.0) as u32;
    // For the placeholder we keep the full image; callers can refine later.
    if w >= image.width && h >= image.height {
        image.clone()
    } else {
        PixelBuffer::new(w, h)
    }
}

/// A layer mask: grayscale coverage (white = reveal). May be offset from the
/// layer's own pixel grid when unlinked.
#[derive(Clone, PartialEq, Debug)]
pub struct LayerMask {
    pub asset: Asset,
    pub is_enabled: bool,
    pub placement: Option<LayerTransform>,
    pub is_linked: bool,
}

impl LayerMask {
    /// The mask's pixels, or `None` while disabled.
    pub fn enabled_image(&self) -> Option<&PixelBuffer> {
        self.is_enabled.then_some(&self.asset.image)
    }
}

/// What a layer draws around itself. Kept with the layer so it follows every edit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StrokeEffect {
    pub enabled: bool,
    pub size: f32,
    pub color: (f32, f32, f32),
    pub opacity: f32,
    pub inside: bool,
}

impl Default for StrokeEffect {
    fn default() -> Self {
        StrokeEffect {
            enabled: true,
            size: 4.0,
            color: (0.0, 0.0, 0.0),
            opacity: 1.0,
            inside: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ShadowEffect {
    pub enabled: bool,
    /// Degrees counter-clockwise from the right (Photoshop's dial: 90 = from above).
    pub angle: f32,
    pub distance: f32,
    pub blur: f32,
    pub color: (f32, f32, f32),
    pub opacity: f32,
}

impl Default for ShadowEffect {
    fn default() -> Self {
        ShadowEffect {
            enabled: true,
            angle: 90.0,
            distance: 20.0,
            blur: 20.0,
            color: (0.0, 0.0, 0.0),
            opacity: 0.5,
        }
    }
}

impl ShadowEffect {
    /// Offset in layer pixels (y grows downward), away from the light.
    pub fn offset(&self) -> (f32, f32) {
        let r = self.angle.to_radians();
        (-r.cos() * self.distance, r.sin() * self.distance)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ColorOverlayEffect {
    pub enabled: bool,
    pub color: (f32, f32, f32),
    pub opacity: f32,
}

impl Default for ColorOverlayEffect {
    fn default() -> Self {
        ColorOverlayEffect {
            enabled: true,
            color: (0.0, 0.0, 0.0),
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct InnerShadowEffect {
    pub enabled: bool,
    pub angle: f32,
    pub distance: f32,
    pub blur: f32,
    pub color: (f32, f32, f32),
    pub opacity: f32,
}

impl Default for InnerShadowEffect {
    fn default() -> Self {
        InnerShadowEffect {
            enabled: true,
            angle: 90.0,
            distance: 10.0,
            blur: 10.0,
            color: (0.0, 0.0, 0.0),
            opacity: 0.5,
        }
    }
}

impl InnerShadowEffect {
    pub fn offset(&self) -> (f32, f32) {
        let r = self.angle.to_radians();
        (-r.cos() * self.distance, r.sin() * self.distance)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerEffectKind {
    Stroke,
    Shadow,
    ColorOverlay,
    InnerShadow,
}

/// A layer's effects bundle (stroke, shadow, color overlay, inner shadow).
#[derive(Clone, PartialEq, Debug)]
pub struct LayerEffects {
    pub stroke: Option<StrokeEffect>,
    pub shadow: Option<ShadowEffect>,
    pub color_overlay: Option<ColorOverlayEffect>,
    pub inner_shadow: Option<InnerShadowEffect>,
}

impl Default for LayerEffects {
    fn default() -> Self {
        LayerEffects {
            stroke: None,
            shadow: None,
            color_overlay: None,
            inner_shadow: None,
        }
    }
}

impl LayerEffects {
    pub fn is_empty(&self) -> bool {
        self.stroke.is_none()
            && self.shadow.is_none()
            && self.color_overlay.is_none()
            && self.inner_shadow.is_none()
    }

    /// Only the enabled effects, for rendering.
    pub fn visible(&self) -> LayerEffects {
        LayerEffects {
            stroke: self.stroke.filter(|e| e.enabled),
            shadow: self.shadow.filter(|e| e.enabled),
            color_overlay: self.color_overlay.filter(|e| e.enabled),
            inner_shadow: self.inner_shadow.filter(|e| e.enabled),
        }
    }

    pub fn contains(&self, kind: LayerEffectKind) -> bool {
        match kind {
            LayerEffectKind::Stroke => self.stroke.is_some(),
            LayerEffectKind::Shadow => self.shadow.is_some(),
            LayerEffectKind::ColorOverlay => self.color_overlay.is_some(),
            LayerEffectKind::InnerShadow => self.inner_shadow.is_some(),
        }
    }
}

// ---------------------------------------------------------------------------
// Adjustments
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdjustmentKind {
    Hsv,
    Levels,
    Curves,
    Exposure,
    GradientMap,
    Grain,
}

impl AdjustmentKind {
    pub fn label(&self) -> &'static str {
        match self {
            AdjustmentKind::Hsv => "Hue/Saturation",
            AdjustmentKind::Levels => "Levels",
            AdjustmentKind::Curves => "Curves",
            AdjustmentKind::Exposure => "Exposure",
            AdjustmentKind::GradientMap => "Gradient Map",
            AdjustmentKind::Grain => "Grain",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AdjustmentColor {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

impl AdjustmentColor {
    pub fn clamped(&self) -> Self {
        let c = |v: f32| v.clamp(0.0, 1.0);
        AdjustmentColor {
            red: c(self.red),
            green: c(self.green),
            blue: c(self.blue),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LevelRange {
    /// Shadow / gamma / highlight in 0..1 (gamma is the exponent).
    pub shadow: f32,
    pub gamma: f32,
    pub highlight: f32,
}

impl Default for LevelRange {
    fn default() -> Self {
        LevelRange {
            shadow: 0.0,
            gamma: 1.0,
            highlight: 1.0,
        }
    }
}

impl LevelRange {
    /// Map a 0..1 input through this range's shadow/gamma/highlight.
    pub fn apply(&self, value: f32) -> f32 {
        let v = value.clamp(0.0, 1.0);
        let lo = self.shadow.min(self.highlight);
        let hi = self.shadow.max(self.highlight);
        if hi <= lo {
            return if v <= lo { 0.0 } else { 1.0 };
        }
        let normalized = (v - lo) / (hi - lo);
        let g = if self.gamma <= 0.0 { 1.0 } else { self.gamma };
        normalized.powf(1.0 / g).clamp(0.0, 1.0)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct LevelsSettings {
    /// Index 0..3 → red, green, blue, composite (rgb).
    pub ranges: [LevelRange; 4],
}

impl Default for LevelsSettings {
    fn default() -> Self {
        LevelsSettings {
            ranges: [LevelRange::default(); 4],
        }
    }
}

impl LevelsSettings {
    pub fn is_identity(&self) -> bool {
        self.ranges.iter().all(|r| *r == LevelRange::default())
    }

    /// Apply the composite then per-channel range to a 0..1 value.
    pub fn apply(&self, value: f32, channel: usize) -> f32 {
        let v = self.ranges[3].apply(value);
        self.ranges[channel.min(2)].apply(v)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CurvesSettings {
    /// 4 channels (R, G, B, RGB), each a monotonic list of control points (x in 0..255).
    pub channels: [Vec<CurvePoint>; 4],
}

impl Default for CurvesSettings {
    fn default() -> Self {
        CurvesSettings {
            channels: [
                vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 255.0, y: 255.0 }],
                vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 255.0, y: 255.0 }],
                vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 255.0, y: 255.0 }],
                vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 255.0, y: 255.0 }],
            ],
        }
    }
}

impl CurvesSettings {
    /// Shape-preserving monotone cubic interpolation of `x` (0..255) on one channel.
    pub fn value(&self, x: f32, channel: usize) -> f32 {
        let p = &self.channels[channel.min(3)];
        if p.len() < 2 {
            return x;
        }
        let x = x.clamp(0.0, 255.0);
        let mut i = 0;
        while i < p.len() - 2 && p[i + 1].x <= x {
            i += 1;
        }
        let h = (p[i + 1].x - p[i].x).max(1e-6);
        let t = ((x - p[i].x) / h).clamp(0.0, 1.0);
        // Slopes (PCHIP-ish, simplified to Catmull-Rom-free monotone).
        let d: Vec<f32> = (0..p.len() - 1).map(|k| (p[k + 1].y - p[k].y) / (p[k + 1].x - p[k].x).max(1e-6)).collect();
        let slope = |j: usize| -> f32 {
            if j == 0 {
                d[0]
            } else if j == p.len() - 1 {
                *d.last().unwrap()
            } else if d[j - 1] * d[j] <= 0.0 {
                0.0
            } else {
                2.0 / (1.0 / d[j - 1] + 1.0 / d[j])
            }
        };
        let y = (2.0 * t * t * t - 3.0 * t * t + 1.0) * p[i].y
            + (t * t * t - 2.0 * t * t + t) * h * slope(i)
            + (-2.0 * t * t * t + 3.0 * t * t) * p[i + 1].y
            + (t * t * t - t * t) * h * slope(i + 1);
        y.clamp(0.0, 255.0)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HueSaturationSettings {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
    pub colorize: bool,
}

impl Default for HueSaturationSettings {
    fn default() -> Self {
        HueSaturationSettings {
            hue: 0.0,
            saturation: 0.0,
            lightness: 0.0,
            colorize: false,
        }
    }
}

impl HueSaturationSettings {
    pub fn is_identity(&self) -> bool {
        !self.colorize && self.hue == 0.0 && self.saturation == 0.0 && self.lightness == 0.0
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ExposureSettings {
    pub exposure: f32,
    pub offset: f32,
    pub gamma: f32,
}

impl Default for ExposureSettings {
    fn default() -> Self {
        ExposureSettings {
            exposure: 0.0,
            offset: 0.0,
            gamma: 1.0,
        }
    }
}

impl ExposureSettings {
    /// 256-entry lookup table (0..1) for one channel.
    pub fn table(&self) -> [f32; 256] {
        let scale = 2.0f32.powf(self.exposure);
        let mut t = [0.0f32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let encoded = i as f32 / 255.0;
            let linear = if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            };
            let lit = (linear * scale + self.offset).max(0.0).powf(1.0 / self.gamma.max(1e-4));
            let out = if lit <= 0.0031308 {
                lit * 12.92
            } else {
                1.055 * lit.powf(1.0 / 2.4) - 0.055
            };
            *slot = out.clamp(0.0, 1.0);
        }
        t
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GradientMapSettings {
    pub shadows: AdjustmentColor,
    pub highlights: AdjustmentColor,
    pub reversed: bool,
}

impl Default for GradientMapSettings {
    fn default() -> Self {
        GradientMapSettings {
            shadows: AdjustmentColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
            },
            highlights: AdjustmentColor {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
            },
            reversed: false,
        }
    }
}

impl GradientMapSettings {
    pub fn ends(&self) -> (AdjustmentColor, AdjustmentColor) {
        if self.reversed {
            (self.highlights, self.shadows)
        } else {
            (self.shadows, self.highlights)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GrainSettings {
    pub amount: f32,
    pub size: f32,
    pub roughness: f32,
    pub seed: u32,
}

impl Default for GrainSettings {
    fn default() -> Self {
        GrainSettings {
            amount: 25.0,
            size: 1.5,
            roughness: 50.0,
            seed: 0,
        }
    }
}

/// A non-destructive adjustment attached to a layer.
#[derive(Clone, PartialEq, Debug)]
pub struct LayerAdjustment {
    pub kind: AdjustmentKind,
    pub hsv: HueSaturationSettings,
    pub levels: LevelsSettings,
    pub curves: CurvesSettings,
    pub exposure: ExposureSettings,
    pub gradient_map: GradientMapSettings,
    pub grain: GrainSettings,
}

impl LayerAdjustment {
    pub fn new(kind: AdjustmentKind) -> Self {
        let mut adj = LayerAdjustment {
            kind,
            hsv: HueSaturationSettings::default(),
            levels: LevelsSettings::default(),
            curves: CurvesSettings::default(),
            exposure: ExposureSettings::default(),
            gradient_map: GradientMapSettings::default(),
            grain: GrainSettings::default(),
        };
        // Match Swift's defaults: Gradient Map runs foreground→background; Grain gets a fresh seed.
        if kind == AdjustmentKind::GradientMap {
            adj.gradient_map = GradientMapSettings::default();
        }
        if kind == AdjustmentKind::Grain {
            adj.grain.seed = fast_seed();
        }
        adj
    }

    pub fn is_valid(&self) -> bool {
        self.levels.ranges.len() == 4 && self.curves.channels.len() == 4
    }
}

fn fast_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0x12345678)
}

// ---------------------------------------------------------------------------
// Shapes & text
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Line,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LayerShapeStyle {
    pub kind: ShapeKind,
    pub color: (f32, f32, f32),
    pub corner_radius: f32,
    pub line_width: Option<f32>,
    pub start: Option<(f32, f32)>,
    pub end: Option<(f32, f32)>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct LayerShape {
    pub style: LayerShapeStyle,
    pub image: PixelBuffer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, PartialEq, Debug)]
pub struct LayerTextStyle {
    pub content: String,
    pub font_name: String,
    pub font_size: f32,
    pub color: (f32, f32, f32),
    pub alignment: TextAlignment,
    pub tracking: f32,
    pub leading: f32,
    pub box_size: Option<(f32, f32)>,
}

impl Default for LayerTextStyle {
    fn default() -> Self {
        LayerTextStyle {
            content: "Text".into(),
            font_name: "Segoe UI".into(),
            font_size: 72.0,
            color: (0.0, 0.0, 0.0),
            alignment: TextAlignment::Left,
            tracking: 0.0,
            leading: 0.0,
            box_size: None,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct LayerText {
    pub style: LayerTextStyle,
    pub image: PixelBuffer,
}

// ---------------------------------------------------------------------------
// The layer itself
// ---------------------------------------------------------------------------

/// A document layer. Mirrors Swift's `ImageLayer`: a flat list with an optional
/// `parent_id` for grouping, plus optional asset / mask / adjustment / shape /
/// text / effects. A group is a layer with `is_group == true` and no asset.
#[derive(Clone, PartialEq, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub asset: Option<Asset>,
    pub transform: LayerTransform,
    pub name: String,
    pub is_visible: bool,
    pub parent_id: Option<LayerId>,
    pub is_group: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub mask_source_id: Option<LayerId>,
    pub mask: Option<LayerMask>,
    pub adjustment: Option<LayerAdjustment>,
    pub shape: Option<LayerShape>,
    pub effects: Option<LayerEffects>,
    pub text: Option<LayerText>,
}

impl Layer {
    /// A new pixel layer from an asset placed at `origin`.
    pub fn from_asset(asset: Asset, origin: (f32, f32)) -> Self {
        let size = (asset.image.width, asset.image.height);
        Layer {
            id: LayerId::new(),
            transform: LayerTransform::from_size(origin, size),
            asset: Some(asset),
            name: "Layer".into(),
            is_visible: true,
            parent_id: None,
            is_group: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            mask_source_id: None,
            mask: None,
            adjustment: None,
            shape: None,
            effects: None,
            text: None,
        }
    }

    /// A new empty (blank) pixel layer of the given document-pixel size.
    pub fn blank(name: String, size: (u32, u32)) -> Self {
        Layer {
            id: LayerId::new(),
            asset: None,
            transform: LayerTransform::from_size((0.0, 0.0), size),
            name,
            is_visible: true,
            parent_id: None,
            is_group: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            mask_source_id: None,
            mask: None,
            adjustment: None,
            shape: None,
            effects: None,
            text: None,
        }
    }

    /// A new folder (group) layer.
    pub fn group(name: String) -> Self {
        Layer {
            id: LayerId::new(),
            asset: None,
            transform: LayerTransform::default(),
            name,
            is_visible: true,
            parent_id: None,
            is_group: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            mask_source_id: None,
            mask: None,
            adjustment: None,
            shape: None,
            effects: None,
            text: None,
        }
    }

    pub fn size(&self) -> (f32, f32) {
        self.transform.size
    }
}
