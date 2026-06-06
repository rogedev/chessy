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

        if name == b"FEN" {
            if let Ok(fen) = Fen::from_ascii(value.as_bytes()) {
                if let Ok(pos) = fen.into_position(CastlingMode::Standard) {
                    tags.1 = Some(pos);
                }
            }
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
        if let Ok(m) = san_plus.san.to_move(&state.position) {
            if let Ok(new_pos) = state.position.clone().play(m.clone()) {
                state.san.push(san_plus.to_string());
                state.moves.push(m);
                state.positions.push(new_pos.clone());
                state.position = new_pos;
            }
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
