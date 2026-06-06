use egui::{Color32, FontId, RichText, ScrollArea, Ui};
use shakmaty::Color as PieceColor;

use shakmaty::Position;

use crate::chess::Game;

pub fn show_moves_panel(ui: &mut Ui, game: &mut Game, dark_theme: bool) {
    let active_color = if dark_theme {
        Color32::from_rgb(100, 160, 240)
    } else {
        Color32::from_rgb(30, 100, 200)
    };
    let text_color = if dark_theme {
        Color32::from_rgb(220, 220, 220)
    } else {
        Color32::from_rgb(30, 30, 30)
    };

    ScrollArea::vertical()
        .id_salt("moves_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(2.0, 1.0);

            let move_count = game.moves.len();
            let cursor = game.cursor;
            let start_turn = game.start_position().turn();

            // Determine move number offset if black starts
            let black_first = start_turn == PieceColor::Black;

            let mut i = 0usize;
            while i < move_count {
                ui.horizontal_wrapped(|ui| {
                    // Move number
                    let move_num = i / 2 + 1;

                    if i == 0 && black_first {
                        // Show "1..." prefix for black's first move
                        ui.label(
                            RichText::new(format!("{}...", move_num))
                                .color(Color32::GRAY)
                                .font(FontId::proportional(13.0)),
                        );
                        // Only black move
                        let is_current = cursor == i + 1;
                        let san = &game.san[i];
                        let label = RichText::new(san.as_str())
                            .font(FontId::proportional(13.0))
                            .color(if is_current { active_color } else { text_color })
                            .strong();
                        if ui.button(label).clicked() {
                            game.go_to(i + 1);
                        }
                        i += 1;
                    } else {
                        // White move
                        if i.is_multiple_of(2) {
                            ui.label(
                                RichText::new(format!("{}.", move_num))
                                    .color(Color32::GRAY)
                                    .font(FontId::proportional(13.0)),
                            );
                        }
                        let is_current = cursor == i + 1;
                        let san = &game.san[i];
                        let label = RichText::new(san.as_str())
                            .font(FontId::proportional(13.0))
                            .color(if is_current { active_color } else { text_color })
                            .strong();
                        if ui.button(label).clicked() {
                            game.go_to(i + 1);
                        }
                        i += 1;

                        // Black move
                        if i < move_count {
                            let is_current = cursor == i + 1;
                            let san = &game.san[i];
                            let label = RichText::new(san.as_str())
                                .font(FontId::proportional(13.0))
                                .color(if is_current { active_color } else { text_color })
                                .strong();
                            if ui.button(label).clicked() {
                                game.go_to(i + 1);
                            }
                            i += 1;
                        }
                    }
                });
            }

            if move_count == 0 {
                ui.label(
                    RichText::new("No moves yet")
                        .color(Color32::GRAY)
                        .font(FontId::proportional(13.0))
                        .italics(),
                );
            }
        });
}
