use std::path::PathBuf;
use std::sync::OnceLock;

pub fn assets_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut candidates = vec![PathBuf::from("assets")];
        if let Ok(exe) = std::env::current_exe()
            && let Some(exe_dir) = exe.parent()
        {
            candidates.push(exe_dir.join("assets"));
            // Chessy.app/Contents/MacOS/chessy -> Chessy.app/Contents/Resources/assets
            candidates.push(exe_dir.join("../Resources/assets"));
        }
        candidates
            .iter()
            .find(|p| p.is_dir())
            .cloned()
            .unwrap_or_else(|| PathBuf::from("assets"))
    })
}

pub fn asset_path(relative: &str) -> PathBuf {
    assets_dir().join(relative)
}
