use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// P1 terminal choice. VibeTrail's own config — the only file it ever writes;
/// agent stores stay strictly read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalKind {
    #[default]
    Terminal,
    Iterm2,
    Ghostty,
    Warp,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub terminal: TerminalKind,
}

fn config_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".config/vibetrail/config.json")
}

pub fn load() -> AppConfig {
    fs::read(config_path())
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

pub fn save(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())
}
