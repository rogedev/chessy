use std::collections::HashMap;

use shakmaty::{
    CastlingMode, Chess, EnPassantMode, Move, Position, fen::Fen, san::SanPlus, uci::UciMove,
};

#[derive(Clone)]
pub struct Game {
    /// positions[0] = starting position, positions[i+1] = after moves[i]
    pub positions: Vec<Chess>,
    pub moves: Vec<Move>,
    /// SAN string for each move
    pub san: Vec<String>,
    /// Index of the currently displayed position (0 = start)
    pub cursor: usize,
    pub headers: HashMap<String, String>,
}

impl Game {
    pub fn new() -> Self {
        Self {
            positions: vec![Chess::default()],
            moves: vec![],
            san: vec![],
            cursor: 0,
            headers: default_headers(),
        }
    }

    pub fn from_fen(fen_str: &str) -> anyhow::Result<Self> {
        let fen: Fen = fen_str.parse()?;
        let pos: Chess = fen.into_position(CastlingMode::Standard)?;
        Ok(Self {
            positions: vec![pos],
            moves: vec![],
            san: vec![],
            cursor: 0,
            headers: default_headers(),
        })
    }

    pub fn current_position(&self) -> &Chess {
        &self.positions[self.cursor]
    }

    pub fn start_position(&self) -> &Chess {
        &self.positions[0]
    }

    pub fn at_end(&self) -> bool {
        self.cursor == self.moves.len()
    }

