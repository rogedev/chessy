//! Captured pieces and material balance, derived by comparing the current
//! board with the position the game started from.

use shakmaty::{Board, Color, Role};

/// Display order: most valuable first.
const ROLES: [Role; 5] = [
    Role::Queen,
    Role::Rook,
    Role::Bishop,
    Role::Knight,
    Role::Pawn,
];

pub fn role_value(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight | Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => 0,
    }
}

fn count(board: &Board, color: Color, role: Role) -> i32 {
    (board.by_color(color) & board.by_role(role)).count() as i32
}

/// Pieces of `color` that have been captured since `start`, most valuable
/// first. A pawn that promoted is not reported as captured: each piece the
/// side has beyond its starting count cancels one missing pawn.
pub fn captured_pieces(start: &Board, current: &Board, color: Color) -> Vec<Role> {
    let missing = |role| count(start, color, role) - count(current, color, role);

    let promotions: i32 = ROLES
        .iter()
        .filter(|&&r| r != Role::Pawn)
        .map(|&r| (-missing(r)).max(0))
        .sum();

    let mut out = vec![];
    for role in ROLES {
        let mut n = missing(role).max(0);
        if role == Role::Pawn {
            n = (n - promotions).max(0);
        }
        out.extend(std::iter::repeat_n(role, n as usize));
    }
    out
}

/// Material on the board, White minus Black, in pawns.
pub fn material_balance(board: &Board) -> i32 {
    ROLES
        .iter()
        .map(|&r| role_value(r) * (count(board, Color::White, r) - count(board, Color::Black, r)))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chess::Game;
    use shakmaty::Position;

    fn boards(g: &Game) -> (&Board, &Board) {
        (g.start_position().board(), g.current_position().board())
    }

    fn play(moves: &[&str]) -> Game {
        let mut g = Game::new();
        for m in moves {
            g.make_uci_move(m).expect("legal move");
        }
        g
    }

    #[test]
    fn nothing_captured_at_start() {
        let g = Game::new();
        let (start, cur) = boards(&g);
        assert!(captured_pieces(start, cur, Color::White).is_empty());
        assert!(captured_pieces(start, cur, Color::Black).is_empty());
        assert_eq!(material_balance(cur), 0);
    }

    #[test]
    fn captures_are_listed_most_valuable_first() {
        // 1. e4 d5 2. exd5 Qxd5 3. Nc3 Qxg2 4. Bxg2
        let g = play(&["e2e4", "d7d5", "e4d5", "d8d5", "b1c3", "d5g2", "f1g2"]);
        let (start, cur) = boards(&g);
        assert_eq!(
            captured_pieces(start, cur, Color::Black),
            vec![Role::Queen, Role::Pawn]
        );
        assert_eq!(
            captured_pieces(start, cur, Color::White),
            vec![Role::Pawn, Role::Pawn]
        );
        // White lost two pawns (2), Black lost queen and pawn (10).
        assert_eq!(material_balance(cur), 8);
    }

    #[test]
    fn promoted_pawn_is_not_counted_as_captured() {
        let mut g = Game::from_fen("7k/P7/8/8/8/8/8/K7 w - - 0 1").expect("valid fen");
        g.make_uci_move("a7a8q").expect("legal");
        let (start, cur) = boards(&g);
        assert!(captured_pieces(start, cur, Color::White).is_empty());
        assert_eq!(material_balance(cur), 9);
    }

    #[test]
    fn promotion_and_real_pawn_loss_are_told_apart() {
        // White starts with two pawns; one promotes, the other is captured.
        let mut g = Game::from_fen("6k1/P7/8/8/8/8/7P/K7 w - - 0 1").expect("valid fen");
        for m in ["a7a8q", "g8g7", "h2h4", "g7g6", "h4h5", "g6h5"] {
            g.make_uci_move(m).expect("legal");
        }
        let (start, cur) = boards(&g);
        assert_eq!(captured_pieces(start, cur, Color::White), vec![Role::Pawn]);
    }

    #[test]
    fn custom_start_only_counts_pieces_lost_since_then() {
        let mut g = Game::from_fen("4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1").expect("valid fen");
        g.make_uci_move("e4d5").expect("legal");
        let (start, cur) = boards(&g);
        assert_eq!(captured_pieces(start, cur, Color::Black), vec![Role::Queen]);
        assert!(captured_pieces(start, cur, Color::White).is_empty());
        assert_eq!(material_balance(cur), 1);
    }
}
