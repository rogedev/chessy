use std::path::Path;

use egui::{Color32, Context, FontId, Key, RichText, Ui};
use shakmaty::{
    Bitboard, Board, CastlingMode, Color as PieceColor, Piece, Position, Role, Setup, Square,
};

use crate::{
    audio::{AudioPlayer, SoundEvent},
    chess::{
        Game, Outcome,
        clock::{self, Clock, format_clock},
        material::{captured_pieces, material_balance},
        openings::OpeningBook,
        pgn,
        review::{PositionEval, Review},
    },
    engine::{EngineInfo, EngineOutput, Score, UciClient},
    ui::{
        Settings,
        analysis::{show_analysis_panel, show_eval_bar},
        arrows::uci_squares,
        board::{
            BOARD_THEMES, BoardInteraction, BoardStyle, BoardTheme, BoardWidget, PieceTextures,
            SetupBoardWidget, available_piece_sets, sound_for_move, try_make_promotion_move,
        },
        captured::show_captured,
        graph::show_eval_graph,
        moves::show_moves_panel,
        picker::show_game_picker,
        theme::{self, Palette},
    },
};

const SHORTCUT_NEW_GAME: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::N);
const SHORTCUT_OPEN_PGN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::O);
const SHORTCUT_SAVE_PGN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::S);
const SHORTCUT_COPY_PGN: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
    Key::C,
);

const HINT_MOVETIME_MS: u64 = 1500;

const BOARD_MARGIN: f32 = 14.0;
const EVAL_BAR_WIDTH: f32 = 30.0;
const EVAL_BAR_GAP: f32 = 10.0;

const SKILL_PRESETS: &[(&str, u32)] = &[
    ("Beginner", 800),
    ("Casual", 1200),
    ("Intermediate", 1600),
    ("Advanced", 2000),
    ("Strong", 2400),
];

const REVIEW_DEPTH: u8 = 14;

struct ReviewRun {
    game: Game,
    start_fen: String,
    evals: Vec<PositionEval>,
    latest: Option<Score>,
    /// False until `readyok`: earlier output belongs to the interrupted analysis.
    ready: bool,
}

#[derive(PartialEq, Clone, Copy)]
pub enum AppMode {
    Play,
    Analyze,
    Setup,
}

pub struct ChessyApp {
    game: Game,
    mode: AppMode,
    player_color: PieceColor,
    engine: Option<UciClient>,
    engine_lines: Vec<EngineInfo>,
    eval_score: Option<Score>,
    engine_running: bool,
    settings: Settings,
    show_settings: bool,
    flipped: bool,
    interaction: BoardInteraction,
    last_analyzed_fen: String,
    waiting_for_bestmove: bool,
    show_about: bool,
    piece_textures: PieceTextures,
    audio: Option<AudioPlayer>,
    setup_board: Board,
    setup_turn: PieceColor,
    setup_palette: Option<Piece>,
    setup_error: Option<String>,
    review_run: Option<ReviewRun>,
    review: Option<Review>,
    clock: Option<Clock>,
    clock_plies: usize,
    now: f64,
    /// Abandoned searches whose output must be ignored when it arrives.
    stale_searches: u32,
    waiting_for_hint: bool,
    hint: Option<(Square, Square, String)>,
    resigned: Option<PieceColor>,
    resign_armed: bool,
    applied_theme: Option<bool>,
    pgn_games: Vec<Game>,
    pgn_current: Option<usize>,
    pgn_filter: String,
    show_game_picker: bool,
}

