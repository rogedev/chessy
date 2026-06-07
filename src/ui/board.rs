use std::collections::HashMap;

use egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, TextureHandle, Ui, Vec2,
};
use resvg::{tiny_skia, usvg};
use shakmaty::{Color as PieceColor, File, Move, Piece, Position, Rank, Role, Square};

use crate::chess::Game;

const PIECE_NAMES: &[(&str, PieceColor, Role)] = &[
    ("wK", PieceColor::White, Role::King),
    ("wQ", PieceColor::White, Role::Queen),
    ("wR", PieceColor::White, Role::Rook),
    ("wB", PieceColor::White, Role::Bishop),
    ("wN", PieceColor::White, Role::Knight),
    ("wP", PieceColor::White, Role::Pawn),
    ("bK", PieceColor::Black, Role::King),
    ("bQ", PieceColor::Black, Role::Queen),
    ("bR", PieceColor::Black, Role::Rook),
    ("bB", PieceColor::Black, Role::Bishop),
    ("bN", PieceColor::Black, Role::Knight),
    ("bP", PieceColor::Black, Role::Pawn),
];

pub struct PieceTextures {
    handles: HashMap<(PieceColor, Role), TextureHandle>,
}

impl PieceTextures {
    pub fn load(ctx: &egui::Context, set_name: &str) -> Self {
        let handles = PIECE_NAMES
            .iter()
            .filter_map(|&(name, color, role)| {
                let path = format!("assets/pieces/{}/{}.svg", set_name, name);
                let bytes = std::fs::read(path).ok()?;
                let handle = rasterize_svg(ctx, name, &bytes, 256)?;
                Some(((color, role), handle))
            })
            .collect();
        Self { handles }
    }

    pub fn get(&self, piece: Piece) -> Option<&TextureHandle> {
        self.handles.get(&(piece.color, piece.role))
    }
}

pub fn available_piece_sets() -> Vec<String> {
    let mut sets: Vec<String> = std::fs::read_dir("assets/pieces")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    sets.sort();
    sets
}

fn rasterize_svg(
    ctx: &egui::Context,
    name: &str,
    bytes: &[u8],
    target: u32,
) -> Option<TextureHandle> {
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_data(bytes, &opts).ok()?;
    let svg_size = tree.size();
    let scale = target as f32 / svg_size.width().max(svg_size.height());
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    let mut pixmap = tiny_skia::Pixmap::new(target, target)?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let color_image =
        egui::ColorImage::from_rgba_unmultiplied([target as usize, target as usize], pixmap.data());
    Some(ctx.load_texture(name, color_image, egui::TextureOptions::default()))
}

// Light theme
const LIGHT_SQ_LIGHT: Color32 = Color32::from_rgb(240, 217, 181);
const LIGHT_SQ_DARK: Color32 = Color32::from_rgb(181, 136, 99);
// Dark theme
const DARK_SQ_LIGHT: Color32 = Color32::from_rgb(118, 150, 86);
const DARK_SQ_DARK: Color32 = Color32::from_rgb(238, 238, 210);

const HIGHLIGHT_LAST: Color32 = Color32::from_rgba_premultiplied(255, 255, 0, 80);
const HIGHLIGHT_SELECTED: Color32 = Color32::from_rgba_premultiplied(20, 85, 30, 120);
const HIGHLIGHT_LEGAL: Color32 = Color32::from_rgba_premultiplied(20, 85, 30, 80);
const HIGHLIGHT_CHECK: Color32 = Color32::from_rgba_premultiplied(220, 0, 0, 120);

/// Mutable board interaction state shared between the app and the widget.
#[derive(Default)]
pub struct BoardInteraction {
    pub selected: Option<Square>,
    pub drag_from: Option<Square>,
    pub drag_pos: Option<Pos2>,
    pub pending_promotion: Option<(Square, Square)>,
}

pub struct BoardWidget<'a> {
    game: &'a mut Game,
    flipped: bool,
    dark_theme: bool,
    interactive: bool,
    interaction: &'a mut BoardInteraction,
    piece_textures: &'a PieceTextures,
}

