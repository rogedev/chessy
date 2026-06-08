use egui::{Color32, FontId, RichText, ScrollArea, Ui};
use shakmaty::Color as PieceColor;

use shakmaty::Position;

use crate::chess::Game;

pub fn show_moves_panel(ui: &mut Ui, game: &mut Game, dark_theme: bool) -> bool {
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

    let move_text =
        |san: &str, font_size: f32| RichText::new(san).font(FontId::proportional(font_size));
    let num_label = |text: String| move_text(&text, 13.0).color(Color32::GRAY);

    let move_count = game.moves.len();
    let cursor = game.cursor;
    let black_first = game.start_position().turn() == PieceColor::Black;

    let mut clicked = false;

    ScrollArea::vertical()
        .id_salt("moves_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(2.0, 1.0);

            if move_count == 0 {
                ui.label(
                    move_text("No moves yet", 13.0)
                        .color(Color32::GRAY)
                        .italics(),
                );
                return;
            }

            // Each row covers one full move pair. When black starts, row 0 has only a black move.
            let mut i = 0usize;
            while i < move_count {
                let move_num = i / 2 + 1;

                ui.horizontal_wrapped(|ui| {
                    if i == 0 && black_first {
                        ui.label(num_label(format!("{}...", move_num)));
                        render_move_button(
                            ui,
                            game,
                            i,
                            cursor,
                            active_color,
                            text_color,
                            &mut clicked,
                        );
                        i += 1;
                    } else {
                        ui.label(num_label(format!("{}.", move_num)));
                        render_move_button(
                            ui,
                            game,
                            i,
                            cursor,
                            active_color,
                            text_color,
                            &mut clicked,
                        );
                        i += 1;

                        if i < move_count {
                            render_move_button(
                                ui,
                                game,
                                i,
                                cursor,
                                active_color,
                                text_color,
                                &mut clicked,
                            );
                            i += 1;
                        }
                    }
                });
            }
        });

    clicked
}

fn render_move_button(
    ui: &mut Ui,
    game: &mut Game,
    index: usize,
    cursor: usize,
    active_color: Color32,
    text_color: Color32,
    clicked: &mut bool,
) {
    let color = if cursor == index + 1 {
        active_color
    } else {
        text_color
    };
    let label = RichText::new(game.san[index].as_str())
        .font(FontId::proportional(13.0))
        .color(color)
        .strong();

    if ui.button(label).clicked() {
        game.go_to(index + 1);
        *clicked = true;
    }
}
