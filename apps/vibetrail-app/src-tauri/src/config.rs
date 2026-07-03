use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vibetrail_core::config::ProviderSettings;

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

/// Full schema of ~/.config/vibetrail/config.json. The GUI shell owns writing
/// it; Core reads only the `providers` slice for store construction
/// (TECH_SPEC §12), so this must stay a superset of that shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub terminal: TerminalKind,
    /// UI language preference: "auto" (follow the system), "en" or "zh".
    /// The frontend owns resolution; the shell only localizes the native
    /// menu item for an explicit choice.
    #[serde(default = "default_language")]
    pub language: String,
    /// Normalized project paths the user hid from the sidebar. A display
    /// preference only — discovery, search and the CLI still see everything.
    #[serde(default)]
    pub hidden_projects: Vec<String>,
    /// Per-provider discovery settings (enable/root), honored by CLI and GUI
    /// alike through Core's store construction.
    #[serde(default)]
    pub providers: std::collections::BTreeMap<String, ProviderSettings>,
    /// The file is plain JSON and hand-editable: keys this build doesn't know
    /// (hand additions, newer versions) must survive a save round-trip.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_language() -> String {
    "auto".to_string()
}

fn config_path() -> PathBuf {
    vibetrail_core::config::config_path()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let raw = r#"{
            "terminal": "ghostty",
            "customNote": "keep me",
            "providers": { "codex": { "enabled": false, "root": "/alt" } }
        }"#;
        let config: AppConfig = serde_json::from_str(raw).unwrap();
        let out = serde_json::to_value(&config).unwrap();
        assert_eq!(out["terminal"], "ghostty");
        assert_eq!(out["customNote"], "keep me");
        assert_eq!(out["providers"]["codex"]["enabled"], false);
        assert_eq!(out["providers"]["codex"]["root"], "/alt");
    }
}
