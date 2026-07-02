use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A project group derived from normalized session cwd values. Projects are
/// never stored; they are aggregated across providers at query time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub real_path: String,
    pub exists: bool,
    pub session_count: usize,
    pub last_active: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_prompt: Option<String>,
    pub providers: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// Globally unique key: "<provider_id>:<native_id>".
    pub id: String,
    pub provider_id: String,
    pub native_id: String,
    pub project_path: String,
    pub title: String,
    pub mtime: DateTime<Utc>,
    pub message_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    /// Seconds between first and last message.
    pub duration: f64,
}

impl SessionSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: &str,
        native_id: &str,
        project_path: String,
        title: String,
        mtime: DateTime<Utc>,
        message_count: usize,
        git_branch: Option<String>,
        duration: f64,
    ) -> Self {
        Self {
            id: format!("{provider_id}:{native_id}"),
            provider_id: provider_id.to_string(),
            native_id: native_id.to_string(),
            project_path,
            title,
            mtime,
            message_count,
            git_branch,
            duration,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        summary: String,
        truncated: bool,
    },
    Thinking {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub summary: SessionSummary,
    pub messages: Vec<Message>,
    /// Provider-specific payloads (CC subagents, AGY artifacts, debug
    /// counters) that must not leak into the generic model.
    pub extensions: serde_json::Map<String, serde_json::Value>,
}

/// Lightweight per-message stub used to render a timeline before paging in
/// full message bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageStub {
    pub index: usize,
    pub uuid: String,
    pub role: Role,
    pub preview: String,
    pub timestamp: DateTime<Utc>,
}
