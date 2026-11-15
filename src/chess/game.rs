use std::collections::HashMap;

use shakmaty::{
    CastlingMode, Chess, Color, EnPassantMode, Move, Position, fen::Fen, san::SanPlus, uci::UciMove,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Checkmate { winner: Color },
    Stalemate,
    InsufficientMaterial,
    ThreefoldRepetition,
    FiftyMoveRule,
}

impl Outcome {
    /// Repetition and the fifty-move rule are not terminal: analysis may continue.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Outcome::Checkmate { .. } | Outcome::Stalemate | Outcome::InsufficientMaterial
        )
    }

    pub fn is_draw(self) -> bool {
        !matches!(self, Outcome::Checkmate { .. })
    }

    pub fn result_tag(self) -> &'static str {
        match self {
            Outcome::Checkmate {
                winner: Color::White,
            } => "1-0",
            Outcome::Checkmate {
                winner: Color::Black,
            } => "0-1",
            _ => "1/2-1/2",
        }
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Checkmate {
                winner: Color::White,
            } => f.write_str("White wins by checkmate"),
            Outcome::Checkmate {
                winner: Color::Black,
            } => f.write_str("Black wins by checkmate"),
            Outcome::Stalemate => f.write_str("Draw by stalemate"),
            Outcome::InsufficientMaterial => f.write_str("Draw by insufficient material"),
            Outcome::ThreefoldRepetition => f.write_str("Draw by threefold repetition"),
            Outcome::FiftyMoveRule => f.write_str("Draw by the fifty-move rule"),
        }
    }
}

fn same_position(a: &Chess, b: &Chess) -> bool {
    a.turn() == b.turn()
        && a.board() == b.board()
        && a.castles().castling_rights() == b.castles().castling_rights()
        && a.legal_ep_square() == b.legal_ep_square()
}

/// One move in the game tree.
#[derive(Clone)]
struct Node {
    parent: Option<usize>,
    mv: Move,
    san: String,
    /// Position after the move.
    pos: Chess,
    /// Continuations; the first one is the main line, the rest variations.
    children: Vec<usize>,
}

/// A game with variations. All moves live in a tree; `positions`, `moves`
/// and `san` describe the *active line*, the path through the tree that is
/// currently being viewed, so most code can treat a game as a flat list.
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
    /// Every move of every line.
    nodes: Vec<Node>,
    /// First moves of the game (main line first).
    root_children: Vec<usize>,
    /// Node of each move on the active line; same length as `moves`.
    line: Vec<usize>,
}

impl Game {
    pub fn new() -> Self {
        Self::from_position(Chess::default())
    }

    pub fn from_fen(fen_str: &str) -> anyhow::Result<Self> {
        let fen: Fen = fen_str.parse()?;
        let pos: Chess = fen.into_position(CastlingMode::Standard)?;
        Ok(Self::from_position(pos))
    }

