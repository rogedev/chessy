use std::path::Path;

use egui::{Color32, Context, FontId, Key, RichText, Ui};
use shakmaty::{Color as PieceColor, Piece, Position, Role};

use crate::{
    audio::AudioPlayer,
    chess::{Game, pgn},
    engine::{EngineInfo, EngineOutput, Score, UciClient},
    ui::{
        Settings,
        analysis::{show_analysis_panel, show_eval_bar},
        board::{
            BoardInteraction, BoardWidget, PieceTextures, available_piece_sets, piece_symbol,
            sound_for_move, try_make_promotion_move,
        },
        moves::show_moves_panel,
    },
};

#[derive(PartialEq, Clone, Copy)]
pub enum AppMode {
    Play,
    Analyze,
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
}

impl ChessyApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();

        let engine = UciClient::new(Path::new(&settings.engine_path)).ok();
        let piece_textures = PieceTextures::load(&_cc.egui_ctx, &settings.piece_set);

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
        };
        app.start_analysis();
        app
    }

    fn reconnect_engine(&mut self) {
        self.engine = UciClient::new(Path::new(&self.settings.engine_path)).ok();
        self.engine_lines.clear();
        self.eval_score = None;
        self.engine_running = false;
        if let Some(engine) = &self.engine {
            if self.settings.limit_strength {
                engine.set_elo(self.settings.engine_elo);
            } else {
                engine.disable_limit_strength();
            }
        }
    }

    fn start_analysis(&mut self) {
        let Some(engine) = &self.engine else { return };
        let fen = self.game.current_fen();
        if fen == self.last_analyzed_fen && self.engine_running {
            return;
        }
        let uci_moves = self.game.uci_moves_to_cursor();
        engine.stop();
        engine.set_position(&fen, &uci_moves);
        engine.go_depth(self.settings.analysis_depth, self.settings.multipv);
        self.engine_running = true;
        self.last_analyzed_fen = fen;
        self.engine_lines.clear();
    }

    fn request_engine_move(&mut self) {
        let Some(engine) = &self.engine else { return };
        let fen = self.game.current_fen();
        let uci_moves = self.game.uci_moves_to_cursor();
        engine.stop();
        engine.set_position(&fen, &uci_moves);
        if self.settings.limit_strength {
            engine.set_elo(self.settings.engine_elo);
        } else {
            engine.disable_limit_strength();
        }
        engine.go_movetime(self.settings.movetime_ms);
        self.engine_running = true;
        self.waiting_for_bestmove = true;
        self.engine_lines.clear();
    }

    fn poll_engine(&mut self) {
        let Some(engine) = &self.engine else { return };
        // Drain all pending output first so the engine borrow ends before we
        // mutate `self` while handling each message.
        let messages: Vec<EngineOutput> = std::iter::from_fn(|| engine.try_recv()).collect();

        for msg in messages {
            match msg {
                EngineOutput::Info(info) => self.upsert_engine_line(info),
                EngineOutput::BestMove(mv_str) => {
                    self.engine_running = false;
                    if self.waiting_for_bestmove && self.mode == AppMode::Play {
                        self.waiting_for_bestmove = false;
                        if self.game.make_uci_move(&mv_str).is_ok() {
                            if let Some(audio) = &self.audio {
                                let m = self.game.moves[self.game.cursor - 1];
                                let event = sound_for_move(&m, self.game.current_position());
                                audio.play(event);
                            }
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

    fn new_game(&mut self) {
        self.game = Game::new();
        self.engine_lines.clear();
        self.eval_score = None;
        self.interaction.selected = None;
        self.interaction.drag_from = None;
        self.interaction.drag_pos = None;
        self.waiting_for_bestmove = false;
        self.last_analyzed_fen = String::new();
        self.interaction.pending_promotion = None;

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
        self.engine_lines.clear();
        self.eval_score = None;
        self.interaction.selected = None;
        if switch_to_analyze {
            self.mode = AppMode::Analyze;
        }
        self.start_analysis();
    }

    fn open_pgn(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PGN files", &["pgn"])
            .pick_file()
            && let Ok(games) = pgn::load_pgn(&path)
            && let Some(first) = games.into_iter().next()
        {
            self.load_game(first, true);
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
        } else if let Some(game) = pgn::load_pgn_str(text).into_iter().next() {
            self.load_game(game, true);
        }
    }

    fn copy_fen(&self) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(self.game.current_fen());
        }
    }

    fn handle_keyboard(&mut self, ctx: &Context) {
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

    fn play_nav_sound(&self) {
        let cursor = self.game.cursor;
        if cursor == 0 {
            return;
        }
        let Some(audio) = &self.audio else { return };
        let m = self.game.moves[cursor - 1];
        let event = sound_for_move(&m, self.game.current_position());
        audio.play(event);
    }

    fn show_menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Game").clicked() {
                    self.new_game();
                    ui.close();
                }
                if ui.button("Open PGN…").clicked() {
                    self.open_pgn();
                    ui.close();
                }
                if ui.button("Save PGN…").clicked() {
                    self.save_pgn();
                    ui.close();
                }
                ui.separator();
                if ui.button("Paste FEN / PGN").clicked() {
                    self.paste_fen();
                    ui.close();
                }
                if ui.button("Copy FEN").clicked() {
                    self.copy_fen();
                    ui.close();
                }
            });

            ui.menu_button("Game", |ui| {
                ui.label(RichText::new("Mode").small().color(Color32::GRAY));
                if ui
                    .selectable_label(self.mode == AppMode::Analyze, "Analyze")
                    .clicked()
                {
                    self.mode = AppMode::Analyze;
                    self.waiting_for_bestmove = false;
                    self.start_analysis();
                    ui.close();
                }
                if ui
                    .selectable_label(self.mode == AppMode::Play, "Play vs Engine")
                    .clicked()
                {
                    self.mode = AppMode::Play;
                    self.engine_lines.clear();
                    ui.close();
                }
                ui.separator();
                if ui.button("Play as White").clicked() {
                    self.player_color = PieceColor::White;
                    self.flipped = false;
                    ui.close();
                }
                if ui.button("Play as Black").clicked() {
                    self.player_color = PieceColor::Black;
                    self.flipped = true;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui.button("Flip Board  [F]").clicked() {
                    self.flipped = !self.flipped;
                    ui.close();
                }
                ui.separator();
                if ui
                    .selectable_label(self.settings.dark_theme, "Dark Theme")
                    .clicked()
                {
                    self.settings.dark_theme = true;
                    self.settings.save();
                    apply_theme(ui.ctx(), true);
                    ui.close();
                }
                if ui
                    .selectable_label(!self.settings.dark_theme, "Light Theme")
                    .clicked()
                {
                    self.settings.dark_theme = false;
                    self.settings.save();
                    apply_theme(ui.ctx(), false);
                    ui.close();
                }
            });

            ui.menu_button("Engine", |ui| {
                if ui.button("Settings…").clicked() {
                    self.show_settings = true;
                    ui.close();
                }
                ui.separator();
                let engine_status = if self.engine.is_some() {
                    if self.engine_running {
                        "Running"
                    } else {
                        "Ready"
                    }
                } else {
                    "Not connected"
                };
                ui.label(
                    RichText::new(format!("Status: {}", engine_status))
                        .small()
                        .color(Color32::GRAY),
                );
            });

            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.show_about = true;
                    ui.close();
                }
                ui.separator();
                ui.label(
                    RichText::new("← → navigate  F flip  Home/End")
                        .small()
                        .color(Color32::GRAY),
                );
            });

            // Mode indicator on the right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mode_str = match self.mode {
                    AppMode::Analyze => "Analysis",
                    AppMode::Play => "Play",
                };
                ui.label(RichText::new(mode_str).small().color(Color32::GRAY));
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
        let piece_sets = available_piece_sets();
        let mut close = false;
        let mut apply = false;

        egui::Window::new("Settings")
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Engine path:");
                    ui.text_edit_singleline(&mut engine_path);
                    if ui.button("Browse…").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_file()
                    {
                        engine_path = path.to_string_lossy().to_string();
                    }
                });

                ui.separator();

                ui.checkbox(&mut limit_strength, "Limit strength");
                if limit_strength {
                    ui.horizontal(|ui| {
                        ui.label("ELO:");
                        ui.add(egui::Slider::new(&mut engine_elo, 800..=3200));
                    });
                }

                ui.horizontal(|ui| {
                    ui.label("Analysis depth:");
                    ui.add(egui::Slider::new(&mut analysis_depth, 10..=30));
                });

                ui.horizontal(|ui| {
                    ui.label("Move time (ms):");
                    ui.add(egui::Slider::new(&mut movetime_ms, 500..=10000));
                });

                ui.horizontal(|ui| {
                    ui.label("Multi-PV lines:");
                    ui.add(egui::Slider::new(&mut multipv, 1..=4));
                });

                ui.separator();

                ui.label("Piece style:");
                if piece_sets.is_empty() {
                    ui.label(
                        egui::RichText::new("No piece sets found in assets/pieces/")
                            .color(Color32::GRAY)
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

                ui.separator();

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

        if close {
            self.show_settings = false;
        }
        if apply {
            self.settings.save();
            self.reconnect_engine();
            self.start_analysis();
            self.piece_textures = PieceTextures::load(ctx, &self.settings.piece_set);
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
                    ui.add_space(8.0);
                    ui.label("← / → — navigate moves");
                    ui.label("F — flip board");
                    ui.label("Home / End — go to start / end");
                });
            });
        self.show_about = open;
    }

    fn show_promotion_dialog(&mut self, ctx: &Context) {
        let Some((from, to)) = self.interaction.pending_promotion else {
            return;
        };
        let turn_color = self.game.current_position().turn();
        let mut chosen_role: Option<Role> = None;

        egui::Window::new("Promote Pawn")
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Choose promotion piece:");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for role in [Role::Queen, Role::Rook, Role::Bishop, Role::Knight] {
                        let symbol = piece_symbol(Piece {
                            color: turn_color,
                            role,
                        });
                        if ui.button(RichText::new(symbol).size(40.0)).clicked() {
                            chosen_role = Some(role);
                        }
                    }
                });
            });

        if let Some(role) = chosen_role {
            if let Some(event) = try_make_promotion_move(&mut self.game, from, to, role) {
                if let Some(audio) = &self.audio {
                    audio.play(event);
                }
            }
            self.interaction.pending_promotion = None;
        }
    }
}

