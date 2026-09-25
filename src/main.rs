#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

fn main() -> eframe::Result {
    eframe::run_native(
        "Fast File Search",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1220.0, 860.0])
                .with_min_inner_size([960.0, 720.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::SearchApp::new(cc)))),
    )
}
