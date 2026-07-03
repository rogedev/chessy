mod app;
mod audio;
mod chess;
mod engine;
mod paths;
mod ui;

use app::ChessyApp;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Chessy")
        .with_inner_size([960.0, 720.0])
        .with_min_inner_size([640.0, 480.0]);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Chessy",
        native_options,
        Box::new(|cc| Ok(Box::new(ChessyApp::new(cc)))),
    )
}

fn load_icon() -> Option<egui::IconData> {
    use resvg::{tiny_skia, usvg};
    let svg_data = std::fs::read(paths::asset_path("icon.svg")).ok()?;
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_data(&svg_data, &opts).ok()?;
    let size = 256u32;
    let scale = size as f32 / tree.size().width();
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let png_bytes = pixmap.encode_png().ok()?;
    eframe::icon_data::from_png_bytes(&png_bytes).ok()
}
