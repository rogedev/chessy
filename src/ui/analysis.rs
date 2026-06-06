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

    // From perspective: if black to move, flip visually? No, eval bar always shows white advantage top.
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
        let text_x = if white_frac > 0.5 {
            rect.min.x + 4.0
        } else {
            rect.max.x - 4.0
        };
        let color = if white_frac > 0.5 {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        let align = if white_frac > 0.5 {
            egui::Align2::LEFT_CENTER
        } else {
            egui::Align2::RIGHT_CENTER
        };
        painter.text(
            egui::Pos2::new(text_x, rect.center().y),
            align,
            &label,
            FontId::proportional(11.0),
            color,
        );
    }
}

pub fn show_analysis_panel(ui: &mut Ui, lines: &[EngineInfo], position: &Chess, dark_theme: bool) {
    let text_color = if dark_theme {
        Color32::from_rgb(210, 210, 210)
    } else {
        Color32::from_rgb(30, 30, 30)
    };
    let score_color = if dark_theme {
        Color32::from_rgb(100, 200, 100)
    } else {
        Color32::from_rgb(0, 130, 0)
    };
    let depth_color = Color32::GRAY;

    if lines.is_empty() {
        ui.label(
            RichText::new("Engine not running")
                .color(Color32::GRAY)
                .font(FontId::proportional(12.0))
                .italics(),
        );
        return;
    }

    for info in lines {
        ui.horizontal(|ui| {
            // Score
            ui.label(
                RichText::new(info.score.display())
                    .font(FontId::monospace(12.0))
                    .color(score_color)
                    .strong(),
            );

            // Depth
            ui.label(
                RichText::new(format!("d{}", info.depth))
                    .font(FontId::proportional(11.0))
                    .color(depth_color),
            );

            // PV in SAN notation
            let pv_san = pv_to_san(position, &info.pv);
            ui.label(
                RichText::new(pv_san)
                    .font(FontId::proportional(12.0))
                    .color(text_color),
            );
        });
        ui.add_space(1.0);
    }
}

fn pv_to_san(position: &Chess, pv: &[String]) -> String {
    let mut pos = position.clone();
    let mut parts = vec![];
    let mut move_num = pos.fullmoves().get();
    let mut first = true;

    for uci_str in pv.iter().take(8) {
        let Ok(uci) = uci_str.parse::<UciMove>() else {
            break;
        };
        let Ok(m) = uci.to_move(&pos) else { break };
        let san = shakmaty::san::SanPlus::from_move(pos.clone(), m.clone());

        if first || pos.turn() == shakmaty::Color::White {
            if pos.turn() == shakmaty::Color::White {
                parts.push(format!("{}.", move_num));
                if !first {
                    move_num += 1;
                }
            } else if first {
                parts.push(format!("{}...", move_num));
            }
        }
        first = false;

        parts.push(san.to_string());

        if pos.turn() == shakmaty::Color::Black {
            move_num += 1;
        }

        if let Ok(new_pos) = pos.play(m) {
            pos = new_pos;
        } else {
            break;
        }
    }

    parts.join(" ")
}