impl<'a> BoardWidget<'a> {
    pub fn new(
        game: &'a mut Game,
        flipped: bool,
        dark_theme: bool,
        interactive: bool,
        interaction: &'a mut BoardInteraction,
        piece_textures: &'a PieceTextures,
    ) -> Self {
        Self {
            game,
            flipped,
            dark_theme,
            interactive,
            interaction,
            piece_textures,
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let available = ui.available_size();
        let board_size = available.x.min(available.y);
        let sq_size = board_size / 8.0;

        let (response, painter) =
            ui.allocate_painter(Vec2::splat(board_size), Sense::click_and_drag());

        let rect = response.rect;
        let pos = self.game.current_position();
        let board = pos.board();

        // Legal moves from selected square
        let legal_dests: Vec<Square> = if let Some(from) = self.interaction.selected {
            pos.legal_moves()
                .iter()
                .filter(|m| m.from() == Some(from))
                .map(|m| m.to())
                .collect()
        } else {
            vec![]
        };

        // Last move squares
        let last_move_sqs: Vec<Square> = if self.game.cursor > 0 {
            let m = &self.game.moves[self.game.cursor - 1];
            let mut v = vec![m.to()];
            if let Some(from) = m.from() {
                v.push(from);
            }
            v
        } else {
            vec![]
        };

        // King in check
        let check_sq: Option<Square> = if pos.is_check() {
            pos.board().king_of(pos.turn())
        } else {
            None
        };

        // Draw squares
        for rank_idx in 0..8u32 {
            for file_idx in 0..8u32 {
                let sq = Square::from_coords(File::new(file_idx), Rank::new(rank_idx));
                let sq_rect = self.sq_to_rect(sq, rect, sq_size);

                let is_light_sq = (rank_idx + file_idx) % 2 == 0;
                let sq_color = if self.dark_theme {
                    if is_light_sq {
                        DARK_SQ_DARK
                    } else {
                        DARK_SQ_LIGHT
                    }
                } else if is_light_sq {
                    LIGHT_SQ_LIGHT
                } else {
                    LIGHT_SQ_DARK
                };
                painter.rect_filled(sq_rect, 0.0, sq_color);

                // Highlight layers
                if last_move_sqs.contains(&sq) {
                    painter.rect_filled(sq_rect, 0.0, HIGHLIGHT_LAST);
                }
                if check_sq == Some(sq) {
                    painter.rect_filled(sq_rect, 0.0, HIGHLIGHT_CHECK);
                }
                if self.interaction.selected == Some(sq) {
                    painter.rect_filled(sq_rect, 0.0, HIGHLIGHT_SELECTED);
                }
                if legal_dests.contains(&sq) {
                    if board.piece_at(sq).is_some() {
                        // Capture hint: ring
                        painter.circle_stroke(
                            sq_rect.center(),
                            sq_size * 0.47,
                            egui::Stroke::new(sq_size * 0.08, HIGHLIGHT_LEGAL),
                        );
                    } else {
                        // Move hint: small dot
                        painter.circle_filled(sq_rect.center(), sq_size * 0.15, HIGHLIGHT_LEGAL);
                    }
                }

                if self.interaction.drag_from != Some(sq)
                    && let Some(piece) = board.piece_at(sq)
                {
                    draw_piece(
                        &painter,
                        piece,
                        sq_rect,
                        sq_size,
                        self.piece_textures.get(piece),
                    );
                }
            }
        }

        // Draw rank/file labels
        draw_labels(&painter, rect, sq_size, self.flipped, self.dark_theme);

        // Draw dragged piece at cursor
        if let (Some(from_sq), Some(cursor_pos)) =
            (self.interaction.drag_from, self.interaction.drag_pos)
            && let Some(piece) = board.piece_at(from_sq)
        {
            let drag_rect = Rect::from_center_size(cursor_pos, Vec2::splat(sq_size));
            draw_piece(
                &painter,
                piece,
                drag_rect,
                sq_size,
                self.piece_textures.get(piece),
            );
        }

        let can_interact = self.interactive
            && self.game.at_end()
            && !self.game.is_game_over()
            && self.interaction.pending_promotion.is_none();

        if can_interact {
            // Update drag position
            if response.dragged()
                && let Some(ptr) = response.interact_pointer_pos()
            {
                self.interaction.drag_pos = Some(ptr);
                // Start drag if we're on a piece of the right color
                if self.interaction.drag_from.is_none()
                    && let Some(sq) = pos_to_sq(ptr, rect, sq_size, self.flipped)
                    && let Some(piece) = board.piece_at(sq)
                    && piece.color == pos.turn()
                {
                    self.interaction.drag_from = Some(sq);
                    self.interaction.selected = Some(sq);
                }
            }

            if response.drag_stopped() {
                if let (Some(from), Some(ptr)) =
                    (self.interaction.drag_from, self.interaction.drag_pos)
                    && let Some(to) = pos_to_sq(ptr, rect, sq_size, self.flipped)
                {
                    if is_promotion_move(self.game, from, to) {
                        self.interaction.pending_promotion = Some((from, to));
                    } else {
                        try_make_move(self.game, from, to);
                    }
                }
                self.interaction.drag_from = None;
                self.interaction.drag_pos = None;
                self.interaction.selected = None;
            }

            if response.clicked()
                && let Some(ptr) = response.interact_pointer_pos()
                && let Some(sq) = pos_to_sq(ptr, rect, sq_size, self.flipped)
            {
                handle_click(
                    self.game,
                    &mut self.interaction.selected,
                    sq,
                    &mut self.interaction.pending_promotion,
                );
            }
        }

        response
    }

    fn sq_to_rect(&self, sq: Square, board_rect: Rect, sq_size: f32) -> Rect {
        let (file_idx, rank_idx) = (sq.file().to_u32(), sq.rank().to_u32());
        let (display_file, display_rank) = if self.flipped {
            (7 - file_idx, rank_idx)
        } else {
            (file_idx, 7 - rank_idx)
        };
        let x = board_rect.min.x + display_file as f32 * sq_size;
        let y = board_rect.min.y + display_rank as f32 * sq_size;
        Rect::from_min_size(Pos2::new(x, y), Vec2::splat(sq_size))
    }
}

fn pos_to_sq(ptr: Pos2, board_rect: Rect, sq_size: f32, flipped: bool) -> Option<Square> {
    let rel_x = ptr.x - board_rect.min.x;
    let rel_y = ptr.y - board_rect.min.y;
    if rel_x < 0.0 || rel_y < 0.0 {
        return None;
    }
    let file_idx = (rel_x / sq_size) as u32;
    let rank_idx = (rel_y / sq_size) as u32;
    if file_idx >= 8 || rank_idx >= 8 {
        return None;
    }
    let (actual_file, actual_rank) = if flipped {
        (7 - file_idx, rank_idx)
    } else {
        (file_idx, 7 - rank_idx)
    };
    Some(Square::from_coords(
        File::new(actual_file),
        Rank::new(actual_rank),
    ))
}

fn handle_click(
    game: &mut Game,
    selected: &mut Option<Square>,
    sq: Square,
    pending_promotion: &mut Option<(Square, Square)>,
) {
    if let Some(from) = *selected {
        if from == sq {
            *selected = None;
            return;
        }
        if is_promotion_move(game, from, sq) {
            *pending_promotion = Some((from, sq));
            *selected = None;
        } else {
            let moved = try_make_move(game, from, sq);
            if moved {
                *selected = None;
            } else {
                let pos = game.current_position();
                if let Some(piece) = pos.board().piece_at(sq) {
                    if piece.color == pos.turn() {
                        *selected = Some(sq);
                    } else {
                        *selected = None;
                    }
                } else {
                    *selected = None;
                }
            }
        }
    } else {
        let pos = game.current_position();
        if let Some(piece) = pos.board().piece_at(sq)
            && piece.color == pos.turn()
        {
            *selected = Some(sq);
        }
    }
}

fn is_promotion_move(game: &Game, from: Square, to: Square) -> bool {
    game.current_position().legal_moves().iter().any(|m| {
        m.from() == Some(from)
            && m.to() == to
            && matches!(
                m,
                Move::Normal {
                    promotion: Some(_),
                    ..
                }
            )
    })
}

pub fn try_make_promotion_move(game: &mut Game, from: Square, to: Square, role: Role) -> bool {
    let legals = game.current_position().legal_moves();
    let m = legals.iter().find(|m| {
        m.from() == Some(from)
            && m.to() == to
            && matches!(m, Move::Normal { promotion: Some(r), .. } if *r == role)
    });
    m.map(|m| game.make_move(*m).is_ok()).unwrap_or(false)
}

fn try_make_move(game: &mut Game, from: Square, to: Square) -> bool {
    let legals = game.current_position().legal_moves();
    let m = legals.iter().find(|m| {
        m.from() == Some(from)
            && m.to() == to
            && !matches!(
                m,
                Move::Normal {
                    promotion: Some(_),
                    ..
                }
            )
    });
    m.map(|m| game.make_move(*m).is_ok()).unwrap_or(false)
}

pub fn piece_symbol(piece: Piece) -> &'static str {
    match (piece.color, piece.role) {
        (PieceColor::White, Role::King) => "♔",
        (PieceColor::White, Role::Queen) => "♕",
        (PieceColor::White, Role::Rook) => "♖",
        (PieceColor::White, Role::Bishop) => "♗",
        (PieceColor::White, Role::Knight) => "♘",
        (PieceColor::White, Role::Pawn) => "♙",
        (PieceColor::Black, Role::King) => "♚",
        (PieceColor::Black, Role::Queen) => "♛",
        (PieceColor::Black, Role::Rook) => "♜",
        (PieceColor::Black, Role::Bishop) => "♝",
        (PieceColor::Black, Role::Knight) => "♞",
        (PieceColor::Black, Role::Pawn) => "♟",
    }
}

