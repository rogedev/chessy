use egui::{Color32, FontId, RichText, ScrollArea, Stroke, Ui, Vec2};
use shakmaty::Color as PieceColor;

use shakmaty::Position;

use super::graph::class_color;
use super::theme::Palette;
use crate::chess::{Game, review::MoveClass};

const NUMBER_WIDTH: f32 = 30.0;
const CELL_WIDTH: f32 = 68.0;
const CELL_HEIGHT: f32 = 22.0;

/// Something the user asked for in the move list. Collected while drawing
/// and applied afterwards, so the list never changes under the loop.
enum Action {
    GoTo(usize),
    Select(usize),
    Promote(usize),
    Delete(usize),
}

/// `N.` for a White move and `N...` for a Black move. `ply` counts
/// half-moves from White's first move, so a game that starts with Black to
/// move begins at ply 1.
fn move_number(ply: usize) -> String {
    let number = ply / 2 + 1;
    if ply.is_multiple_of(2) {
        format!("{number}.")
    } else {
        format!("{number}...")
    }
}

/// Draw the move list with its variations. Returns true when the displayed
/// position changed because of a click.
pub fn show_moves_panel(
    ui: &mut Ui,
    game: &mut Game,
    palette: &Palette,
    classes: Option<&[MoveClass]>,
) -> bool {
    let move_text =
        |san: &str, font_size: f32| RichText::new(san).font(FontId::proportional(font_size));

    let move_count = game.moves.len();
    let cursor = game.cursor;
    // Ply of the first move: 1 when the game starts with Black to move.
    let offset = usize::from(game.start_position().turn() == PieceColor::Black);

    let mut action: Option<Action> = None;

    ScrollArea::vertical()
        .id_salt("moves_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(2.0, 1.0);

            if move_count == 0 {
                ui.label(
                    move_text("No moves yet", 13.0)
                        .color(palette.text_weak)
                        .italics(),
                );
                return;
            }

            // Each row holds White's move and Black's reply. A game that
            // starts with Black to move has a first row with one move.
            let mut i = 0usize;
            while i < move_count {
                let row_start = i;

                let band = ui.painter().add(egui::Shape::Noop);
                let row = ui.horizontal(|ui| {
                    let white_to_move = (i + offset).is_multiple_of(2);
                    let (num_rect, _) = ui.allocate_exact_size(
                        Vec2::new(NUMBER_WIDTH, CELL_HEIGHT),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        num_rect.right_center(),
                        egui::Align2::RIGHT_CENTER,
                        move_number(i + offset),
                        FontId::proportional(13.0),
                        palette.text_weak,
                    );
                    let class = classes.and_then(|c| c.get(i)).copied();
                    render_move_button(ui, game, i, cursor, palette, class, &mut action);
                    i += 1;

                    if white_to_move && i < move_count {
                        let class = classes.and_then(|c| c.get(i)).copied();
                        render_move_button(ui, game, i, cursor, palette, class, &mut action);
                        i += 1;
                    }
                });
                if (row_start + offset) / 2 % 2 == 1 {
                    let rect = row.response.rect.expand2(Vec2::new(4.0, 0.0));
                    ui.painter().set(
                        band,
                        egui::Shape::rect_filled(rect, 3.0, palette.chip.gamma_multiply(0.35)),
                    );
                }

                // Other moves tried in place of the ones on this row.
                for ply in row_start..i {
                    let alternatives = game.alternatives(ply);
                    if alternatives.is_empty() {
                        continue;
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(14.0);
                        ui.label(move_text("or", 11.0).color(palette.text_weak).italics());
                        for (node, san) in alternatives {
                            let text = format!("{} {}", move_number(ply + offset), san);
                            let label = move_text(&text, 12.0).color(palette.text_weak).italics();
                            if ui
                                .button(label)
                                .on_hover_text("Switch to this line")
                                .clicked()
                            {
                                action = Some(Action::Select(node));
                            }
                        }
                    });
                }
            }
        });

    match action {
        Some(Action::GoTo(idx)) => {
            game.go_to(idx);
            true
        }
        Some(Action::Select(node)) => {
            game.select_variation(node);
            true
        }
        Some(Action::Promote(ply)) => {
            game.promote_variation(ply);
            false
        }
        Some(Action::Delete(ply)) => {
            game.delete_from(ply);
            false
        }
        None => false,
    }
}

fn render_move_button(
    ui: &mut Ui,
    game: &Game,
    index: usize,
    cursor: usize,
    palette: &Palette,
    class: Option<MoveClass>,
    action: &mut Option<Action>,
) {
    let current = cursor == index + 1;
    let color = if current {
        palette.background
    } else {
        class.and_then(class_color).unwrap_or(palette.text)
    };
    let is_main = game.is_main_move(index);
    let text = match class {
        Some(c) => format!("{}{}", game.san[index], c.symbol()),
        None => game.san[index].clone(),
    };
    let mut label = RichText::new(text)
        .font(FontId::proportional(13.0))
        .color(color)
        .strong();
    if !is_main {
        // Moves of a side line are set in italics.
        label = label.italics();
    }

    let fill = if current {
        palette.accent
    } else {
        Color32::TRANSPARENT
    };
    let mut response = ui.add(
        egui::Button::new(label)
            .fill(fill)
            .stroke(Stroke::NONE)
            .min_size(Vec2::new(CELL_WIDTH, CELL_HEIGHT)),
    );
    if let Some(c) = class {
        response = response.on_hover_text(match c {
            MoveClass::Best => "Best move",
            MoveClass::Good => "Good move",
            MoveClass::Inaccuracy => "Inaccuracy",
            MoveClass::Mistake => "Mistake",
            MoveClass::Blunder => "Blunder",
        });
    }
    if response.clicked() {
        *action = Some(Action::GoTo(index + 1));
    }
    response.context_menu(|ui| {
        if ui
            .add_enabled(!is_main, egui::Button::new("Promote to main line"))
            .clicked()
        {
            *action = Some(Action::Promote(index));
            ui.close();
        }
        if ui.button("Delete from here").clicked() {
            *action = Some(Action::Delete(index));
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_number_for_white_and_black() {
        assert_eq!(move_number(0), "1.");
        assert_eq!(move_number(1), "1...");
        assert_eq!(move_number(2), "2.");
        assert_eq!(move_number(3), "2...");
        assert_eq!(move_number(40), "21.");
    }
}
