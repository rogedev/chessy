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
    #[serde(default = "default_piece_set")]
    pub piece_set: String,
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
            piece_set: default_piece_set(),
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

fn default_piece_set() -> String {
    "cburnett".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_piece_set_is_cburnett() {
        assert_eq!(Settings::default().piece_set, "cburnett");
    }

    #[test]
    fn deserialize_without_piece_set_defaults_to_cburnett() {
        // Simulates a saved config from before piece_set was added.
        let json = r#"{
            "engine_path": "stockfish",
            "engine_elo": 1500,
            "limit_strength": false,
            "multipv": 3,
            "dark_theme": true,
            "movetime_ms": 2000,
            "analysis_depth": 20
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.piece_set, "cburnett");
    }

    #[test]
    fn deserialize_with_piece_set_uses_provided_value() {
        let json = r#"{
            "engine_path": "stockfish",
            "engine_elo": 1500,
            "limit_strength": false,
            "multipv": 3,
            "dark_theme": true,
            "movetime_ms": 2000,
            "analysis_depth": 20,
            "piece_set": "alpha"
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.piece_set, "alpha");
    }

    #[test]
    fn piece_set_roundtrips_through_json() {
        let mut s = Settings::default();
        s.piece_set = "cardinal".to_string();
        let json = serde_json::to_string(&s).unwrap();
        let s2: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s2.piece_set, "cardinal");
    }
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
