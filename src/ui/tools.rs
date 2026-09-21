//! Tool state machines and on-canvas overlay drawing (brush cursor, marquee,
//! selection outline, free-transform box).

use eframe::egui::{self, Color32, Painter, Pos2, Shape, Stroke, Vec2};

use crate::core::selection::Selection;
use crate::ui::canvas::CanvasView;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolKind {
    Move,
    Brush,
    Marquee,
    Wand,
    Text,
}

impl ToolKind {
    pub fn label(&self) -> &'static str {
        match self {
            ToolKind::Move => "移动",
            ToolKind::Brush => "画笔",
            ToolKind::Marquee => "矩形选框",
            ToolKind::Wand => "魔棒",
            ToolKind::Text => "文字",
        }
    }
}

// ---------------------------------------------------------------------------
// Brush cursor
// ---------------------------------------------------------------------------

/// Draw the circular brush cursor around the pointer (screen coords).
pub fn draw_brush_cursor(painter: &Painter, view: &CanvasView, pointer: Pos2, size_px: f32) {
    let r = (size_px / 2.0 * view.zoom).max(2.0);
    painter.circle_stroke(pointer, r, Stroke::new(1.0_f32, Color32::WHITE));
    let c = Color32::from_rgba_unmultiplied(255, 255, 255, 140);
    painter.line_segment([egui::pos2(pointer.x - r - 4.0, pointer.y), egui::pos2(pointer.x - r + 4.0, pointer.y)], Stroke::new(1.0_f32, c));
    painter.line_segment([egui::pos2(pointer.x + r - 4.0, pointer.y), egui::pos2(pointer.x + r + 4.0, pointer.y)], Stroke::new(1.0_f32, c));
    painter.line_segment([egui::pos2(pointer.x, pointer.y - r - 4.0), egui::pos2(pointer.x, pointer.y - r + 4.0)], Stroke::new(1.0_f32, c));
    painter.line_segment([egui::pos2(pointer.x, pointer.y + r - 4.0), egui::pos2(pointer.x, pointer.y + r + 4.0)], Stroke::new(1.0_f32, c));
}

// ---------------------------------------------------------------------------
// Marquee (rectangle selection)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct MarqueeState {
    /// Document-space anchor where the drag started.
    pub anchor: Option<(f32, f32)>,
    /// Document-space current drag position.
    pub current: Option<(f32, f32)>,
}

/// Normalize a marquee into (x0, y0, x1, y1) in document pixels.
pub fn marquee_rect(a: (f32, f32), b: (f32, f32)) -> (f32, f32, f32, f32) {
    (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
}

/// Build a selection coverage mask from a marquee rect (clamped to canvas).
pub fn selection_from_rect(
    (x0, y0, x1, y1): (f32, f32, f32, f32),
    width: u32,
    height: u32,
) -> Selection {
    let mut sel = Selection::empty(width, height);
    let x0 = x0.floor().clamp(0.0, width as f32) as usize;
    let y0 = y0.floor().clamp(0.0, height as f32) as usize;
    let x1 = x1.ceil().clamp(0.0, width as f32) as usize;
    let y1 = y1.ceil().clamp(0.0, height as f32) as usize;
    let w = width as usize;
    for y in y0..y1.min(height as usize) {
        for x in x0..x1.min(width as usize) {
            sel.coverage[y * w + x] = 255;
        }
    }
    sel
}

/// Draw the in-progress marquee as a dashed rectangle (screen coords).
pub fn draw_marquee(painter: &Painter, view: &CanvasView, anchor: (f32, f32), current: (f32, f32)) {
    let a = view.doc_to_screen(Vec2::new(anchor.0, anchor.1));
    let b = view.doc_to_screen(Vec2::new(current.0, current.1));
    let rect = egui::Rect::from_two_pos(egui::pos2(a.x, a.y), egui::pos2(b.x, b.y));
    dashed_rect(painter, rect, Stroke::new(1.0_f32, Color32::from_rgb(0x5a, 0x9b, 0xe8)));
    // Light fill.
    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(90, 155, 232, 28));
}

/// Draw the committed selection outline (marching ants style, static dashed).
pub fn draw_selection(painter: &Painter, view: &CanvasView, bounds: Option<(u32, u32, u32, u32)>) {
    if let Some((x0, y0, x1, y1)) = bounds {
        let a = view.doc_to_screen(Vec2::new(x0 as f32, y0 as f32));
        let b = view.doc_to_screen(Vec2::new(x1 as f32 + 1.0, y1 as f32 + 1.0));
        let rect = egui::Rect::from_two_pos(egui::pos2(a.x, a.y), egui::pos2(b.x, b.y));
        dashed_rect(painter, rect, Stroke::new(1.0_f32, Color32::from_rgb(0x5a, 0x9b, 0xe8)));
    }
}

fn dashed_rect(painter: &Painter, rect: egui::Rect, stroke: Stroke) {
    let pts = vec![
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.add(Shape::dashed_line(&pts, stroke, 6.0, 4.0));
}

// ---------------------------------------------------------------------------
// Free transform
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct TransformState {
    pub active: bool,
    /// Handle being dragged: `(-1|-1 .. 1|1)` for corners/edges, `(0,0)` = move.
    pub grab: Option<(i8, i8)>,
    /// Last pointer position in document space during a grab.
    pub last_pointer: Option<(f32, f32)>,
}

/// Compute the screen positions of the transform box handles for a layer box
/// defined by `(origin, size)` in document space.
pub fn transform_handles(view: &CanvasView, origin: (f32, f32), size: (f32, f32)) -> Vec<(i8, i8, Pos2)> {
    let (ox, oy) = (origin.0, origin.1);
    let (sx, sy) = (size.0, size.1);
    let at = |gx: f32, gy: f32| {
        let p = view.doc_to_screen(Vec2::new(ox + sx * gx, oy + sy * gy));
        egui::pos2(p.x, p.y)
    };
    vec![
        (-1, -1, at(0.0, 0.0)),
        (1, -1, at(1.0, 0.0)),
        (-1, 1, at(0.0, 1.0)),
        (1, 1, at(1.0, 1.0)),
        (0, -1, at(0.5, 0.0)),
        (0, 1, at(0.5, 1.0)),
        (-1, 0, at(0.0, 0.5)),
        (1, 0, at(1.0, 0.5)),
        (0, 0, at(0.5, 0.5)),
    ]
}

/// Draw the free-transform box and handles for a layer box.
pub fn draw_transform_box(painter: &Painter, view: &CanvasView, origin: (f32, f32), size: (f32, f32)) {
    let handles = transform_handles(view, origin, size);
    let a = handles[0].2;
    let b = handles[3].2;
    let box_rect = egui::Rect::from_two_pos(a, b);
    painter.rect_stroke(box_rect, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(0x5a, 0x9b, 0xe8)));
    for (gx, gy, pos) in &handles {
        if *gx == 0 && *gy == 0 {
            continue;
        }
        let s = 5.0;
        let r = egui::Rect::from_center_size(*pos, egui::vec2(s, s));
        painter.rect_filled(r, 1.0, Color32::WHITE);
        painter.rect_stroke(r, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(0x2a, 0x2a, 0x2a)));
    }
}

/// Find the handle within `threshold` screen pixels of `pointer`.
pub fn hit_handle(handles: &[(i8, i8, Pos2)], pointer: Pos2, threshold: f32) -> Option<(i8, i8)> {
    handles
        .iter()
        .find(|(_, _, pos)| pos.distance(pointer) <= threshold)
        .map(|(gx, gy, _)| (*gx, *gy))
}