impl ChessyApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();

        let engine = UciClient::new(Path::new(&settings.engine_path)).ok();
        let piece_textures = PieceTextures::load(&settings.piece_set);

        let mut app = Self {
            game: Game::new(),
            mode: AppMode::Analyze,
            player_color: PieceColor::White,
            engine,
            engine_lines: vec![],
            eval_score: None,
            engine_running: false,
            settings,
            show_settings: false,
            flipped: false,
            interaction: BoardInteraction::default(),
            last_analyzed_fen: String::new(),
            waiting_for_bestmove: false,
            show_about: false,
            piece_textures,
            audio: AudioPlayer::new(),
            setup_board: Board::empty(),
            setup_turn: PieceColor::White,
            setup_palette: None,
            setup_error: None,
            review_run: None,
            review: None,
            clock: None,
            clock_plies: 0,
            now: 0.0,
            stale_searches: 0,
            waiting_for_hint: false,
            hint: None,
            resigned: None,
            resign_armed: false,
            applied_theme: None,
            pgn_games: vec![],
            pgn_current: None,
            pgn_filter: String::new(),
            show_game_picker: false,
        };

        if let Some(arg) = std::env::args().nth(1)
            && let Ok(games) = pgn::load_pgn(Path::new(&arg))
        {
            app.open_games(games);
        }

        app.start_analysis();
        app
    }

    fn board_style(&self) -> BoardStyle {
        BoardStyle {
            theme: BoardTheme::resolve(&self.settings.board_theme, self.settings.dark_theme),
            show_coordinates: self.settings.show_coordinates,
        }
    }

    fn reconnect_engine(&mut self) {
        self.engine = UciClient::new(Path::new(&self.settings.engine_path)).ok();
        self.engine_lines.clear();
        self.eval_score = None;
        self.engine_running = false;
        self.stale_searches = 0;
        self.waiting_for_bestmove = false;
        self.waiting_for_hint = false;
        if let Some(engine) = &self.engine {
            if self.settings.limit_strength {
                engine.set_elo(self.settings.engine_elo);
            } else {
                engine.disable_limit_strength();
            }
        }
    }

    fn start_analysis(&mut self) {
        self.review_run = None;
        let Some(engine) = &self.engine else { return };
        let fen = self.game.current_fen();
        if fen == self.last_analyzed_fen && self.engine_running {
            return;
        }
        let uci_moves = self.game.uci_moves_to_cursor();
        if self.engine_running {
            engine.stop();
            self.stale_searches += 1;
        }
        engine.set_position(&self.game.start_fen(), &uci_moves);
        engine.go_depth(self.settings.analysis_depth, self.settings.multipv);
        self.engine_running = true;
        self.last_analyzed_fen = fen;
        self.engine_lines.clear();
    }

    fn request_engine_move(&mut self) {
        let Some(engine) = &self.engine else { return };
        let uci_moves = self.game.uci_moves_to_cursor();
        if self.engine_running {
            engine.stop();
            self.stale_searches += 1;
        }
        engine.set_position(&self.game.start_fen(), &uci_moves);
        if self.settings.limit_strength {
            engine.set_elo(self.settings.engine_elo);
        } else {
            engine.disable_limit_strength();
        }
        match &self.clock {
            Some(clock) => engine.go_clock(
                clock.remaining_ms(PieceColor::White).max(1) as u64,
                clock.remaining_ms(PieceColor::Black).max(1) as u64,
                clock.increment_ms() as u64,
                clock.increment_ms() as u64,
                self.settings.movetime_ms,
            ),
            None => engine.go_movetime(self.settings.movetime_ms),
        }
        self.engine_running = true;
        self.waiting_for_bestmove = true;
        self.waiting_for_hint = false;
        self.engine_lines.clear();
    }

    fn start_review(&mut self) {
        let Some(engine) = &self.engine else { return };
        if self.game.moves.is_empty() {
            return;
        }
        engine.stop();
        engine.is_ready();
        self.review = None;
        self.review_run = Some(ReviewRun {
            start_fen: self.game.start_fen(),
            game: self.game.clone(),
            evals: vec![],
            latest: None,
            ready: false,
        });
        self.engine_running = true;
        self.engine_lines.clear();
    }

    fn cancel_review(&mut self) {
        if self.review_run.take().is_some() {
            if let Some(engine) = &self.engine {
                engine.stop();
            }
            self.engine_running = false;
            self.last_analyzed_fen.clear();
        }
    }

    fn review_advance(&mut self) {
        let Some(run) = &mut self.review_run else {
            return;
        };

        while let Some(pos) = run.game.positions.get(run.evals.len()) {
            match PositionEval::terminal(pos) {
                Some(eval) => run.evals.push(eval),
                None => break,
            }
        }

        let ply = run.evals.len();
        if ply < run.game.positions.len() {
            if let Some(engine) = &self.engine {
                run.latest = None;
                engine.set_position(&run.start_fen, &run.game.uci_moves(ply));
                engine.go_depth(REVIEW_DEPTH, 1);
            }
            return;
        }

        let run = self.review_run.take().expect("checked above");
        self.review = Review::build(&run.game, run.evals);
        self.engine_running = false;
        self.last_analyzed_fen.clear();
    }

    fn review_handle(&mut self, msg: EngineOutput) {
        let Some(run) = &mut self.review_run else {
            return;
        };
        match msg {
            EngineOutput::Ready => {
                run.ready = true;
                self.review_advance();
            }
            _ if !run.ready => {}
            EngineOutput::Info(info) => {
                if info.multipv == 1 && !info.pv.is_empty() {
                    run.latest = Some(info.score);
                }
            }
            EngineOutput::BestMove(best) => {
                let ply = run.evals.len();
                if let Some(pos) = run.game.positions.get(ply) {
                    let score = run.latest.take().unwrap_or(Score::Cp(0));
                    run.evals
                        .push(PositionEval::from_engine(&score, pos.turn(), best));
                }
                self.review_advance();
            }
        }
    }

    fn review_progress(&self) -> Option<f32> {
        self.review_run
            .as_ref()
            .map(|run| run.evals.len() as f32 / run.game.positions.len() as f32)
    }

    fn current_review(&self) -> Option<&Review> {
        self.review.as_ref().filter(|r| r.matches(&self.game))
    }

    fn stop_search(&mut self) {
        if self.engine_running
            && let Some(engine) = &self.engine
        {
            engine.stop();
            self.stale_searches += 1;
        }
        self.engine_running = false;
        self.waiting_for_bestmove = false;
        self.waiting_for_hint = false;
    }

    fn play_is_over(&self) -> bool {
        self.resigned.is_some() || self.flagged().is_some() || self.game.outcome().is_some()
    }

    fn players_turn(&self) -> bool {
        self.mode == AppMode::Play
            && !self.play_is_over()
            && self.game.at_end()
            && self.game.current_position().turn() == self.player_color
    }

    fn request_hint(&mut self) {
        if !self.players_turn() || self.engine.is_none() {
            return;
        }
        self.stop_search();
        let Some(engine) = &self.engine else { return };
        let fen = self.game.current_fen();
        engine.set_position(&fen, &self.game.uci_moves_to_cursor());
        engine.disable_limit_strength();
        engine.go_movetime(self.settings.movetime_ms.min(HINT_MOVETIME_MS));
        self.engine_running = true;
        self.waiting_for_hint = true;
        self.hint = None;
    }

    fn takeback(&mut self) {
        let player_to_move = self.game.positions[self.game.moves.len()].turn() == self.player_color;
        let plies = takeback_plies(self.game.moves.len(), player_to_move);
        if plies == 0 {
            return;
        }
        self.stop_search();
        self.game.take_back(plies);
        self.game
            .headers
            .insert("Result".to_string(), "*".to_string());
        self.resigned = None;
        self.resign_armed = false;
        self.hint = None;
        self.interaction.selected = None;
        self.interaction.pending_promotion = None;
    }

    fn resign(&mut self) {
        if self.mode != AppMode::Play || self.play_is_over() {
            return;
        }
        self.stop_search();
        self.resigned = Some(self.player_color);
        self.resign_armed = false;
        self.hint = None;
        let result = match self.player_color {
            PieceColor::White => "0-1",
            PieceColor::Black => "1-0",
        };
        self.game
            .headers
            .insert("Result".to_string(), result.to_string());
    }

    fn play_from_here(&mut self) {
        self.stop_search();
        self.game.truncate_to_cursor();
        self.game
            .headers
            .insert("Result".to_string(), "*".to_string());
        self.player_color = self.game.current_position().turn();
        self.flipped = self.player_color == PieceColor::Black;
        self.mode = AppMode::Play;
        self.resigned = None;
        self.resign_armed = false;
        self.hint = None;
        self.engine_lines.clear();
        self.interaction.selected = None;
    }

    fn poll_engine(&mut self) {
        let Some(engine) = &self.engine else { return };
        // Drain all pending output first so the engine borrow ends before we
        // mutate `self` while handling each message.
        let messages: Vec<EngineOutput> = std::iter::from_fn(|| engine.try_recv()).collect();

        for msg in messages {
            if self.review_run.is_some() {
                self.review_handle(msg);
                continue;
            }
            match msg {
                EngineOutput::Info(_) if self.stale_searches > 0 => {}
                EngineOutput::BestMove(_) if self.stale_searches > 0 => {
                    self.stale_searches -= 1;
                }
                EngineOutput::Info(info) => self.upsert_engine_line(info),
                EngineOutput::BestMove(mv_str) => {
                    self.engine_running = false;
                    if self.waiting_for_hint {
                        self.waiting_for_hint = false;
                        self.hint = uci_squares(&mv_str)
                            .map(|(from, to)| (from, to, self.game.current_fen()));
                    } else if self.waiting_for_bestmove && self.mode == AppMode::Play {
                        self.waiting_for_bestmove = false;
                        if self.game.make_uci_move(&mv_str).is_ok() {
                            self.play_nav_sound();
                        }
                    }
                }
                EngineOutput::Ready => {}
            }
        }
    }

    /// Store an engine line at its MultiPV slot, growing `engine_lines` with
    /// placeholders if a higher-numbered line arrives before the lower ones.
    fn upsert_engine_line(&mut self, mut info: EngineInfo) {
        if self.game.current_position().turn() != PieceColor::White {
            info.score = info.score.negated();
        }

        if info.multipv == 1 {
            self.eval_score = Some(info.score.clone());
        }

        let idx = info.multipv.saturating_sub(1) as usize;
        while self.engine_lines.len() <= idx {
            let slot = self.engine_lines.len() as u8 + 1;
            self.engine_lines.push(EngineInfo::placeholder(slot));
        }
        self.engine_lines[idx] = info;
    }

    fn reset_clock(&mut self) {
        self.clock = (self.mode == AppMode::Play)
            .then(|| clock::preset(&self.settings.time_control))
            .flatten()
            .map(Clock::new);
        self.clock_plies = self.game.moves.len();
    }

    fn update_clock(&mut self) {
        let now = self.now;
        let active = self.mode == AppMode::Play && !self.play_is_over();
        let Some(clock) = &mut self.clock else { return };

        let plies = self.game.moves.len();
        for before in self
            .game
            .positions
            .iter()
            .take(plies)
            .skip(self.clock_plies)
        {
            clock.on_move(before.turn(), now);
        }
        self.clock_plies = plies;

        if active {
            clock.tick(now);
        } else {
            clock.pause(now);
        }

        if let Some(loser) = clock.flagged()
            && self.game.headers.get("Result").map(String::as_str) == Some("*")
        {
            let result = match loser {
                PieceColor::White => "0-1",
                PieceColor::Black => "1-0",
            };
            self.game
                .headers
                .insert("Result".to_string(), result.to_string());
            if let Some(engine) = &self.engine {
                engine.stop();
            }
            self.waiting_for_bestmove = false;
        }
    }

    fn flagged(&self) -> Option<PieceColor> {
        self.clock.as_ref().and_then(Clock::flagged)
    }

    fn new_game(&mut self) {
        self.game = Game::new();
        self.reset_clock();
        self.engine_lines.clear();
        self.eval_score = None;
        self.interaction.selected = None;
        self.interaction.drag_from = None;
        self.interaction.drag_pos = None;
        self.stop_search();
        self.last_analyzed_fen = String::new();
        self.interaction.pending_promotion = None;
        self.resigned = None;
        self.resign_armed = false;
        self.hint = None;

        if self.mode == AppMode::Analyze {
            self.start_analysis();
        } else {
            // In play mode, if player is black, engine makes first move
            if self.player_color == PieceColor::Black {
                self.request_engine_move();
            }
        }
    }

    /// Replace the current game and reset analysis state, optionally switching
    /// to analyze mode.
    fn load_game(&mut self, game: Game, switch_to_analyze: bool) {
        self.game = game;
        self.clock = None;
        self.clock_plies = self.game.moves.len();
        self.engine_lines.clear();
        self.eval_score = None;
        self.interaction.selected = None;
        self.resigned = None;
        self.resign_armed = false;
        self.hint = None;
        if switch_to_analyze {
            self.mode = AppMode::Analyze;
        }
        self.start_analysis();
    }

    fn enter_setup(&mut self) {
        let pos = self.game.current_position();
        self.setup_board = pos.board().clone();
        self.setup_turn = pos.turn();
        self.setup_palette = None;
        self.setup_error = None;
        self.engine_lines.clear();
        self.mode = AppMode::Setup;
    }

    fn analyze_setup_position(&mut self) {
        let mut setup = Setup::empty();
        setup.board = self.setup_board.clone();
        setup.turn = self.setup_turn;
        setup.castling_rights = infer_castling(&self.setup_board);

        match setup.position::<shakmaty::Chess>(CastlingMode::Standard) {
            Ok(pos) => {
                self.setup_error = None;
                self.load_game(Game::from_position(pos), true);
            }

            Err(e) => {
                self.setup_error = Some(format!("Invalid position: {e}"));
            }
        }
    }

    fn open_pgn(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PGN files", &["pgn"])
            .pick_file()
            && let Ok(games) = pgn::load_pgn(&path)
        {
            self.open_games(games);
        }
    }

    fn open_games(&mut self, games: Vec<Game>) {
        let Some(first) = games.first().cloned() else {
            return;
        };
        self.show_game_picker = games.len() > 1;
        self.pgn_filter.clear();
        self.pgn_games = games;
        self.pgn_current = Some(0);
        self.load_game(first, true);
    }

    fn pick_game(&mut self, idx: usize) {
        if let Some(game) = self.pgn_games.get(idx).cloned() {
            self.pgn_current = Some(idx);
            self.load_game(game, true);
        }
    }

    fn save_pgn(&self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PGN files", &["pgn"])
            .save_file()
        {
            let _ = pgn::save_pgn(std::slice::from_ref(&self.game), &path);
        }
    }

    fn paste_fen(&mut self) {
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return;
        };
        let Ok(text) = clipboard.get_text() else {
            return;
        };
        let text = text.trim();

        // Accept either a raw FEN or a full PGN; the PGN path also switches to
        // analyze mode.
        if let Ok(game) = Game::from_fen(text) {
            self.load_game(game, false);
        } else {
            self.open_games(pgn::load_pgn_str(text));
        }
    }

    fn copy_pgn(&self) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(self.game.to_pgn());
        }
    }

    fn copy_fen(&self) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(self.game.current_fen());
        }
    }

    fn handle_keyboard(&mut self, ctx: &Context) {
        let (copy_pgn, new_game, open_pgn, save_pgn) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&SHORTCUT_COPY_PGN),
                i.consume_shortcut(&SHORTCUT_NEW_GAME),
                i.consume_shortcut(&SHORTCUT_OPEN_PGN),
                i.consume_shortcut(&SHORTCUT_SAVE_PGN),
            )
        });

        if copy_pgn {
            self.copy_pgn();
        }

        if new_game {
            self.new_game();
        }

        if open_pgn {
            self.open_pgn();
        }

        if save_pgn {
            self.save_pgn();
        }

        ctx.input(|i| {
            if i.key_pressed(Key::ArrowLeft) {
                self.game.go_back();
                self.interaction.selected = None;
                self.interaction.drag_from = None;
                self.play_nav_sound();
            }

            if i.key_pressed(Key::ArrowRight) {
                self.game.go_forward();
                self.interaction.selected = None;
                self.interaction.drag_from = None;
                self.play_nav_sound();
            }

            if i.key_pressed(Key::Home) {
                self.game.go_to_start();
                self.interaction.selected = None;
            }

            if i.key_pressed(Key::End) {
                self.game.go_to_end();
                self.interaction.selected = None;
                self.play_nav_sound();
            }

            if i.key_pressed(Key::F) {
                self.flipped = !self.flipped;
            }
        });
    }

    fn play_move_sound(&self, event: SoundEvent) {
        let Some(audio) = &self.audio else { return };
        audio.play(game_end_sound(
            event,
            self.game.outcome(),
            (self.mode == AppMode::Play).then_some(self.player_color),
        ));
    }

    fn play_nav_sound(&self) {
        let cursor = self.game.cursor;
        if cursor == 0 {
            return;
        }
        let m = self.game.moves[cursor - 1];
        let event = sound_for_move(&m, self.game.current_position());
        self.play_move_sound(event);
    }

    fn show_menu_bar(&mut self, ui: &mut Ui) {
        let weak = Palette::new(self.settings.dark_theme).text_weak;
        let ctx = ui.ctx().clone();
        let menu_item = |ui: &mut Ui, label: &str, shortcut: Option<&egui::KeyboardShortcut>| {
            let mut button = egui::Button::new(label);
            if let Some(shortcut) = shortcut {
                button = button.shortcut_text(ctx.format_shortcut(shortcut));
            }
            ui.add(button).clicked()
        };

        let section = |ui: &mut Ui, label: &str| {
            ui.label(RichText::new(label).small().color(weak));
        };

        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if menu_item(ui, "New Game", Some(&SHORTCUT_NEW_GAME)) {
                    self.new_game();
                    ui.close();
                }
                ui.separator();
                if menu_item(ui, "Open PGN…", Some(&SHORTCUT_OPEN_PGN)) {
                    self.open_pgn();
                    ui.close();
                }
                if menu_item(ui, "Save PGN…", Some(&SHORTCUT_SAVE_PGN)) {
                    self.save_pgn();
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.pgn_games.len() > 1,
                        egui::Button::new(format!("Games in File ({})…", self.pgn_games.len())),
                    )
                    .clicked()
                {
                    self.show_game_picker = true;
                    ui.close();
                }
                ui.separator();
                if menu_item(ui, "Copy FEN", None) {
                    self.copy_fen();
                    ui.close();
                }
                if menu_item(ui, "Copy PGN", Some(&SHORTCUT_COPY_PGN)) {
                    self.copy_pgn();
                    ui.close();
                }
                if menu_item(ui, "Paste FEN / PGN", None) {
                    self.paste_fen();
                    ui.close();
                }
            });

            ui.menu_button("Game", |ui| {
                section(ui, "Mode");
                if ui
                    .selectable_label(self.mode == AppMode::Analyze, "Analyze")
                    .clicked()
                {
                    self.stop_search();
                    self.mode = AppMode::Analyze;
                    self.last_analyzed_fen.clear();
                    self.start_analysis();
                    ui.close();
                }
                if ui
                    .selectable_label(self.mode == AppMode::Play, "Play vs Engine")
                    .clicked()
                {
                    self.stop_search();
                    self.mode = AppMode::Play;
                    self.engine_lines.clear();
                    if self.game.moves.is_empty() {
                        self.reset_clock();
                    }
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.mode != AppMode::Setup && !self.game.is_game_over(),
                        egui::Button::new("Play from Current Position"),
                    )
                    .on_hover_text("Play the engine from this position, as the side to move")
                    .clicked()
                {
                    self.play_from_here();
                    ui.close();
                }
                if ui
                    .selectable_label(self.mode == AppMode::Setup, "Set Up Position")
                    .clicked()
                {
                    self.enter_setup();
                    ui.close();
                }
                ui.separator();
                section(ui, "Play as");
                if ui
                    .selectable_label(self.player_color == PieceColor::White, "White")
                    .clicked()
                {
                    self.player_color = PieceColor::White;
                    self.flipped = false;
                    ui.close();
                }
                if ui
                    .selectable_label(self.player_color == PieceColor::Black, "Black")
                    .clicked()
                {
                    self.player_color = PieceColor::Black;
                    self.flipped = true;
                    ui.close();
                }
                ui.separator();
                section(ui, "Time control (next game)");
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(190.0);
                    for (label, _) in clock::PRESETS {
                        if ui
                            .selectable_label(self.settings.time_control == *label, *label)
                            .clicked()
                        {
                            self.settings.time_control = label.to_string();
                            self.settings.save();
                            if self.game.moves.is_empty() {
                                self.reset_clock();
                            }
                        }
                    }
                });
            });

            ui.menu_button("View", |ui| {
                if ui
                    .add(egui::Button::new("Flip Board").shortcut_text("F"))
                    .clicked()
                {
                    self.flipped = !self.flipped;
                    ui.close();
                }
                let mut engine_arrow = self.settings.engine_arrow;
                if ui
                    .checkbox(&mut engine_arrow, "Best-Move Arrow")
                    .on_hover_text("Show the engine's first choice on the board while analyzing")
                    .changed()
                {
                    self.settings.engine_arrow = engine_arrow;
                    self.settings.save();
                }
                let mut animate = self.settings.animate_moves;
                if ui
                    .checkbox(&mut animate, "Animate Moves")
                    .on_hover_text("Slide pieces between squares")
                    .changed()
                {
                    self.settings.animate_moves = animate;
                    self.settings.save();
                }
                ui.separator();
                section(ui, "Theme");
                if ui
                    .selectable_label(self.settings.dark_theme, "Dark")
                    .clicked()
                {
                    self.settings.dark_theme = true;
                    self.settings.save();
                    ui.close();
                }
                if ui
                    .selectable_label(!self.settings.dark_theme, "Light")
                    .clicked()
                {
                    self.settings.dark_theme = false;
                    self.settings.save();
                    ui.close();
                }
            });

            ui.menu_button("Engine", |ui| {
                if menu_item(ui, "Settings…", None) {
                    self.show_settings = true;
                    ui.close();
                }
                ui.separator();
                let (dot_color, engine_status) = if self.engine.is_some() {
                    if self.engine_running {
                        (Color32::from_rgb(230, 180, 40), "Analyzing")
                    } else {
                        (Color32::from_rgb(70, 180, 70), "Ready")
                    }
                } else {
                    (Color32::from_rgb(200, 60, 60), "Not connected")
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").small().color(dot_color));
                    ui.label(RichText::new(engine_status).small().color(weak));
                });

                if self.engine.is_none() {
                    ui.label(
                        RichText::new("Set the Stockfish path in Settings")
                            .small()
                            .color(weak),
                    );
                }
            });

            ui.menu_button("Help", |ui| {
                if menu_item(ui, "About Chessy", None) {
                    self.show_about = true;
                    ui.close();
                }
                ui.separator();
                section(ui, "Shortcuts");
                ui.label(RichText::new("← / →   navigate moves").small().color(weak));
                ui.label(
                    RichText::new("Home / End   start / end")
                        .small()
                        .color(weak),
                );

                ui.label(RichText::new("F   flip board").small().color(weak));
                ui.label(
                    RichText::new("Right-drag   draw arrow (Shift, Ctrl, Alt: colours)")
                        .small()
                        .color(weak),
                );
                ui.label(
                    RichText::new("Right-click   mark square")
                        .small()
                        .color(weak),
                );
            });

            // Mode + engine status indicator on the right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mode_str = match self.mode {
                    AppMode::Analyze => "Analysis",
                    AppMode::Play => match self.player_color {
                        PieceColor::White => "Play · White",
                        PieceColor::Black => "Play · Black",
                    },
                    AppMode::Setup => "Setup",
                };
                ui.label(RichText::new(mode_str).small().color(weak));
                if self.engine.is_none() {
                    ui.label(
                        RichText::new("engine offline")
                            .small()
                            .color(Color32::from_rgb(200, 60, 60)),
                    );
                }
            });
        });
    }

    fn show_settings_window(&mut self, ctx: &Context) {
        let mut engine_path = self.settings.engine_path.clone();
        let mut limit_strength = self.settings.limit_strength;
        let mut engine_elo = self.settings.engine_elo;
        let mut analysis_depth = self.settings.analysis_depth;
        let mut movetime_ms = self.settings.movetime_ms;
        let mut multipv = self.settings.multipv;
        let mut piece_set = self.settings.piece_set.clone();
        let mut board_theme = self.settings.board_theme.clone();
        let mut show_coordinates = self.settings.show_coordinates;
        let piece_sets = available_piece_sets();
        let mut close = false;
        let mut apply = false;
        let palette = Palette::new(self.settings.dark_theme);

        egui::Window::new("Settings")
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                settings_section(ui, "Engine", &palette);
                egui::Grid::new("settings_engine")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Path");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut engine_path).desired_width(240.0),
                            );
                            if ui.button("Browse…").clicked()
                                && let Some(path) = rfd::FileDialog::new().pick_file()
                            {
                                engine_path = path.to_string_lossy().to_string();
                            }
                        });
                        ui.end_row();
                    });

                settings_section(ui, "Strength", &palette);
                egui::Grid::new("settings_strength")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Limit strength");
                        ui.checkbox(&mut limit_strength, "");
                        ui.end_row();

                        ui.label("Preset");
                        ui.horizontal(|ui| {
                            for &(label, elo) in SKILL_PRESETS {
                                let selected = limit_strength && engine_elo == elo;
                                if ui.selectable_label(selected, label).clicked() {
                                    engine_elo = elo;
                                    limit_strength = true;
                                    apply = true;
                                }
                            }
                        });
                        ui.end_row();

                        if limit_strength {
                            ui.label("ELO");
                            ui.add(egui::Slider::new(&mut engine_elo, 800..=3200));
                            ui.end_row();
                        }
                    });

                settings_section(ui, "Analysis", &palette);
                egui::Grid::new("settings_analysis")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Depth");
                        ui.add(egui::Slider::new(&mut analysis_depth, 10..=30));
                        ui.end_row();

                        ui.label("Move time (ms)");
                        ui.add(egui::Slider::new(&mut movetime_ms, 500..=10000));
                        ui.end_row();

                        ui.label("Lines (Multi-PV)");
                        ui.add(egui::Slider::new(&mut multipv, 1..=4));
                        ui.end_row();
                    });

                settings_section(ui, "Appearance", &palette);
                egui::Grid::new("settings_appearance")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Pieces");
                        if piece_sets.is_empty() {
                            ui.label(
                                egui::RichText::new("No piece sets found in assets/pieces/")
                                    .color(palette.text_weak)
                                    .italics(),
                            );
                        } else {
                            ui.horizontal(|ui| {
                                for set in &piece_sets {
                                    if ui
                                        .selectable_label(piece_set == *set, set.as_str())
                                        .clicked()
                                    {
                                        piece_set = set.clone();
                                    }
                                }
                            });
                        }
                        ui.end_row();

                        ui.label("Board");
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .selectable_label(board_theme.is_empty(), "Auto")
                                .on_hover_text(
                                    "Green with the dark interface, brown with the light one",
                                )
                                .clicked()
                            {
                                board_theme.clear();
                            }
                            for theme in BOARD_THEMES {
                                let selected = board_theme.eq_ignore_ascii_case(theme.name);
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(20.0, 10.0),
                                    egui::Sense::hover(),
                                );
                                let (left, right) = rect.split_left_right_at_fraction(0.5);
                                ui.painter().rect_filled(left, 0.0, theme.light);
                                ui.painter().rect_filled(right, 0.0, theme.dark);
                                if ui.selectable_label(selected, theme.name).clicked() {
                                    board_theme = theme.name.to_string();
                                }
                            }
                        });
                        ui.end_row();

                        ui.label("Coordinates");
                        ui.checkbox(&mut show_coordinates, "Show");
                        ui.end_row();
                    });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Apply & Reconnect").clicked() {
                        apply = true;
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });

        self.settings.engine_path = engine_path;
        self.settings.limit_strength = limit_strength;
        self.settings.engine_elo = engine_elo;
        self.settings.analysis_depth = analysis_depth;
        self.settings.movetime_ms = movetime_ms;
        self.settings.multipv = multipv;
        self.settings.piece_set = piece_set;
        self.settings.board_theme = board_theme;
        self.settings.show_coordinates = show_coordinates;

        if close {
            self.show_settings = false;
        }
        if apply {
            self.settings.save();
            self.reconnect_engine();
            self.start_analysis();
            self.piece_textures = PieceTextures::load(&self.settings.piece_set);
        }
    }

    fn show_about_window(&mut self, ctx: &Context) {
        let mut open = self.show_about;
        egui::Window::new("About Chessy")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Chessy");
                    ui.label("A chess GUI built with Rust + egui + Stockfish");
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label("← / → — navigate moves");
                    ui.label("F — flip board");
                    ui.label("Home / End — go to start / end");
                });
            });
        self.show_about = open;
    }

    fn show_review_panel(&mut self, ui: &mut Ui, text_col: Color32) {
        let weak = Palette::new(self.settings.dark_theme).text_weak;
        if let Some(progress) = self.review_progress() {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    self.cancel_review();
                }
                ui.add(
                    egui::ProgressBar::new(progress)
                        .text(format!("Reviewing… {:.0}%", progress * 100.0)),
                );
            });
            return;
        }

        let Some(review) = self.current_review() else {
            ui.add_space(2.0);
            let enabled = self.engine.is_some() && !self.game.moves.is_empty();
            if ui
                .add_enabled(enabled, egui::Button::new("Review Game"))
                .on_hover_text("Have the engine judge every move of this game")
                .on_disabled_hover_text("Needs an engine and at least one move")
                .clicked()
            {
                self.start_review();
            }
            return;
        };

        ui.add_space(4.0);
        let gray = weak;
        egui::Grid::new("review_summary")
            .num_columns(5)
            .spacing([10.0, 2.0])
            .show(ui, |ui| {
                for title in ["", "Accuracy", "?!", "?", "??"] {
                    ui.label(RichText::new(title).small().color(gray));
                }
                ui.end_row();
                for (name, color) in [("White", PieceColor::White), ("Black", PieceColor::Black)] {
                    let s = review.summary(color);
                    ui.label(RichText::new(name).small().color(text_col));
                    ui.label(
                        RichText::new(format!("{:.1}%", s.accuracy))
                            .strong()
                            .color(text_col),
                    );
                    for n in [s.inaccuracies, s.mistakes, s.blunders] {
                        ui.label(RichText::new(n.to_string()).color(text_col));
                    }
                    ui.end_row();
                }
            });

        ui.add_space(4.0);
        let jump = show_eval_graph(ui, &review.white_win(), &review.classes(), self.game.cursor);
        if let Some(idx) = jump
            && idx != self.game.cursor
        {
            self.game.go_to(idx);
            self.interaction.selected = None;
        }
    }

    fn show_play_controls(&mut self, ui: &mut Ui) {
        let weak = Palette::new(self.settings.dark_theme).text_weak;
        let player_to_move = self.game.positions[self.game.moves.len()].turn() == self.player_color;
        let can_take_back = takeback_plies(self.game.moves.len(), player_to_move) > 0;
        let can_act = self.players_turn();

        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_take_back, egui::Button::new("Takeback"))
                .on_hover_text("Undo your last move")
                .clicked()
            {
                self.takeback();
            }

            if ui
                .add_enabled(
                    can_act && self.engine.is_some() && !self.waiting_for_hint,
                    egui::Button::new("Hint"),
                )
                .on_hover_text("Show the engine's suggestion for your move")
                .clicked()
            {
                self.request_hint();
            }

            if self.play_is_over() {
                if ui.button("New Game").clicked() {
                    self.new_game();
                }
            } else if self.resign_armed {
                if ui
                    .button(RichText::new("Confirm resign").color(Color32::from_rgb(220, 80, 80)))
                    .clicked()
                {
                    self.resign();
                }
                if ui.button("Cancel").clicked() {
                    self.resign_armed = false;
                }
            } else if ui.button("Resign").clicked() {
                self.resign_armed = true;
            }
        });

        let status = if self.waiting_for_hint {
            Some("Finding a hint…")
        } else if self.waiting_for_bestmove {
            Some("Engine is thinking…")
        } else if self.engine.is_none() {
            Some("Engine offline: set its path in Engine → Settings")
        } else {
            None
        };
        if let Some(text) = status {
            ui.label(RichText::new(text).small().italics().color(weak));
        }
    }

    fn show_setup_panel(&mut self, ui: &mut Ui) {
        let weak = Palette::new(self.settings.dark_theme).text_weak;
        ui.label(RichText::new("Set Up Position").strong());
        ui.add_space(6.0);

        ui.label(RichText::new("Side to move").small().color(weak));

        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.setup_turn == PieceColor::White, "White")
                .clicked()
            {
                self.setup_turn = PieceColor::White;
            }

            if ui
                .selectable_label(self.setup_turn == PieceColor::Black, "Black")
                .clicked()
            {
                self.setup_turn = PieceColor::Black;
            }
        });

        ui.add_space(8.0);

        ui.label(
            RichText::new("Click a piece, then a square")
                .small()
                .color(weak),
        );

        let roles = [
            Role::King,
            Role::Queen,
            Role::Rook,
            Role::Bishop,
            Role::Knight,
            Role::Pawn,
        ];

        for color in [PieceColor::White, PieceColor::Black] {
            ui.horizontal(|ui| {
                for role in roles {
                    let piece = Piece { color, role };
                    let selected = self.setup_palette == Some(piece);
                    let size_px = 30.0 * ui.ctx().pixels_per_point();
                    if let Some(tex) = self.piece_textures.get(ui.ctx(), piece, size_px) {
                        let img = egui::Image::from_texture(egui::load::SizedTexture::new(
                            tex.id(),
                            egui::vec2(30.0, 30.0),
                        ));
                        if ui
                            .add(egui::Button::image(img).selected(selected))
                            .clicked()
                        {
                            self.setup_palette = Some(piece);
                        }
                    }
                }
            });
        }

        ui.add_space(4.0);

        if ui
            .selectable_label(self.setup_palette.is_none(), "Erase (or right-click)")
            .clicked()
        {
            self.setup_palette = None;
        }

        ui.add_space(10.0);
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Clear board").clicked() {
                self.setup_board = Board::empty();
                self.setup_error = None;
            }
            if ui.button("Start position").clicked() {
                self.setup_board = Board::new();
                self.setup_turn = PieceColor::White;
                self.setup_error = None;
            }
        });

        ui.add_space(6.0);

        if ui
            .button(RichText::new("Analyze Position").strong())
            .clicked()
        {
            self.analyze_setup_position();
        }

        if let Some(err) = &self.setup_error {
            ui.add_space(6.0);
            ui.label(RichText::new(err).color(Color32::from_rgb(220, 80, 80)));
        }
    }

    fn show_promotion_dialog(&mut self, ctx: &Context) {
        let weak = Palette::new(self.settings.dark_theme).text_weak;
        let Some((from, to)) = self.interaction.pending_promotion else {
            return;
        };
        let mut chosen_role: Option<Role> = None;

        let color = self.game.current_position().turn();
        let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));

        egui::Window::new("Promote Pawn")
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for role in [Role::Queen, Role::Rook, Role::Bishop, Role::Knight] {
                        let name = match role {
                            Role::Queen => "Queen",
                            Role::Rook => "Rook",
                            Role::Bishop => "Bishop",
                            Role::Knight => "Knight",
                            _ => unreachable!(),
                        };
                        let piece = Piece { color, role };
                        let size_px = 56.0 * ui.ctx().pixels_per_point();
                        let clicked = match self.piece_textures.get(ui.ctx(), piece, size_px) {
                            Some(tex) => {
                                let img = egui::Image::from_texture(egui::load::SizedTexture::new(
                                    tex.id(),
                                    egui::vec2(56.0, 56.0),
                                ));
                                ui.add(egui::Button::image(img))
                                    .on_hover_text(name)
                                    .clicked()
                            }
                            None => ui.button(name).clicked(),
                        };
                        if clicked {
                            chosen_role = Some(role);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                    ui.label(RichText::new("Esc").small().color(weak));
                });
            });

        if cancelled && chosen_role.is_none() {
            self.interaction.pending_promotion = None;
            return;
        }

        if let Some(role) = chosen_role {
            if let Some(event) = try_make_promotion_move(&mut self.game, from, to, role) {
                self.play_move_sound(event);
            }
            self.interaction.pending_promotion = None;
        }
    }
}