    pub fn from_position(pos: Chess) -> Self {
        Self {
            positions: vec![pos],
            moves: vec![],
            san: vec![],
            cursor: 0,
            headers: default_headers(),
            nodes: vec![],
            root_children: vec![],
            line: vec![],
        }
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

    /// Make a move from the displayed position. If the move is already in
    /// the tree the game simply follows it; otherwise it is added. Moves
    /// that used to follow are kept as another line rather than discarded.
    pub fn make_move(&mut self, m: Move) -> anyhow::Result<()> {
        let parent = self.cursor.checked_sub(1).map(|i| self.line[i]);
        let child = self.add_child(parent, m)?;

        if self.line.get(self.cursor) != Some(&child) {
            self.line.truncate(self.cursor);
            self.line.push(child);
            self.extend_line();
            self.sync_line();
        }
        self.cursor += 1;

        let result = self.outcome().map_or("*", |o| o.result_tag());
        self.headers
            .insert("Result".to_string(), result.to_string());
        Ok(())
    }

    // ---- game tree ---------------------------------------------------

    fn children_of(&self, parent: Option<usize>) -> &[usize] {
        match parent {
            Some(id) => &self.nodes[id].children,
            None => &self.root_children,
        }
    }

    fn children_of_mut(&mut self, parent: Option<usize>) -> &mut Vec<usize> {
        match parent {
            Some(id) => &mut self.nodes[id].children,
            None => &mut self.root_children,
        }
    }

    /// Position after the given move, or the start position for `None`.
    pub(crate) fn position_after(&self, node: Option<usize>) -> &Chess {
        match node {
            Some(id) => &self.nodes[id].pos,
            None => &self.positions[0],
        }
    }

    pub(crate) fn parent_of(&self, node: usize) -> Option<usize> {
        self.nodes[node].parent
    }

    /// Add `m` as a continuation of `parent` and return its node. Returns
    /// the existing node when that move is already there. Does not change
    /// which line is active.
    pub(crate) fn add_child(&mut self, parent: Option<usize>, m: Move) -> anyhow::Result<usize> {
        if let Some(&existing) = self
            .children_of(parent)
            .iter()
            .find(|&&c| self.nodes[c].mv == m)
        {
            return Ok(existing);
        }

        let before = self.position_after(parent).clone();
        let san = SanPlus::from_move(before.clone(), m).to_string();
        let pos = before.play(m)?;

        let id = self.nodes.len();
        self.nodes.push(Node {
            parent,
            mv: m,
            san,
            pos,
            children: vec![],
        });
        self.children_of_mut(parent).push(id);
        Ok(id)
    }

    /// Continue the active line along main-line moves until it ends.
    fn extend_line(&mut self) {
        let mut tip = self.line.last().copied();
        while let Some(&next) = self.children_of(tip).first() {
            self.line.push(next);
            tip = Some(next);
        }
    }

    /// Rebuild `positions`, `moves` and `san` from the active line.
    fn sync_line(&mut self) {
        self.positions.truncate(1);
        self.moves.clear();
        self.san.clear();
        for &id in &self.line {
            let node = &self.nodes[id];
            self.positions.push(node.pos.clone());
            self.moves.push(node.mv);
            self.san.push(node.san.clone());
        }
        self.cursor = self.cursor.min(self.moves.len());
    }

    /// Make the main line active and show its last position.
    pub(crate) fn activate_main_line(&mut self) {
        self.line.clear();
        self.extend_line();
        self.sync_line();
        self.cursor = self.moves.len();
    }

    /// Other moves that were played instead of move `ply` of the active
    /// line, as `(node, san)` pairs.
    pub fn alternatives(&self, ply: usize) -> Vec<(usize, &str)> {
        let Some(&current) = self.line.get(ply) else {
            return vec![];
        };
        self.children_of(self.nodes[current].parent)
            .iter()
            .filter(|&&c| c != current)
            .map(|&c| (c, self.nodes[c].san.as_str()))
            .collect()
    }

    /// Whether move `ply` of the active line is the main-line move there.
    pub fn is_main_move(&self, ply: usize) -> bool {
        self.line.get(ply).is_some_and(|&current| {
            self.children_of(self.nodes[current].parent).first() == Some(&current)
        })
    }

    #[cfg(test)]
    pub fn has_variations(&self) -> bool {
        self.root_children.len() > 1 || self.nodes.iter().any(|n| n.children.len() > 1)
    }

    /// Switch the active line so that it passes through `node`, and show
    /// the position after that move.
    pub fn select_variation(&mut self, node: usize) {
        if node >= self.nodes.len() {
            return;
        }
        let mut path = vec![node];
        while let Some(parent) = self.nodes[path[path.len() - 1]].parent {
            path.push(parent);
        }
        path.reverse();

        self.cursor = path.len();
        self.line = path;
        self.extend_line();
        self.sync_line();
    }

    /// Make move `ply` of the active line the main line at its branch point.
    pub fn promote_variation(&mut self, ply: usize) {
        let Some(&current) = self.line.get(ply) else {
            return;
        };
        let siblings = self.children_of_mut(self.nodes[current].parent);
        if let Some(idx) = siblings.iter().position(|&c| c == current) {
            siblings.remove(idx);
            siblings.insert(0, current);
        }
    }

    /// Remove move `ply` of the active line and everything after it. The
    /// active line falls back to what remains at that point.
    pub fn delete_from(&mut self, ply: usize) {
        let Some(&current) = self.line.get(ply) else {
            return;
        };
        let parent = self.nodes[current].parent;
        self.children_of_mut(parent).retain(|&c| c != current);

        self.line.truncate(ply);
        self.extend_line();
        self.cursor = self.cursor.min(ply);
        self.sync_line();
    }

    /// Also drops side lines.
    pub fn truncate_to(&mut self, ply: usize) {
        let ply = ply.min(self.line.len());
        let parent = ply.checked_sub(1).map(|i| self.line[i]);
        self.children_of_mut(parent).clear();
        self.line.truncate(ply);
        self.sync_line();
        self.cursor = ply;
    }

    pub fn take_back(&mut self, plies: usize) {
        self.truncate_to(self.moves.len().saturating_sub(plies));
    }

    pub fn truncate_to_cursor(&mut self) {
        self.truncate_to(self.cursor);
    }

    /// Returns UCI move strings for positions[0..cursor].
    pub fn uci_moves_to_cursor(&self) -> Vec<String> {
        self.moves[..self.cursor]
            .iter()
            .zip(&self.positions)
            .map(|(m, pos)| m.to_uci(pos.castles().mode()).to_string())
            .collect()
    }

    pub fn start_fen(&self) -> String {
        Fen::from_position(self.start_position(), EnPassantMode::Legal).to_string()
    }

    /// FEN of the current position.
    pub fn current_fen(&self) -> String {
        Fen::from_position(self.current_position(), EnPassantMode::Legal).to_string()
    }

    /// Checkmate, stalemate or insufficient material. History-based draws come from `outcome`.
    pub fn is_game_over(&self) -> bool {
        self.outcome().is_some_and(|o| o.is_terminal())
    }

    /// Positions are equal when pieces, side to move, castling rights and en passant match.
    pub fn repetition_count(&self) -> usize {
        let current = self.current_position();
        self.positions[..=self.cursor]
            .iter()
            .filter(|p| same_position(p, current))
            .count()
    }

    pub fn outcome(&self) -> Option<Outcome> {
        let position = self.current_position();

        if position.is_checkmate() {
            return Some(Outcome::Checkmate {
                winner: position.turn().other(),
            });
        }
        if position.is_stalemate() {
            return Some(Outcome::Stalemate);
        }
        if position.is_insufficient_material() {
            return Some(Outcome::InsufficientMaterial);
        }
        if position.halfmoves() >= 100 {
            return Some(Outcome::FiftyMoveRule);
        }
        if self.repetition_count() >= 3 {
            return Some(Outcome::ThreefoldRepetition);
        }
        None
    }

    pub fn outcome_string(&self) -> Option<String> {
        self.outcome().map(|o| o.to_string())
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

        // FEN/SetUp are regenerated below from the real start position.
        let mut extra: Vec<(&String, &String)> = self
            .headers
            .iter()
            .filter(|(k, _)| {
                !header_order.contains(&k.as_str()) && k.as_str() != "FEN" && k.as_str() != "SetUp"
            })
            .collect();
        extra.sort();
        for (k, v) in extra {
            out.push_str(&format!("[{} \"{}\"]\n", k, v));
        }

        if self.positions[0] != Chess::default() {
            let fen = Fen::from_position(&self.positions[0], EnPassantMode::Legal);
            out.push_str("[SetUp \"1\"]\n");
            out.push_str(&format!("[FEN \"{}\"]\n", fen));
        }

        out.push('\n');

        // Moves, with variations in parentheses.
        let start = &self.positions[0];
        let first_ply = (start.fullmoves().get() as usize - 1) * 2
            + usize::from(start.turn() == shakmaty::Color::Black);
        self.write_line(&mut out, &self.root_children, first_ply, true);

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

impl Game {
    /// Write the line that starts with the first of `children`, followed
    /// after each move by the variations that could have been played instead.
    /// `ply` counts half-moves from White's first move of the game.
    fn write_line(&self, out: &mut String, children: &[usize], ply: usize, number_black: bool) {
        let mut children = children;
        let mut ply = ply;
        let mut number_black = number_black;

        while let Some((&main, variations)) = children.split_first() {
            self.write_move(out, main, ply, number_black);
            number_black = false;

            for &variation in variations {
                out.push('(');
                self.write_move(out, variation, ply, true);
                self.write_line(out, &self.nodes[variation].children, ply + 1, false);
                if out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(") ");
                // After a variation, Black's move needs its number again.
                number_black = true;
            }

            children = &self.nodes[main].children;
            ply += 1;
        }
    }

    fn write_move(&self, out: &mut String, node: usize, ply: usize, number_black: bool) {
        let number = ply / 2 + 1;
        if ply.is_multiple_of(2) {
            out.push_str(&format!("{number}. "));
        } else if number_black {
            out.push_str(&format!("{number}... "));
        }
        out.push_str(&self.nodes[node].san);
        out.push(' ');
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
    h.insert("Date".to_string(), today_pgn_date());
    h.insert("Round".to_string(), "?".to_string());
    h.insert("White".to_string(), "?".to_string());
    h.insert("Black".to_string(), "?".to_string());
    h.insert("Result".to_string(), "*".to_string());
    h
}

fn today_pgn_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}.{m:02}.{d:02}")
}

/// Convert days since 1970-01-01 to a (year, month, day) Gregorian date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
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
    fn new_move_mid_game_starts_a_new_active_line() {
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
        assert_eq!(
            g.outcome_string(),
            Some("Black wins by checkmate".to_string())
        );
        assert_eq!(g.headers.get("Result").map(String::as_str), Some("0-1"));
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

    #[test]
    fn take_back_removes_moves_from_the_end() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3", "b8c6"]);
        g.go_to(1);
        g.take_back(2);
        assert_eq!(g.san, vec!["e4", "e5"]);
        assert_eq!(g.positions.len(), 3);
        assert_eq!(g.cursor, 2);
        assert!(g.at_end());

        g.take_back(5);
        assert!(g.moves.is_empty());
        assert_eq!(g.positions.len(), 1);
        assert_eq!(g.cursor, 0);
        assert_eq!(g.current_fen(), START_FEN);
    }

    #[test]
    fn truncate_to_cursor_makes_shown_position_the_end() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3", "b8c6"]);
        g.go_to(1);
        g.truncate_to_cursor();
        assert_eq!(g.san, vec!["e4"]);
        assert_eq!(g.cursor, 1);
        assert!(g.at_end());

        g.truncate_to_cursor();
        assert_eq!(g.san, vec!["e4"]);
    }

