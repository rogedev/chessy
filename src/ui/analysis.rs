use egui::{Color32, FontId, Rect, RichText, Ui, Vec2};
use shakmaty::{Chess, Position, uci::UciMove};

use crate::engine::{EngineInfo, Score};

pub fn show_eval_bar(ui: &mut Ui, score: Option<&Score>, _white_to_move: bool) {
    let (total_width, bar_height) = (ui.available_width(), 20.0);
    let (response, painter) =
        ui.allocate_painter(Vec2::new(total_width, bar_height), egui::Sense::hover());
    let rect = response.rect;

    let cp = score.map(|s| s.as_cp_f32()).unwrap_or(0.0);
    // Clamp to ±600 cp for display purposes
    let clamped = cp.clamp(-600.0, 600.0);
    // white_fraction: 0 = black winning, 1 = white winning
    let white_frac = (clamped + 600.0) / 1200.0;

    let white_width = total_width * white_frac;

    painter.rect_filled(rect, 0.0, Color32::from_rgb(220, 220, 220));
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(white_width, bar_height)),
        0.0,
        Color32::WHITE,
    );
    painter.rect_filled(
        Rect::from_min_size(
            egui::Pos2::new(rect.min.x + white_width, rect.min.y),
            Vec2::new(total_width - white_width, bar_height),
        ),
        0.0,
        Color32::from_rgb(30, 30, 30),
    );

    // Score label
    if let Some(s) = score {
        let label = s.display();

        if white_frac > 0.5 {
            painter.text(
                egui::Pos2::new(rect.min.x + 4.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &label,
                FontId::proportional(11.0),
                Color32::BLACK,
            );
        } else {
            painter.text(
                egui::Pos2::new(rect.max.x - 4.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &label,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
    }
}

pub fn show_analysis_panel(ui: &mut Ui, lines: &[EngineInfo], position: &Chess, dark_theme: bool) {
    let text_color = if dark_theme {
        Color32::from_rgb(210, 210, 210)
    } else {
        Color32::from_rgb(30, 30, 30)
    };

    if lines.is_empty() {
        ui.label(
            RichText::new("Engine not running")
                .color(Color32::GRAY)
                .font(FontId::proportional(12.0))
                .italics(),
        );
        return;
    }

    // Fixed-width eval/depth columns keep the table aligned across rows; the move
    const EVAL_W: f32 = 48.0;
    const DEPTH_W: f32 = 32.0;
    const ROW_H: f32 = 18.0;

    for info in lines {
        ui.horizontal(|ui| {
            ui.add_sized(
                Vec2::new(EVAL_W, ROW_H),
                egui::Label::new(
                    RichText::new(info.score.display())
                        .font(FontId::monospace(12.0))
                        .color(eval_color(&info.score, dark_theme))
                        .strong(),
                ),
            );
            ui.add_sized(
                Vec2::new(DEPTH_W, ROW_H),
                egui::Label::new(
                    RichText::new(format!("d{}", info.depth))
                        .font(FontId::proportional(11.0))
                        .color(Color32::GRAY),
                ),
            );

            let pv_san = pv_to_san(position, &info.pv);
            ui.add(
                egui::Label::new(
                    RichText::new(pv_san)
                        .font(FontId::proportional(12.0))
                        .color(text_color),
                )
                .truncate(),
            );
        });
    }
}

fn eval_color(score: &Score, dark_theme: bool) -> Color32 {
    let cp = score.as_cp_f32();
    if cp > 10.0 {
        if dark_theme {
            Color32::from_rgb(100, 200, 100)
        } else {
            Color32::from_rgb(0, 130, 0)
        }
    } else if cp < -10.0 {
        if dark_theme {
            Color32::from_rgb(220, 100, 100)
        } else {
            Color32::from_rgb(170, 0, 0)
        }
    } else {
        Color32::GRAY
    }
}

pub(crate) fn pv_to_san(position: &Chess, pv: &[String]) -> String {
    let mut pos = position.clone();
    let mut parts = vec![];
    let mut first = true;

    for uci_str in pv.iter().take(8) {
        let Ok(uci) = uci_str.parse::<UciMove>() else {
            break;
        };

        let Ok(m) = uci.to_move(&pos) else { break };
        let san = shakmaty::san::SanPlus::from_move(pos.clone(), m);

        let move_num = pos.fullmoves();

        if pos.turn() == shakmaty::Color::White {
            parts.push(format!("{}.", move_num));
        } else if first {
            parts.push(format!("{}...", move_num));
        }

        first = false;

        parts.push(san.to_string());

        if let Ok(new_pos) = pos.play(m) {
            pos = new_pos;
        } else {
            break;
        }
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::uci::UciMove;

    fn pos_after(moves: &[&str]) -> Chess {
        let mut pos = Chess::default();
        for uci_str in moves {
            let uci = uci_str.parse::<UciMove>().unwrap();
            let m = uci.to_move(&pos).unwrap();
            pos = pos.play(m).unwrap();
        }
        pos
    }

    #[test]
    fn single_white_move_from_start() {
        let result = pv_to_san(&Chess::default(), &["e2e4".to_string()]);
        assert_eq!(result, "1. e4");
    }

    #[test]
    fn single_black_move_includes_ellipsis() {
        let pos = pos_after(&["e2e4"]);
        let result = pv_to_san(&pos, &["e7e5".to_string()]);
        assert_eq!(result, "1... e5");
    }

    #[test]
    fn two_moves_share_move_number() {
        let result = pv_to_san(&Chess::default(), &["e2e4".to_string(), "e7e5".to_string()]);
        assert_eq!(result, "1. e4 e5");
    }

    #[test]
    fn move_number_increments_after_black_responds() {
        let result = pv_to_san(
            &Chess::default(),
            &["e2e4".to_string(), "e7e5".to_string(), "g1f3".to_string()],
        );
        assert_eq!(result, "1. e4 e5 2. Nf3");
    }

    #[test]
    fn invalid_uci_stops_output_early() {
        let result = pv_to_san(
            &Chess::default(),
            &[
                "e2e4".to_string(),
                "notamove".to_string(),
                "d2d4".to_string(),
            ],
        );
        assert_eq!(result, "1. e4");
    }

    #[test]
    fn respects_eight_move_limit() {
        let moves: Vec<String> = [
            "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let result = pv_to_san(&Chess::default(), &moves);
        let move_tokens = result
            .split_whitespace()
            .filter(|t| !t.ends_with('.') && !t.ends_with("..."))
            .count();
        assert_eq!(move_tokens, 8);
    }

    #[test]
    fn empty_pv_returns_empty_string() {
        assert_eq!(pv_to_san(&Chess::default(), &[]), "");
    }
}
