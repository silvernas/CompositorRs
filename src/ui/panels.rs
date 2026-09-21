//! Side panels: the layers list and document tabs.

use eframe::egui::{self, RichText};

use crate::core::blend::BlendMode;
use crate::core::document::Document;
use crate::core::layer::{
    AdjustmentColor, AdjustmentKind, Layer, LayerAdjustment, LevelRange, ShadowEffect,
};
use crate::core::pixel_buffer::LayerId;

/// Edits requested by the layers panel in one frame.
#[derive(Default)]
pub struct LayerEdits {
    pub toggle_visible: Option<LayerId>,
    pub delete: Option<LayerId>,
    pub select: Option<LayerId>,
    pub blend_mode: Option<(LayerId, BlendMode)>,
    pub opacity: Option<(LayerId, f32)>,
    /// Move the layer one step up/down in the stack (visual top/bottom).
    pub up: Option<LayerId>,
    pub down: Option<LayerId>,
}

/// Clickable eye icon drawn with the painter (font-independent).
fn eye_icon(ui: &mut egui::Ui, visible: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let c = rect.center();
        let color = if visible {
            egui::Color32::from_rgb(122, 178, 122)
        } else {
            egui::Color32::from_gray(150)
        };
        if visible {
            let r = 5.5;
            p.circle_stroke(c, r, egui::Stroke::new(1.3, color));
            p.circle_filled(c, 2.2, color);
        } else {
            p.line_segment(
                [egui::pos2(c.x - 6.0, c.y), egui::pos2(c.x + 6.0, c.y)],
                egui::Stroke::new(1.5, color),
            );
        }
    }
    resp
}

/// Small kind glyph drawn with the painter: folder / half-filled circle /
/// hollow square / filled square. Font-independent.
fn type_icon(ui: &mut egui::Ui, layer: &Layer) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 12.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let c = rect.center();
    let gray = egui::Color32::from_gray(180);
    let stroke = egui::Stroke::new(1.2, gray);
    if layer.is_group {
        // Folder: rect body + tab.
        let body = egui::Rect::from_center_size(c, egui::vec2(11.0, 8.0));
        p.rect(body, 1.0, egui::Color32::TRANSPARENT, stroke);
        p.line_segment(
            [egui::pos2(body.left(), body.top()), egui::pos2(c.x - 3.5, c.y - 4.0)],
            stroke,
        );
        p.line_segment(
            [egui::pos2(c.x - 3.5, c.y - 4.0), egui::pos2(c.x - 0.5, c.y - 4.0)],
            stroke,
        );
    } else if layer.adjustment.is_some() {
        // Upper half-disc.
        let r = 4.5;
        let mut pts: Vec<egui::Pos2> = Vec::with_capacity(12);
        for i in 0..=10 {
            let a = std::f32::consts::PI * (i as f32 / 10.0); // 0..=PI
            pts.push(egui::pos2(c.x + a.cos() * r, c.y - a.sin() * r));
        }
        pts.push(egui::pos2(c.x + r, c.y));
        pts.push(egui::pos2(c.x - r, c.y));
        p.add(egui::Shape::convex_polygon(
            pts,
            egui::Color32::from_rgba_unmultiplied(180, 180, 180, 70),
            stroke,
        ));
    } else if layer.shape.is_some() {
        // Hollow square.
        p.rect(
            egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0)),
            1.0,
            egui::Color32::TRANSPARENT,
            stroke,
        );
    } else {
        // Filled square (plain pixel layer).
        p.rect_filled(
            egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0)),
            1.0,
            egui::Color32::from_gray(170),
        );
    }
}

