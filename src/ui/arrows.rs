use egui::{Color32, Modifiers, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use shakmaty::{Board, Square};

pub const ENGINE_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(20, 90, 190, 215);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkColor {
    Red,
    Green,
    Blue,
    Yellow,
}

impl MarkColor {
    pub fn from_modifiers(m: &Modifiers) -> Self {
        if m.shift {
            return Self::Green;
        }
        if m.command || m.ctrl {
            return Self::Blue;
        }
        if m.alt {
            return Self::Yellow;
        }
        Self::Red
    }

    pub fn color(self, alpha: u8) -> Color32 {
        let (r, g, b) = match self {
            Self::Red => (235, 80, 70),
            Self::Green => (70, 165, 55),
            Self::Blue => (45, 115, 215),
            Self::Yellow => (235, 185, 35),
        };
        Color32::from_rgba_unmultiplied(r, g, b, alpha)
    }
}

#[derive(Default)]
pub struct Annotations {
    pub arrows: Vec<((Square, Square), MarkColor)>,
    pub squares: Vec<(Square, MarkColor)>,
    pub drag_start: Option<Square>,
    /// Annotations are dropped as soon as a different position is shown.
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

    pub fn sync(&mut self, board: &Board) {
        if self.board.as_ref().is_some_and(|b| b != board) {
            self.clear();
        }
    }

    pub fn square_color(&self, sq: Square) -> Option<MarkColor> {
        self.squares.iter().find(|(s, _)| *s == sq).map(|(_, c)| *c)
    }

    /// The same square marks it, different squares draw an arrow. Repeating
    /// a mark in the same colour removes it, another colour recolours it.
    pub fn toggle(&mut self, from: Square, to: Square, color: MarkColor, board: &Board) {
        if from == to {
            toggle_item(&mut self.squares, from, color);
        } else {
            toggle_item(&mut self.arrows, (from, to), color);
        }
        self.board = (!self.is_empty()).then(|| board.clone());
    }
}

fn toggle_item<K: PartialEq>(items: &mut Vec<(K, MarkColor)>, key: K, color: MarkColor) {
    match items.iter().position(|(k, _)| *k == key) {
        Some(idx) if items[idx].1 == color => {
            items.remove(idx);
        }
        Some(idx) => items[idx].1 = color,
        None => items.push((key, color)),
    }
}

pub fn uci_squares(uci: &str) -> Option<(Square, Square)> {
    let from = uci.get(0..2)?.parse().ok()?;
    let to = uci.get(2..4)?.parse().ok()?;
    (from != to).then_some((from, to))
}

#[derive(Debug, PartialEq)]
pub struct ArrowShape {
    /// Rectangles that never overlap, so a translucent colour stays even.
    pub shaft: Vec<[Pos2; 4]>,
    pub head: [Pos2; 3],
}

/// A knight move is drawn as an L: the long leg first, then the short one.
pub fn is_knight_delta(delta: Vec2, sq_size: f32) -> bool {
    let (dx, dy) = (
        (delta.x / sq_size).abs().round(),
        (delta.y / sq_size).abs().round(),
    );
    (dx == 1.0 && dy == 2.0) || (dx == 2.0 && dy == 1.0)
}

fn rect_along(a: Pos2, b: Pos2, width: f32) -> [Pos2; 4] {
    let dir = (b - a).normalized();
    let n = Vec2::new(-dir.y, dir.x) * (width / 2.0);
    [a + n, b + n, b - n, a - n]
}

/// The shaft stops at the base of the head so translucent parts never overlap.
pub fn arrow_shape(from: Pos2, to: Pos2, sq_size: f32) -> Option<ArrowShape> {
    let delta = to - from;
    let len = delta.length();
    if len < f32::EPSILON {
        return None;
    }

    let width = sq_size * 0.15;
    let head_len = (sq_size * 0.42).min(len * 0.6);
    let head_half = sq_size * 0.22;
    let start_gap = (sq_size * 0.3).min(len * 0.25);

    let (shaft, last_dir) = if is_knight_delta(delta, sq_size) {
        let corner = if delta.x.abs() > delta.y.abs() {
            Pos2::new(to.x, from.y)
        } else {
            Pos2::new(from.x, to.y)
        };
        let first = (corner - from).normalized();
        let second = (to - corner).normalized();
        let base = to - second * head_len;
        let shaft = vec![
            rect_along(
                from + first * start_gap,
                corner + first * (width / 2.0),
                width,
            ),
            rect_along(corner + second * (width / 2.0), base, width),
        ];
        (shaft, second)
    } else {
        let dir = delta / len;
        let base = to - dir * head_len;
        (vec![rect_along(from + dir * start_gap, base, width)], dir)
    };

    let normal = Vec2::new(-last_dir.y, last_dir.x);
    let base = to - last_dir * head_len;
    Some(ArrowShape {
        shaft,
        head: [to, base + normal * head_half, base - normal * head_half],
    })
}

pub fn draw_arrow(painter: &Painter, from: Pos2, to: Pos2, sq_size: f32, color: Color32) {
    let Some(shape) = arrow_shape(from, to, sq_size) else {
        return;
    };
    for rect in shape.shaft {
        painter.add(Shape::convex_polygon(rect.to_vec(), color, Stroke::NONE));
    }
    painter.add(Shape::convex_polygon(
        shape.head.to_vec(),
        color,
        Stroke::NONE,
    ));
}

pub fn draw_square_mark(painter: &Painter, rect: Rect, color: MarkColor) {
    painter.rect_filled(rect, 0.0, color.color(135));
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
    fn toggle_adds_removes_and_recolours() {
        let board = Board::new();
        let mut a = Annotations::default();
        let red = MarkColor::Red;

        a.toggle(Square::E2, Square::E4, red, &board);
        a.toggle(Square::D4, Square::D4, red, &board);
        assert_eq!(a.arrows, vec![((Square::E2, Square::E4), red)]);
        assert_eq!(a.squares, vec![(Square::D4, red)]);

        a.toggle(Square::E4, Square::E2, red, &board);
        assert_eq!(a.arrows.len(), 2);

        a.toggle(Square::E2, Square::E4, MarkColor::Green, &board);
        assert_eq!(a.arrows[0].1, MarkColor::Green);

        a.toggle(Square::E2, Square::E4, MarkColor::Green, &board);
        a.toggle(Square::E4, Square::E2, red, &board);
        a.toggle(Square::D4, Square::D4, red, &board);
        assert!(a.is_empty());
    }

    #[test]
    fn sync_clears_only_when_the_position_changes() {
        let start = Board::new();
        let mut a = Annotations::default();
        a.toggle(Square::E2, Square::E4, MarkColor::Red, &start);

        a.sync(&start);
        assert!(!a.is_empty());

        a.sync(&Board::empty());
        assert!(a.is_empty());
    }

    #[test]
    fn modifiers_pick_the_colour() {
        assert_eq!(MarkColor::from_modifiers(&Modifiers::NONE), MarkColor::Red);
        assert_eq!(
            MarkColor::from_modifiers(&Modifiers::SHIFT),
            MarkColor::Green
        );
        assert_eq!(MarkColor::from_modifiers(&Modifiers::CTRL), MarkColor::Blue);
        assert_eq!(
            MarkColor::from_modifiers(&Modifiers::ALT),
            MarkColor::Yellow
        );
    }

    #[test]
    fn knight_deltas_are_detected_in_every_direction() {
        for (dx, dy) in [
            (1, 2),
            (2, 1),
            (-1, 2),
            (-2, 1),
            (1, -2),
            (2, -1),
            (-1, -2),
            (-2, -1),
        ] {
            assert!(is_knight_delta(
                Vec2::new(dx as f32 * 50.0, dy as f32 * 50.0),
                50.0
            ));
        }
        for (dx, dy) in [(0, 3), (1, 1), (2, 2), (1, 3), (0, 1)] {
            assert!(!is_knight_delta(
                Vec2::new(dx as f32 * 50.0, dy as f32 * 50.0),
                50.0
            ));
        }
    }

    #[test]
    fn straight_arrow_points_at_the_target() {
        let from = Pos2::new(50.0, 50.0);
        let to = Pos2::new(50.0, 250.0);
        let shape = arrow_shape(from, to, 100.0).expect("non-degenerate");

        assert_eq!(shape.head[0], to);
        assert_eq!(shape.shaft.len(), 1);
        let base_y = shape.head[1].y;
        assert_eq!(base_y, shape.head[2].y);
        assert!((base_y - 208.0).abs() < 0.01);
        assert!(
            shape.shaft[0]
                .iter()
                .all(|p| p.y > from.y && p.y <= base_y + 0.01)
        );
    }

    #[test]
    fn knight_arrow_bends_with_the_long_leg_first() {
        let from = Pos2::new(50.0, 50.0);
        let to = Pos2::new(150.0, 250.0);
        let shape = arrow_shape(from, to, 100.0).expect("non-degenerate");

        assert_eq!(shape.shaft.len(), 2);
        // The first leg runs down the file, the head points sideways at the target.
        assert!(
            shape.shaft[0]
                .iter()
                .all(|p| (p.x - 50.0).abs() <= 7.5 + 0.01)
        );
        assert_eq!(shape.head[0], to);
        assert_eq!(shape.head[1].x, shape.head[2].x);
    }

    #[test]
    fn arrow_shape_is_none_for_zero_length() {
        let p = Pos2::new(10.0, 10.0);
        assert!(arrow_shape(p, p, 100.0).is_none());
    }
}
