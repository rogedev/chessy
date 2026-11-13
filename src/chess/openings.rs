//! Opening names (ECO) looked up by position, so transpositions are named
//! correctly. Data is the CC0 lichess `chess-openings` table, compiled in.

use std::{collections::HashMap, sync::OnceLock};

use shakmaty::{Chess, EnPassantMode, Position, fen::Fen, san::San};

use super::Game;

const TABLES: [&str; 5] = [
    include_str!("../../assets/openings/a.tsv"),
    include_str!("../../assets/openings/b.tsv"),
    include_str!("../../assets/openings/c.tsv"),
    include_str!("../../assets/openings/d.tsv"),
    include_str!("../../assets/openings/e.tsv"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    pub eco: String,
    pub name: String,
}

pub struct OpeningBook {
    by_position: HashMap<String, Opening>,
    /// Longest line in the book, in plies. Positions deeper than this can
    /// never match, which bounds the work done per lookup.
    max_ply: usize,
}

impl OpeningBook {
    /// The built-in book, parsed once on first use.
    pub fn builtin() -> &'static OpeningBook {
        static BOOK: OnceLock<OpeningBook> = OnceLock::new();
        BOOK.get_or_init(|| OpeningBook::from_tsv(TABLES.iter().copied()))
    }

    pub fn from_tsv<'a>(tables: impl IntoIterator<Item = &'a str>) -> Self {
        let mut by_position = HashMap::new();
        let mut max_ply = 0;

        for line in tables.into_iter().flat_map(str::lines) {
            let mut cols = line.split('\t');
            let (Some(eco), Some(name), Some(pgn)) = (cols.next(), cols.next(), cols.next()) else {
                continue;
            };
            let Some((pos, plies)) = play_line(pgn) else {
                continue; // header row or unparsable line
            };
            max_ply = max_ply.max(plies);
            by_position.insert(
                position_key(&pos),
                Opening {
                    eco: eco.to_string(),
                    name: name.to_string(),
                },
            );
        }

        Self {
            by_position,
            max_ply,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.by_position.len()
    }

    /// The most specific named opening reached at or before the cursor.
    pub fn lookup(&self, game: &Game) -> Option<&Opening> {
        let last = game.cursor.min(self.max_ply);
        game.positions[..=last]
            .iter()
            .rev()
            .find_map(|pos| self.by_position.get(&position_key(pos)))
    }
}

/// Play a movetext line such as `1. e4 e5 2. Nf3` from the start position.
/// Returns the final position and the number of plies, or `None` if any
/// token is not a legal move (or there are no moves at all).
fn play_line(pgn: &str) -> Option<(Chess, usize)> {
    let mut pos = Chess::default();
    let mut plies = 0;
    for token in pgn.split_whitespace() {
        if token.ends_with('.') {
            continue; // move number
        }
        let san: San = token.parse().ok()?;
        let m = san.to_move(&pos).ok()?;
        pos = pos.play(m).ok()?;
        plies += 1;
    }
    (plies > 0).then_some((pos, plies))
}

/// Board, side to move, castling rights and en-passant square: everything
/// that identifies a position except the move counters.
fn position_key(pos: &Chess) -> String {
    let fen = Fen::from_position(pos, EnPassantMode::Legal).to_string();
    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(moves: &[&str]) -> Game {
        let mut g = Game::new();
        for m in moves {
            g.make_uci_move(m).expect("legal move");
        }
        g
    }

    fn name_of(g: &Game) -> Option<String> {
        OpeningBook::builtin()
            .lookup(g)
            .map(|o| format!("{} {}", o.eco, o.name))
    }

    #[test]
    fn builtin_book_loads_every_line() {
        let rows: usize = TABLES
            .iter()
            .map(|t| t.lines().filter(|l| !l.starts_with("eco\t")).count())
            .sum();
        let parsed: usize = TABLES
            .iter()
            .flat_map(|t| t.lines())
            .filter(|l| l.split('\t').nth(2).and_then(play_line).is_some())
            .count();
        assert_eq!(parsed, rows, "every data row must contain legal moves");
        assert!(OpeningBook::builtin().len() > 3000);
    }

    #[test]
    fn start_position_has_no_opening() {
        assert_eq!(name_of(&Game::new()), None);
    }

    #[test]
    fn names_main_lines() {
        assert_eq!(
            name_of(&game(&["e2e4", "c7c5"])).as_deref(),
            Some("B20 Sicilian Defense")
        );
        assert_eq!(
            name_of(&game(&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"])).as_deref(),
            Some("C60 Ruy Lopez")
        );
        assert_eq!(
            name_of(&game(&["d2d4", "d7d5", "c2c4"])).as_deref(),
            Some("D06 Queen's Gambit")
        );
    }

    #[test]
    fn transposition_gets_the_same_name() {
        // Queen's Gambit Declined reached through 1. c4 e6 2. d4 d5.
        let direct = game(&["d2d4", "d7d5", "c2c4", "e7e6"]);
        let transposed = game(&["c2c4", "e7e6", "d2d4", "d7d5"]);
        assert!(name_of(&direct).is_some());
        assert_eq!(name_of(&direct), name_of(&transposed));
    }

    #[test]
    fn keeps_last_known_name_after_leaving_the_book() {
        let mut g = game(&["e2e4", "c7c5", "h2h4", "h7h5", "a2a4", "a7a5"]);
        let deep = name_of(&g).expect("still named");
        assert!(deep.contains("Sicilian"), "got {deep}");

        // Stepping back to before 1... c5 changes the name.
        g.go_to(1);
        assert_eq!(name_of(&g).as_deref(), Some("B00 King's Pawn Game"));
    }

    #[test]
    fn custom_start_position_is_not_named() {
        let g = Game::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").expect("valid fen");
        assert_eq!(name_of(&g), None);
    }

    #[test]
    fn from_tsv_skips_header_and_bad_rows() {
        let book = OpeningBook::from_tsv(["eco\tname\tpgn\nA00\tTest\t1. e4\nZ99\tBad\t1. e5\n"]);
        assert_eq!(book.len(), 1);
    }
}
