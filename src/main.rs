#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("Compositor"),
        ..Default::default()
    };
    eframe::run_native(
        "Compositor",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            Ok(Box::new(compositor::app::CompositorApp::new(cc)))
        }),
    )
}
