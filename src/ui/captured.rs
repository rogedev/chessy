use egui::{Color32, RichText, Ui, Vec2};
use shakmaty::{Color as PieceColor, Piece, Role};

use super::board::PieceTextures;

const ICON: f32 = 16.0;
/// Horizontal step between icons of the same kind; smaller than the icon so
/// identical pieces overlap like a fanned stack.
const SAME_STEP: f32 = 8.0;
const GROUP_GAP: f32 = 3.0;

/// Draw the pieces a player has captured, followed by `+N` when that player
/// is ahead on material.
pub fn show_captured(
    ui: &mut Ui,
    textures: &PieceTextures,
    captured_color: PieceColor,
    captured: &[Role],
    advantage: i32,
) {
    if !captured.is_empty() {
        // Width: one full icon per group plus a partial step per repeat.
        let groups = captured.windows(2).filter(|w| w[0] != w[1]).count() + 1;
        let repeats = captured.len() - groups;
        let width =
            groups as f32 * ICON + repeats as f32 * SAME_STEP + (groups - 1) as f32 * GROUP_GAP;

        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ICON), egui::Sense::hover());
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let mut x = rect.min.x;
        let mut prev: Option<Role> = None;

        for &role in captured {
            if let Some(p) = prev {
                x += if p == role {
                    SAME_STEP
                } else {
                    ICON + GROUP_GAP
                };
            }
            prev = Some(role);

            let piece = Piece {
                color: captured_color,
                role,
            };
            if let Some(tex) = textures.get(piece) {
                let icon = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), Vec2::splat(ICON));
                ui.painter().image(tex.id(), icon, uv, Color32::WHITE);
            }
        }
    }

    if advantage > 0 {
        ui.label(
            RichText::new(format!("+{advantage}"))
                .small()
                .color(Color32::GRAY),
        );
    }
}
