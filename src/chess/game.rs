use std::collections::HashMap;

use shakmaty::{
    CastlingMode, Chess, EnPassantMode, Move, Position, fen::Fen, san::SanPlus, uci::UciMove,
};

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
        let position = self.current_position();

        if position.is_checkmate() {
            let winner = position.turn().other();
            return Some(format!("{} wins by checkmate", winner));
        }

        if position.is_stalemate() {
            return Some("Draw by stalemate".to_string());
        }

        if position.is_insufficient_material() {
            return Some("Draw by insufficient material".to_string());
        }

        None
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
