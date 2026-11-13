//! Game review: turn one engine evaluation per position into a verdict on
//! every move, an accuracy figure per side and data for the eval graph.
//!
//! The scale follows the public Lichess formulas: centipawns are mapped to
//! a winning percentage, and a move is judged by how much winning
//! percentage it gave away.

use shakmaty::{Color, EnPassantMode, Position, fen::Fen};

use super::Game;
use crate::engine::Score;

/// Chance of winning, 0 to 100, for the side the score is given for.
pub fn win_percent(score: &Score) -> f32 {
    match score {
        Score::Mate(n) if *n > 0 => 100.0,
        Score::Mate(_) => 0.0,
        Score::Cp(cp) => {
            let cp = (*cp as f32).clamp(-1000.0, 1000.0);
            50.0 + 50.0 * (2.0 / (1.0 + (-0.003_682_08 * cp).exp()) - 1.0)
        }
    }
}

/// Accuracy of one move, 0 to 100, from the mover's winning percentage
/// before and after it.
pub fn move_accuracy(before: f32, after: f32) -> f32 {
    if after >= before {
        return 100.0;
    }
    let raw = 103.1668 * (-0.04354 * (before - after)).exp() - 3.1669;
    raw.clamp(0.0, 100.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveClass {
    /// The engine's first choice.
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
}

impl MoveClass {
    /// Classify by winning percentage given away.
    pub fn from_loss(loss: f32) -> Self {
        if loss >= 15.0 {
            MoveClass::Blunder
        } else if loss >= 10.0 {
            MoveClass::Mistake
        } else if loss >= 5.0 {
            MoveClass::Inaccuracy
        } else {
            MoveClass::Good
        }
    }

    /// Conventional annotation glyph, empty for moves without one.
    pub fn symbol(self) -> &'static str {
        match self {
            MoveClass::Inaccuracy => "?!",
            MoveClass::Mistake => "?",
            MoveClass::Blunder => "??",
            MoveClass::Best | MoveClass::Good => "",
        }
    }
}

/// The engine's view of one position.
#[derive(Clone, Debug)]
pub struct PositionEval {
    /// White's chance of winning, 0 to 100.
    pub white_win: f32,
    /// Best move in UCI form; `None` when the game is over here.
    pub best_move: Option<String>,
}

impl PositionEval {
    /// From an engine score, which is given for the side to move.
    pub fn from_engine(score: &Score, turn: Color, best_move: String) -> Self {
        let mover_win = win_percent(score);
        Self {
            white_win: match turn {
                Color::White => mover_win,
                Color::Black => 100.0 - mover_win,
            },
            best_move: Some(best_move),
        }
    }

    /// Positions the engine cannot search because the game has ended.
    pub fn terminal(pos: &shakmaty::Chess) -> Option<Self> {
        let white_win = if pos.is_checkmate() {
            match pos.turn() {
                Color::White => 0.0,
                Color::Black => 100.0,
            }
        } else if pos.is_stalemate() || pos.is_insufficient_material() {
            50.0
        } else {
            return None;
        };
        Some(Self {
            white_win,
            best_move: None,
        })
    }

    fn win_for(&self, color: Color) -> f32 {
        match color {
            Color::White => self.white_win,
            Color::Black => 100.0 - self.white_win,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MoveReview {
    pub mover: Color,
    pub class: MoveClass,
    pub accuracy: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SideSummary {
    pub moves: usize,
    pub accuracy: f32,
    pub inaccuracies: usize,
    pub mistakes: usize,
    pub blunders: usize,
}

/// A finished review of one game.
#[derive(Clone, Debug)]
pub struct Review {
    /// One evaluation per position (moves + 1).
    pub evals: Vec<PositionEval>,
    /// One verdict per move.
    pub moves: Vec<MoveReview>,
    /// Identity of the reviewed game, to notice when it has changed.
    start_fen: String,
    san: Vec<String>,
}

impl Review {
    /// Judge every move of `game` given an evaluation of each position.
    /// Returns `None` unless there is exactly one evaluation per position.
    pub fn build(game: &Game, evals: Vec<PositionEval>) -> Option<Self> {
        if evals.len() != game.positions.len() {
            return None;
        }

        let moves = game
            .moves
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let pos = &game.positions[i];
                let mover = pos.turn();
                let before = evals[i].win_for(mover);
                let after = evals[i + 1].win_for(mover);

                let played = m.to_uci(pos.castles().mode()).to_string();
                let class = if evals[i].best_move.as_deref() == Some(played.as_str()) {
                    MoveClass::Best
                } else {
                    MoveClass::from_loss(before - after)
                };

                MoveReview {
                    mover,
                    class,
                    accuracy: move_accuracy(before, after),
                }
            })
            .collect();

        Some(Self {
            evals,
            moves,
            start_fen: start_fen(game),
            san: game.san.clone(),
        })
    }

    /// Whether this review still describes `game`.
    pub fn matches(&self, game: &Game) -> bool {
        self.san == game.san && self.start_fen == start_fen(game)
    }

    pub fn summary(&self, color: Color) -> SideSummary {
        let mine: Vec<&MoveReview> = self.moves.iter().filter(|m| m.mover == color).collect();
        let count = |class| mine.iter().filter(|m| m.class == class).count();
        SideSummary {
            moves: mine.len(),
            accuracy: if mine.is_empty() {
                0.0
            } else {
                mine.iter().map(|m| m.accuracy).sum::<f32>() / mine.len() as f32
            },
            inaccuracies: count(MoveClass::Inaccuracy),
            mistakes: count(MoveClass::Mistake),
            blunders: count(MoveClass::Blunder),
        }
    }

    pub fn classes(&self) -> Vec<MoveClass> {
        self.moves.iter().map(|m| m.class).collect()
    }

    pub fn white_win(&self) -> Vec<f32> {
        self.evals.iter().map(|e| e.white_win).collect()
    }
}

pub fn start_fen(game: &Game) -> String {
    Fen::from_position(game.start_position(), EnPassantMode::Legal).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(moves: &[&str]) -> Game {
        let mut g = Game::new();
        for m in moves {
            g.make_uci_move(m).expect("legal move");
        }
        g
    }

    fn eval(white_win: f32, best: &str) -> PositionEval {
        PositionEval {
            white_win,
            best_move: Some(best.to_string()),
        }
    }

    #[test]
    fn win_percent_is_symmetric_and_bounded() {
        assert!((win_percent(&Score::Cp(0)) - 50.0).abs() < 0.001);
        let up = win_percent(&Score::Cp(100));
        let down = win_percent(&Score::Cp(-100));
        assert!(up > 55.0 && up < 65.0, "got {up}");
        assert!((up + down - 100.0).abs() < 0.001);
        assert_eq!(win_percent(&Score::Mate(3)), 100.0);
        assert_eq!(win_percent(&Score::Mate(-1)), 0.0);
        // Huge scores are clamped rather than overflowing.
        assert!(win_percent(&Score::Cp(50_000)) < 100.0);
        assert!(win_percent(&Score::Cp(50_000)) > 95.0);
    }

    #[test]
    fn move_accuracy_falls_with_loss() {
        assert_eq!(move_accuracy(50.0, 50.0), 100.0);
        assert_eq!(move_accuracy(50.0, 60.0), 100.0);
        let small = move_accuracy(50.0, 47.0);
        let big = move_accuracy(50.0, 20.0);
        assert!(small > 80.0 && small < 100.0, "got {small}");
        assert!(big < 40.0, "got {big}");
        assert_eq!(move_accuracy(100.0, 0.0), 0.0);
    }

    #[test]
    fn classes_follow_loss_thresholds() {
        assert_eq!(MoveClass::from_loss(-3.0), MoveClass::Good);
        assert_eq!(MoveClass::from_loss(4.9), MoveClass::Good);
        assert_eq!(MoveClass::from_loss(5.0), MoveClass::Inaccuracy);
        assert_eq!(MoveClass::from_loss(10.0), MoveClass::Mistake);
        assert_eq!(MoveClass::from_loss(15.0), MoveClass::Blunder);
        assert_eq!(MoveClass::Blunder.symbol(), "??");
        assert_eq!(MoveClass::Best.symbol(), "");
    }

    #[test]
    fn engine_score_is_converted_to_whites_point_of_view() {
        let w = PositionEval::from_engine(&Score::Cp(200), Color::White, "e2e4".into());
        let b = PositionEval::from_engine(&Score::Cp(200), Color::Black, "e7e5".into());
        assert!(w.white_win > 50.0);
        assert!((w.white_win + b.white_win - 100.0).abs() < 0.001);
    }

    #[test]
    fn terminal_positions_need_no_engine() {
        let mate = play(&["f2f3", "e7e5", "g2g4", "d8h4"]);
        let e = PositionEval::terminal(mate.current_position()).expect("checkmate");
        assert_eq!(e.white_win, 0.0);
        assert!(e.best_move.is_none());

        let stalemate = Game::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        let e = PositionEval::terminal(stalemate.current_position()).expect("stalemate");
        assert_eq!(e.white_win, 50.0);

        assert!(PositionEval::terminal(Game::new().current_position()).is_none());
    }

    #[test]
    fn build_classifies_each_move_from_the_movers_side() {
        // 1. e4 (best) e5 (fine) 2. Ke2 (White throws away 25 points)
        let g = play(&["e2e4", "e7e5", "e1e2"]);
        let evals = vec![
            eval(55.0, "e2e4"),
            eval(55.0, "c7c5"),
            eval(56.0, "g1f3"),
            eval(31.0, "d8h4"),
        ];
        let r = Review::build(&g, evals).expect("one eval per position");

        assert_eq!(
            r.classes(),
            vec![MoveClass::Best, MoveClass::Good, MoveClass::Blunder]
        );
        assert_eq!(r.moves[0].mover, Color::White);
        assert_eq!(r.moves[1].mover, Color::Black);
        // Black's 1... e5 let White's chances rise by one point: a tiny loss.
        assert!(r.moves[1].accuracy > 90.0);
        assert!(r.moves[2].accuracy < 40.0);
    }

    #[test]
    fn a_blunder_by_black_is_measured_against_blacks_chances() {
        // After Black's move White's winning chances jump from 50 to 80.
        let g = play(&["e2e4", "f7f6"]);
        let evals = vec![eval(50.0, "e2e4"), eval(50.0, "e7e5"), eval(80.0, "d2d4")];
        let r = Review::build(&g, evals).unwrap();
        assert_eq!(r.moves[1].class, MoveClass::Blunder);
    }

    #[test]
    fn summary_counts_per_side() {
        let g = play(&["e2e4", "e7e5", "e1e2", "f7f6"]);
        let evals = vec![
            eval(55.0, "e2e4"),
            eval(55.0, "c7c5"),
            eval(56.0, "g1f3"),
            eval(31.0, "d8h4"),
            eval(38.0, "g1f3"),
        ];
        let r = Review::build(&g, evals).unwrap();

        let white = r.summary(Color::White);
        assert_eq!(white.moves, 2);
        assert_eq!(white.blunders, 1);
        assert_eq!(white.mistakes + white.inaccuracies, 0);

        let black = r.summary(Color::Black);
        assert_eq!(black.moves, 2);
        assert_eq!(black.inaccuracies, 1); // 69 -> 62 for Black
        assert_eq!(black.blunders, 0);
        assert!(black.accuracy > white.accuracy);
    }

    #[test]
    fn build_rejects_wrong_number_of_evaluations() {
        let g = play(&["e2e4"]);
        assert!(Review::build(&g, vec![eval(50.0, "e2e4")]).is_none());
    }

    #[test]
    fn review_stops_matching_when_the_game_changes() {
        let mut g = play(&["e2e4", "e7e5"]);
        let evals = vec![eval(50.0, "e2e4"), eval(50.0, "e7e5"), eval(50.0, "g1f3")];
        let r = Review::build(&g, evals).unwrap();
        assert!(r.matches(&g));

        g.go_back(); // navigation alone keeps it valid
        assert!(r.matches(&g));

        g.make_uci_move("c7c5").unwrap();
        assert!(!r.matches(&g));
        assert!(!r.matches(&Game::new()));
    }

    #[test]
    fn empty_side_has_zero_summary() {
        let g = play(&["e2e4"]);
        let r = Review::build(&g, vec![eval(50.0, "e2e4"), eval(52.0, "e7e5")]).unwrap();
        assert_eq!(r.summary(Color::Black), SideSummary::default());
    }
}
