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
        let san_str = SanPlus::from_move(pos.clone(), m.clone()).to_string();
        let new_pos = pos.play(m.clone())?;

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
            } else if i % 2 == 0 {
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
