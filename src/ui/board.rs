use std::{cell::RefCell, collections::HashMap};

use egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Shadow, TextureHandle, Ui,
    Vec2,
};
use resvg::{tiny_skia, usvg};
use shakmaty::{
    Board, Chess, Color as PieceColor, File, Move, Piece, Position, Rank, Role, Square,
};

use super::animation::MoveAnimator;
use super::arrows::{self, Annotations};
use crate::audio::SoundEvent;
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
    trees: HashMap<(PieceColor, Role), usvg::Tree>,
    cache: RefCell<HashMap<(PieceColor, Role, u32), TextureHandle>>,
}

impl PieceTextures {
    pub fn load(set_name: &str) -> Self {
        let opts = usvg::Options::default();
        let trees = PIECE_NAMES
            .iter()
            .filter_map(|&(name, color, role)| {
                let path = crate::paths::asset_path(&format!("pieces/{}/{}.svg", set_name, name));
                let bytes = std::fs::read(path).ok()?;
                let tree = usvg::Tree::from_data(&bytes, &opts).ok()?;
                Some(((color, role), tree))
            })
            .collect();
        Self {
            trees,
            cache: RefCell::default(),
        }
    }

    /// Rendered close to `size_px` physical pixels, so it is drawn almost 1:1.
    pub fn get(&self, ctx: &egui::Context, piece: Piece, size_px: f32) -> Option<TextureHandle> {
        let key = (piece.color, piece.role, texture_size(size_px));
        if let Some(handle) = self.cache.borrow().get(&key) {
            return Some(handle.clone());
        }
        let tree = self.trees.get(&(piece.color, piece.role))?;
        let handle = rasterize_svg(ctx, tree, key.2)?;
        self.cache.borrow_mut().insert(key, handle.clone());
        Some(handle)
    }
}

/// Multiples of 16, so resizing the window does not render a new texture every frame.
fn texture_size(size_px: f32) -> u32 {
    (size_px.ceil().max(1.0) as u32)
        .div_ceil(16)
        .saturating_mul(16)
        .clamp(16, 1024)
}

