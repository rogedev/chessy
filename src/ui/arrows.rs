//! Arrows and square highlights drawn on top of the board: the user's own
//! right-click annotations and the engine's suggested move.

use egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2};
use shakmaty::{Board, Square};

pub const USER_COLOR: Color32 = Color32::from_rgba_premultiplied(18, 110, 24, 200);
pub const ENGINE_COLOR: Color32 = Color32::from_rgba_premultiplied(20, 80, 170, 170);

/// Annotations the user has drawn on the current position.
#[derive(Default)]
pub struct Annotations {
    pub arrows: Vec<(Square, Square)>,
    pub squares: Vec<Square>,
    /// Square where a right-button drag began, while it is in progress.
    pub drag_start: Option<Square>,
    /// The board the annotations were drawn on. They are dropped as soon as
    /// a different position is shown.
    board: Option<Board>,
}

impl Annotations {
    pub fn is_empty(&self) -> bool {
        self.arrows.is_empty() && self.squares.is_empty()
    }

    pub fn clear(&mut self) {
        self.arrows.clear();
        self.squares.clear();
        self.drag_start = None;
        self.board = None;
    }

    /// Forget everything if the position on the board has changed.
    pub fn sync(&mut self, board: &Board) {
        if self.board.as_ref().is_some_and(|b| b != board) {
            self.clear();
        }
    }

    /// Finish a right-button gesture from `from` to `to`: the same square
    /// toggles a highlight, different squares toggle an arrow.
    pub fn toggle(&mut self, from: Square, to: Square, board: &Board) {
        if from == to {
            toggle_item(&mut self.squares, from);
        } else {
            toggle_item(&mut self.arrows, (from, to));
        }
        self.board = (!self.is_empty()).then(|| board.clone());
    }
}

fn toggle_item<T: PartialEq>(items: &mut Vec<T>, item: T) {
    match items.iter().position(|i| *i == item) {
        Some(idx) => {
            items.remove(idx);
        }
        None => items.push(item),
    }
}

/// From and to squares of a UCI move such as `e2e4` or `e7e8q`.
pub fn uci_squares(uci: &str) -> Option<(Square, Square)> {
    let from = uci.get(0..2)?.parse().ok()?;
    let to = uci.get(2..4)?.parse().ok()?;
    (from != to).then_some((from, to))
}

/// The pieces of an arrow: a shaft and a triangular head.
#[derive(Debug, PartialEq)]
pub struct ArrowShape {
    pub shaft: [Pos2; 2],
    pub head: [Pos2; 3],
    pub width: f32,
}

/// Geometry for an arrow between two square centres. The shaft starts a
/// little way out from the origin square's centre and stops at the base of
/// the head, so translucent parts never overlap.
pub fn arrow_shape(from: Pos2, to: Pos2, sq_size: f32) -> Option<ArrowShape> {
    let delta = to - from;
    let len = delta.length();
    if len < f32::EPSILON {
        return None;
    }
    let dir = delta / len;
    let normal = Vec2::new(-dir.y, dir.x);

    let width = sq_size * 0.15;
    let head_len = (sq_size * 0.42).min(len * 0.6);
    let head_half = sq_size * 0.22;
    let start_gap = (sq_size * 0.3).min(len * 0.25);

    let base = to - dir * head_len;
    Some(ArrowShape {
        shaft: [from + dir * start_gap, base],
        head: [to, base + normal * head_half, base - normal * head_half],
        width,
    })
}

pub fn draw_arrow(painter: &Painter, from: Pos2, to: Pos2, sq_size: f32, color: Color32) {
    let Some(shape) = arrow_shape(from, to, sq_size) else {
        return;
    };
    painter.line_segment(shape.shaft, Stroke::new(shape.width, color));
    painter.add(Shape::convex_polygon(
        shape.head.to_vec(),
        color,
        Stroke::NONE,
    ));
}

pub fn draw_square_mark(painter: &Painter, center: Pos2, sq_size: f32, color: Color32) {
    painter.circle_stroke(center, sq_size * 0.44, Stroke::new(sq_size * 0.07, color));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uci_squares_parses_plain_and_promotion_moves() {
        assert_eq!(uci_squares("e2e4"), Some((Square::E2, Square::E4)));
        assert_eq!(uci_squares("e7e8q"), Some((Square::E7, Square::E8)));
        assert_eq!(uci_squares("e1g1"), Some((Square::E1, Square::G1)));
    }

    #[test]
    fn uci_squares_rejects_garbage() {
        assert_eq!(uci_squares(""), None);
        assert_eq!(uci_squares("e2"), None);
        assert_eq!(uci_squares("z9e4"), None);
        assert_eq!(uci_squares("0000"), None);
        assert_eq!(uci_squares("e2e2"), None);
    }

    #[test]
    fn toggle_adds_and_removes_arrows_and_squares() {
        let board = Board::new();
        let mut a = Annotations::default();

        a.toggle(Square::E2, Square::E4, &board);
        a.toggle(Square::D4, Square::D4, &board);
        assert_eq!(a.arrows, vec![(Square::E2, Square::E4)]);
        assert_eq!(a.squares, vec![Square::D4]);

        // The reverse arrow is a different arrow.
        a.toggle(Square::E4, Square::E2, &board);
        assert_eq!(a.arrows.len(), 2);

        // Repeating a gesture removes it again.
        a.toggle(Square::E2, Square::E4, &board);
        a.toggle(Square::E4, Square::E2, &board);
        a.toggle(Square::D4, Square::D4, &board);
        assert!(a.is_empty());
    }

    #[test]
    fn sync_clears_only_when_the_position_changes() {
        let start = Board::new();
        let mut a = Annotations::default();
        a.toggle(Square::E2, Square::E4, &start);

        a.sync(&start);
        assert!(!a.is_empty());

        a.sync(&Board::empty());
        assert!(a.is_empty());
    }

    #[test]
    fn arrow_shape_points_at_the_target() {
        let from = Pos2::new(50.0, 50.0);
        let to = Pos2::new(50.0, 250.0);
        let shape = arrow_shape(from, to, 100.0).expect("non-degenerate");

        // Tip is exactly on the target centre.
        assert_eq!(shape.head[0], to);
        // The shaft runs straight down and ends at the head's base.
        assert_eq!(shape.shaft[0].x, 50.0);
        assert!(shape.shaft[0].y > from.y && shape.shaft[0].y < shape.shaft[1].y);
        assert!((shape.shaft[1].y - 208.0).abs() < 0.01);
        // Head base is symmetric about the shaft.
        assert!((shape.head[1].x - 50.0 + (shape.head[2].x - 50.0)).abs() < 0.01);
        assert_eq!(shape.head[1].y, shape.head[2].y);
    }

    #[test]
    fn arrow_shape_is_none_for_zero_length() {
        let p = Pos2::new(10.0, 10.0);
        assert!(arrow_shape(p, p, 100.0).is_none());
    }
}