/// Draw the layers panel. Rows are top-most first (the stack is bottom-to-top).
pub fn layers_panel(ui: &mut egui::Ui, doc: &Document, active: Option<LayerId>) -> LayerEdits {
    let mut edits = LayerEdits::default();
    ui.set_min_width(220.0);
    ui.add_space(4.0);
    ui.label(RichText::new("图层").strong().size(13.0));
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for layer in doc.layers.iter().rev() {
            let selected = active == Some(layer.id);
            let indent = if layer.parent_id.is_some() { 16.0 } else { 0.0 };

            let row = ui.horizontal(|ui| {
                ui.add_space(indent);
                if eye_icon(ui, layer.is_visible).on_hover_text("显示/隐藏").clicked() {
                    edits.toggle_visible = Some(layer.id);
                }
                type_icon(ui, layer);
                if ui
                    .selectable_label(selected, RichText::new(&layer.name).size(12.5))
                    .on_hover_text(&layer.name)
                    .clicked()
                {
                    edits.select = Some(layer.id);
                }
                if ui.small_button("×").on_hover_text("删除图层").clicked() {
                    edits.delete = Some(layer.id);
                }
            });
            let _ = row.inner;
            if selected && !layer.is_group {
                ui.horizontal(|ui| {
                    ui.add_space(indent + 4.0);
                    if ui.small_button("▲").on_hover_text("上移一层").clicked() {
                        edits.up = Some(layer.id);
                    }
                    if ui.small_button("▼").on_hover_text("下移一层").clicked() {
                        edits.down = Some(layer.id);
                    }
                    ui.label(RichText::new("顺序").size(11.0).color(egui::Color32::from_gray(130)));
                });
            }

            if selected {
                ui.horizontal(|ui| {
                    ui.add_space(indent + 4.0);
                    ui.label(RichText::new("不透明度").size(11.0));
                    let mut op = layer.opacity;
                    if ui.add(egui::Slider::new(&mut op, 0.0..=1.0).show_value(false)).changed() {
                        edits.opacity = Some((layer.id, op));
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(indent + 4.0);
                    ui.label(RichText::new("混合").size(11.0));
                    let mut label = layer.blend_mode.label().to_string();
                    egui::ComboBox::from_id_salt(("blend", layer.id.0)).selected_text(&label).show_ui(ui, |ui| {
                        for mode in BlendMode::ALL {
                            let text = mode.label();
                            if ui.selectable_value(&mut label, text.to_string(), text).changed() {
                                edits.blend_mode = Some((layer.id, mode));
                            }
                        }
                    });
                });
            }
            ui.add_space(2.0);
        }
    });
    edits
}

/// Document tab bar at the top of the window; returns the newly selected index.
pub fn doc_tabs(ui: &mut egui::Ui, titles: &[String], active: usize) -> usize {
    let mut result = active;
    ui.horizontal(|ui| {
        for (i, title) in titles.iter().enumerate() {
            if ui.selectable_label(i == active, RichText::new(title).size(12.0)).clicked() {
                result = i;
            }
        }
    });
    result
}

// ---------------------------------------------------------------------------
// Adjustment layer parameter editor
// ---------------------------------------------------------------------------

/// Edits requested by the adjustment panel in one frame.
#[derive(Default)]
pub struct AdjustmentEdits {
    /// A full new adjustment object to replace the selected layer's one.
    pub apply: Option<(LayerId, LayerAdjustment)>,
}

/// Edits requested by the layer-styles panel in one frame.
#[derive(Default)]
pub struct EffectsEdits {
    /// Replace the selected layer's whole effects bundle.
    pub apply: Option<(LayerId, crate::core::layer::LayerEffects)>,
    /// Create a default effects bundle on the selected layer.
    pub create: Option<LayerId>,
}