pub fn available_piece_sets() -> Vec<String> {
    let mut sets: Vec<String> = std::fs::read_dir(crate::paths::asset_path("pieces"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    sets.sort();
    sets
}

fn rasterize_svg(ctx: &egui::Context, tree: &usvg::Tree, target: u32) -> Option<TextureHandle> {
    let svg_size = tree.size();
    let scale = target as f32 / svg_size.width().max(svg_size.height());
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    let mut pixmap = tiny_skia::Pixmap::new(target, target)?;
    resvg::render(tree, transform, &mut pixmap.as_mut());
    let color_image =
        egui::ColorImage::from_rgba_unmultiplied([target as usize, target as usize], pixmap.data());
    Some(ctx.load_texture(
        format!("piece-{target}"),
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardTheme {
    pub name: &'static str,
    pub light: Color32,
    pub dark: Color32,
}

pub const BOARD_THEMES: &[BoardTheme] = &[
    BoardTheme {
        name: "Green",
        light: Color32::from_rgb(238, 238, 210),
        dark: Color32::from_rgb(118, 150, 86),
    },
    BoardTheme {
        name: "Brown",
        light: Color32::from_rgb(240, 217, 181),
        dark: Color32::from_rgb(181, 136, 99),
    },
    BoardTheme {
        name: "Blue",
        light: Color32::from_rgb(222, 227, 230),
        dark: Color32::from_rgb(140, 162, 173),
    },
    BoardTheme {
        name: "Grey",
        light: Color32::from_rgb(220, 220, 220),
        dark: Color32::from_rgb(134, 134, 134),
    },
];

impl BoardTheme {
    /// An empty or unknown name means automatic: green with the dark
    /// interface, brown with the light one.
    pub fn resolve(name: &str, dark_ui: bool) -> BoardTheme {
        BOARD_THEMES
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .copied()
            .unwrap_or(BOARD_THEMES[if dark_ui { 0 } else { 1 }])
    }

    fn square(&self, is_light: bool) -> Color32 {
        if is_light { self.light } else { self.dark }
    }
}

#[derive(Clone, Copy)]
pub struct BoardStyle {
    pub theme: BoardTheme,
    pub show_coordinates: bool,
}

const HIGHLIGHT_LAST: Color32 = Color32::from_rgba_unmultiplied_const(255, 214, 64, 95);
const HIGHLIGHT_SELECTED: Color32 = Color32::from_rgba_unmultiplied_const(255, 214, 64, 150);
const HIGHLIGHT_LEGAL: Color32 = Color32::from_rgba_unmultiplied_const(20, 85, 0, 105);
const HIGHLIGHT_CHECK: Color32 = Color32::from_rgba_unmultiplied_const(230, 30, 30, 22);
const HIGHLIGHT_HINT: Color32 = Color32::from_rgba_unmultiplied_const(40, 110, 220, 120);
const BOARD_CORNER_RADIUS: u8 = 6;

/// Mutable board interaction state shared between the app and the widget.
#[derive(Default)]
pub struct BoardInteraction {
    pub selected: Option<Square>,
    pub drag_from: Option<Square>,
    pub drag_pos: Option<Pos2>,
    pub pending_promotion: Option<(Square, Square)>,
    pub animator: MoveAnimator,
    pub annotations: Annotations,
}

pub struct BoardWidget<'a> {
    game: &'a mut Game,
    flipped: bool,
    style: BoardStyle,
    interactive: bool,
    interaction: &'a mut BoardInteraction,
    piece_textures: &'a PieceTextures,
    hint: Option<(Square, Square)>,
    animate: bool,
    engine_arrow: Option<(Square, Square)>,
}

impl<'a> BoardWidget<'a> {
    pub fn new(
        game: &'a mut Game,
        flipped: bool,
        style: BoardStyle,
        interactive: bool,
        interaction: &'a mut BoardInteraction,
        piece_textures: &'a PieceTextures,
    ) -> Self {
        Self {
            game,
            flipped,
            style,
            interactive,
            interaction,
            piece_textures,
            hint: None,
            animate: true,
            engine_arrow: None,
        }
    }

    pub fn with_hint(mut self, hint: Option<(Square, Square)>) -> Self {
        self.hint = hint;
        self
    }

    pub fn with_animation(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    pub fn with_engine_arrow(mut self, arrow: Option<(Square, Square)>) -> Self {
        self.engine_arrow = arrow;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Option<SoundEvent> {
        let available = ui.available_size();
        let board_size = available.x.min(available.y);
        let sq_size = board_size / 8.0;

        let (response, painter) =
            ui.allocate_painter(Vec2::splat(board_size), Sense::click_and_drag());

        let rect = response.rect;
        paint_board_shadow(ui.painter(), rect);
        let pos = self.game.current_position();
        let board = pos.board();

        let now = ui.input(|i| i.time);
        self.interaction
            .animator
            .update(self.game, now, self.animate);

        // Legal moves from selected square
        let legal_dests: Vec<Square> = if let Some(from) = self.interaction.selected {
            pos.legal_moves()
                .iter()
                .filter(|m| m.from() == Some(from))
                .map(king_destination)
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

        self.interaction.annotations.sync(board);

        // Draw squares
        for rank_idx in 0..8u32 {
            for file_idx in 0..8u32 {
                let sq = Square::from_coords(File::new(file_idx), Rank::new(rank_idx));
                let sq_rect = sq_to_rect(sq, rect, sq_size, self.flipped);

                // a1 is dark, so odd coordinate sums are light.
                let is_light_sq = (rank_idx + file_idx) % 2 == 1;
                let sq_color = self.style.theme.square(is_light_sq);
                let corners = square_corners(sq, self.flipped);
                painter.rect_filled(sq_rect, corners, sq_color);

                if last_move_sqs.contains(&sq) {
                    painter.rect_filled(sq_rect, corners, HIGHLIGHT_LAST);
                }
                if check_sq == Some(sq) {
                    paint_check_glow(&painter, sq_rect);
                }
                if self.hint.is_some_and(|(from, to)| sq == from || sq == to) {
                    painter.rect_filled(sq_rect, corners, HIGHLIGHT_HINT);
                }
                if self.interaction.selected == Some(sq) {
                    painter.rect_filled(sq_rect, corners, HIGHLIGHT_SELECTED);
                }
                if let Some(mark) = self.interaction.annotations.square_color(sq) {
                    arrows::draw_square_mark(&painter, sq_rect, mark);
                }
                if legal_dests.contains(&sq) {
                    if board.piece_at(sq).is_some() {
                        painter.circle_stroke(
                            sq_rect.center(),
                            sq_size * 0.46,
                            egui::Stroke::new(sq_size * 0.09, HIGHLIGHT_LEGAL),
                        );
                    } else {
                        painter.circle_filled(sq_rect.center(), sq_size * 0.16, HIGHLIGHT_LEGAL);
                    }
                }

                if self.interaction.drag_from != Some(sq)
                    && !self.interaction.animator.hides(sq)
                    && let Some(piece) = board.piece_at(sq)
                {
                    draw_piece(&painter, sq_rect, self.piece_textures, piece);
                }
            }
        }

        for (slide, t) in self.interaction.animator.slides(now) {
            if let Some(piece) = board.piece_at(slide.end) {
                let start = sq_to_rect(slide.start, rect, sq_size, self.flipped);
                let end = sq_to_rect(slide.end, rect, sq_size, self.flipped);
                let moving = start.translate((end.min - start.min) * t);
                draw_piece(&painter, moving, self.piece_textures, piece);
            }
        }
        if self.interaction.animator.is_active() {
            ui.ctx().request_repaint();
        }

        // Draw rank/file labels
        if self.style.show_coordinates {
            draw_labels(&painter, rect, sq_size, self.flipped, self.style.theme);
        }

        let center = |sq: Square| sq_to_rect(sq, rect, sq_size, self.flipped).center();
        if let Some((from, to)) = self.engine_arrow
            // Ignore a stale suggestion from another position.
            && board.piece_at(from).is_some_and(|p| p.color == pos.turn())
        {
            arrows::draw_arrow(
                &painter,
                center(from),
                center(to),
                sq_size,
                arrows::ENGINE_COLOR,
            );
        }
        for &((from, to), mark) in &self.interaction.annotations.arrows {
            arrows::draw_arrow(&painter, center(from), center(to), sq_size, mark.color(215));
        }

        // Works even when moves are not allowed (engine thinking, game over).
        let pointer_sq = ui
            .input(|i| i.pointer.latest_pos())
            .and_then(|p| pos_to_sq(p, rect, sq_size, self.flipped));
        let mark_color = ui.input(|i| arrows::MarkColor::from_modifiers(&i.modifiers));
        let secondary = egui::PointerButton::Secondary;

        if response.drag_started_by(secondary) {
            self.interaction.annotations.drag_start = ui
                .input(|i| i.pointer.press_origin())
                .and_then(|p| pos_to_sq(p, rect, sq_size, self.flipped));
        }
        if let Some(from) = self.interaction.annotations.drag_start {
            if response.drag_stopped_by(secondary) {
                self.interaction.annotations.drag_start = None;
                if let Some(to) = pointer_sq {
                    self.interaction
                        .annotations
                        .toggle(from, to, mark_color, board);
                }
            } else if let Some(to) = pointer_sq {
                arrows::draw_arrow(
                    &painter,
                    center(from),
                    center(to),
                    sq_size,
                    mark_color.color(150),
                );
            }
        } else if response.secondary_clicked()
            && let Some(sq) = pointer_sq
        {
            self.interaction
                .annotations
                .toggle(sq, sq, mark_color, board);
        }

        let primary = egui::PointerButton::Primary;
        if response.clicked() || response.drag_started_by(primary) {
            self.interaction.annotations.clear();
        }

        // Draw dragged piece at cursor
        if let (Some(from_sq), Some(cursor_pos)) =
            (self.interaction.drag_from, self.interaction.drag_pos)
            && let Some(piece) = board.piece_at(from_sq)
        {
            let drag_rect = Rect::from_center_size(cursor_pos, Vec2::splat(sq_size * 1.12));
            let shadow_center = cursor_pos + Vec2::new(0.0, sq_size * 0.08);
            painter.circle_filled(shadow_center, sq_size * 0.38, Color32::from_black_alpha(45));
            draw_piece(&painter, drag_rect, self.piece_textures, piece);
        }

        // Moves may be made from any position of the game: playing one in
        // the middle starts a variation. The caller decides when that is
        // not wanted (for example while playing the engine).
        let can_interact = self.interactive
            && !self.game.is_game_over()
            && self.interaction.pending_promotion.is_none();

        let mut pending_sound: Option<SoundEvent> = None;

        if can_interact {
            // Update drag position
            if response.dragged_by(primary)
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

            if response.drag_stopped_by(primary) {
                if let (Some(from), Some(ptr)) =
                    (self.interaction.drag_from, self.interaction.drag_pos)
                    && let Some(to) = pos_to_sq(ptr, rect, sq_size, self.flipped)
                {
                    if is_promotion_move(self.game, from, to) {
                        self.interaction.pending_promotion = Some((from, to));
                    } else {
                        pending_sound = try_make_move(self.game, from, to);
                        self.interaction.animator.skip_next = pending_sound.is_some();
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
                pending_sound = handle_click(
                    self.game,
                    &mut self.interaction.selected,
                    sq,
                    &mut self.interaction.pending_promotion,
                );
            }
        }

        pending_sound
    }
}

fn display_coords(sq: Square, flipped: bool) -> (u32, u32) {
    let (file_idx, rank_idx) = (sq.file().to_u32(), sq.rank().to_u32());
    if flipped {
        (7 - file_idx, rank_idx)
    } else {
        (file_idx, 7 - rank_idx)
    }
}

fn sq_to_rect(sq: Square, board_rect: Rect, sq_size: f32, flipped: bool) -> Rect {
    let (display_file, display_rank) = display_coords(sq, flipped);
    let x = board_rect.min.x + display_file as f32 * sq_size;
    let y = board_rect.min.y + display_rank as f32 * sq_size;
    Rect::from_min_size(Pos2::new(x, y), Vec2::splat(sq_size))
}

/// Only the four outer corners of the board are rounded.
fn square_corners(sq: Square, flipped: bool) -> CornerRadius {
    let (file, rank) = display_coords(sq, flipped);
    let r = BOARD_CORNER_RADIUS;
    CornerRadius {
        nw: if file == 0 && rank == 0 { r } else { 0 },
        ne: if file == 7 && rank == 0 { r } else { 0 },
        sw: if file == 0 && rank == 7 { r } else { 0 },
        se: if file == 7 && rank == 7 { r } else { 0 },
    }
}

fn paint_board_shadow(painter: &Painter, rect: Rect) {
    let shadow = Shadow {
        offset: [0, 3],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(70),
    };
    painter.add(shadow.as_shape(rect, BOARD_CORNER_RADIUS));
}

fn paint_check_glow(painter: &Painter, sq_rect: Rect) {
    const RINGS: u32 = 9;
    let radius = sq_rect.width() * 0.62;
    for i in 0..RINGS {
        let r = radius * (1.0 - i as f32 / RINGS as f32);
        painter.circle_filled(sq_rect.center(), r, HIGHLIGHT_CHECK);
    }
}

pub struct SetupBoardWidget<'a> {
    board: &'a mut Board,
    flipped: bool,
    style: BoardStyle,
    palette: Option<Piece>,
    piece_textures: &'a PieceTextures,
}

impl<'a> SetupBoardWidget<'a> {
    pub fn new(
        board: &'a mut Board,
        flipped: bool,
        style: BoardStyle,
        palette: Option<Piece>,
        piece_textures: &'a PieceTextures,
    ) -> Self {
        Self {
            board,
            flipped,
            style,
            palette,
            piece_textures,
        }
    }

    pub fn show(self, ui: &mut Ui) {
        let available = ui.available_size();
        let board_size = available.x.min(available.y);
        let sq_size = board_size / 8.0;

        let (response, painter) =
            ui.allocate_painter(Vec2::splat(board_size), Sense::click_and_drag());
        let rect = response.rect;
        paint_board_shadow(ui.painter(), rect);

        for rank_idx in 0..8u32 {
            for file_idx in 0..8u32 {
                let sq = Square::from_coords(File::new(file_idx), Rank::new(rank_idx));
                let sq_rect = sq_to_rect(sq, rect, sq_size, self.flipped);

                // a1 is dark, so odd coordinate sums are light.
                let is_light_sq = (rank_idx + file_idx) % 2 == 1;
                let sq_color = self.style.theme.square(is_light_sq);

                painter.rect_filled(sq_rect, square_corners(sq, self.flipped), sq_color);

                if let Some(piece) = self.board.piece_at(sq) {
                    draw_piece(&painter, sq_rect, self.piece_textures, piece);
                }
            }
        }

        if self.style.show_coordinates {
            draw_labels(&painter, rect, sq_size, self.flipped, self.style.theme);
        }

        if response.secondary_clicked()
            && let Some(ptr) = response.interact_pointer_pos()
            && let Some(sq) = pos_to_sq(ptr, rect, sq_size, self.flipped)
        {
            self.board.discard_piece_at(sq);
        } else if response.clicked()
            && let Some(ptr) = response.interact_pointer_pos()
            && let Some(sq) = pos_to_sq(ptr, rect, sq_size, self.flipped)
        {
            match self.palette {
                Some(piece) => self.board.set_piece_at(sq, piece),
                None => self.board.discard_piece_at(sq),
            }
        }
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

fn own_piece_at(game: &Game, sq: Square) -> Option<Square> {
    let pos = game.current_position();
    let piece = pos.board().piece_at(sq)?;
    (piece.color == pos.turn()).then_some(sq)
}

fn handle_click(
    game: &mut Game,
    selected: &mut Option<Square>,
    sq: Square,
    pending_promotion: &mut Option<(Square, Square)>,
) -> Option<SoundEvent> {
    let Some(from) = *selected else {
        *selected = own_piece_at(game, sq);
        return None;
    };

    if from == sq {
        *selected = None;
        return None;
    }

    if is_promotion_move(game, from, sq) {
        *pending_promotion = Some((from, sq));
        *selected = None;
        return None;
    }

    let sound = try_make_move(game, from, sq);

    if sound.is_some() {
        *selected = None;
        return sound;
    }

    *selected = own_piece_at(game, sq);
    None
}

/// shakmaty encodes castling as "king takes own rook"; this returns the
/// king's real destination (g1, c1, g8, c8) instead.
fn king_destination(m: &Move) -> Square {
    match (m, m.castling_side()) {
        (Move::Castle { king, .. }, Some(side)) => {
            side.king_to(PieceColor::from_white(king.rank() == Rank::First))
        }
        _ => m.to(),
    }
}

fn lands_on(m: &Move, to: Square) -> bool {
    m.to() == to || king_destination(m) == to
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

pub(crate) fn sound_for_move(m: &Move, resulting_pos: &Chess) -> SoundEvent {
    if resulting_pos.is_checkmate() {
        return SoundEvent::Checkmate;
    }

    if resulting_pos.is_stalemate() || resulting_pos.is_insufficient_material() {
        return SoundEvent::Draw;
    }

    if m.capture().is_some() || m.is_en_passant() {
        return SoundEvent::Capture;
    }

    SoundEvent::Move
}

pub fn try_make_promotion_move(
    game: &mut Game,
    from: Square,
    to: Square,
    role: Role,
) -> Option<SoundEvent> {
    let legals = game.current_position().legal_moves();
    let m = legals.iter().find(|m| {
        m.from() == Some(from)
            && m.to() == to
            && matches!(m, Move::Normal { promotion: Some(r), .. } if *r == role)
    })?;
    let m = *m;
    game.make_move(m).ok()?;
    Some(sound_for_move(&m, game.current_position()))
}

fn try_make_move(game: &mut Game, from: Square, to: Square) -> Option<SoundEvent> {
    let legals = game.current_position().legal_moves();
    let m = legals.iter().find(|m| {
        m.from() == Some(from)
            && lands_on(m, to)
            && !matches!(
                m,
                Move::Normal {
                    promotion: Some(_),
                    ..
                }
            )
    })?;
    let m = *m;
    game.make_move(m).ok()?;
    Some(sound_for_move(&m, game.current_position()))
}

fn draw_piece(painter: &Painter, rect: Rect, textures: &PieceTextures, piece: Piece) {
    let ppp = painter.ctx().pixels_per_point();
    let rect = snap_rect(rect, ppp);
    if let Some(tex) = textures.get(painter.ctx(), piece, rect.width() * ppp) {
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(tex.id(), rect, uv, Color32::WHITE);
    }
}

/// Whole physical pixels in position and size, so textures are not resampled at half-pixel offsets.
fn snap_rect(rect: Rect, pixels_per_point: f32) -> Rect {
    let snap = |v: f32| (v * pixels_per_point).round() / pixels_per_point;
    let size = Vec2::new(snap(rect.width()), snap(rect.height()));
    Rect::from_min_size(Pos2::new(snap(rect.min.x), snap(rect.min.y)), size)
}

fn draw_labels(
    painter: &Painter,
    board_rect: Rect,
    sq_size: f32,
    flipped: bool,
    theme: BoardTheme,
) {
    let font = FontId::proportional(sq_size * 0.2);

    let files = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
    let ranks = ['1', '2', '3', '4', '5', '6', '7', '8'];

    for i in 0..8 {
        // Labels take the opposite square's colour so they stay legible.
        let label_color = theme.square(i % 2 == 0);

        let file_char = if flipped { files[7 - i] } else { files[i] };
        let x = board_rect.min.x + i as f32 * sq_size + sq_size * 0.05;
        let y = board_rect.max.y - sq_size * 0.14;
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            file_char,
            font.clone(),
            label_color,
        );

        let rank_char = if flipped { ranks[i] } else { ranks[7 - i] };
        let x2 = board_rect.max.x - sq_size * 0.1;
        let y2 = board_rect.min.y + i as f32 * sq_size + sq_size * 0.05;
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
    fn setup_board_stamps_and_erases_pieces() {
        let mut board = Board::empty();
        let piece = Piece {
            color: PieceColor::White,
            role: Role::Queen,
        };
        board.set_piece_at(Square::D4, piece);
        assert_eq!(board.piece_at(Square::D4), Some(piece));
        board.discard_piece_at(Square::D4);
        assert_eq!(board.piece_at(Square::D4), None);
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
        assert!(try_make_move(&mut game, Square::E2, Square::E4).is_some());
        assert_eq!(game.cursor, 1);

        let mut game2 = Game::new();
        assert!(try_make_move(&mut game2, Square::E2, Square::E5).is_none());
        assert_eq!(game2.cursor, 0);
    }

    #[test]
    fn try_make_promotion_move_promotes_to_chosen_role() {
        let mut game = Game::from_fen("8/P7/8/8/8/8/8/k6K w - - 0 1").expect("valid fen");
        assert!(try_make_promotion_move(&mut game, Square::A7, Square::A8, Role::Queen).is_some());
        assert_eq!(game.cursor, 1);
        assert_eq!(game.san, vec!["a8=Q+"]);
    }

    fn pos_from_fen(fen: &str) -> Chess {
        use shakmaty::fen::Fen;
        let f: Fen = fen.parse().unwrap();
        f.into_position(shakmaty::CastlingMode::Standard).unwrap()
    }

    #[test]
    fn sound_for_move_regular_move() {
        let pos = Chess::default();
        let m = pos
            .legal_moves()
            .into_iter()
            .find(|m| m.from() == Some(Square::E2) && m.to() == Square::E4)
            .unwrap();
        let mut next = pos.clone();
        next.play_unchecked(m);
        assert!(matches!(sound_for_move(&m, &next), SoundEvent::Move));
    }

    #[test]
    fn sound_for_move_capture() {
        // White pawn on e4 can capture black pawn on d5
        let pos = pos_from_fen("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2");
        let m = pos
            .legal_moves()
            .into_iter()
            .find(|m| m.from() == Some(Square::E4) && m.to() == Square::D5)
            .unwrap();
        let mut next = pos.clone();
        next.play_unchecked(m);
        assert!(matches!(sound_for_move(&m, &next), SoundEvent::Capture));
    }

    #[test]
    fn sound_for_move_check_plays_move_sound() {
        // Check plays Move sound (no dedicated check sound in standard theme)
        let pos = pos_from_fen("4k3/8/8/8/8/8/8/4K2Q w - - 0 1");
        let m = pos
            .legal_moves()
            .into_iter()
            .find(|m| m.from() == Some(Square::H1) && m.to() == Square::H8)
            .unwrap();
        let mut next = pos.clone();
        next.play_unchecked(m);
        assert!(next.is_check(), "expected resulting position to be check");
        assert!(matches!(sound_for_move(&m, &next), SoundEvent::Move));
    }

    #[test]
    fn sound_for_move_checkmate() {
        let pos = pos_from_fen("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2");
        let m = pos
            .legal_moves()
            .into_iter()
            .find(|m| m.from() == Some(Square::D8) && m.to() == Square::H4)
            .unwrap();
        let mut next = pos.clone();
        next.play_unchecked(m);
        assert!(next.is_checkmate(), "expected checkmate");
        assert!(matches!(sound_for_move(&m, &next), SoundEvent::Checkmate));
    }

    #[test]
    fn sound_for_move_stalemate() {
        // Qb6-c7 stalemates the black king on a8 (Ka6 covers a7/b7, Qc7 covers b8)
        let pos = pos_from_fen("k7/8/KQ6/8/8/8/8/8 w - - 0 1");
        let m = pos
            .legal_moves()
            .into_iter()
            .find(|m| m.from() == Some(Square::B6) && m.to() == Square::C7)
            .unwrap();
        let mut next = pos.clone();
        next.play_unchecked(m);
        assert!(next.is_stalemate(), "expected stalemate");
        assert!(matches!(sound_for_move(&m, &next), SoundEvent::Draw));
    }

    #[test]
    fn board_theme_resolves_by_name_case_insensitively() {
        assert_eq!(BoardTheme::resolve("Blue", true).name, "Blue");
        assert_eq!(BoardTheme::resolve("blue", false).name, "Blue");
        assert_eq!(BoardTheme::resolve("GREY", true).name, "Grey");
    }

    #[test]
    fn board_theme_auto_follows_interface_theme() {
        assert_eq!(BoardTheme::resolve("", true).name, "Green");
        assert_eq!(BoardTheme::resolve("", false).name, "Brown");
        assert_eq!(BoardTheme::resolve("no-such-theme", true).name, "Green");
    }

    #[test]
    fn board_themes_have_unique_names_and_distinct_squares() {
        let mut seen = std::collections::HashSet::new();
        for theme in BOARD_THEMES {
            assert!(seen.insert(theme.name), "duplicate theme {}", theme.name);
            assert_ne!(
                theme.light, theme.dark,
                "{} has identical squares",
                theme.name
            );
        }
    }

    #[test]
    fn theme_square_picks_light_or_dark() {
        let t = BOARD_THEMES[0];
        assert_eq!(t.square(true), t.light);
        assert_eq!(t.square(false), t.dark);
    }

    const CASTLE_BOTH: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";

    #[test]
    fn castles_by_moving_king_two_squares() {
        let mut game = Game::from_fen(CASTLE_BOTH).expect("valid fen");
        assert!(try_make_move(&mut game, Square::E1, Square::G1).is_some());
        assert_eq!(game.san, vec!["O-O"]);

        assert!(try_make_move(&mut game, Square::E8, Square::C8).is_some());
        assert_eq!(game.san, vec!["O-O", "O-O-O"]);
    }

    #[test]
    fn castles_by_moving_king_onto_rook() {
        let mut game = Game::from_fen(CASTLE_BOTH).expect("valid fen");
        assert!(try_make_move(&mut game, Square::E1, Square::A1).is_some());
        assert_eq!(game.san, vec!["O-O-O"]);
    }

    #[test]
    fn king_step_next_to_castling_square_is_still_a_king_move() {
        let mut game = Game::from_fen(CASTLE_BOTH).expect("valid fen");
        assert!(try_make_move(&mut game, Square::E1, Square::F1).is_some());
        assert_eq!(game.san, vec!["Kf1"]);
    }

    #[test]
    fn castling_hint_is_on_the_kings_destination() {
        let game = Game::from_fen(CASTLE_BOTH).expect("valid fen");
        let dests: Vec<Square> = game
            .current_position()
            .legal_moves()
            .iter()
            .filter(|m| m.from() == Some(Square::E1))
            .map(king_destination)
            .collect();
        assert!(dests.contains(&Square::G1));
        assert!(dests.contains(&Square::C1));
        assert!(!dests.contains(&Square::H1));
        assert!(!dests.contains(&Square::A1));
    }

    #[test]
    fn no_castling_through_the_click_when_it_is_illegal() {
        let mut game = Game::from_fen("4kr2/8/8/8/8/8/8/R3K2R w KQ - 0 1").expect("valid fen");
        assert!(try_make_move(&mut game, Square::E1, Square::G1).is_none());
        assert_eq!(game.cursor, 0);
    }

    #[test]
    fn texture_size_rounds_up_to_multiples_of_16() {
        assert_eq!(texture_size(1.0), 16);
        assert_eq!(texture_size(16.0), 16);
        assert_eq!(texture_size(16.1), 32);
        assert_eq!(texture_size(87.0), 96);
        assert_eq!(texture_size(174.0), 176);
    }

    #[test]
    fn texture_size_is_bounded() {
        assert_eq!(texture_size(0.0), 16);
        assert_eq!(texture_size(-5.0), 16);
        assert_eq!(texture_size(100_000.0), 1024);
    }

    #[test]
    fn snap_rect_lands_on_whole_pixels() {
        let r = snap_rect(
            Rect::from_min_size(Pos2::new(10.3, 20.6), Vec2::splat(87.4)),
            1.0,
        );
        assert_eq!(r.min, Pos2::new(10.0, 21.0));
        assert_eq!(r.size(), Vec2::splat(87.0));

        let r = snap_rect(
            Rect::from_min_size(Pos2::new(10.3, 20.6), Vec2::splat(87.4)),
            2.0,
        );
        assert_eq!(r.min, Pos2::new(10.5, 20.5));
        assert_eq!(r.size(), Vec2::splat(87.5));
    }

    #[test]
    fn only_outer_corner_squares_are_rounded() {
        let r = BOARD_CORNER_RADIUS;
        let a8 = square_corners(Square::A8, false);
        assert_eq!((a8.nw, a8.ne, a8.sw, a8.se), (r, 0, 0, 0));
        let h1 = square_corners(Square::H1, false);
        assert_eq!((h1.nw, h1.ne, h1.sw, h1.se), (0, 0, 0, r));
        assert_eq!(square_corners(Square::D4, false), CornerRadius::ZERO);
    }

    #[test]
    fn rounded_corners_follow_the_board_orientation() {
        let r = BOARD_CORNER_RADIUS;
        let flipped = square_corners(Square::H1, true);
        assert_eq!((flipped.nw, flipped.se), (r, 0));
        let a8 = square_corners(Square::A8, true);
        assert_eq!((a8.nw, a8.ne, a8.sw, a8.se), (0, 0, 0, r));
    }

    #[test]
    fn display_coords_mirror_when_flipped() {
        assert_eq!(display_coords(Square::A1, false), (0, 7));
        assert_eq!(display_coords(Square::A1, true), (7, 0));
        assert_eq!(display_coords(Square::E4, false), (4, 4));
    }
}
