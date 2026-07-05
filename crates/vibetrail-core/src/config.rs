//! Discovery settings shared by both shells (TECH_SPEC §12): which providers
//! participate and where their stores live. Both the CLI and the GUI build
//! their `SessionStore` through here, so enabling/disabling a provider or
//! overriding a store root behaves identically everywhere — shell discovery
//! logic stays sunk into Core.
//!
//! Core only ever *reads* `~/.config/vibetrail/config.json`; writing it stays
//! in the GUI shell (ADR-4: config.json is the only file VibeTrail writes,
//! agent stores are strictly read-only).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::provider::Provider;
use crate::providers::antigravity::AntigravityProvider;
use crate::providers::claude_code::ClaudeCodeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::cursor::CursorProvider;
use crate::providers::qoder::QoderProvider;
use crate::store::SessionStore;

/// Per-provider discovery settings under config.json's `providers` map.
/// Serialize lives here too so the GUI shell round-trips the same shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Store root override; `~` expands to the home directory. `None` or an
    /// empty string means the provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            root: None,
        }
    }
}

fn default_enabled() -> bool {
    true
}

/// The Core-relevant slice of config.json. GUI-only fields (terminal choice,
/// hidden projects) are owned by the app shell and ignored here.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryConfig {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderSettings>,
}

impl DiscoveryConfig {
    pub fn provider(&self, id: &str) -> ProviderSettings {
        self.providers.get(id).cloned().unwrap_or_default()
    }

    /// Effective root override for a provider, `~` expanded; `None` when the
    /// provider default applies.
    pub fn root_override(&self, id: &str) -> Option<PathBuf> {
        let root = self.provider(id).root?;
        let trimmed = root.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(expand_tilde(trimmed))
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest);
    }
    PathBuf::from(path)
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/vibetrail/config.json")
}

/// Missing or malformed config degrades to defaults (all providers enabled at
/// their default roots) — a broken config file must never brick discovery.
pub fn load_discovery() -> DiscoveryConfig {
    std::fs::read(config_path())
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Static provider registry: the one place that knows which providers exist,
/// their display names, default roots and constructors.
struct Registration {
    id: &'static str,
    name: &'static str,
    default_root: fn() -> PathBuf,
    build: fn(Option<PathBuf>) -> Box<dyn Provider>,
}

fn registry() -> [Registration; 5] {
    [
        Registration {
            id: "claude-code",
            name: "Claude Code",
            default_root: ClaudeCodeProvider::default_root,
            build: |root| Box::new(ClaudeCodeProvider::new(root)),
        },
        Registration {
            id: "codex",
            name: "Codex",
            default_root: CodexProvider::default_root,
            build: |root| Box::new(CodexProvider::new(root)),
        },
        Registration {
            id: "antigravity",
            name: "Antigravity (experimental)",
            default_root: AntigravityProvider::default_root,
            build: |root| Box::new(AntigravityProvider::new(root)),
        },
        Registration {
            id: "cursor",
            name: "Cursor (experimental)",
            default_root: CursorProvider::default_root,
            build: |root| Box::new(CursorProvider::new(root)),
        },
        Registration {
            id: "qoder",
            name: "Qoder",
            default_root: QoderProvider::default_root,
            build: |root| Box::new(QoderProvider::new(root)),
        },
    ]
}

/// Store construction from settings: disabled providers are excluded from
/// discovery, search and resume alike.
pub fn store_from_config(config: &DiscoveryConfig) -> SessionStore {
    SessionStore::new(
        registry()
            .into_iter()
            .filter(|reg| config.provider(reg.id).enabled)
            .map(|reg| (reg.build)(config.root_override(reg.id)))
            .collect(),
    )
}

/// The one-liner both shells call: config file → effective store.
pub fn default_store() -> SessionStore {
    store_from_config(&load_discovery())
}

/// Effective discovery state of one provider — a row of `vibetrail config`
/// and of the GUI settings pane. Part of the shared `--json` schema.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Root in effect (override when set, default otherwise).
    pub root: String,
    pub default_root: String,
    pub root_is_custom: bool,
    pub root_exists: bool,
}

/// Effective configuration report shared by `vibetrail config --json` and the
/// GUI settings pane.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReport {
    /// Where the settings live — the file is plain JSON and hand-editable.
    pub path: String,
    pub providers: Vec<ProviderStatus>,
}

pub fn report(config: &DiscoveryConfig) -> ConfigReport {
    let providers = registry()
        .into_iter()
        .map(|reg| {
            let settings = config.provider(reg.id);
            let default_root = (reg.default_root)();
            let root = config
                .root_override(reg.id)
                .unwrap_or_else(|| default_root.clone());
            ProviderStatus {
                id: reg.id.to_string(),
                name: reg.name.to_string(),
                enabled: settings.enabled,
                root_is_custom: root != default_root,
                root_exists: root.is_dir(),
                root: root.to_string_lossy().into_owned(),
                default_root: default_root.to_string_lossy().into_owned(),
            }
        })
        .collect();
    ConfigReport {
        path: config_path().to_string_lossy().into_owned(),
        providers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_provider_defaults_to_enabled_default_root() {
        let config = DiscoveryConfig::default();
        assert!(config.provider("claude-code").enabled);
        assert_eq!(config.root_override("claude-code"), None);
        let store = store_from_config(&config);
        let ids: Vec<&str> = store.providers().iter().map(|p| p.id()).collect();
        assert_eq!(
            ids,
            ["claude-code", "codex", "antigravity", "cursor", "qoder"]
        );
    }

    #[test]
    fn disabled_provider_is_excluded_from_the_store() {
        let config: DiscoveryConfig = serde_json::from_str(
            r#"{ "providers": { "codex": { "enabled": false } },
                 "terminal": "ghostty", "hiddenProjects": ["/x"] }"#,
        )
        .unwrap();
        let store = store_from_config(&config);
        assert!(store.provider("codex").is_none());
        assert!(store.provider("claude-code").is_some());
    }

    #[test]
    fn root_override_expands_tilde_and_ignores_blank() {
        let config: DiscoveryConfig = serde_json::from_str(
            r#"{ "providers": {
                   "codex": { "root": "~/alt/codex" },
                   "claude-code": { "root": "  " } } }"#,
        )
        .unwrap();
        let expected = dirs::home_dir().unwrap().join("alt/codex");
        assert_eq!(config.root_override("codex"), Some(expected));
        assert_eq!(config.root_override("claude-code"), None);
    }

    #[test]
    fn enabled_defaults_to_true_when_only_root_is_set() {
        let config: DiscoveryConfig =
            serde_json::from_str(r#"{ "providers": { "codex": { "root": "/tmp" } } }"#).unwrap();
        assert!(config.provider("codex").enabled);
    }

    #[test]
    fn report_flags_custom_and_missing_roots() {
        let config: DiscoveryConfig = serde_json::from_str(
            r#"{ "providers": { "codex": { "enabled": false, "root": "/nonexistent/codex" } } }"#,
        )
        .unwrap();
        let report = report(&config);
        assert_eq!(report.providers.len(), 5);
        let codex = report.providers.iter().find(|p| p.id == "codex").unwrap();
        assert!(!codex.enabled);
        assert!(codex.root_is_custom);
        assert!(!codex.root_exists);
        assert_eq!(codex.root, "/nonexistent/codex");
        let cc = report
            .providers
            .iter()
            .find(|p| p.id == "claude-code")
            .unwrap();
        assert!(cc.enabled);
        assert!(!cc.root_is_custom);
        assert_eq!(cc.root, cc.default_root);
    }
}
