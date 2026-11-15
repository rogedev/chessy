use shakmaty::{Board, Color, Move, Position, Rank, Square};

use crate::chess::Game;

pub const DURATION: f64 = 0.15;

/// `end` is the square the piece occupies on the board being displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slide {
    pub start: Square,
    pub end: Square,
}

/// Castling moves both king and rook. With `forward` false every slide is reversed.
pub fn slides_for(m: &Move, forward: bool) -> Vec<Slide> {
    let mut slides = match *m {
        Move::Castle { king, rook } => {
            let color = Color::from_white(king.rank() == Rank::First);
            match m.castling_side() {
                Some(side) => vec![
                    Slide {
                        start: king,
                        end: side.king_to(color),
                    },
                    Slide {
                        start: rook,
                        end: side.rook_to(color),
                    },
                ],
                None => vec![],
            }
        }
        _ => m
            .from()
            .map(|from| {
                vec![Slide {
                    start: from,
                    end: m.to(),
                }]
            })
            .unwrap_or_default(),
    };

    if !forward {
        for s in &mut slides {
            std::mem::swap(&mut s.start, &mut s.end);
        }
    }
    slides
}

pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Default)]
pub struct MoveAnimator {
    seen: Option<(usize, Board)>,
    slides: Vec<Slide>,
    started: f64,
    /// Set when a piece was dropped by hand, so the next change is not animated.
    pub skip_next: bool,
}

impl MoveAnimator {
    /// Call once per frame. Animates only a single step forward or back.
    pub fn update(&mut self, game: &Game, now: f64, enabled: bool) {
        let board = game.current_position().board();
        let cursor = game.cursor;

        let changed = match &self.seen {
            Some((_, seen)) => seen != board,
            None => false,
        };

        if changed {
            let (prev_cursor, prev_board) = self.seen.as_ref().expect("changed implies seen");
            let step = step_between(game, *prev_cursor, prev_board);
            self.slides = match step {
                Some((m, forward)) if enabled && !self.skip_next => slides_for(&m, forward),
                _ => vec![],
            };
            self.started = now;
            self.skip_next = false;
        }

        if self.seen.is_none() || changed {
            self.seen = Some((cursor, board.clone()));
        } else if let Some((c, _)) = &mut self.seen {
            *c = cursor;
        }

        if now - self.started >= DURATION {
            self.slides.clear();
        }
    }

    pub fn is_active(&self) -> bool {
        !self.slides.is_empty()
    }

    pub fn slides(&self, now: f64) -> impl Iterator<Item = (Slide, f32)> + '_ {
        let t = ease_out(((now - self.started) / DURATION) as f32);
        self.slides.iter().map(move |s| (*s, t))
    }

    pub fn hides(&self, sq: Square) -> bool {
        self.slides.iter().any(|s| s.end == sq)
    }
}

/// The move and direction when the game moved exactly one ply, else `None`.
fn step_between(game: &Game, prev_cursor: usize, prev_board: &Board) -> Option<(Move, bool)> {
    let cursor = game.cursor;
    let shown = |idx: usize| game.positions.get(idx).map(|p| p.board());

    if cursor == prev_cursor + 1 && shown(prev_cursor) == Some(prev_board) {
        return game.moves.get(prev_cursor).map(|m| (*m, true));
    }
    if cursor + 1 == prev_cursor && shown(prev_cursor) == Some(prev_board) {
        return game.moves.get(cursor).map(|m| (*m, false));
    }
    None
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

    #[test]
    fn normal_move_slides_one_piece() {
        let g = play(&["e2e4"]);
        assert_eq!(
            slides_for(&g.moves[0], true),
            vec![Slide {
                start: Square::E2,
                end: Square::E4
            }]
        );
        assert_eq!(
            slides_for(&g.moves[0], false),
            vec![Slide {
                start: Square::E4,
                end: Square::E2
            }]
        );
    }

    #[test]
    fn castling_slides_king_and_rook() {
        let g = play(&["e2e4", "e7e5", "g1f3", "g8f6", "f1c4", "f8c5", "e1g1"]);
        let slides = slides_for(g.moves.last().unwrap(), true);
        assert_eq!(
            slides,
            vec![
                Slide {
                    start: Square::E1,
                    end: Square::G1
                },
                Slide {
                    start: Square::H1,
                    end: Square::F1
                },
            ]
        );
    }

    #[test]
    fn black_queenside_castling_uses_black_squares() {
        let mut g = Game::from_fen("r3k3/8/8/8/8/8/8/4K3 b q - 0 1").expect("valid fen");
        g.make_uci_move("e8c8").expect("legal");
        assert_eq!(
            slides_for(&g.moves[0], true),
            vec![
                Slide {
                    start: Square::E8,
                    end: Square::C8
                },
                Slide {
                    start: Square::A8,
                    end: Square::D8
                },
            ]
        );
    }

    #[test]
    fn ease_out_is_monotonic_and_clamped() {
        assert_eq!(ease_out(-1.0), 0.0);
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert!(ease_out(0.25) < ease_out(0.5));
        assert!(ease_out(0.5) > 0.5, "should decelerate");
    }

    #[test]
    fn animator_starts_on_single_step_and_finishes() {
        let mut g = Game::new();
        let mut a = MoveAnimator::default();
        a.update(&g, 0.0, true);
        assert!(!a.is_active(), "nothing to animate on first frame");

        g.make_uci_move("e2e4").expect("legal");
        a.update(&g, 1.0, true);
        assert!(a.is_active());
        assert!(a.hides(Square::E4));
        assert!(!a.hides(Square::E2));

        a.update(&g, 1.0 + DURATION / 2.0, true);
        assert!(a.is_active());
        a.update(&g, 1.0 + DURATION + 0.001, true);
        assert!(!a.is_active());
    }

    #[test]
    fn animator_reverses_when_stepping_back() {
        let mut g = play(&["e2e4", "e7e5"]);
        let mut a = MoveAnimator::default();
        a.update(&g, 0.0, true);

        g.go_back();
        a.update(&g, 1.0, true);
        let slides: Vec<Slide> = a.slides(1.0).map(|(s, _)| s).collect();
        assert_eq!(
            slides,
            vec![Slide {
                start: Square::E5,
                end: Square::E7
            }]
        );
    }

    #[test]
    fn animator_ignores_jumps_new_games_and_disabled() {
        let g = play(&["e2e4", "e7e5", "g1f3"]);
        let mut a = MoveAnimator::default();
        a.update(&g, 0.0, true);

        // A different game one ply longer: the previous board does not match.
        let other = play(&["d2d4", "d7d5", "c2c4", "e7e6"]);
        a.update(&other, 1.0, true);
        assert!(!a.is_active());

        let mut other = other;
        other.go_to_start();
        a.update(&other, 2.0, true);
        assert!(!a.is_active());

        let mut g = Game::new();
        let mut a = MoveAnimator::default();
        a.update(&g, 0.0, false);
        g.make_uci_move("e2e4").expect("legal");
        a.update(&g, 1.0, false);
        assert!(!a.is_active());
    }

    #[test]
    fn animator_skips_a_hand_dropped_piece_once() {
        let mut g = Game::new();
        let mut a = MoveAnimator::default();
        a.update(&g, 0.0, true);

        a.skip_next = true;
        g.make_uci_move("e2e4").expect("legal");
        a.update(&g, 1.0, true);
        assert!(!a.is_active());

        g.make_uci_move("e7e5").expect("legal");
        a.update(&g, 2.0, true);
        assert!(a.is_active());
    }
}