fn draw_piece(
    painter: &Painter,
    piece: Piece,
    rect: Rect,
    sq_size: f32,
    texture: Option<&TextureHandle>,
) {
    if let Some(tex) = texture {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(tex.id(), rect, uv, Color32::WHITE);
        return;
    }

    let symbol = piece_symbol(piece);
    let (fg, shadow) = match piece.color {
        PieceColor::White => (Color32::WHITE, Color32::from_rgb(40, 40, 40)),
        PieceColor::Black => (
            Color32::from_rgb(20, 20, 20),
            Color32::from_rgb(200, 200, 200),
        ),
    };
    let font = FontId::proportional(sq_size * 0.78);
    for dx in [-1.0f32, 0.0, 1.0] {
        for dy in [-1.0f32, 0.0, 1.0] {
            if dx != 0.0 || dy != 0.0 {
                painter.text(
                    rect.center() + Vec2::new(dx * 1.5, dy * 1.5),
                    Align2::CENTER_CENTER,
                    symbol,
                    font.clone(),
                    shadow,
                );
            }
        }
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, symbol, font, fg);
}

fn draw_labels(painter: &Painter, board_rect: Rect, sq_size: f32, flipped: bool, dark_theme: bool) {
    let label_color = if dark_theme {
        Color32::from_rgb(180, 180, 180)
    } else {
        Color32::from_rgb(100, 100, 100)
    };
    let font = FontId::proportional(sq_size * 0.2);

    let files = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
    let ranks = ['1', '2', '3', '4', '5', '6', '7', '8'];

    for i in 0..8 {
        let file_char = if flipped { files[7 - i] } else { files[i] };
        let x = board_rect.min.x + i as f32 * sq_size + sq_size * 0.05;
        let y = board_rect.max.y - sq_size * 0.22;
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            file_char,
            font.clone(),
            label_color,
        );

        let rank_char = if flipped { ranks[i] } else { ranks[7 - i] };
        let x2 = board_rect.max.x - sq_size * 0.18;
        let y2 = board_rect.min.y + i as f32 * sq_size + sq_size * 0.12;
        painter.text(
            Pos2::new(x2, y2),
            Align2::CENTER_TOP,
            rank_char,
            font.clone(),
            label_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::splat(800.0))
    }

    fn center_of(file: u32, rank_from_top: u32) -> Pos2 {
        // 100px squares; offset to the middle of the target cell.
        Pos2::new(
            file as f32 * 100.0 + 50.0,
            rank_from_top as f32 * 100.0 + 50.0,
        )
    }

    #[test]
    fn pos_to_sq_maps_corners_unflipped() {
        let rect = board_rect();
        // Top-left is a8, bottom-left is a1, bottom-right is h1.
        assert_eq!(
            pos_to_sq(center_of(0, 0), rect, 100.0, false),
            Some(Square::A8)
        );
        assert_eq!(
            pos_to_sq(center_of(0, 7), rect, 100.0, false),
            Some(Square::A1)
        );
        assert_eq!(
            pos_to_sq(center_of(7, 7), rect, 100.0, false),
            Some(Square::H1)
        );
    }

    #[test]
    fn pos_to_sq_maps_corner_flipped() {
        let rect = board_rect();
        // Flipped: top-left becomes h1.
        assert_eq!(
            pos_to_sq(center_of(0, 0), rect, 100.0, true),
            Some(Square::H1)
        );
    }

    #[test]
    fn pos_to_sq_rejects_points_off_board() {
        let rect = board_rect();
        assert_eq!(pos_to_sq(Pos2::new(850.0, 50.0), rect, 100.0, false), None);
        assert_eq!(pos_to_sq(Pos2::new(-10.0, 50.0), rect, 100.0, false), None);
    }

    #[test]
    fn piece_symbol_covers_all_twelve_pieces() {
        use PieceColor::{Black, White};
        let cases = [
            (White, Role::King, "♔"),
            (White, Role::Queen, "♕"),
            (White, Role::Rook, "♖"),
            (White, Role::Bishop, "♗"),
            (White, Role::Knight, "♘"),
            (White, Role::Pawn, "♙"),
            (Black, Role::King, "♚"),
            (Black, Role::Queen, "♛"),
            (Black, Role::Rook, "♜"),
            (Black, Role::Bishop, "♝"),
            (Black, Role::Knight, "♞"),
            (Black, Role::Pawn, "♟"),
        ];
        for (color, role, expected) in cases {
            assert_eq!(piece_symbol(Piece { color, role }), expected);
        }
    }

    #[test]
    fn available_piece_sets_returns_sorted_known_sets() {
        let sets = available_piece_sets();
        // The three bundled sets must all be present.
        assert!(sets.contains(&"alpha".to_string()));
        assert!(sets.contains(&"cardinal".to_string()));
        assert!(sets.contains(&"cburnett".to_string()));
        // The list must be sorted.
        let mut sorted = sets.clone();
        sorted.sort();
        assert_eq!(sets, sorted);
    }

    #[test]
    fn available_piece_sets_contains_no_duplicates() {
        let sets = available_piece_sets();
        let mut unique = sets.clone();
        unique.dedup();
        assert_eq!(sets, unique);
    }

    #[test]
    fn is_promotion_move_detects_pawn_to_last_rank() {
        let game = Game::from_fen("8/P7/8/8/8/8/8/k6K w - - 0 1").expect("valid fen");
        assert!(is_promotion_move(&game, Square::A7, Square::A8));
        // A king step is not a promotion.
        assert!(!is_promotion_move(&game, Square::H1, Square::H2));
    }

    #[test]
    fn try_make_move_executes_legal_and_rejects_illegal() {
        let mut game = Game::new();
        assert!(try_make_move(&mut game, Square::E2, Square::E4));
        assert_eq!(game.cursor, 1);

        let mut game2 = Game::new();
        assert!(!try_make_move(&mut game2, Square::E2, Square::E5));
        assert_eq!(game2.cursor, 0);
    }

    #[test]
    fn try_make_promotion_move_promotes_to_chosen_role() {
        let mut game = Game::from_fen("8/P7/8/8/8/8/8/k6K w - - 0 1").expect("valid fen");
        assert!(try_make_promotion_move(
            &mut game,
            Square::A7,
            Square::A8,
            Role::Queen
        ));
        assert_eq!(game.cursor, 1);
        assert_eq!(game.san, vec!["a8=Q+"]);
    }
}
