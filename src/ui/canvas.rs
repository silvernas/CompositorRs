//! The central canvas viewport: zoom, pan, checkerboard, image, pixel grid
//! and a tool overlay (selection, transform box, brush cursor).

use eframe::egui::{self, Color32, Rect, Sense, TextureHandle, Vec2};

pub const CHECKER_A: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
pub const CHECKER_B: Color32 = Color32::from_rgb(0x3d, 0x3d, 0x3d);

#[derive(Clone, Copy, Debug)]
pub struct CanvasView {
    pub zoom: f32,
    pub pan: Vec2,
    pub fit_pending: bool,
}

impl Default for CanvasView {
    fn default() -> Self {
        CanvasView { zoom: 1.0, pan: Vec2::ZERO, fit_pending: true }
    }
}

impl CanvasView {
    /// Doc pixel → screen position.
    pub fn doc_to_screen(&self, doc: Vec2) -> Vec2 {
        self.pan + doc * self.zoom
    }

    /// Screen position → doc pixel.
    pub fn screen_to_doc(&self, screen: Vec2) -> Vec2 {
        (screen - self.pan) / self.zoom.max(1e-4)
    }

    /// Fit the document into the given viewport, centering it.
    pub fn fit(&mut self, doc_size: Vec2, viewport: Vec2) {
        let margin = 32.0;
        let avail = (viewport - Vec2::splat(2.0 * margin)).max(Vec2::splat(1.0));
        self.zoom = (avail.x / doc_size.x.max(1.0)).min(avail.y / doc_size.y.max(1.0)).min(8.0).max(0.02);
        self.pan = viewport / 2.0 - doc_size * self.zoom / 2.0;
    }
}

/// Draw the canvas and handle pan/zoom input inside `ui`. `overlay` draws any
/// tool feedback (selection marquee, transform box, brush cursor) on top of the
/// image; it receives the painter, the view and the on-screen image rectangle.
pub fn draw_canvas(
    ui: &mut egui::Ui,
    view: &mut CanvasView,
    texture: Option<&TextureHandle>,
    doc_size: (u32, u32),
    overlay: impl FnOnce(&egui::Painter, &CanvasView, &Rect),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let doc = Vec2::new(doc_size.0 as f32, doc_size.1 as f32);
    let ctx = ui.ctx();

    // Zoom at the pointer (ctrl+scroll or pinch), clamp and keep the anchor fixed.
    let hover = ctx.pointer_hover_pos().filter(|p| rect.contains(*p));
    let zoom_delta = ctx.input(|i| i.zoom_delta());
    let scroll = ctx.input(|i| i.raw_scroll_delta.y);
    let mut target_zoom = view.zoom;
    if scroll.abs() > 0.0 {
        target_zoom *= (scroll * 0.002).exp();
    }
    if (zoom_delta - 1.0).abs() > 1e-4 {
        target_zoom *= zoom_delta;
    }
    if (target_zoom - view.zoom).abs() > 1e-5 {
        let anchor = hover.unwrap_or(rect.center());
        let before = (anchor - view.pan) / view.zoom.max(1e-4);
        view.zoom = target_zoom.clamp(0.02, 64.0);
        view.pan = anchor - before * view.zoom;
    }

    // Pan with middle-drag or space + left-drag.
    let space = ctx.input(|i| i.key_down(egui::Key::Space));
    let middle = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
    if response.dragged() && (middle || space) {
        view.pan += response.drag_delta();
    }

    if view.fit_pending && doc != Vec2::ZERO {
        view.fit(doc, rect.size());
        view.fit_pending = false;
    }

    // Background + checkerboard behind the image.
    painter.rect_filled(rect, 0.0, Color32::from_rgb(0x26, 0x26, 0x26));
    let tl = view.doc_to_screen(Vec2::ZERO);
    let br = view.doc_to_screen(doc);
    let img_rect = Rect::from_min_max(egui::pos2(tl.x, tl.y), egui::pos2(br.x, br.y));
    let cell = 8.0 * view.zoom;
    if cell >= 1.0 {
        let cell = cell.max(1.0);
        let x0 = ((tl.x - rect.left()) / cell).floor() as i32;
        let x1 = ((br.x - rect.left()) / cell).ceil() as i32;
        let y0 = ((tl.y - rect.top()) / cell).floor() as i32;
        let y1 = ((br.y - rect.top()) / cell).ceil() as i32;
        for gy in y0..y1 {
            for gx in x0..x1 {
                let c = if (gx + gy) % 2 == 0 { CHECKER_A } else { CHECKER_B };
                let r = Rect::from_min_size(
                    rect.min + Vec2::new(gx as f32 * cell, gy as f32 * cell),
                    Vec2::splat(cell),
                )
                .intersect(rect);
                if r.is_positive() {
                    painter.rect_filled(r, 0.0, c);
                }
            }
        }
    }

    // The composited image.
    if let Some(tex) = texture {
        painter.image(tex.id(), img_rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }

    // Pixel grid at high zoom.
    if view.zoom >= 6.0 {
        let step = view.zoom;
        let gx0 = (tl.x / step).ceil() as i32;
        let gx1 = (br.x / step).floor() as i32;
        let gy0 = (tl.y / step).ceil() as i32;
        let gy1 = (br.y / step).floor() as i32;
        let grid = egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0, 0, 0, 90));
        for gx in gx0..=gx1 {
            let x = tl.x + gx as f32 * step;
            painter.line_segment([egui::pos2(x, tl.y), egui::pos2(x, br.y)], grid);
        }
        for gy in gy0..=gy1 {
            let y = tl.y + gy as f32 * step;
            painter.line_segment([egui::pos2(tl.x, y), egui::pos2(br.x, y)], grid);
        }
    }

    // Canvas border.
    painter.rect_stroke(img_rect, 0.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(0x55, 0x55, 0x55)));

    // Tool overlay.
    overlay(&painter, view, &img_rect);

    // Double-click to reset fit.
    if response.double_clicked() {
        view.fit_pending = true;
    }

    response
}
