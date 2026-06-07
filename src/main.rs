mod app;
mod audio;
mod chess;
mod engine;
mod ui;

use app::ChessyApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Chessy")
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Chessy",
        native_options,
        Box::new(|cc| Ok(Box::new(ChessyApp::new(cc)))),
    )
}