impl eframe::App for ChessyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.now = ctx.input(|i| i.time);
        self.poll_engine();
        self.handle_keyboard(&ctx);
        self.update_clock();

        // Trigger engine move if it's the engine's turn in play mode
        if self.mode == AppMode::Play
            && !self.waiting_for_bestmove
            && !self.play_is_over()
            && self.game.at_end()
            && self.game.current_position().turn() != self.player_color
        {
            self.request_engine_move();
        }

        // Restart analysis when cursor moves in analyze mode
        if self.review_run.is_some() && self.mode != AppMode::Analyze {
            self.cancel_review();
        }
        if self.mode == AppMode::Analyze && self.review_run.is_none() {
            let current_fen = self.game.current_fen();
            if current_fen != self.last_analyzed_fen {
                self.start_analysis();
            }
        }

        if self.applied_theme != Some(self.settings.dark_theme) {
            theme::apply(&ctx, self.settings.dark_theme);
            self.applied_theme = Some(self.settings.dark_theme);
        }
        let palette = Palette::new(self.settings.dark_theme);

        // Settings, about, and promotion dialog windows
        if self.show_settings {
            self.show_settings_window(&ctx);
        }
        if self.show_about {
            self.show_about_window(&ctx);
        }
        if self.interaction.pending_promotion.is_some() {
            self.show_promotion_dialog(&ctx);
        }
        if self.show_game_picker {
            let picked = show_game_picker(
                &ctx,
                &self.pgn_games,
                &mut self.pgn_filter,
                &mut self.show_game_picker,
                self.pgn_current,
            );
            if let Some(idx) = picked {
                self.pick_game(idx);
            }
        }

        egui::Panel::top("menu_bar").show_inside(ui, |ui| {
            self.show_menu_bar(ui);
        });

        let panel_frame = egui::Frame::new()
            .fill(palette.panel)
            .inner_margin(egui::Margin::same(12));
        egui::Panel::right("right_panel")
            .default_size(288.0)
            .min_size(220.0)
            .resizable(false)
            .frame(panel_frame)
            .show_inside(ui, |ui| {
                if self.mode == AppMode::Setup {
                    self.show_setup_panel(ui);
                    return;
                }

                // Game info
                let white = self
                    .game
                    .headers
                    .get("White")
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                let black = self
                    .game
                    .headers
                    .get("Black")
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                let text_col = palette.text;

                let clock = self.clock.clone();
                let start_board = self.game.start_position().board();
                let board = self.game.current_position().board();
                let white_lost = captured_pieces(start_board, board, PieceColor::White);
                let black_lost = captured_pieces(start_board, board, PieceColor::Black);
                let balance = material_balance(board);

                for (name, side, lost, advantage) in [
                    (black, PieceColor::Black, &white_lost, -balance),
                    (white, PieceColor::White, &black_lost, balance),
                ] {
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
                        let fill = if side == PieceColor::White {
                            Color32::WHITE
                        } else {
                            Color32::from_gray(25)
                        };
                        ui.painter().circle(
                            dot.center(),
                            5.5,
                            fill,
                            egui::Stroke::new(1.0_f32, palette.text_weak),
                        );
                        ui.label(RichText::new(name).color(text_col).strong().size(15.0));
                        show_clock(ui, clock.as_ref(), side, text_col);
                    });
                    if lost.is_empty() && advantage <= 0 {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        show_captured(ui, &self.piece_textures, !side, lost, advantage, &palette);
                    });
                }

                if let Some(opening) = OpeningBook::builtin().lookup(&self.game) {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(&opening.eco)
                                .small()
                                .strong()
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(&opening.name)
                                .small()
                                .color(palette.text_weak),
                        );
                    })
                    .response
                    .on_hover_text("Opening");
                }

                ui.separator();

                // Eval bar + analysis lines
                if self.mode == AppMode::Analyze && !self.engine_lines.is_empty() {
                    let pos = self.game.current_position().clone();
                    show_analysis_panel(ui, &self.engine_lines, &pos, self.settings.dark_theme);
                }

                if self.mode == AppMode::Analyze {
                    self.show_review_panel(ui, text_col);
                }

                if self.mode == AppMode::Play {
                    self.show_play_controls(ui);
                }

                ui.separator();

                // Navigation buttons
                ui.horizontal(|ui| {
                    if ui.button("|◀").clicked() {
                        self.game.go_to_start();
                        self.interaction.selected = None;
                    }
                    if ui.button("◀").clicked() {
                        self.game.go_back();
                        self.interaction.selected = None;
                        self.play_nav_sound();
                    }
                    if ui.button("▶").clicked() {
                        self.game.go_forward();
                        self.interaction.selected = None;
                        self.play_nav_sound();
                    }
                    if ui.button("▶|").clicked() {
                        self.game.go_to_end();
                        self.interaction.selected = None;
                        self.play_nav_sound();
                    }
                    if ui.button("⟳").on_hover_text("Flip board").clicked() {
                        self.flipped = !self.flipped;
                    }
                });

                ui.separator();

                // Move list
                let available = ui.available_size();
                ui.allocate_ui(available, |ui| {
                    let classes = self.current_review().map(Review::classes);
                    if show_moves_panel(ui, &mut self.game, &palette, classes.as_deref()) {
                        self.play_nav_sound();
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.background))
            .show_inside(ui, |ui| {
                // Status message (not shown while editing a position)
                let outcome = match (self.resigned, self.flagged()) {
                    (Some(PieceColor::White), _) => Some("White resigned · Black wins".to_string()),
                    (Some(PieceColor::Black), _) => Some("Black resigned · White wins".to_string()),
                    (None, Some(PieceColor::White)) => {
                        Some("White lost on time · Black wins".to_string())
                    }
                    (None, Some(PieceColor::Black)) => {
                        Some("Black lost on time · White wins".to_string())
                    }
                    (None, None) => self.game.outcome_string(),
                };
                if self.mode != AppMode::Setup
                    && let Some(outcome) = outcome
                {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(outcome)
                                .color(Color32::from_rgb(220, 180, 50))
                                .font(FontId::proportional(16.0))
                                .strong(),
                        );
                    });
                }

                let board_style = self.board_style();
                let available = ui.available_rect_before_wrap();
                let bar_space = if self.mode == AppMode::Setup {
                    0.0
                } else {
                    EVAL_BAR_WIDTH + EVAL_BAR_GAP
                };
                let board_size =
                    (available.width().min(available.height()) - 2.0 * BOARD_MARGIN - bar_space)
                        .max(80.0);
                let group_width = board_size + bar_space;
                let board_rect = egui::Rect::from_min_size(
                    egui::pos2(
                        available.center().x - group_width / 2.0 + bar_space,
                        available.center().y - board_size / 2.0,
                    ),
                    egui::Vec2::splat(board_size),
                );

                if self.mode == AppMode::Setup {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(board_rect), |ui| {
                        SetupBoardWidget::new(
                            &mut self.setup_board,
                            self.flipped,
                            board_style,
                            self.setup_palette,
                            &self.piece_textures,
                        )
                        .show(ui);
                    });

                    return;
                }

                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(board_rect.min.x - bar_space, board_rect.min.y),
                    egui::vec2(EVAL_BAR_WIDTH, board_size),
                );
                show_eval_bar(
                    ui,
                    bar_rect,
                    self.eval_score.as_ref(),
                    self.flipped,
                    &palette,
                );

                let is_interactive = match self.mode {
                    AppMode::Analyze => true,
                    AppMode::Play => {
                        !self.waiting_for_bestmove && !self.play_is_over() && self.game.at_end()
                    }
                    AppMode::Setup => unreachable!(),
                };

                let hint = self
                    .hint
                    .as_ref()
                    .filter(|(_, _, fen)| *fen == self.game.current_fen())
                    .map(|(from, to, _)| (*from, *to));

                let engine_arrow = (self.mode == AppMode::Analyze && self.settings.engine_arrow)
                    .then(|| self.engine_lines.first())
                    .flatten()
                    .and_then(|line| line.pv.first())
                    .and_then(|uci| uci_squares(uci));

                let mut move_sound = None;
                ui.scope_builder(egui::UiBuilder::new().max_rect(board_rect), |ui| {
                    move_sound = BoardWidget::new(
                        &mut self.game,
                        self.flipped,
                        board_style,
                        is_interactive,
                        &mut self.interaction,
                        &self.piece_textures,
                    )
                    .with_hint(hint)
                    .with_engine_arrow(engine_arrow)
                    .with_animation(self.settings.animate_moves)
                    .show(ui);
                });
                if let Some(event) = move_sound {
                    self.play_move_sound(event);
                }
            });

        // Request continuous repaint while engine is running
        if self.engine_running || self.interaction.drag_from.is_some() {
            ctx.request_repaint();
        }
        if self.clock.as_ref().is_some_and(|c| c.running().is_some()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

fn settings_section(ui: &mut Ui, title: &str, palette: &Palette) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().color(palette.accent));
    ui.add_space(2.0);
}