    #[test]
    fn take_back_drops_side_lines_too() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3"]);
        g.go_to(1);
        g.make_uci_move("c7c5").expect("legal");
        assert!(g.has_variations());
        g.take_back(1);
        assert_eq!(g.san, vec!["e4"]);
        assert!(!g.has_variations());
    }

    const SHUFFLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];

    #[test]
    fn threefold_repetition_is_a_draw_but_not_terminal() {
        let mut g = Game::new();
        for m in SHUFFLE {
            g.make_uci_move(m).expect("legal");
        }
        assert_eq!(g.repetition_count(), 2);
        assert_eq!(g.outcome(), None);
        assert_eq!(g.headers.get("Result").map(String::as_str), Some("*"));

        for m in SHUFFLE {
            g.make_uci_move(m).expect("legal");
        }
        assert_eq!(g.repetition_count(), 3);
        assert_eq!(g.outcome(), Some(Outcome::ThreefoldRepetition));
        assert_eq!(
            g.outcome_string(),
            Some("Draw by threefold repetition".to_string())
        );
        assert_eq!(g.headers.get("Result").map(String::as_str), Some("1/2-1/2"));
        assert!(!g.is_game_over());
    }

    #[test]
    fn repetition_only_counts_positions_up_to_the_cursor() {
        let mut g = Game::new();
        for m in SHUFFLE.iter().chain(SHUFFLE.iter()) {
            g.make_uci_move(m).expect("legal");
        }
        g.go_to(4);
        assert_eq!(g.repetition_count(), 2);
        assert_eq!(g.outcome(), None);
    }

    #[test]
    fn repetition_requires_same_castling_rights() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        let mut g = Game::from_fen(fen).expect("valid fen");
        for m in ["e1e2", "e8e7", "e2e1", "e7e8"] {
            g.make_uci_move(m).expect("legal");
        }
        assert_eq!(g.repetition_count(), 1);
        for m in ["e1e2", "e8e7", "e2e1", "e7e8"] {
            g.make_uci_move(m).expect("legal");
        }
        assert_eq!(g.repetition_count(), 2);
        assert_eq!(g.outcome(), None);
    }

    #[test]
    fn fifty_move_rule_is_a_draw() {
        let mut g = Game::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 80").expect("valid fen");
        assert_eq!(g.outcome(), None);
        g.make_uci_move("a1a2").expect("legal");
        assert_eq!(g.outcome(), Some(Outcome::FiftyMoveRule));
        assert!(!g.is_game_over());
    }

    #[test]
    fn checkmate_takes_priority_over_fifty_move_rule() {
        let mut g = Game::from_fen("7k/8/6K1/8/8/8/8/R7 w - - 99 80").expect("valid fen");
        g.make_uci_move("a1a8").expect("legal");
        assert_eq!(
            g.outcome(),
            Some(Outcome::Checkmate {
                winner: Color::White
            })
        );
        assert_eq!(g.headers.get("Result").map(String::as_str), Some("1-0"));
    }

    #[test]
    fn result_resets_when_a_finished_line_is_replaced() {
        let mut g = game_from_uci(&["f2f3", "e7e5", "g2g4", "d8h4"]);
        assert_eq!(g.headers.get("Result").map(String::as_str), Some("0-1"));
        g.go_back();
        g.make_uci_move("d7d6").expect("legal");
        assert_eq!(g.headers.get("Result").map(String::as_str), Some("*"));
    }

    #[test]
    fn start_fen_does_not_change_as_moves_are_played() {
        let mut g = Game::new();
        assert_eq!(g.start_fen(), START_FEN);
        g.make_uci_move("e2e4").expect("legal");
        g.make_uci_move("e7e5").expect("legal");
        assert_eq!(g.start_fen(), START_FEN);
        assert_ne!(g.current_fen(), START_FEN);

        let fen = "4k3/8/8/8/8/8/8/4K2R w K - 0 1";
        let custom = Game::from_fen(fen).expect("valid fen");
        assert_eq!(custom.start_fen(), fen);
    }

    #[test]
    fn to_pgn_writes_each_move_exactly_once() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3", "b8c6"]);
        g.headers.insert("Result".to_string(), "*".to_string());
        let pgn = g.to_pgn();
        let movetext = pgn.rsplit("\n\n").next().unwrap().trim();
        assert_eq!(movetext, "1. e4 e5 2. Nf3 Nc6 *");
    }

    #[test]
    fn to_pgn_black_first_numbers_from_start_position() {
        let fen = "4k3/8/8/8/8/8/8/4K2R b K - 0 12";
        let mut g = Game::from_fen(fen).expect("valid fen");
        g.make_uci_move("e8d8").expect("legal");
        g.make_uci_move("e1g1").expect("legal");
        g.make_uci_move("d8c8").expect("legal");
        let pgn = g.to_pgn();
        let movetext = pgn.rsplit("\n\n").next().unwrap().trim();
        assert_eq!(movetext, "12... Kd8 13. O-O Kc8 *");
        assert!(pgn.contains("[SetUp \"1\"]"));
        assert!(pgn.contains(&format!("[FEN \"{fen}\"]")));
    }

    #[test]
    fn to_pgn_standard_start_has_no_fen_tag() {
        let g = game_from_uci(&["e2e4"]);
        assert!(!g.to_pgn().contains("[FEN"));
    }

    #[test]
    fn new_game_has_real_date_header() {
        let g = Game::new();
        let date = g.headers.get("Date").expect("date header");
        assert_eq!(date.len(), 10);
        assert!(!date.contains('?'), "date was {date}");
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(20_454), (2026, 1, 1));
    }

    fn movetext(g: &Game) -> String {
        g.to_pgn().rsplit("\n\n").next().unwrap().trim().to_string()
    }

    /// 1. e4 e5 2. Nf3, then back to move 1 and 1... c5 2. Nc3.
    fn game_with_variation() -> Game {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3"]);
        g.go_to(1);
        g.make_uci_move("c7c5").expect("legal");
        g.make_uci_move("b1c3").expect("legal");
        g
    }

    #[test]
    fn old_continuation_is_kept_as_a_variation() {
        let g = game_with_variation();
        // The new moves are the active line...
        assert_eq!(g.san, vec!["e4", "c5", "Nc3"]);
        assert_eq!(g.cursor, 3);
        assert!(g.has_variations());
        // ...and the original reply is offered as an alternative to 1... c5.
        let alts: Vec<&str> = g.alternatives(1).into_iter().map(|(_, s)| s).collect();
        assert_eq!(alts, vec!["e5"]);
        assert!(g.alternatives(0).is_empty());
        assert!(g.alternatives(2).is_empty());
        // The first move played stays the main line.
        assert!(g.is_main_move(0));
        assert!(!g.is_main_move(1));
    }

    #[test]
    fn replaying_the_next_move_just_steps_forward() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3"]);
        g.go_to(1);
        g.make_uci_move("e7e5").expect("legal");
        assert_eq!(g.cursor, 2);
        assert_eq!(g.san, vec!["e4", "e5", "Nf3"], "nothing may be lost");
        assert!(!g.has_variations());
    }

    #[test]
    fn replaying_a_variation_move_switches_to_that_line() {
        let mut g = game_with_variation();
        g.go_to(1);
        g.make_uci_move("e7e5").expect("legal");
        assert_eq!(g.san, vec!["e4", "e5", "Nf3"]);
        assert_eq!(g.cursor, 2);
    }

    #[test]
    fn select_variation_switches_line_and_shows_that_move() {
        let mut g = game_with_variation();
        let (node, _) = g.alternatives(1)[0];
        g.select_variation(node);
        assert_eq!(g.san, vec!["e4", "e5", "Nf3"]);
        assert_eq!(g.cursor, 2);
        // Now c5 is the alternative.
        let alts: Vec<&str> = g.alternatives(1).into_iter().map(|(_, s)| s).collect();
        assert_eq!(alts, vec!["c5"]);

        g.select_variation(9999); // unknown node is ignored
        assert_eq!(g.san, vec!["e4", "e5", "Nf3"]);
    }

    #[test]
    fn to_pgn_writes_variations_in_parentheses() {
        let g = game_with_variation();
        assert_eq!(movetext(&g), "1. e4 e5 (1... c5 2. Nc3) 2. Nf3 *");
    }

    #[test]
    fn to_pgn_numbers_black_move_after_a_white_variation() {
        // 1. e4 (1. d4 d5) 1... e5
        let mut g = game_from_uci(&["e2e4", "e7e5"]);
        g.go_to_start();
        g.make_uci_move("d2d4").expect("legal");
        g.make_uci_move("d7d5").expect("legal");
        assert_eq!(movetext(&g), "1. e4 (1. d4 d5) 1... e5 *");
    }

    #[test]
    fn to_pgn_writes_nested_variations() {
        let mut g = game_with_variation();
        // Inside the 1... c5 line, add 2. Nf3 as an alternative to 2. Nc3.
        g.go_to(2);
        g.make_uci_move("g1f3").expect("legal");
        assert_eq!(movetext(&g), "1. e4 e5 (1... c5 2. Nc3 (2. Nf3)) 2. Nf3 *");
    }

    #[test]
    fn promote_variation_makes_it_the_main_line() {
        let mut g = game_with_variation();
        g.promote_variation(1);
        assert!(g.is_main_move(1));
        assert_eq!(movetext(&g), "1. e4 c5 (1... e5 2. Nf3) 2. Nc3 *");
        // The view does not move.
        assert_eq!(g.san, vec!["e4", "c5", "Nc3"]);
        assert_eq!(g.cursor, 3);
    }

    #[test]
    fn delete_from_removes_the_line_and_falls_back() {
        let mut g = game_with_variation();
        g.delete_from(1); // delete 1... c5 and what follows
        assert_eq!(g.san, vec!["e4", "e5", "Nf3"]);
        assert_eq!(g.cursor, 1);
        assert!(!g.has_variations());
        assert_eq!(movetext(&g), "1. e4 e5 2. Nf3 *");
    }

    #[test]
    fn delete_from_the_only_line_truncates_the_game() {
        let mut g = game_from_uci(&["e2e4", "e7e5", "g1f3"]);
        g.delete_from(1);
        assert_eq!(g.san, vec!["e4"]);
        assert_eq!(g.cursor, 1);
        g.delete_from(0);
        assert!(g.moves.is_empty());
        assert_eq!(g.cursor, 0);
        assert_eq!(g.positions.len(), 1);
    }
}