/// Layer-styles (effects) editor for the selected pixel layer.
pub fn effects_panel(ui: &mut egui::Ui, doc: &Document, active: Option<LayerId>) -> EffectsEdits {
    use crate::core::layer::{ColorOverlayEffect, InnerShadowEffect, StrokeEffect};

    let mut edits = EffectsEdits::default();
    let Some(id) = active else { return edits };
    let Some(layer) = doc.layer(id) else { return edits };
    if layer.adjustment.is_some() || layer.asset.is_none() {
        return edits;
    }

    ui.add_space(6.0);
    ui.label(RichText::new("图层样式").strong().size(13.0));
    ui.separator();

    let Some(mut fx) = layer.effects.clone() else {
        if ui.button("＋ 添加特效").clicked() {
            edits.create = Some(id);
        }
        return edits;
    };
    let mut changed = false;

    egui::CollapsingHeader::new("描边")
        .default_open(false)
        .show(ui, |ui| {
            if fx.stroke.is_none() {
                if ui.small_button("添加描边").clicked() {
                    fx.stroke = Some(StrokeEffect::default());
                    changed = true;
                }
            } else if let Some(s) = fx.stroke.as_mut() {
                changed |= ui.checkbox(&mut s.enabled, "启用").changed();
                changed |= ui
                    .add(egui::Slider::new(&mut s.size, 0.0..=500.0).text("大小"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut s.opacity, 0.0..=1.0).text("不透明度"))
                    .changed();
                changed |= ui.checkbox(&mut s.inside, "内部描边").changed();
                let mut c = [s.color.0, s.color.1, s.color.2];
                changed |= ui.color_edit_button_rgb(&mut c).changed();
                s.color = (c[0], c[1], c[2]);
            }
        });

    egui::CollapsingHeader::new("投影")
        .default_open(false)
        .show(ui, |ui| {
            if fx.shadow.is_none() {
                if ui.small_button("添加投影").clicked() {
                    fx.shadow = Some(ShadowEffect::default());
                    changed = true;
                }
            } else if let Some(s) = fx.shadow.as_mut() {
                changed |= shadow_ui(ui, s);
            }
        });

    egui::CollapsingHeader::new("颜色叠加")
        .default_open(false)
        .show(ui, |ui| {
            if fx.color_overlay.is_none() {
                if ui.small_button("添加颜色叠加").clicked() {
                    fx.color_overlay = Some(ColorOverlayEffect::default());
                    changed = true;
                }
            } else if let Some(ov) = fx.color_overlay.as_mut() {
                changed |= ui.checkbox(&mut ov.enabled, "启用").changed();
                changed |= ui
                    .add(egui::Slider::new(&mut ov.opacity, 0.0..=1.0).text("不透明度"))
                    .changed();
                let mut c = [ov.color.0, ov.color.1, ov.color.2];
                changed |= ui.color_edit_button_rgb(&mut c).changed();
                ov.color = (c[0], c[1], c[2]);
            }
        });

    egui::CollapsingHeader::new("内阴影")
        .default_open(false)
        .show(ui, |ui| {
            if fx.inner_shadow.is_none() {
                if ui.small_button("添加内阴影").clicked() {
                    fx.inner_shadow = Some(InnerShadowEffect::default());
                    changed = true;
                }
            } else if let Some(s) = fx.inner_shadow.as_mut() {
                changed |= shadow_ui(ui, s);
            }
        });

    if changed {
        edits.apply = Some((id, fx));
    }
    edits
}

/// Common fields of `ShadowEffect` and `InnerShadowEffect`.
trait ShadowParams {
    fn enabled(&mut self) -> &mut bool;
    fn angle(&mut self) -> &mut f32;
    fn distance(&mut self) -> &mut f32;
    fn blur(&mut self) -> &mut f32;
    fn opacity(&mut self) -> &mut f32;
    fn color(&mut self) -> &mut (f32, f32, f32);
}

impl ShadowParams for ShadowEffect {
    fn enabled(&mut self) -> &mut bool { &mut self.enabled }
    fn angle(&mut self) -> &mut f32 { &mut self.angle }
    fn distance(&mut self) -> &mut f32 { &mut self.distance }
    fn blur(&mut self) -> &mut f32 { &mut self.blur }
    fn opacity(&mut self) -> &mut f32 { &mut self.opacity }
    fn color(&mut self) -> &mut (f32, f32, f32) { &mut self.color }
}

impl ShadowParams for crate::core::layer::InnerShadowEffect {
    fn enabled(&mut self) -> &mut bool { &mut self.enabled }
    fn angle(&mut self) -> &mut f32 { &mut self.angle }
    fn distance(&mut self) -> &mut f32 { &mut self.distance }
    fn blur(&mut self) -> &mut f32 { &mut self.blur }
    fn opacity(&mut self) -> &mut f32 { &mut self.opacity }
    fn color(&mut self) -> &mut (f32, f32, f32) { &mut self.color }
}

/// Angle / distance / blur / color / opacity controls shared by shadow types.
fn shadow_ui<T: ShadowParams>(ui: &mut egui::Ui, s: &mut T) -> bool {
    let mut c = [s.color().0, s.color().1, s.color().2];
    let mut ch = false;
    ch |= ui.checkbox(s.enabled(), "启用").changed();
    ch |= ui
        .add(egui::Slider::new(s.angle(), -360.0..=360.0).text("角度"))
        .changed();
    ch |= ui
        .add(egui::Slider::new(s.distance(), 0.0..=5000.0).text("距离"))
        .changed();
    ch |= ui
        .add(egui::Slider::new(s.blur(), 0.0..=500.0).text("模糊"))
        .changed();
    ch |= ui
        .add(egui::Slider::new(s.opacity(), 0.0..=1.0).text("不透明度"))
        .changed();
    ch |= ui.color_edit_button_rgb(&mut c).changed();
    *s.color() = (c[0], c[1], c[2]);
    ch
}

