use std::{
    collections::HashMap,
    fs,
    io::{self, Cursor},
    ops::ControlFlow,
    path::Path,
};

use pgn_reader::{RawTag, Reader, SanPlus, Visitor};
use shakmaty::{CastlingMode, Chess, Position, fen::Fen};

use super::game::Game;

struct GameLoader {
    games: Vec<Game>,
}

impl GameLoader {
    fn new() -> Self {
        Self { games: vec![] }
    }
}

/// State carried through the tags phase: (headers map, optional non-standard start)
type TagsState = (HashMap<String, String>, Option<Chess>);

/// State carried through the movetext phase
struct MovetextState {
    headers: HashMap<String, String>,
    position: Chess,
    positions: Vec<Chess>,
    moves: Vec<shakmaty::Move>,
    san: Vec<String>,
}

impl Visitor for GameLoader {
    type Tags = TagsState;
    type Movetext = MovetextState;
    type Output = ();

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue((HashMap::new(), None))
    }

    fn tag(
        &mut self,
        tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        let name_str = String::from_utf8_lossy(name).into_owned();
        let value_str = value.decode_utf8_lossy().into_owned();

        if name == b"FEN"
            && let Ok(fen) = Fen::from_ascii(value.as_bytes())
            && let Ok(pos) = fen.into_position(CastlingMode::Standard)
        {
            tags.1 = Some(pos);
        }
        tags.0.insert(name_str, value_str);
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        let (headers, custom_start) = tags;
        let start = custom_start.unwrap_or_default();
        let positions = vec![start.clone()];
        ControlFlow::Continue(MovetextState {
            headers,
            position: start,
            positions,
            moves: vec![],
            san: vec![],
        })
    }

    fn san(&mut self, state: &mut Self::Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        if let Ok(m) = san_plus.san.to_move(&state.position)
            && let Ok(new_pos) = state.position.clone().play(m)
        {
            state.san.push(san_plus.to_string());
            state.moves.push(m);
            state.positions.push(new_pos.clone());
            state.position = new_pos;
        }
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, state: Self::Movetext) -> Self::Output {
        let cursor = state.moves.len();
        let game = Game {
            positions: state.positions,
            moves: state.moves,
            san: state.san,
            cursor,
            headers: state.headers,
        };
        self.games.push(game);
    }
}

pub fn load_pgn(path: &Path) -> io::Result<Vec<Game>> {
    let data = fs::read(path)?;
    let cursor = Cursor::new(data);
    let mut reader = Reader::new(cursor);
    let mut loader = GameLoader::new();
    while reader.read_game(&mut loader)?.is_some() {}
    Ok(loader.games)
}

pub fn load_pgn_str(pgn: &str) -> Vec<Game> {
    let cursor = Cursor::new(pgn.as_bytes());
    let mut reader = Reader::new(cursor);
    let mut loader = GameLoader::new();
    while let Ok(Some(())) = reader.read_game(&mut loader) {}
    loader.games
}

pub fn save_pgn(games: &[Game], path: &Path) -> io::Result<()> {
    let mut out = String::new();
    for game in games {
        out.push_str(&game.to_pgn());
        out.push('\n');
    }
    fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_single_game_with_moves() {
        let pgn = "[Event \"Test\"]\n\n1. e4 e5 2. Nf3 Nc6 1/2-1/2\n";
        let games = load_pgn_str(pgn);
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.san, vec!["e4", "e5", "Nf3", "Nc6"]);
        assert_eq!(g.headers.get("Event").map(String::as_str), Some("Test"));
    }

    #[test]
    fn loads_multiple_games() {
        let pgn = "[Event \"A\"]\n\n1. e4 e5 *\n\n[Event \"B\"]\n\n1. d4 d5 *\n";
        let games = load_pgn_str(pgn);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].san, vec!["e4", "e5"]);
        assert_eq!(games[1].san, vec!["d4", "d5"]);
    }

    #[test]
    fn respects_custom_fen_start_position() {
        use shakmaty::{Color, Square};
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
        let pgn = format!("[FEN \"{fen}\"]\n[SetUp \"1\"]\n\n1... c5 *\n");
        let games = load_pgn_str(&pgn);
        assert_eq!(games.len(), 1);
        let g = &games[0];
        // Custom start: Black to move with a white pawn already on e4.
        assert_eq!(g.positions[0].turn(), Color::Black);
        assert!(g.positions[0].board().piece_at(Square::E4).is_some());
        assert_eq!(g.san, vec!["c5"]);
    }

    #[test]
    fn round_trips_through_to_pgn() {
        let mut original = Game::new();
        for m in ["e2e4", "e7e5", "g1f3", "b8c6"] {
            original.make_uci_move(m).expect("legal");
        }
        let reloaded = load_pgn_str(&original.to_pgn());
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].san, original.san);
    }
}
