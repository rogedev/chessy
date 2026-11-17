use egui::{Color32, FontId, Rect, RichText, Ui, Vec2};
use shakmaty::{Chess, Position, uci::UciMove};

use super::theme::Palette;
use crate::engine::{EngineInfo, Score};

/// Share of the bar that belongs to White, from 0 to 1. Uses the same
/// logistic curve as Lichess so small edges still register visually.
pub fn white_fraction(score: Option<&Score>) -> f32 {
    let cp = score.map_or(0.0, Score::as_cp_f32);
    0.5 + 0.5 * (2.0 / (1.0 + (-0.003_682_08 * cp).exp()) - 1.0)
}

/// Draw a vertical evaluation bar filling `rect`, White at the bottom
/// unless the board is flipped.
pub fn show_eval_bar(
    ui: &mut Ui,
    rect: Rect,
    score: Option<&Score>,
    flipped: bool,
    palette: &Palette,
) {
    let target = white_fraction(score);
    let frac = ui
        .ctx()
        .animate_value_with_time(ui.id().with("eval_bar"), target, 0.25);
    let painter = ui.painter();
    let radius = 4.0;

    let light = Color32::from_gray(235);
    let dark = Color32::from_gray(38);
    painter.rect_filled(rect, radius, dark);

    let white_height = rect.height() * frac;
    let white_rect = if flipped {
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), white_height))
    } else {
        Rect::from_min_size(
            egui::pos2(rect.min.x, rect.max.y - white_height),
            Vec2::new(rect.width(), white_height),
        )
    };
    painter.rect_filled(white_rect, radius, light);
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(1.0_f32, palette.text_weak.gamma_multiply(0.5)),
    );

    if let Some(score) = score {
        let white_ahead = target >= 0.5;
        let at_bottom = white_ahead != flipped;
        let (pos, align) = if at_bottom {
            (
                rect.center_bottom() - Vec2::new(0.0, 4.0),
                egui::Align2::CENTER_BOTTOM,
            )
        } else {
            (
                rect.center_top() + Vec2::new(0.0, 4.0),
                egui::Align2::CENTER_TOP,
            )
        };
        let color = if white_ahead {
            Color32::from_gray(30)
        } else {
            Color32::from_gray(230)
        };
        painter.text(
            pos,
            align,
            score.display(),
            FontId::proportional(11.0),
            color,
        );
    }
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0_f32, palette.text_weak),
        egui::StrokeKind::Inside,
    );
}

pub fn show_analysis_panel(ui: &mut Ui, lines: &[EngineInfo], position: &Chess, dark_theme: bool) {
    let text_color = if dark_theme {
        Color32::from_rgb(210, 210, 210)
    } else {
        Color32::from_rgb(30, 30, 30)
    };

    let weak = Palette::new(dark_theme).text_weak;
    if lines.is_empty() {
        ui.label(
            RichText::new("Engine not running")
                .color(weak)
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
                        .color(weak),
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
        Palette::new(dark_theme).text_weak
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
    #[test]
    fn white_fraction_is_symmetric_and_bounded() {
        assert!((white_fraction(None) - 0.5).abs() < 1e-6);
        let up = white_fraction(Some(&Score::Cp(150)));
        let down = white_fraction(Some(&Score::Cp(-150)));
        assert!((up + down - 1.0).abs() < 1e-5);
        assert!(up > 0.5 && up < 1.0);
        assert!(white_fraction(Some(&Score::Mate(3))) > 0.99);
        assert!(white_fraction(Some(&Score::Mate(-3))) < 0.01);
    }

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
