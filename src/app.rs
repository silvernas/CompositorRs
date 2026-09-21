//! The top-level eframe application: documents, history, tools, panels and canvas.

use std::path::PathBuf;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

use crate::core::document::Document;
use crate::core::history::History;
use crate::core::layer::{AdjustmentKind, Asset, HueSaturationSettings, LayerAdjustment};
use crate::core::pixel_buffer::{LayerId, PixelBuffer};
use crate::io::{export, import};
use crate::rendering::{brush, compositor, filters};
use crate::ui::canvas::{self, CanvasView};
use crate::ui::panels;
use crate::ui::tools::{self, MarqueeState, ToolKind, TransformState};

/// Cap on the display texture dimension (high-quality area average).
const MAX_TEXTURE_DIM: u32 = 4096;

/// Load system CJK fonts so Chinese UI strings render instead of empty
/// boxes. egui's built-in fonts cover Latin/Greek/Cyrillic only; the
/// matching system font (Microsoft YaHei on Windows, Noto Sans CJK on
/// Linux, PingFang on macOS) is appended as a fallback for each family.
fn install_cjk_fonts(ctx: &egui::Context) {
    const CANDIDATES: &[(&str, &str)] = &[
        // Windows
        ("msyh", "C:/Windows/Fonts/msyh.ttc"), // Microsoft YaHei
        ("msyhbd", "C:/Windows/Fonts/msyhbd.ttc"), // Microsoft YaHei Bold
        ("simhei", "C:/Windows/Fonts/simhei.ttf"),
        ("simsun", "C:/Windows/Fonts/simsun.ttc"),
        ("deng", "C:/Windows/Fonts/Deng.ttf"),
        // Linux
        ("noto", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        ("noto2", "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
        // macOS
        ("pingfang", "/System/Library/Fonts/PingFang.ttc"),
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = false;
    for (name, path) in CANDIDATES {
        if !std::path::Path::new(path).exists() {
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if ab_glyph::FontArc::try_from_vec(bytes.clone()).is_err() {
            continue; // present but unparsable (e.g. damaged file)
        }
        fonts
            .font_data
            .insert(name.to_string(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push(name.to_string());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push(name.to_string());
        loaded = true;
    }
    if loaded {
        ctx.set_fonts(fonts);
    }
}

/// Export file formats offered by the "Export as" dialog.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
}

impl ExportFormat {
    fn label(&self) -> &'static str {
        match self {
            ExportFormat::Png => "PNG",
            ExportFormat::Jpeg => "JPEG",
        }
    }

    fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpeg => "jpg",
        }
    }
}

pub struct CompositorApp {
    docs: Vec<Document>,
    histories: Vec<History>,
    active: usize,
    active_layer: Option<LayerId>,
    tool: ToolKind,
    brush: brush::BrushParams,
    foreground: [u8; 3],
    marquee: MarqueeState,
    transform: TransformState,
    transform_before: Option<Document>,
    version: u64,
    rendered_version: u64,
    texture: Option<TextureHandle>,
    view: CanvasView,
    status: String,
    last_path: Option<PathBuf>,
    new_doc_open: bool,
    new_w: String,
    new_h: String,
    /// Gaussian blur dialog state.
    blur_dialog_open: bool,
    blur_radius: f32,
    /// Hue/Saturation dialog state.
    hsv_dialog_open: bool,
    hsv_settings: HueSaturationSettings,
    /// Pixel snapshot of the active layer taken when the dialog opens; live
    /// preview/reset re-apply settings from it, cancel restores it.
    hsv_preview: Option<PixelBuffer>,
    /// Live preview checkbox (default on): when on, slider changes re-apply
    /// from the snapshot immediately; when off, they only take effect on Apply.
    hsv_preview_enabled: bool,
    /// True while the layer pixels differ from the snapshot (preview applied).
    hsv_dirty: bool,
    /// "Export as" dialog state.
    export_open: bool,
    export_format: ExportFormat,
    export_quality: u8,
    /// Magic-wand tool settings.
    wand_tolerance: i32,
    /// Text tool dialog state.
    text_dialog_open: bool,
    text_string: String,
    text_size: f32,
    text_color: [f32; 3],
    text_pos: Option<(f32, f32)>,
    /// Move-tool drag: (layer id, before snapshot).
    moving: Option<(LayerId, Document)>,
    /// Brush stroke in progress: (layer id, before snapshot, last doc pos).
    painting: Option<(LayerId, Document, (f32, f32))>,
}

impl CompositorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_cjk_fonts(&cc.egui_ctx);
        let docs = vec![import::new_document(1200, 800)];
        let histories = vec![History::new()];
        let active_layer = docs[0].layers.last().map(|l| l.id);
        let status = "新建文档 1200×800 — 打开图像 (Ctrl+O) 或新建 (Ctrl+N)".to_string();
        CompositorApp {
            docs,
            histories,
            active: 0,
            active_layer,
            tool: ToolKind::Move,
            brush: brush::BrushParams::default(),
            foreground: [0x1a, 0x99, 0xe8],
            marquee: MarqueeState::default(),
            transform: TransformState::default(),
            transform_before: None,
            version: 1,
            rendered_version: 0,
            texture: None,
            view: CanvasView::default(),
            status,
            last_path: None,
            new_doc_open: false,
            new_w: "1920".into(),
            new_h: "1080".into(),
            blur_dialog_open: false,
            blur_radius: 2.0,
            hsv_dialog_open: false,
            hsv_settings: HueSaturationSettings::default(),
            hsv_preview: None,
            hsv_preview_enabled: true,
            hsv_dirty: false,
            export_open: false,
            export_format: ExportFormat::Png,
            export_quality: 90,
            wand_tolerance: 32,
            text_dialog_open: false,
            text_string: String::new(),
            text_size: 48.0,
            text_color: [1.0, 1.0, 1.0],
            text_pos: None,
            moving: None,
            painting: None,
        }
    }

    // ------------------------------------------------------------ documents

    fn doc(&self) -> &Document {
        &self.docs[self.active]
    }

    fn doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.active]
    }

    fn snapshot(&self) -> Document {
        self.docs[self.active].clone()
    }

    /// Commit an edit for undo/redo and invalidate the composite cache.
    fn record_edit(&mut self, name: &str, before: Document) {
        let after = self.docs[self.active].clone();
        let active = self.active_layer;
        self.histories[self.active].record(name, &before, &after, active, active);
        self.version = self.version.wrapping_add(1);
    }

    fn ensure_active_layer(&mut self) {
        if self.active_layer.is_none() || self.doc().layer(self.active_layer.unwrap()).is_none() {
            self.active_layer = self.doc().layers.last().map(|l| l.id);
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        match import::open_document(&path) {
            Ok(doc) => {
                self.docs.push(doc);
                self.histories.push(History::new());
                self.active = self.docs.len() - 1;
                self.active_layer = self.docs[self.active].layers.last().map(|l| l.id);
                self.version = self.version.wrapping_add(1);
                self.last_path = Some(path.clone());
                self.status = format!("已打开 {}", path.display());
            }
            Err(e) => self.status = format!("打开失败: {e}"),
        }
    }

    fn new_document(&mut self) {
        let w = Document::valid_dimension(&self.new_w).unwrap_or(800);
        let h = Document::valid_dimension(&self.new_h).unwrap_or(600);
        self.docs.push(import::new_document(w, h));
        self.histories.push(History::new());
        self.active = self.docs.len() - 1;
        self.active_layer = self.docs[self.active].layers.last().map(|l| l.id);
        self.version = self.version.wrapping_add(1);
        self.new_doc_open = false;
        self.status = format!("新建文档 {w}×{h}");
    }

    fn save_current(&mut self) {
        let composite = compositor::composite_document(self.doc());
        let path = self.last_path.clone().unwrap_or_else(|| PathBuf::from("output.png"));
        match export::save_png(&path, &composite) {
            Ok(()) => self.status = format!("已保存 {}", path.display()),
            Err(e) => self.status = format!("保存失败: {e}"),
        }
    }

    fn rebuild_texture(&mut self, ctx: &egui::Context) {
        let composite = compositor::composite_document(self.doc());
        let display = crate::rendering::downsample::downsample(&composite, MAX_TEXTURE_DIM);
        let img = ColorImage::from_rgba_premultiplied(
            [display.width as usize, display.height as usize],
            &display.data,
        );
        self.texture = Some(ctx.load_texture("canvas", img, TextureOptions::LINEAR));
    }

    // ----------------------------------------------------------- layer edits

    fn add_layer(&mut self) {
        let before = self.snapshot();
        let size = self.doc().size();
        let mut layer = crate::core::layer::Layer::blank("图层".into(), size);
        layer.name = format!("图层 {}", self.doc().layers.len() + 1);
        let id = layer.id;
        let index = self.doc().layers.len();
        self.doc_mut().insert_layer(layer, index);
        self.active_layer = Some(id);
        self.record_edit("新建图层", before);
    }

    /// Insert a new adjustment layer (affects everything below it, like PS).
    fn add_adjustment_layer(&mut self, kind: AdjustmentKind) {
        let before = self.snapshot();
        let size = self.doc().size();
        let mut layer = crate::core::layer::Layer::blank(kind.label().into(), size);
        layer.adjustment = Some(LayerAdjustment::new(kind));
        let id = layer.id;
        let idx = self
            .doc()
            .index_of(self.active_layer.unwrap_or_else(|| self.doc().layers.last().map(|l| l.id).unwrap_or(id)))
            .map(|i| i + 1)
            .unwrap_or(self.doc().layers.len());
        self.doc_mut().insert_layer(layer, idx);
        self.active_layer = Some(id);
        self.record_edit(&format!("新建{}调整图层", kind.label()), before);
    }

    /// Destructive Gaussian blur on the active pixel layer.
    fn apply_gaussian_blur(&mut self, radius: f32) {
        let Some(id) = self.active_layer else { return };
        if !self.layer_is_paintable(id) {
            self.status = "高斯模糊仅支持全尺寸像素图层".into();
            return;
        }
        self.ensure_asset(id);
        let before = self.snapshot();
        if let Some(l) = self.doc_mut().layer_mut(id) {
            if let Some(a) = l.asset.as_mut() {
                filters::gaussian_blur(&mut a.image, radius);
            }
        }
        self.record_edit("高斯模糊", before);
        self.blur_dialog_open = false;
        self.status = format!("已应用高斯模糊（半径 {radius}）");
    }

    /// Open the hue/saturation dialog and snapshot the active layer's pixels so
    /// preview/reset/cancel can always work from the untouched original.
    fn open_hsv_dialog(&mut self) {
        self.hsv_settings = HueSaturationSettings::default();
        self.hsv_preview = None;
        if let Some(id) = self.active_layer {
            if self.layer_is_paintable(id) {
                self.ensure_asset(id);
                let snapshot = self
                    .doc()
                    .layer(id)
                    .and_then(|l| l.asset.as_ref())
                    .map(|a| a.image.clone());
                self.hsv_preview = snapshot;
            }
        }
        self.hsv_dirty = false;
        self.hsv_dialog_open = true;
    }

    /// Re-apply the current settings from the pristine snapshot: live preview,
    /// and the base for the final apply and for "reset". Marks the composite
    /// dirty so the canvas actually redraws (this was the missing piece).
    fn preview_hue_saturation(&mut self) {
        let Some(mut base) = self.hsv_preview.take() else {
            // Defensive: if the snapshot was lost (e.g. dialog closed via Apply
            // but the window somehow stayed open), re-snapshot the current layer
            // and apply once so adjustments always take effect.
            let Some(id) = self.active_layer else { return };
            if !self.layer_is_paintable(id) {
                return;
            }
            self.ensure_asset(id);
            let snapshot = self
                .doc()
                .layer(id)
                .and_then(|l| l.asset.as_ref())
                .map(|a| a.image.clone());
            let Some(snap) = snapshot else { return };
            self.hsv_preview = Some(snap);
            return self.preview_hue_saturation();
        };
        let Some(id) = self.active_layer else {
            self.hsv_preview = Some(base);
            return;
        };
        if !self.layer_is_paintable(id) {
            self.hsv_preview = Some(base);
            return;
        }
        let settings = self.hsv_settings;
        let sel = self.doc().selection.clone();
        if let Some(l) = self.doc_mut().layer_mut(id) {
            if let Some(a) = l.asset.as_mut() {
                if a.image.data.len() == base.data.len() {
                    a.image.data.copy_from_slice(&base.data);
                    filters::apply_hue_saturation(&mut a.image, &settings, sel.as_ref());
                }
            }
        }
        self.hsv_preview = Some(base);
        self.hsv_dirty = true;
        self.version = self.version.wrapping_add(1);
    }

    /// Restore the layer pixels to the pristine snapshot (when the live-preview
    /// checkbox is off, or when cancelling). No-op unless a preview is showing.
    fn restore_hsv_original(&mut self) {
        if !self.hsv_dirty {
            return;
        }
        let Some(id) = self.active_layer else { return };
        let Some(mut base) = self.hsv_preview.take() else { return };
        if let Some(l) = self.doc_mut().layer_mut(id) {
            if let Some(a) = l.asset.as_mut() {
                if a.image.data.len() == base.data.len() {
                    a.image.data.copy_from_slice(&base.data);
                }
            }
        }
        self.hsv_preview = Some(base);
        self.hsv_dirty = false;
        self.version = self.version.wrapping_add(1);
    }

    /// Destructive hue/saturation on the active pixel layer, restricted to the
    /// current selection when one exists (PS "Image ▸ Adjustments ▸ Hue/Saturation").
    /// Applies exactly once (single undo step) from the pristine snapshot, then
    /// closes the dialog.
    fn apply_hue_saturation(&mut self) {
        let Some(id) = self.active_layer else {
            self.hsv_dialog_open = false;
            self.hsv_preview = None;
            return;
        };
        if !self.layer_is_paintable(id) {
            self.status = "色相/饱和度仅支持全尺寸像素图层".into();
            self.hsv_dialog_open = false;
            self.hsv_preview = None;
            return;
        }
        let before = self.snapshot();
        self.preview_hue_saturation();
        self.record_edit("色相/饱和度", before);
        self.hsv_dialog_open = false;
        self.hsv_preview = None;
        self.hsv_dirty = false;
        self.status = "已应用色相/饱和度".into();
    }

    /// Discard the preview and restore the layer's original pixels.
    fn cancel_hsv_dialog(&mut self) {
        self.restore_hsv_original();
        self.hsv_dialog_open = false;
        self.hsv_preview = None;
        self.hsv_dirty = false;
    }

    fn duplicate_layer(&mut self, id: LayerId) {
        let before = self.snapshot();
        let layer = self.doc().layer(id).cloned();
        if let Some(mut l) = layer {
            l.id = LayerId::new();
            l.name = format!("{} 副本", l.name);
            let new_id = l.id;
            let idx = self.doc().index_of(id).map(|i| i + 1).unwrap_or(self.doc().layers.len());
            self.doc_mut().insert_layer(l, idx);
            self.active_layer = Some(new_id);
            self.record_edit("复制图层", before);
        }
    }

    fn delete_layer(&mut self, id: LayerId) {
        let before = self.snapshot();
        self.doc_mut().remove_layer(id);
        if self.active_layer == Some(id) {
            self.active_layer = self.doc().layers.last().map(|l| l.id);
        }
        self.record_edit("删除图层", before);
    }

    fn move_layer(&mut self, id: LayerId, dir: i8) {
        let Some(idx) = self.doc().index_of(id) else { return };
        let target = idx as i64 + dir as i64;
        if target < 0 || target >= self.doc().layers.len() as i64 {
            return;
        }
        let before = self.snapshot();
        let doc = self.doc_mut();
        let layer = doc.layers.remove(idx);
        doc.layers.insert(target as usize, layer);
        self.record_edit(if dir > 0 { "上移图层" } else { "下移图层" }, before);
    }

    fn merge_down(&mut self) {
        let Some(id) = self.active_layer else { return };
        let Some(idx) = self.doc().index_of(id) else { return };
        if idx == 0 {
            self.status = "没有可合并的下层图层".into();
            return;
        }
        let (ok, below_id, above) = {
            let doc = self.doc();
            let above = doc.layers[idx].clone();
            let below = &doc.layers[idx - 1];
            let a_ok = above.asset.is_some() && !above.is_group && above.adjustment.is_none();
            let b_ok = below.asset.is_some() && !below.is_group;
            let b_identity = below.transform.origin.0 == 0.0
                && below.transform.origin.1 == 0.0
                && below.transform.size.0 == below.asset.as_ref().unwrap().image.width as f32
                && below.transform.size.1 == below.asset.as_ref().unwrap().image.height as f32;
            (a_ok && b_ok && b_identity, below.id, above)
        };
        if !ok {
            self.status = "暂不支持该组合的合并（下层需为全尺寸像素图层）".into();
            return;
        }
        let before = self.snapshot();
        {
            let doc = self.doc_mut();
            let below_layer = doc.layer_mut(below_id).expect("below layer exists");
            let mut dst = below_layer.asset.as_ref().unwrap().image.clone();
            compositor::rasterize_layer_into(&mut dst, &above);
            below_layer.asset.as_mut().unwrap().image = dst;
            doc.remove_layer(id);
        }
        self.active_layer = Some(below_id);
        self.record_edit("合并图层", before);
    }

    // ------------------------------------------------------------- selection

    fn select_all(&mut self) {
        let before = self.snapshot();
        let (w, h) = self.doc().size();
        let mut sel = crate::core::selection::Selection::empty(w, h);
        sel.coverage.fill(255);
        self.doc_mut().selection = Some(sel);
        self.record_edit("全选", before);
    }

    fn deselect(&mut self) {
        if self.doc().selection.is_none() {
            return;
        }
        let before = self.snapshot();
        self.doc_mut().selection = None;
        self.record_edit("取消选择", before);
    }

    /// Del: erase the selected area of the active raster layer.
    fn delete_selection(&mut self) {
        let Some(sel) = self.doc().selection.clone() else {
            self.status = "没有选区".into();
            return;
        };
        let Some(id) = self.active_layer else { return };
        if !self.layer_is_paintable(id) {
            self.status = "选区删除仅支持像素图层".into();
            return;
        }
        self.ensure_asset(id);
        let before = self.snapshot();
        if let Some(l) = self.doc_mut().layer_mut(id) {
            if let Some(a) = l.asset.as_mut() {
                brush::clear_selection(&mut a.image, &sel);
            }
        }
        self.record_edit("清除选区内容", before);
    }

    fn fill_foreground(&mut self) {
        let Some(sel) = self.doc().selection.clone() else {
            self.status = "没有选区，无法填充".into();
            return;
        };
        let Some(id) = self.active_layer else { return };
        if !self.layer_is_paintable(id) {
            self.status = "填充仅支持像素图层".into();
            return;
        }
        self.ensure_asset(id);
        let before = self.snapshot();
        let color = [
            self.foreground[0] as f32 / 255.0,
            self.foreground[1] as f32 / 255.0,
            self.foreground[2] as f32 / 255.0,
        ];
        if let Some(l) = self.doc_mut().layer_mut(id) {
            if let Some(a) = l.asset.as_mut() {
                brush::fill_selection(&mut a.image, &sel, color, 1.0);
            }
        }
        self.record_edit("填充", before);
    }

    /// Magic-wand click: build a selection from the active layer's pixels.
    /// Shift adds to the current selection, Alt subtracts.
    fn wand_select(&mut self, seed: (f32, f32), add: bool, sub: bool) {
        let Some(id) = self.active_layer else { return };
        let Some(layer) = self.doc().layer(id) else { return };
        let Some(asset) = &layer.asset else {
            self.status = "请先在图层上填充内容再使用魔棒".into();
            return;
        };
        let (w, h) = (self.doc().width, self.doc().height);
        let sampled = compositor::sample_asset(&asset.image, &layer.transform, w, h);
        let x = (seed.0 as i64).clamp(0, w as i64 - 1) as usize;
        let y = (seed.1 as i64).clamp(0, h as i64 - 1) as usize;
        let mut mask = vec![0u8; w as usize * h as usize];
        let rc = crate::ffi::wand_mask(
            &sampled.data,
            w as usize,
            h as usize,
            w as usize * 4,
            x,
            y,
            2,
            self.wand_tolerance,
            true,
            &mut mask,
        );
        if let Err(e) = rc {
            self.status = format!("魔棒失败：{e}");
            return;
        }
        let before = self.snapshot();
        let mut sel = crate::core::selection::Selection::empty(w, h);
        let merged = self
            .doc()
            .selection
            .as_ref()
            .map(|s| s.coverage.clone())
            .unwrap_or_else(|| vec![0u8; w as usize * h as usize]);
        for (i, v) in mask.iter().enumerate() {
            let a = merged[i];
            let b = *v;
            sel.coverage[i] = if sub {
                a.saturating_sub(b)
            } else if add {
                a.max(b)
            } else {
                b
            };
        }
        self.doc_mut().selection = Some(sel);
        self.status = if sub {
            "已从选区减去魔棒区域".into()
        } else if add {
            "已向选区添加魔棒区域".into()
        } else {
            "已建立魔棒选区".into()
        };
        self.record_edit("魔棒选区", before);
    }

    /// Content-aware fill of the current selection on the active pixel layer
    /// (reuses the original `ContentFill.c` patch-match algorithm).
    fn content_fill_selection(&mut self) {
        let Some(sel) = self.doc().selection.clone() else {
            self.status = "没有选区".into();
            return;
        };
        let Some(id) = self.active_layer else { return };
        if !self.layer_is_paintable(id) {
            self.status = "内容识别填充仅支持像素图层".into();
            return;
        }
        let before = self.snapshot();
        {
            let (w, h) = (self.doc().width, self.doc().height);
            let sampled = compositor::sample_asset(
                &self.doc().layer(id).unwrap().asset.as_ref().unwrap().image,
                &self.doc().layer(id).unwrap().transform,
                w,
                h,
            );
            let mut buf = sampled.data.clone();
            if let Err(e) = crate::ffi::content_fill(
                &mut buf,
                w as usize * 4,
                &sel.coverage,
                w as usize,
                w as i32,
                h as i32,
            ) {
                self.status = format!("内容识别填充失败：{e}");
                return;
            }
            if let Some(l) = self.doc_mut().layer_mut(id) {
                if let Some(a) = l.asset.as_mut() {
                    a.image.data = buf;
                }
            }
        }
        self.record_edit("内容识别填充", before);
        self.status = "内容识别填充完成".into();
    }

    /// Simple background removal ("smart cutout"): sample the four corners,
    /// select every pixel close to any corner color, then delete it.
    fn remove_background(&mut self) {
        let Some(id) = self.active_layer else { return };
        if !self.layer_is_paintable(id) {
            self.status = "移除背景仅支持像素图层".into();
            return;
        }
        let (w, h) = (self.doc().width, self.doc().height);
        let sampled = {
            let l = self.doc().layer(id).unwrap();
            compositor::sample_asset(
                &l.asset.as_ref().unwrap().image,
                &l.transform,
                w,
                h,
            )
        };
        let (wus, hus) = (w as usize, h as usize);
        let sw = sampled.width as usize;
        // Average color over a 10×10 block at each corner.
        let mut bg: Vec<(f32, f32, f32)> = Vec::new();
        for (cx, cy) in [(0usize, 0usize), (sw - 10, 0usize), (0usize, hus - 10), (sw - 10, hus - 10)] {
            let mut sum = (0.0f64, 0.0f64, 0.0f64);
            let mut n = 0u32;
            for yy in cy..(cy + 10).min(hus) {
                for xx in cx..(cx + 10).min(sw) {
                    let i = (yy * sw + xx) * 4;
                    sum.0 += sampled.data[i] as f64;
                    sum.1 += sampled.data[i + 1] as f64;
                    sum.2 += sampled.data[i + 2] as f64;
                    n += 1;
                }
            }
            if n > 0 {
                bg.push(((sum.0 / n as f64) as f32, (sum.1 / n as f64) as f32, (sum.2 / n as f64) as f32));
            }
        }
        if bg.is_empty() {
            self.status = "无法采样背景颜色".into();
            return;
        }
        let tol = 30.0f32;
        let mut mask = vec![0u8; wus * hus];
        for y in 0..hus {
            for x in 0..sw.min(wus) {
                let i = (y * sw + x) * 4;
                let (r, g, b) = (
                    sampled.data[i] as f32,
                    sampled.data[i + 1] as f32,
                    sampled.data[i + 2] as f32,
                );
                let near = bg.iter().any(|(br, bg_, bb)| {
                    let dr = r - br;
                    let dg = g - bg_;
                    let db = b - bb;
                    (dr * dr + dg * dg + db * db).sqrt() < tol
                });
                if near {
                    mask[y * wus + x] = 255;
                }
            }
        }
        let mut sel = crate::core::selection::Selection::empty(w, h);
        sel.coverage = mask;
        self.doc_mut().selection = Some(sel);
        // delete_selection snapshots + records one undo step itself.
        self.delete_selection();
        self.status = "已移除背景（可在“编辑”中撤销）".into();
    }

    /// Create a text layer from the text-dialog state at the recorded position.
    fn create_text_layer(&mut self) {
        let Some(pos) = self.text_pos else { return };
        let text = self.text_string.trim().to_string();
        if text.is_empty() {
            self.status = "文本内容为空".into();
            return;
        }
        let color = [
            (self.text_color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.text_color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.text_color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        ];
        match crate::rendering::text::render_text(&text, self.text_size, color) {
            Ok((buf, tw, th)) => {
                let before = self.snapshot();
                let mut layer = crate::core::layer::Layer::blank("文字".into(), (tw, th));
                layer.name = "文字".into();
                layer.asset = Some(Asset::new(buf, "文字".into()));
                layer.transform.origin = (pos.0, pos.1);
                let id = layer.id;
                let idx = self
                    .doc()
                    .index_of(self.active_layer.unwrap_or_else(|| self.doc().layers.last().map(|l| l.id).unwrap_or(id)))
                    .map(|i| i + 1)
                    .unwrap_or(self.doc().layers.len());
                self.doc_mut().insert_layer(layer, idx);
                self.active_layer = Some(id);
                self.record_edit("新建文字图层", before);
                self.text_dialog_open = false;
                self.text_string.clear();
                self.status = "已创建文字图层".into();
            }
            Err(e) => self.status = e,
        }
    }

    fn text_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.text_dialog_open;
        egui::Window::new("文字工具")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("输入文字（Enter 换行）");
                ui.add(
                    egui::TextEdit::multiline(&mut self.text_string)
                        .desired_rows(3)
                        .desired_width(240.0),
                );
                ui.add(egui::Slider::new(&mut self.text_size, 12.0..=300.0).text("字号"));
                ui.horizontal(|ui| {
                    ui.label("颜色");
                    ui.color_edit_button_rgb(&mut self.text_color);
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("确定").clicked() {
                        self.create_text_layer();
                    }
                    if ui.button("取消").clicked() {
                        self.text_dialog_open = false;
                    }
                });
            });
        self.text_dialog_open = open;
    }

    // ----------------------------------------------- painting / transform

    /// True if the layer can be painted/edited in place (raster, doc-aligned).
    fn layer_is_paintable(&self, id: LayerId) -> bool {
        let Some(l) = self.doc().layer(id) else { return false };
        if l.is_group || l.adjustment.is_some() || l.shape.is_some() {
            return false;
        }
        match &l.asset {
            None => true,
            Some(a) => {
                let t = &l.transform;
                t.origin.0 == 0.0
                    && t.origin.1 == 0.0
                    && t.size.0 == a.image.width as f32
                    && t.size.1 == a.image.height as f32
            }
        }
    }

    /// Give the blank layer a full-size transparent asset so it can be painted.
    fn ensure_asset(&mut self, id: LayerId) {
        if self.doc().layer(id).map(|l| l.asset.is_some()).unwrap_or(false) {
            return;
        }
        let (w, h) = {
            let d = self.doc();
            (d.width, d.height)
        };
        if let Some(l) = self.doc_mut().layer_mut(id) {
            if l.asset.is_none() {
                l.asset = Some(Asset::new(PixelBuffer::new(w, h), "画笔".into()));
            }
        }
    }

    fn toggle_transform(&mut self) {
        if self.transform.active {
            self.commit_transform();
            return;
        }
        let Some(id) = self.active_layer else { return };
        let has_asset = self.doc().layer(id).map(|l| l.asset.is_some()).unwrap_or(false);
        if !has_asset {
            self.status = "该图层不支持自由变换".into();
            return;
        }
        self.transform.active = true;
        self.transform.grab = None;
        self.transform.last_pointer = None;
        self.transform_before = Some(self.snapshot());
        self.status = "自由变换：拖动角点缩放，框内拖动移动，Enter 提交，Esc 取消".into();
    }

    fn commit_transform(&mut self) {
        if let Some(before) = self.transform_before.take() {
            self.record_edit("自由变换", before);
        }
        self.transform.active = false;
        self.transform.grab = None;
        self.transform.last_pointer = None;
    }

    fn cancel_transform(&mut self) {
        if let Some(before) = self.transform_before.take() {
            self.docs[self.active] = before;
            self.version = self.version.wrapping_add(1);
            self.status = "已取消自由变换".into();
        }
        self.transform.active = false;
        self.transform.grab = None;
        self.transform.last_pointer = None;
    }

    /// Apply a transform grab: corner/edge handles scale, center handle moves.
    fn apply_transform_grab(&mut self, grab: (i8, i8), last: (f32, f32), cur: (f32, f32)) {
        let Some(id) = self.active_layer else { return };
        let Some(l) = self.doc_mut().layer_mut(id) else { return };
        let t = &mut l.transform;
        if grab == (0, 0) {
            t.origin.0 += cur.0 - last.0;
            t.origin.1 += cur.1 - last.1;
            return;
        }
        let (ox, oy) = (t.origin.0, t.origin.1);
        let (sx, sy) = (t.size.0, t.size.1);
        let anchor_x = if grab.0 < 0 { ox + sx } else { ox };
        let anchor_y = if grab.1 < 0 { oy + sy } else { oy };
        if grab.0 != 0 {
            let ns = (cur.0 - anchor_x).abs().max(1.0);
            t.origin.0 = if grab.0 < 0 { anchor_x - ns } else { anchor_x };
            t.size.0 = ns;
        }
        if grab.1 != 0 {
            let ns = (cur.1 - anchor_y).abs().max(1.0);
            t.origin.1 = if grab.1 < 0 { anchor_y - ns } else { anchor_y };
            t.size.1 = ns;
        }
    }

    fn nudge_layer(&mut self, dx: f32, dy: f32) {
        let Some(id) = self.active_layer else { return };
        if self.doc().layer(id).is_none() {
            return;
        }
        let before = self.snapshot();
        if let Some(l) = self.doc_mut().layer_mut(id) {
            l.transform.origin.0 += dx;
            l.transform.origin.1 += dy;
        }
        self.record_edit("微调图层", before);
    }

    // ----------------------------------------------------------- undo / redo

    fn undo(&mut self) {
        if let Some((doc, active)) = self.histories[self.active].undo() {
            self.docs[self.active] = doc;
            self.active_layer = active;
            self.version = self.version.wrapping_add(1);
        }
    }

    fn redo(&mut self) {
        if let Some((doc, active)) = self.histories[self.active].redo() {
            self.docs[self.active] = doc;
            self.active_layer = active;
            self.version = self.version.wrapping_add(1);
        }
    }

    // --------------------------------------------------------------- menus

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.menu_button("文件", |ui| {
                if ui.button("新建 (Ctrl+N)").clicked() {
                    self.new_doc_open = true;
                    ui.close_menu();
                }
                if ui.button("打开… (Ctrl+O)").clicked() {
                    ui.close_menu();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("图像", &["png", "jpg", "jpeg", "tif", "tiff"])
                        .pick_file()
                    {
                        self.open_file(path);
                    }
                }
                if ui.button("保存 (Ctrl+S)").clicked() {
                    ui.close_menu();
                    self.save_current();
                }
                if ui.button("导出为…").clicked() {
                    ui.close_menu();
                    self.export_open = true;
                }
                ui.separator();
                if ui.button("退出").clicked() {
                    ui.close_menu();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("编辑", |ui| {
                if ui.add_enabled(self.histories[self.active].can_undo(), egui::Button::new("撤销 (Ctrl+Z)")).clicked() {
                    ui.close_menu();
                    self.undo();
                }
                if ui.add_enabled(self.histories[self.active].can_redo(), egui::Button::new("重做 (Ctrl+Shift+Z)")).clicked() {
                    ui.close_menu();
                    self.redo();
                }
                ui.separator();
                if ui.button("填充前景色 (Shift+F5)").clicked() {
                    ui.close_menu();
                    self.fill_foreground();
                }
                if ui.button("清除选区内容 (Del)").clicked() {
                    ui.close_menu();
                    self.delete_selection();
                }
                if ui.button("内容识别填充选区").clicked() {
                    ui.close_menu();
                    self.content_fill_selection();
                }
            });
            ui.menu_button("图像", |ui| {
                ui.menu_button("调整", |ui| {
                    if ui.button("色相/饱和度…").clicked() {
                        ui.close_menu();
                        self.open_hsv_dialog();
                    }
                });
            });
            ui.menu_button("选择", |ui| {
                if ui.button("全部 (Ctrl+A)").clicked() {
                    ui.close_menu();
                    self.select_all();
                }
                if ui.button("取消选择 (Ctrl+D)").clicked() {
                    ui.close_menu();
                    self.deselect();
                }
            });
            ui.menu_button("滤镜", |ui| {
                if ui.button("高斯模糊…").clicked() {
                    ui.close_menu();
                    self.blur_radius = 2.0;
                    self.blur_dialog_open = true;
                }
            });
            ui.menu_button("图层", |ui| {
                if ui.button("新建图层").clicked() {
                    ui.close_menu();
                    self.add_layer();
                }
                if ui.button("新建文字图层…").clicked() {
                    ui.close_menu();
                    let (w, h) = self.doc().size();
                    self.text_pos = Some((w as f32 / 2.0, h as f32 / 2.0));
                    self.text_string.clear();
                    self.text_dialog_open = true;
                }
                if ui.button("移除背景").clicked() {
                    ui.close_menu();
                    self.remove_background();
                }
                ui.menu_button("新建调整图层", |ui| {
                    for kind in [
                        AdjustmentKind::Levels,
                        AdjustmentKind::Curves,
                        AdjustmentKind::Hsv,
                        AdjustmentKind::Exposure,
                        AdjustmentKind::GradientMap,
                        AdjustmentKind::Grain,
                    ] {
                        if ui.button(kind.label()).clicked() {
                            ui.close_menu();
                            self.add_adjustment_layer(kind);
                        }
                    }
                });
                if let Some(id) = self.active_layer {
                    if ui.button("复制图层").clicked() {
                        ui.close_menu();
                        self.duplicate_layer(id);
                    }
                    if ui.button("删除图层").clicked() {
                        ui.close_menu();
                        self.delete_layer(id);
                    }
                    ui.separator();
                    if ui.button("合并向下 (Ctrl+E)").clicked() {
                        ui.close_menu();
                        self.merge_down();
                    }
                    if ui.button("上移一层").clicked() {
                        ui.close_menu();
                        self.move_layer(id, 1);
                    }
                    if ui.button("下移一层").clicked() {
                        ui.close_menu();
                        self.move_layer(id, -1);
                    }
                }
            });
        });
    }

    fn tool_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("工具").strong().size(12.0));
            for tool in [
                ToolKind::Move,
                ToolKind::Brush,
                ToolKind::Marquee,
                ToolKind::Wand,
                ToolKind::Text,
            ] {
                if ui.selectable_label(self.tool == tool, tool.label()).clicked() {
                    self.tool = tool;
                    self.status = match tool {
                        ToolKind::Move => "移动工具：拖动移动选中图层，Space/中键拖动画布".into(),
                        ToolKind::Brush => "画笔工具：左键绘制，[ ] 调整大小".into(),
                        ToolKind::Marquee => "矩形选框：拖动建立选区，Del 清除选区内内容".into(),
                        ToolKind::Wand => "魔棒工具：点击选择相似颜色区域，Shift 加选 / Alt 减选".into(),
                        ToolKind::Text => "文字工具：点击画布输入文字，或使用图层菜单".into(),
                    };
                }
            }
            ui.separator();
            if self.tool == ToolKind::Brush {
                ui.label("大小");
                ui.add(egui::Slider::new(&mut self.brush.size, 1.0..=500.0).show_value(false));
                ui.label("硬度");
                ui.add(egui::Slider::new(&mut self.brush.hardness, 0.0..=1.0).show_value(false));
                ui.label("不透明度");
                ui.add(egui::Slider::new(&mut self.brush.opacity, 0.01..=1.0).show_value(false));
                ui.label("颜色");
                ui.color_edit_button_srgb(&mut self.foreground);
                self.brush.color = [
                    self.foreground[0] as f32 / 255.0,
                    self.foreground[1] as f32 / 255.0,
                    self.foreground[2] as f32 / 255.0,
                ];
            }
            if self.tool == ToolKind::Wand {
                ui.label("容差");
                ui.add(egui::Slider::new(&mut self.wand_tolerance, 0..=255).show_value(false));
                ui.label(self.wand_tolerance.to_string());
            }
            ui.separator();
            ui.label(format!("缩放 {:.0}%", self.view.zoom * 100.0));
            if ui.button("适合窗口").clicked() {
                self.view.fit_pending = true;
            }
            if ui.button("100%").clicked() {
                self.view.zoom = 1.0;
            }
        });
    }

    fn layers_ui(&mut self, ui: &mut egui::Ui) {
        let edits = panels::layers_panel(ui, self.doc(), self.active_layer);
        if let Some(id) = edits.toggle_visible {
            let before = self.snapshot();
            if let Some(l) = self.doc_mut().layer_mut(id) {
                l.is_visible = !l.is_visible;
            }
            self.record_edit("切换可见性", before);
        }
        if let Some(id) = edits.select {
            self.active_layer = Some(id);
        }
        if let Some((id, mode)) = edits.blend_mode {
            let before = self.snapshot();
            if let Some(l) = self.doc_mut().layer_mut(id) {
                l.blend_mode = mode;
            }
            self.record_edit("更改混合模式", before);
        }
        if let Some((id, opacity)) = edits.opacity {
            let before = self.snapshot();
            if let Some(l) = self.doc_mut().layer_mut(id) {
                l.opacity = opacity;
            }
            self.record_edit("更改不透明度", before);
        }
        if let Some(id) = edits.delete {
            self.delete_layer(id);
        }
        if let Some(id) = edits.up {
            self.move_layer(id, 1);
        }
        if let Some(id) = edits.down {
            self.move_layer(id, -1);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("＋ 新建图层").clicked() {
                self.add_layer();
            }
            if let Some(id) = self.active_layer {
                if ui.button("复制").clicked() {
                    self.duplicate_layer(id);
                }
            }
        });

        // Adjustment-layer parameter editor (only shows for adjustment layers).
        let adj_edits = panels::adjustment_panel(ui, self.doc(), self.active_layer);
        if let Some((id, adj)) = adj_edits.apply {
            let before = self.snapshot();
            if let Some(l) = self.doc_mut().layer_mut(id) {
                l.adjustment = Some(adj);
            }
            self.record_edit("调整参数", before);
        }

        // Layer-styles (effects) editor for pixel layers.
        let fx_edits = panels::effects_panel(ui, self.doc(), self.active_layer);
        if let Some(id) = fx_edits.create {
            let before = self.snapshot();
            if let Some(l) = self.doc_mut().layer_mut(id) {
                l.effects = Some(crate::core::layer::LayerEffects {
                    stroke: Some(crate::core::layer::StrokeEffect::default()),
                    ..Default::default()
                });
            }
            self.record_edit("添加图层样式", before);
        }
        if let Some((id, fx)) = fx_edits.apply {
            let before = self.snapshot();
            if let Some(l) = self.doc_mut().layer_mut(id) {
                l.effects = Some(fx);
            }
            self.record_edit("图层样式", before);
        }
    }

    fn blur_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.blur_dialog_open;
        egui::Window::new("高斯模糊")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("应用于当前像素图层（破坏性操作，可撤销）");
                ui.add(
                    egui::Slider::new(&mut self.blur_radius, 0.5..=100.0)
                        .text("半径")
                        .suffix(" px"),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let radius = self.blur_radius;
                    if ui.button("应用").clicked() {
                        self.apply_gaussian_blur(radius);
                    }
                    if ui.button("取消").clicked() {
                        self.blur_dialog_open = false;
                    }
                });
            });
        self.blur_dialog_open = open;
    }

    fn hsv_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.hsv_dialog_open;
        let mut acted = false; // 本帧是否点击了确定/取消
        egui::Window::new("色相/饱和度")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("应用于当前像素图层（有选区时仅作用于选区）。「确定」应用并关闭；「重置」立即恢复原始效果；「取消」还原为打开前的图像");
                let pb_changed = ui
                    .checkbox(&mut self.hsv_preview_enabled, "实时预览")
                    .changed();
                if pb_changed {
                    if self.hsv_preview_enabled {
                        self.preview_hue_saturation();
                    } else {
                        self.restore_hsv_original();
                    }
                }
                let mut preview = false;
                preview |= ui
                    .add(
                        egui::Slider::new(&mut self.hsv_settings.hue, -180.0..=180.0)
                            .text("色相"),
                    )
                    .changed();
                preview |= ui
                    .add(
                        egui::Slider::new(
                            &mut self.hsv_settings.saturation,
                            -100.0..=100.0,
                        )
                        .text("饱和度"),
                    )
                    .changed();
                preview |= ui
                    .add(
                        egui::Slider::new(&mut self.hsv_settings.lightness, -100.0..=100.0)
                            .text("明度"),
                    )
                    .changed();
                preview |= ui
                    .checkbox(&mut self.hsv_settings.colorize, "着色")
                    .changed();
                if preview {
                    if self.hsv_preview_enabled {
                        self.preview_hue_saturation();
                    } else {
                        // Preview off: keep showing the untouched original
                        // until "确定" applies the current settings.
                        self.restore_hsv_original();
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("重置").clicked() {
                        // 重置总是立即渲染计算，无需再点确定。
                        self.hsv_settings = HueSaturationSettings::default();
                        self.preview_hue_saturation();
                    }
                    if ui.button("确定").clicked() {
                        self.apply_hue_saturation();
                        acted = true;
                    }
                    if ui.button("取消").clicked() {
                        self.cancel_hsv_dialog();
                        acted = true;
                    }
                });
            });
        // 确定/取消已由按钮处理（self.hsv_dialog_open 已被置 false）。
        if !acted {
            if !open && self.hsv_dialog_open {
                // Closed via the window's X button: restore original pixels.
                self.cancel_hsv_dialog();
            } else {
                self.hsv_dialog_open = open;
            }
        }
    }

    fn export_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.export_open;
        let mut chosen = self.export_format;
        egui::Window::new("导出为")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("格式");
                    egui::ComboBox::from_id_salt("export_fmt")
                        .selected_text(chosen.label())
                        .show_ui(ui, |ui| {
                            for fmt in [ExportFormat::Png, ExportFormat::Jpeg] {
                                ui.selectable_value(&mut chosen, fmt, fmt.label());
                            }
                        });
                });
                if chosen == ExportFormat::Jpeg {
                    ui.add(
                        egui::Slider::new(&mut self.export_quality, 1..=100)
                            .text("质量")
                            .suffix("%"),
                    );
                }
                ui.add_space(6.0);
                let composite = compositor::composite_document(self.doc());
                let result = ui.horizontal(|ui| {
                    if ui.button("保存到…").clicked() {
                        let base = self
                            .doc()
                            .layers
                            .iter()
                            .find_map(|l| l.asset.as_ref().map(|a| a.name.clone()))
                            .unwrap_or_else(|| "untitled".into());
                        let default_name = format!("{}.{}", base, chosen.extension());
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(&default_name)
                            .add_filter(chosen.label(), &[chosen.extension()])
                            .save_file()
                        {
                            let out = match chosen {
                                ExportFormat::Png => export::save_png(&path, &composite),
                                ExportFormat::Jpeg => {
                                    export::save_jpeg(&path, &composite, self.export_quality)
                                }
                            };
                            match out {
                                Ok(()) => {
                                    self.status = format!("已导出 {}", path.display());
                                    self.export_open = false;
                                    return (true, None);
                                }
                                Err(e) => return (false, Some(e)),
                            }
                        }
                    }
                    (false, None)
                })
                .inner;
                if let (true, _) = result {
                    return;
                }
                if let (_, Some(e)) = result {
                    ui.colored_label(egui::Color32::from_rgb(229, 57, 53), e);
                }
            });
        self.export_format = chosen;
        self.export_open = open;
    }

    // --------------------------------------------------------------- canvas

    fn canvas_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.ensure_active_layer();
        if self.rendered_version != self.version {
            self.rebuild_texture(ctx);
            self.rendered_version = self.version;
        }
        let size = self.doc().size();

        let sel_bounds = self.doc().selection.as_ref().and_then(|s| s.bounds());
        let marquee = self.marquee;
        let (transform_on, t_origin, t_size) = if self.transform.active {
            match self.active_layer.and_then(|id| self.doc().layer(id)) {
                Some(l) if l.asset.is_some() => (true, l.transform.origin, l.transform.size),
                _ => (false, (0.0, 0.0), (0.0, 0.0)),
            }
        } else {
            (false, (0.0, 0.0), (0.0, 0.0))
        };
        let cursor = ui.ctx().pointer_hover_pos();
        let brush_size = self.brush.size;
        let show_cursor = self.tool == ToolKind::Brush;

        let resp = canvas::draw_canvas(ui, &mut self.view, self.texture.as_ref(), size, |painter, view, _rect| {
            tools::draw_selection(painter, view, sel_bounds);
            if let (Some(a), Some(c)) = (marquee.anchor, marquee.current) {
                tools::draw_marquee(painter, view, a, c);
            }
            if transform_on {
                tools::draw_transform_box(painter, view, t_origin, t_size);
            }
            if show_cursor {
                if let Some(p) = cursor {
                    tools::draw_brush_cursor(painter, view, p, brush_size);
                }
            }
        });

        let pointer_doc = resp.hover_pos().map(|p| {
            let v = self.view.screen_to_doc(p.to_vec2() - resp.rect.min.to_vec2());
            (v.x, v.y)
        });

        let (primary_down, primary_pressed) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.primary_pressed()));
        let space = ctx.input(|i| i.key_down(egui::Key::Space));
        let middle = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        let (shift_down, alt_down) = ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
        let interacting = space || middle;

        // ---- Free transform ----
        if self.transform.active {
            if let Some(id) = self.active_layer {
                let (origin, size_box) = {
                    let l = self.doc().layer(id).unwrap();
                    (l.transform.origin, l.transform.size)
                };
                let handles = tools::transform_handles(&self.view, origin, size_box);
                if primary_pressed && !space && !middle {
                    if let Some(p) = resp.hover_pos() {
                        if let Some(g) = tools::hit_handle(&handles, p, 9.0) {
                            self.transform.grab = Some(g);
                            self.transform.last_pointer = pointer_doc;
                        }
                    }
                }
                if let Some(g) = self.transform.grab {
                    if primary_down {
                        if let Some(d) = pointer_doc {
                            let last = self.transform.last_pointer.unwrap_or(d);
                            self.apply_transform_grab(g, last, d);
                            self.transform.last_pointer = Some(d);
                            self.version = self.version.wrapping_add(1);
                        }
                    } else {
                        self.transform.grab = None;
                    }
                }
            }
        }

        // ---- Brush ----
        if self.tool == ToolKind::Brush && !interacting {
            if primary_down {
                if let Some(d) = pointer_doc {
                    if self.painting.is_none() {
                        if let Some(id) = self.active_layer {
                            if !self.layer_is_paintable(id) {
                                self.status = "画笔仅支持全尺寸像素图层（组/调整层不可绘制）".into();
                            } else {
                                self.ensure_asset(id);
                                let before = self.snapshot();
                                self.painting = Some((id, before, d));
                            }
                        }
                    }
                    if let Some((id, before, last)) = self.painting.take() {
                        let params = self.brush;
                        if let Some(l) = self.doc_mut().layer_mut(id) {
                            if let Some(a) = l.asset.as_mut() {
                                brush::stroke(&mut a.image, last, d, &params, 0.15);
                            }
                        }
                        self.painting = Some((id, before, d));
                        self.version = self.version.wrapping_add(1);
                    }
                }
            } else if let Some((_, before, _)) = self.painting.take() {
                self.record_edit("画笔", before);
            }
        }

        // ---- Marquee ----
        if self.tool == ToolKind::Marquee && !interacting {
            if primary_pressed && !space && !middle {
                if let Some(d) = pointer_doc {
                    self.marquee.anchor = Some(d);
                    self.marquee.current = Some(d);
                }
            } else if self.marquee.anchor.is_some() && primary_down {
                if let Some(d) = pointer_doc {
                    self.marquee.current = Some(d);
                }
            }
            if self.marquee.anchor.is_some() && !primary_down {
                let a = self.marquee.anchor.take().unwrap();
                let c = self.marquee.current.take().unwrap_or(a);
                let rect = tools::marquee_rect(a, c);
                let (w, h) = {
                    let d = self.doc();
                    (d.width, d.height)
                };
                let area = (rect.2 - rect.0) * (rect.3 - rect.1);
                let before = self.snapshot();
                if area < 4.0 {
                    self.doc_mut().selection = None;
                    self.record_edit("取消选择", before);
                } else {
                    self.doc_mut().selection = Some(tools::selection_from_rect(rect, w, h));
                    self.record_edit("建立选区", before);
                }
            }
        }

        // ---- Magic wand ----
        if self.tool == ToolKind::Wand && !interacting {
            if primary_pressed && !space && !middle {
                if let Some(d) = pointer_doc {
                    self.wand_select(d, shift_down, alt_down);
                }
            }
        }

        // ---- Text tool: click places the text layer and opens the dialog ----
        if self.tool == ToolKind::Text && !interacting {
            if primary_pressed && !space && !middle {
                if let Some(d) = pointer_doc {
                    self.text_pos = Some(d);
                    self.text_string.clear();
                    self.text_dialog_open = true;
                }
            }
        }

        // ---- Move tool ----
        if self.tool == ToolKind::Move && resp.dragged() && !interacting && !self.transform.active {
            if self.moving.is_none() {
                if let Some(id) = self.active_layer {
                    self.moving = Some((id, self.snapshot()));
                }
            }
            if let Some((id, _)) = self.moving {
                let zoom = self.view.zoom;
                let delta = resp.drag_delta();
                if let Some(l) = self.doc_mut().layer_mut(id) {
                    l.transform.origin.0 += delta.x / zoom;
                    l.transform.origin.1 += delta.y / zoom;
                }
                self.version = self.version.wrapping_add(1);
            }
        }
        if resp.drag_stopped() {
            if let Some((_, before)) = self.moving.take() {
                self.record_edit("移动图层", before);
            }
        }
    }

    fn new_doc_window(&mut self, ctx: &egui::Context) {
        let mut open = self.new_doc_open;
        egui::Window::new("新建文档").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("宽度");
                ui.text_edit_singleline(&mut self.new_w);
            });
            ui.horizontal(|ui| {
                ui.label("高度");
                ui.text_edit_singleline(&mut self.new_h);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("创建").clicked() {
                    self.new_document();
                }
                if ui.button("取消").clicked() {
                    self.new_doc_open = false;
                }
            });
        });
        self.new_doc_open = open;
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (ctrl, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));

        // Global keys.
        if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
            self.brush.size = (self.brush.size * 0.9).max(1.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
            self.brush.size = (self.brush.size * 1.1).min(500.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if self.transform.active {
                self.commit_transform();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.transform.active {
                self.cancel_transform();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
            self.delete_selection();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F5) && i.modifiers.shift) {
            self.fill_foreground();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::B)) && !ctrl {
            self.tool = ToolKind::Brush;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::M)) && !ctrl {
            self.tool = ToolKind::Marquee;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::V)) && !ctrl {
            self.tool = ToolKind::Move;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::W)) && !ctrl {
            self.tool = ToolKind::Wand;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::T)) && !ctrl {
            self.tool = ToolKind::Text;
        }
        let (left, right, up, down) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        let step = if shift { 10.0 } else { 1.0 };
        if left {
            self.nudge_layer(-step, 0.0);
        }
        if right {
            self.nudge_layer(step, 0.0);
        }
        if up {
            self.nudge_layer(0.0, -step);
        }
        if down {
            self.nudge_layer(0.0, step);
        }

        if !ctrl {
            return;
        }
        ctx.input(|i| {
            if i.key_pressed(egui::Key::N) {
                self.new_doc_open = true;
            }
            if i.key_pressed(egui::Key::O) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("图像", &["png", "jpg", "jpeg", "tif", "tiff"])
                    .pick_file()
                {
                    self.open_file(path);
                }
            }
            if i.key_pressed(egui::Key::S) {
                self.save_current();
            }
            if i.key_pressed(egui::Key::T) {
                self.toggle_transform();
            }
            if i.key_pressed(egui::Key::D) {
                self.deselect();
            }
            if i.key_pressed(egui::Key::A) {
                self.select_all();
            }
            if i.key_pressed(egui::Key::E) {
                self.merge_down();
            }
            if i.key_pressed(egui::Key::Z) && shift {
                self.redo();
            } else if i.key_pressed(egui::Key::Z) {
                self.undo();
            }
        });
    }
}

impl eframe::App for CompositorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let titles: Vec<String> = self
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                format!(
                    "{} — {}×{}",
                    if i == self.active { "●" } else { "○" },
                    d.width,
                    d.height
                )
            })
            .collect();

        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::top("tools").show(ctx, |ui| self.tool_bar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status).size(11.5).color(egui::Color32::from_gray(170)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{}×{}", self.doc().width, self.doc().height));
                    ui.label(egui::RichText::new("Compositor").size(11.5).color(egui::Color32::from_gray(120)));
                });
            });
        });
        egui::SidePanel::right("layers")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| self.layers_ui(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            let tab = panels::doc_tabs(ui, &titles, self.active);
            if tab != self.active {
                self.active = tab;
                self.active_layer = self.doc().layers.last().map(|l| l.id);
                self.version = self.version.wrapping_add(1);
                self.view.fit_pending = true;
            }
            ui.separator();
            self.canvas_ui(ui, ctx);
        });

        self.new_doc_window(ctx);
        self.blur_dialog(ctx);
        self.hsv_dialog(ctx);
        self.export_dialog(ctx);
        self.text_dialog(ctx);
        self.handle_shortcuts(ctx);
    }
}