/// Draw parameter controls for the selected layer when it is an adjustment
/// layer. Reads the current settings, edits a local copy, and returns the
/// updated whole `LayerAdjustment` so the caller can record one undo step.
pub fn adjustment_panel(
    ui: &mut egui::Ui,
    doc: &Document,
    active: Option<LayerId>,
) -> AdjustmentEdits {
    let mut edits = AdjustmentEdits::default();
    let Some(id) = active else { return edits };
    let Some(layer) = doc.layer(id) else { return edits };
    let Some(adj) = &layer.adjustment else { return edits };

    ui.add_space(6.0);
    ui.label(RichText::new("调整").strong().size(13.0));
    ui.separator();

    let mut changed = false;
    let mut adj = adj.clone();
    match adj.kind {
        AdjustmentKind::Levels => {
            ui.label(RichText::new("色阶").size(12.0).color(egui::Color32::from_gray(170)));
            changed |= level_range_ui(ui, &mut adj.levels.ranges[3], "RGB 合成");
            egui::CollapsingHeader::new("红 / 绿 / 蓝")
                .default_open(false)
                .show(ui, |ui| {
                    changed |= level_range_ui(ui, &mut adj.levels.ranges[0], "红");
                    changed |= level_range_ui(ui, &mut adj.levels.ranges[1], "绿");
                    changed |= level_range_ui(ui, &mut adj.levels.ranges[2], "蓝");
                });
        }
        AdjustmentKind::Curves => {
            ui.label(RichText::new("曲线").size(12.0).color(egui::Color32::from_gray(170)));
            let channel_key = egui::Id::new(("curve_channel", id.0));
            let mut channel: usize = ui
                .data_mut(|d| d.get_temp(channel_key))
                .unwrap_or(3);
            ui.horizontal(|ui| {
                for (i, label) in ["RGB", "R", "G", "B"].iter().enumerate() {
                    if ui.selectable_label(channel == i, RichText::new(*label).size(11.0)).clicked() {
                        channel = i;
                    }
                }
            });
            ui.data_mut(|d| d.insert_temp(channel_key, channel));
            let points = &mut adj.curves.channels[channel];
            changed |= curve_editor(ui, points);
            if ui
                .small_button("重置曲线")
                .on_hover_text("恢复为 0-255 直线")
                .clicked()
            {
                *points = vec![
                    crate::core::layer::CurvePoint { x: 0.0, y: 0.0 },
                    crate::core::layer::CurvePoint { x: 255.0, y: 255.0 },
                ];
                changed = true;
            }
        }
        AdjustmentKind::Hsv => {
            ui.label(RichText::new("色相 / 饱和度").size(12.0).color(egui::Color32::from_gray(170)));
            changed |= ui
                .add(egui::Slider::new(&mut adj.hsv.hue, -180.0..=180.0).text("色相"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut adj.hsv.saturation, -100.0..=100.0).text("饱和度"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut adj.hsv.lightness, -100.0..=100.0).text("明度"))
                .changed();
            changed |= ui.checkbox(&mut adj.hsv.colorize, "着色").changed();
        }
        AdjustmentKind::Exposure => {
            ui.label(RichText::new("曝光").size(12.0).color(egui::Color32::from_gray(170)));
            changed |= ui
                .add(egui::Slider::new(&mut adj.exposure.exposure, -4.0..=4.0).text("曝光"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut adj.exposure.offset, -0.25..=0.25).text("偏移"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut adj.exposure.gamma, 0.1..=4.0).text("伽马"))
                .changed();
        }
        AdjustmentKind::GradientMap => {
            ui.label(RichText::new("渐变映射").size(12.0).color(egui::Color32::from_gray(170)));
            let mut dark = [
                adj.gradient_map.shadows.red,
                adj.gradient_map.shadows.green,
                adj.gradient_map.shadows.blue,
            ];
            let mut light = [
                adj.gradient_map.highlights.red,
                adj.gradient_map.highlights.green,
                adj.gradient_map.highlights.blue,
            ];
            changed |= ui
                .horizontal(|ui| {
                    ui.label(RichText::new("阴影").size(11.0));
                    ui.color_edit_button_rgb(&mut dark).changed()
                })
                .inner;
            changed |= ui
                .horizontal(|ui| {
                    ui.label(RichText::new("高光").size(11.0));
                    ui.color_edit_button_rgb(&mut light).changed()
                })
                .inner;
            adj.gradient_map.shadows = AdjustmentColor { red: dark[0], green: dark[1], blue: dark[2] };
            adj.gradient_map.highlights = AdjustmentColor {
                red: light[0],
                green: light[1],
                blue: light[2],
            };
            changed |= ui.checkbox(&mut adj.gradient_map.reversed, "反相").changed();
        }
        AdjustmentKind::Grain => {
            ui.label(RichText::new("颗粒").size(12.0).color(egui::Color32::from_gray(170)));
            changed |= ui
                .add(egui::Slider::new(&mut adj.grain.amount, 0.0..=100.0).text("数量"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut adj.grain.size, 0.5..=12.0).text("大小"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut adj.grain.roughness, 0.0..=100.0).text("粗糙度"))
                .changed();
            if ui.small_button("重新随机").clicked() {
                use std::time::{SystemTime, UNIX_EPOCH};
                adj.grain.seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u32)
                    .unwrap_or(0);
                changed = true;
            }
        }
    }
    if changed {
        edits.apply = Some((id, adj));
    }
    edits
}