fn show_clock(ui: &mut Ui, clock: Option<&Clock>, color: PieceColor, text_col: Color32) {
    let Some(clock) = clock else { return };
    let ms = clock.remaining_ms(color);
    let running = clock.running() == Some(color);

    let (fg, bg) = if clock.flagged() == Some(color) || (running && ms < 10_000) {
        (Color32::WHITE, Color32::from_rgb(190, 50, 50))
    } else if running {
        (Color32::BLACK, Color32::from_rgb(235, 235, 235))
    } else {
        (text_col, Color32::TRANSPARENT)
    };

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        egui::Frame::new()
            .fill(bg)
            .corner_radius(3.0)
            .inner_margin(egui::Margin::symmetric(6, 1))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format_clock(ms))
                        .font(FontId::monospace(15.0))
                        .color(fg)
                        .strong(),
                );
            });
    });
}

/// The player's last move plus the engine's reply if it has answered; 0 before the player has moved.
fn takeback_plies(moves: usize, player_to_move: bool) -> usize {
    let wanted = if player_to_move { 2 } else { 1 };
    if moves >= wanted { wanted } else { 0 }
}

/// `player` is the human's colour when playing the engine.
fn game_end_sound(
    event: SoundEvent,
    outcome: Option<Outcome>,
    player: Option<PieceColor>,
) -> SoundEvent {
    match outcome {
        Some(Outcome::Checkmate { winner }) => match player {
            Some(color) if color != winner => SoundEvent::Defeat,
            _ => SoundEvent::Victory,
        },
        Some(o) if o.is_draw() => SoundEvent::Draw,
        _ => event,
    }
}

