use egui::{Color32, Context, RichText, ScrollArea};

use crate::chess::Game;

/// One row of the game picker, extracted from a game's PGN headers.
#[derive(Debug, PartialEq)]
pub struct GameSummary {
    pub white: String,
    pub black: String,
    pub result: String,
    pub date: String,
    pub event: String,
    pub moves: usize,
}

impl GameSummary {
    pub fn of(game: &Game) -> Self {
        let tag = |key: &str, fallback: &str| {
            game.headers
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback)
                .to_string()
        };
        Self {
            white: tag("White", "?"),
            black: tag("Black", "?"),
            result: tag("Result", "*"),
            date: tag("Date", ""),
            event: tag("Event", ""),
            // Full moves, rounded up so a game ending on White's move counts.
            moves: game.moves.len().div_ceil(2),
        }
    }

    /// Case-insensitive match against players, event and date. An empty
    /// filter matches everything.
    pub fn matches(&self, filter: &str) -> bool {
        let needle = filter.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        [&self.white, &self.black, &self.event, &self.date]
            .iter()
            .any(|field| field.to_lowercase().contains(&needle))
    }
}

/// Show the list of games loaded from a PGN file. Returns the index of the
/// game the user picked, if any.
pub fn show_game_picker(
    ctx: &Context,
    games: &[Game],
    filter: &mut String,
    open: &mut bool,
    current: Option<usize>,
) -> Option<usize> {
    let mut picked = None;

    egui::Window::new(format!("Games ({})", games.len()))
        .id(egui::Id::new("game_picker"))
        .open(open)
        .collapsible(false)
        .default_size([520.0, 420.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text("player, event or date")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.separator();

            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    egui::Grid::new("game_picker_grid")
                        .striped(true)
                        .num_columns(6)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            for title in ["#", "White", "Black", "Result", "Moves", "Date / Event"]
                            {
                                ui.label(RichText::new(title).small().color(Color32::GRAY));
                            }
                            ui.end_row();

                            let mut shown = 0;
                            for (idx, game) in games.iter().enumerate() {
                                let summary = GameSummary::of(game);
                                if !summary.matches(filter) {
                                    continue;
                                }
                                shown += 1;

                                ui.label(RichText::new((idx + 1).to_string()).color(Color32::GRAY));
                                let white = RichText::new(&summary.white).strong();
                                if ui
                                    .selectable_label(current == Some(idx), white)
                                    .on_hover_text("Open this game")
                                    .clicked()
                                {
                                    picked = Some(idx);
                                }
                                ui.label(RichText::new(&summary.black).strong());
                                ui.label(&summary.result);
                                ui.label(summary.moves.to_string());
                                let info = match (summary.date.is_empty(), summary.event.is_empty())
                                {
                                    (false, false) => {
                                        format!("{} · {}", summary.date, summary.event)
                                    }
                                    (false, true) => summary.date.clone(),
                                    (true, _) => summary.event.clone(),
                                };
                                ui.label(RichText::new(info).small());
                                ui.end_row();
                            }

                            if shown == 0 {
                                ui.label("");
                                ui.label(
                                    RichText::new("No games match")
                                        .italics()
                                        .color(Color32::GRAY),
                                );
                                ui.end_row();
                            }
                        });
                });
        });

    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chess::pgn::load_pgn_str;

    const TWO_GAMES: &str = "[Event \"Club Championship\"]\n[White \"Carlsen, Magnus\"]\n\
        [Black \"Nakamura, Hikaru\"]\n[Result \"1-0\"]\n[Date \"2024.05.01\"]\n\n\
        1. e4 e5 2. Nf3 1-0\n\n\
        [Event \"Blitz\"]\n[White \"Anna\"]\n[Black \"Boris\"]\n[Result \"1/2-1/2\"]\n\n\
        1. d4 d5 1/2-1/2\n";

    #[test]
    fn summary_reads_headers_and_counts_full_moves() {
        let games = load_pgn_str(TWO_GAMES);
        let s = GameSummary::of(&games[0]);
        assert_eq!(s.white, "Carlsen, Magnus");
        assert_eq!(s.black, "Nakamura, Hikaru");
        assert_eq!(s.result, "1-0");
        assert_eq!(s.date, "2024.05.01");
        assert_eq!(s.event, "Club Championship");
        // Three plies round up to two full moves.
        assert_eq!(s.moves, 2);
    }

    #[test]
    fn summary_falls_back_when_headers_missing() {
        let games = load_pgn_str("1. e4 *\n");
        let s = GameSummary::of(&games[0]);
        assert_eq!(s.white, "?");
        assert_eq!(s.black, "?");
        assert_eq!(s.result, "*");
        assert_eq!(s.date, "");
    }

    #[test]
    fn filter_matches_players_event_and_date_case_insensitively() {
        let games = load_pgn_str(TWO_GAMES);
        let first = GameSummary::of(&games[0]);
        let second = GameSummary::of(&games[1]);

        assert!(first.matches(""));
        assert!(first.matches("  "));
        assert!(first.matches("carlsen"));
        assert!(first.matches("NAKA"));
        assert!(first.matches("championship"));
        assert!(first.matches("2024"));
        assert!(!first.matches("anna"));

        assert!(second.matches("boris"));
        assert!(second.matches("blitz"));
        assert!(!second.matches("carlsen"));
    }
}