/// Shadow / gamma / highlight sliders for one level range.
fn level_range_ui(ui: &mut egui::Ui, r: &mut LevelRange, label: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.0).color(egui::Color32::from_gray(150)));
    });
    changed |= ui
        .add(egui::Slider::new(&mut r.shadow, 0.0..=1.0).text("阴影"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut r.gamma, 0.1..=3.0).text("伽马"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut r.highlight, 0.0..=1.0).text("高光"))
        .changed();
    changed
}

/// Interactive curve editor: click to add a point, drag to move it (x stays
/// monotonic), double-click near a point to delete it. Returns true if changed.
fn curve_editor(ui: &mut egui::Ui, points: &mut Vec<crate::core::layer::CurvePoint>) -> bool {
    use crate::core::layer::CurvePoint;

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(200.0, 200.0), egui::Sense::click_and_drag());
    let painter = ui.painter();
    let to_px = |p: &CurvePoint| {
        egui::pos2(
            rect.left() + (p.x / 255.0) * rect.width(),
            rect.bottom() - (p.y / 255.0) * rect.height(),
        )
    };
    let to_curve = |pos: egui::Pos2| CurvePoint {
        x: (((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * 255.0).round(),
        y: (((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0) * 255.0).round(),
    };

    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(36));
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, egui::Color32::from_gray(80)));
    // Grid.
    for i in 0..=4 {
        let t = i as f32 / 4.0;
        let x = rect.left() + rect.width() * t;
        let y = rect.top() + rect.height() * t;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(58)),
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(58)),
        );
    }
    // Identity diagonal.
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(82)),
    );
    // Curve polyline (evaluate through the shared interpolation logic).
    let cs = {
        let mut cs = crate::core::layer::CurvesSettings::default();
        cs.channels[3] = points.clone();
        cs
    };
    let curve: Vec<egui::Pos2> = (0..=64)
        .map(|i| {
            let x = i as f32 / 64.0 * 255.0;
            let y = cs.value(x, 3);
            to_px(&CurvePoint { x, y })
        })
        .collect();
    painter.add(egui::Shape::line(
        curve,
        egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(96, 168, 255)),
    ));
    // Control points.
    for p in points.iter() {
        painter.circle_filled(to_px(p), 4.0, egui::Color32::from_gray(235));
        painter.circle_stroke(
            to_px(p),
            4.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(96, 168, 255)),
        );
    }

    let mut changed = false;
    let drag_key = resp.id;
    let mut dragged: Option<usize> = ui.data_mut(|d| d.get_temp(drag_key));

    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let target = to_curve(pos);
            match points.iter().position(|p| (to_px(p) - pos).length() < 8.0) {
                Some(idx) => dragged = Some(idx),
                None => {
                    points.push(target);
                    points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
                    dragged = Some(points.iter().position(|p| p.x == target.x).unwrap());
                    changed = true;
                }
            }
        }
    }
    if resp.dragged() {
        if let (Some(idx), Some(pos)) = (dragged, resp.interact_pointer_pos()) {
            let t = to_curve(pos);
            let lo = if idx > 0 { points[idx - 1].x + 1.0 } else { 0.0 };
            let hi = if idx + 1 < points.len() {
                points[idx + 1].x - 1.0
            } else {
                255.0
            };
            points[idx].x = t.x.clamp(lo, hi);
            points[idx].y = t.y.clamp(0.0, 255.0);
            changed = true;
        }
    }
    if resp.drag_stopped() {
        dragged = None;
    }
    if resp.double_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some(idx) = points.iter().position(|p| (to_px(p) - pos).length() < 8.0) {
                if points.len() > 2 {
                    points.remove(idx);
                    changed = true;
                }
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(drag_key, dragged));
    changed
}
