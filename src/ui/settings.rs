use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub engine_path: String,
    pub engine_elo: u32,
    pub limit_strength: bool,
    pub multipv: u8,
    pub dark_theme: bool,
    pub movetime_ms: u64,
    pub analysis_depth: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine_path: default_engine_path(),
            engine_elo: 1500,
            limit_strength: false,
            multipv: 3,
            dark_theme: true,
            movetime_ms: 2000,
            analysis_depth: 20,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        if let Some(path) = config_path()
            && let Ok(data) = std::fs::read_to_string(&path)
            && let Ok(s) = serde_json::from_str(&data)
        {
            return s;
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(path) = config_path()
            && let Ok(json) = serde_json::to_string_pretty(self)
        {
            let _ = std::fs::write(path, json);
        }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    dirs_next::config_dir().map(|d| d.join("chessy").join("settings.json"))
}

fn default_engine_path() -> String {
    // Try common locations
    let candidates = [
        "/usr/local/bin/stockfish",
        "/usr/bin/stockfish",
        "/opt/homebrew/bin/stockfish",
        "stockfish",
    ];
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return c.to_string();
        }
    }
    "stockfish".to_string()
}
