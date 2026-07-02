use chrono::{DateTime, Utc};
use serde::Deserialize;

/// One physical line of a Claude Code session JSONL file. All fields
/// optional: unknown shapes must decode (and be skipped later), never throw
/// (rule 3).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CcEntry {
    #[serde(rename = "type")]
    pub entry_type: Option<String>,
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub is_sidechain: Option<bool>,
    pub is_meta: Option<bool>,
    pub message: Option<CcMessage>,
    // Metadata entry payloads (type == "ai-title" / "last-prompt").
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct CcUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CcMessage {
    /// API message id ("msg_…"). Streaming splits one logical message across
    /// several lines sharing this id (rule 1).
    pub id: Option<String>,
    pub role: Option<String>,
    /// Either a plain string (typed user prompt) or an array of content
    /// blocks; kept as raw JSON and interpreted tolerantly downstream.
    pub content: Option<serde_json::Value>,
    pub usage: Option<CcUsage>,
}

/// `subagents/agent-<id>.meta.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CcSubagentMeta {
    pub agent_type: Option<String>,
    pub description: Option<String>,
}

/// ISO8601 with or without fractional seconds.
pub fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?).ok().map(|dt| dt.with_timezone(&Utc))
}