impl eframe::App for ChessyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.poll_engine();
        self.handle_keyboard(&ctx);

        // Trigger engine move if it's the engine's turn in play mode
        if self.mode == AppMode::Play
            && !self.waiting_for_bestmove
            && !self.game.is_game_over()
            && self.game.at_end()
            && self.game.current_position().turn() != self.player_color
        {
            self.request_engine_move();
        }

        // Restart analysis when cursor moves in analyze mode
        if self.mode == AppMode::Analyze {
            let current_fen = self.game.current_fen();
            if current_fen != self.last_analyzed_fen {
                self.start_analysis();
            }
        }

        apply_theme(&ctx, self.settings.dark_theme);

        let bg_color = if self.settings.dark_theme {
            Color32::from_rgb(30, 30, 30)
        } else {
            Color32::from_rgb(240, 240, 240)
        };

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

        egui::Panel::top("menu_bar").show_inside(ui, |ui| {
            self.show_menu_bar(ui);
        });

        egui::Panel::right("right_panel")
            .default_size(260.0)
            .min_size(200.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                let panel_bg = if self.settings.dark_theme {
                    Color32::from_rgb(38, 38, 38)
                } else {
                    Color32::from_rgb(248, 248, 248)
                };
                ui.painter().rect_filled(ui.max_rect(), 0.0, panel_bg);

                ui.add_space(4.0);

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
                let text_col = if self.settings.dark_theme {
                    Color32::from_rgb(220, 220, 220)
                } else {
                    Color32::from_rgb(30, 30, 30)
                };

                ui.horizontal(|ui| {
                    ui.label(RichText::new("♟ ").color(Color32::from_rgb(50, 50, 50)));
                    ui.label(RichText::new(black).color(text_col).strong());
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("♙ ").color(Color32::WHITE));
                    ui.label(RichText::new(white).color(text_col).strong());
                });

                ui.separator();

                // Eval bar + analysis lines
                let eval_ref = self.eval_score.as_ref();
                let white_turn = self.game.current_position().turn() == PieceColor::White;
                show_eval_bar(ui, eval_ref, white_turn);
                ui.add_space(4.0);

                if self.mode == AppMode::Analyze && !self.engine_lines.is_empty() {
                    let pos = self.game.current_position().clone();
                    show_analysis_panel(ui, &self.engine_lines, &pos, self.settings.dark_theme);
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
                    if show_moves_panel(ui, &mut self.game, self.settings.dark_theme) {
                        self.play_nav_sound();
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg_color))
            .show_inside(ui, |ui| {
                // Status message
                if let Some(outcome) = self.game.outcome_string() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(outcome)
                                .color(Color32::from_rgb(220, 180, 50))
                                .font(FontId::proportional(16.0))
                                .strong(),
                        );
                    });
                }

                let is_interactive = match self.mode {
                    AppMode::Analyze => true,
                    AppMode::Play => !self.waiting_for_bestmove,
                };

                let available = ui.available_rect_before_wrap();
                let board_size = available.width().min(available.height());
                let board_rect =
                    egui::Rect::from_center_size(available.center(), egui::Vec2::splat(board_size));

                ui.scope_builder(egui::UiBuilder::new().max_rect(board_rect), |ui| {
                    if let Some(event) = BoardWidget::new(
                        &mut self.game,
                        self.flipped,
                        self.settings.dark_theme,
                        is_interactive,
                        &mut self.interaction,
                        &self.piece_textures,
                    )
                    .show(ui)
                    {
                        if let Some(audio) = &self.audio {
                            audio.play(event);
                        }
                    }
                });
            });

        // Request continuous repaint while engine is running
        if self.engine_running || self.interaction.drag_from.is_some() {
            ctx.request_repaint();
        }
    }
}

fn apply_theme(ctx: &Context, dark: bool) {
    if dark {
        ctx.set_visuals(egui::Visuals::dark());
    } else {
        ctx.set_visuals(egui::Visuals::light());
    }
}