fn infer_castling(board: &Board) -> Bitboard {
    let mut rights = Bitboard::EMPTY;
    let has = |sq: Square, color: PieceColor, role: Role| {
        board.piece_at(sq) == Some(Piece { color, role })
    };

    if has(Square::E1, PieceColor::White, Role::King) {
        for corner in [Square::A1, Square::H1] {
            if has(corner, PieceColor::White, Role::Rook) {
                rights |= Bitboard::from_square(corner);
            }
        }
    }

    if has(Square::E8, PieceColor::Black, Role::King) {
        for corner in [Square::A8, Square::H8] {
            if has(corner, PieceColor::Black, Role::Rook) {
                rights |= Bitboard::from_square(corner);
            }
        }
    }

    rights
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELO_MIN: u32 = 800;
    const ELO_MAX: u32 = 3200;

    fn build_setup(board: Board, turn: PieceColor) -> Result<shakmaty::Chess, String> {
        let mut setup = Setup::empty();
        setup.castling_rights = infer_castling(&board);
        setup.board = board;
        setup.turn = turn;
        setup
            .position::<shakmaty::Chess>(CastlingMode::Standard)
            .map_err(|e| e.to_string())
    }

    fn place(board: &mut Board, sq: Square, color: PieceColor, role: Role) {
        board.set_piece_at(sq, Piece { color, role });
    }

    #[test]
    fn takeback_removes_own_move_and_engine_reply() {
        assert_eq!(takeback_plies(2, true), 2);
        assert_eq!(takeback_plies(7, true), 2);
        assert_eq!(takeback_plies(1, false), 1);
        assert_eq!(takeback_plies(6, false), 1);
    }

    #[test]
    fn takeback_is_unavailable_before_the_player_has_moved() {
        assert_eq!(takeback_plies(0, true), 0);
        assert_eq!(takeback_plies(1, true), 0);
        assert_eq!(takeback_plies(0, false), 0);
    }

    #[test]
    fn game_end_sound_depends_on_who_won() {
        let white_mates = Some(Outcome::Checkmate {
            winner: PieceColor::White,
        });
        let e = SoundEvent::Checkmate;
        assert_eq!(
            game_end_sound(e, white_mates, Some(PieceColor::White)),
            SoundEvent::Victory
        );
        assert_eq!(
            game_end_sound(e, white_mates, Some(PieceColor::Black)),
            SoundEvent::Defeat
        );
        assert_eq!(game_end_sound(e, white_mates, None), SoundEvent::Victory);
    }

    #[test]
    fn game_end_sound_for_draws_and_ordinary_moves() {
        assert_eq!(
            game_end_sound(
                SoundEvent::Capture,
                Some(Outcome::ThreefoldRepetition),
                Some(PieceColor::White)
            ),
            SoundEvent::Draw
        );
        assert_eq!(
            game_end_sound(SoundEvent::Move, Some(Outcome::FiftyMoveRule), None),
            SoundEvent::Draw
        );
        assert_eq!(
            game_end_sound(SoundEvent::Capture, None, None),
            SoundEvent::Capture
        );
    }

    #[test]
    fn infer_castling_only_when_king_and_rooks_home() {
        // Standard starting board grants all four rights.
        assert_eq!(infer_castling(&Board::new()), Bitboard::CORNERS);

        // King home but only one rook present -> only that corner.
        let mut board = Board::empty();
        place(&mut board, Square::E1, PieceColor::White, Role::King);
        place(&mut board, Square::H1, PieceColor::White, Role::Rook);
        assert_eq!(infer_castling(&board), Bitboard::from_square(Square::H1));

        // King off its home square -> no rights even with rooks in corners.
        let mut board = Board::empty();
        place(&mut board, Square::G1, PieceColor::White, Role::King);
        place(&mut board, Square::A1, PieceColor::White, Role::Rook);
        place(&mut board, Square::H1, PieceColor::White, Role::Rook);
        assert_eq!(infer_castling(&board), Bitboard::EMPTY);
    }

    #[test]
    fn build_setup_accepts_legal_position() {
        let mut board = Board::empty();
        place(&mut board, Square::E1, PieceColor::White, Role::King);
        place(&mut board, Square::E8, PieceColor::Black, Role::King);
        place(&mut board, Square::D1, PieceColor::White, Role::Queen);
        assert!(build_setup(board, PieceColor::White).is_ok());
    }

    #[test]
    fn build_setup_rejects_missing_king() {
        let mut board = Board::empty();
        place(&mut board, Square::E8, PieceColor::Black, Role::King);
        assert!(build_setup(board, PieceColor::White).is_err());
    }

    #[test]
    fn build_setup_rejects_pawn_on_back_rank() {
        let mut board = Board::empty();
        place(&mut board, Square::E1, PieceColor::White, Role::King);
        place(&mut board, Square::E8, PieceColor::Black, Role::King);
        place(&mut board, Square::A8, PieceColor::White, Role::Pawn);
        assert!(build_setup(board, PieceColor::White).is_err());
    }

    #[test]
    fn skill_presets_all_within_slider_range() {
        for &(label, elo) in SKILL_PRESETS {
            assert!(
                (ELO_MIN..=ELO_MAX).contains(&elo),
                "preset '{label}' ELO {elo} is outside slider range {ELO_MIN}..={ELO_MAX}"
            );
        }
    }

    #[test]
    fn skill_presets_elos_are_unique() {
        let elos: Vec<u32> = SKILL_PRESETS.iter().map(|&(_, e)| e).collect();
        let mut seen = std::collections::HashSet::new();
        for elo in &elos {
            assert!(seen.insert(elo), "duplicate ELO {elo} in SKILL_PRESETS");
        }
    }

    #[test]
    fn skill_presets_labels_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for &(label, _) in SKILL_PRESETS {
            assert!(
                seen.insert(label),
                "duplicate label '{label}' in SKILL_PRESETS"
            );
        }
    }

    #[test]
    fn skill_presets_sorted_ascending() {
        let elos: Vec<u32> = SKILL_PRESETS.iter().map(|&(_, e)| e).collect();
        let mut sorted = elos.clone();
        sorted.sort_unstable();
        assert_eq!(
            elos, sorted,
            "SKILL_PRESETS must be ordered by ascending ELO"
        );
    }

    #[test]
    fn skill_presets_expected_tiers() {
        let map: std::collections::HashMap<&str, u32> = SKILL_PRESETS.iter().copied().collect();
        assert_eq!(map.get("Beginner").copied(), Some(800));
        assert_eq!(map.get("Casual").copied(), Some(1200));
        assert_eq!(map.get("Intermediate").copied(), Some(1600));
        assert_eq!(map.get("Advanced").copied(), Some(2000));
        assert_eq!(map.get("Strong").copied(), Some(2400));
    }
}