    pub fn go_back(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn go_forward(&mut self) {
        if self.cursor < self.moves.len() {
            self.cursor += 1;
        }
    }

    pub fn go_to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn go_to_end(&mut self) {
        self.cursor = self.moves.len();
    }

    pub fn go_to(&mut self, idx: usize) {
        self.cursor = idx.min(self.moves.len());
    }

    /// Make a move from the current cursor position, truncating any future moves.
    pub fn make_move(&mut self, m: Move) -> anyhow::Result<()> {
        let pos = self.current_position().clone();
        let san_str = SanPlus::from_move(pos.clone(), m).to_string();
        let new_pos = pos.play(m)?;

        // Truncate future branch if navigating in the middle
        self.moves.truncate(self.cursor);
        self.san.truncate(self.cursor);
        self.positions.truncate(self.cursor + 1);

        self.moves.push(m);
        self.san.push(san_str);
        self.positions.push(new_pos);
        self.cursor += 1;
        Ok(())
    }

    /// Returns UCI move strings for positions[0..cursor].
    pub fn uci_moves_to_cursor(&self) -> Vec<String> {
        self.moves[..self.cursor]
            .iter()
            .zip(&self.positions)
            .map(|(m, pos)| m.to_uci(pos.castles().mode()).to_string())
            .collect()
    }

    /// FEN of the current position.
    pub fn current_fen(&self) -> String {
        Fen::from_position(self.current_position(), EnPassantMode::Legal).to_string()
    }

    /// Whether the game is over at the current position.
    pub fn is_game_over(&self) -> bool {
        let pos = self.current_position();
        pos.is_checkmate() || pos.is_stalemate() || pos.is_insufficient_material()
    }

    pub fn outcome_string(&self) -> Option<String> {
        let pos = self.current_position();
        if pos.is_checkmate() {
            let winner = pos.turn().other();
            Some(format!("{} wins by checkmate", winner))
        } else if pos.is_stalemate() {
            Some("Draw by stalemate".to_string())
        } else if pos.is_insufficient_material() {
            Some("Draw by insufficient material".to_string())
        } else {
            None
        }
    }

    /// Try to make a move from UCI string (e.g. "e2e4") in the current position.
    pub fn make_uci_move(&mut self, uci_str: &str) -> anyhow::Result<()> {
        let uci: UciMove = uci_str.parse()?;
        let m = uci.to_move(self.current_position())?;
        self.make_move(m)
    }

    /// Returns the PGN string for all moves in this game.
    pub fn to_pgn(&self) -> String {
        let mut out = String::new();

        // Headers
        let header_order = ["Event", "Site", "Date", "Round", "White", "Black", "Result"];
        for key in &header_order {
            let val = self.headers.get(*key).map(|s| s.as_str()).unwrap_or("?");
            out.push_str(&format!("[{} \"{}\"]\n", key, val));
        }
        for (k, v) in &self.headers {
            if !header_order.contains(&k.as_str()) {
                out.push_str(&format!("[{} \"{}\"]\n", k, v));
            }
        }
        out.push('\n');

        // Moves
        let start_color = self.positions[0].turn();
        let is_black_first = start_color == shakmaty::Color::Black;

        for (i, san_str) in self.san.iter().enumerate() {
            let move_num = i / 2 + 1;
            if i == 0 {
                if is_black_first {
                    out.push_str(&format!("{}... ", move_num));
                } else {
                    out.push_str(&format!("{}. ", move_num));
                }
            } else if i.is_multiple_of(2) {
                out.push_str(&format!("{}. ", move_num));
            }
            out.push_str(san_str);
            out.push(' ');
        }

        let result = self
            .headers
            .get("Result")
            .map(|s| s.as_str())
            .unwrap_or("*");
        out.push_str(result);
        out.push('\n');
        out
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

fn default_headers() -> HashMap<String, String> {
    let mut h = HashMap::new();
    h.insert("Event".to_string(), "?".to_string());
    h.insert("Site".to_string(), "?".to_string());
    h.insert("Date".to_string(), "????.??.??".to_string());
    h.insert("Round".to_string(), "?".to_string());
    h.insert("White".to_string(), "?".to_string());
    h.insert("Black".to_string(), "?".to_string());
    h.insert("Result".to_string(), "*".to_string());
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    /// Build a game by playing a sequence of UCI moves from the start position.
    fn game_from_uci(moves: &[&str]) -> Game {
        let mut g = Game::new();
        for m in moves {
            g.make_uci_move(m).expect("legal move");
        }
        g
    }

    #[test]
    fn new_starts_at_standard_position() {
        let g = Game::new();
        assert_eq!(g.cursor, 0);
        assert!(g.moves.is_empty());
        assert!(g.san.is_empty());
        assert!(g.at_end());
        assert_eq!(g.current_fen(), START_FEN);
    }

    #[test]
    fn from_fen_parses_valid_and_rejects_garbage() {
        let g = Game::from_fen(START_FEN).expect("valid fen");
        assert_eq!(g.current_fen(), START_FEN);
        assert!(Game::from_fen("not a fen").is_err());
    }

    #[test]
    fn make_uci_move_advances_and_records_san() {
        let mut g = Game::new();
        g.make_uci_move("e2e4").expect("legal");
        assert_eq!(g.cursor, 1);
        assert_eq!(g.moves.len(), 1);
        assert_eq!(g.san, vec!["e4"]);
        assert!(g.at_end());
    }

    #[test]
    fn make_uci_move_rejects_illegal_and_leaves_state() {
        let mut g = Game::new();
        assert!(g.make_uci_move("e2e5").is_err());
        assert_eq!(g.cursor, 0);
        assert!(g.moves.is_empty());
    }

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3"]);
        assert_eq!(g.cursor, 3);
        assert!(g.at_end());

        g.go_back();
        assert_eq!(g.cursor, 2);
        assert!(!g.at_end());

        g.go_to_start();
        assert_eq!(g.cursor, 0);
        g.go_back(); // already at start, no underflow
        assert_eq!(g.cursor, 0);

        g.go_to_end();
        assert_eq!(g.cursor, 3);
        g.go_forward(); // already at end, no overflow
        assert_eq!(g.cursor, 3);

        g.go_to(1);
        assert_eq!(g.cursor, 1);
        g.go_to(99); // clamps to number of moves
        assert_eq!(g.cursor, 3);
    }

    #[test]
    fn make_move_truncates_future_branch() {
        let mut g = game_from_uci(&["e2e4", "e7e5"]);
        g.go_back(); // cursor == 1, black to move after 1.e4
        g.make_uci_move("c7c5").expect("legal alternative");
        assert_eq!(g.cursor, 2);
        assert_eq!(g.moves.len(), 2);
        assert_eq!(g.positions.len(), 3);
        assert_eq!(g.san, vec!["e4", "c5"]);
    }

    #[test]
    fn uci_moves_to_cursor_respects_position() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3"]);
        assert_eq!(g.uci_moves_to_cursor(), vec!["e2e4", "e7e5", "g1f3"]);
        g.go_to(1);
        assert_eq!(g.uci_moves_to_cursor(), vec!["e2e4"]);
        g.go_to_start();
        assert!(g.uci_moves_to_cursor().is_empty());
    }

    #[test]
    fn detects_checkmate() {
        // Fool's mate: 1. f3 e5 2. g4 Qh4#
        let g = game_from_uci(&["f2f3", "e7e5", "g2g4", "d8h4"]);
        assert!(g.is_game_over());
        // shakmaty's Color renders lowercase.
        assert_eq!(
            g.outcome_string(),
            Some("black wins by checkmate".to_string())
        );
    }

    #[test]
    fn detects_stalemate() {
        let g = Game::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("valid fen");
        assert!(g.is_game_over());
        assert_eq!(g.outcome_string(), Some("Draw by stalemate".to_string()));
    }

    #[test]
    fn detects_insufficient_material() {
        let g = Game::from_fen("8/8/8/4k3/8/4K3/8/8 w - - 0 1").expect("valid fen");
        assert!(g.is_game_over());
        assert_eq!(
            g.outcome_string(),
            Some("Draw by insufficient material".to_string())
        );
    }

    #[test]
    fn ongoing_position_has_no_outcome() {
        let g = Game::new();
        assert!(!g.is_game_over());
        assert!(g.outcome_string().is_none());
    }

    #[test]
    fn to_pgn_formats_white_first_game() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3"]);
        g.headers.insert("Result".to_string(), "1-0".to_string());
        let pgn = g.to_pgn();
        assert!(pgn.contains("[White \"?\"]"));
        assert!(pgn.contains("1. e4 e5 2. Nf3"));
        assert!(pgn.trim_end().ends_with("1-0"));
    }

    #[test]
    fn to_pgn_prefixes_black_first_move() {
        // Position after 1.e4 with Black to move.
        let mut g = Game::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1")
            .expect("valid fen");
        g.make_uci_move("c7c5").expect("legal");
        let pgn = g.to_pgn();
        assert!(pgn.contains("1... c5"), "pgn was: {pgn}");
    }
}
