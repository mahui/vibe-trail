//! Handoff (TECH_SPEC §14, template tier): derive a structured "capsule"
//! from a parsed session and render it as a prompt a fresh agent session can
//! continue from. Pure derivation — no LLM, no new dependencies; everything
//! here comes from data the providers already parse.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{ContentBlock, Role, Session};

/// Display caps: a capsule is a briefing, not a transcript.
const MAX_FILES: usize = 20;
const MAX_EXCERPT_CHARS: usize = 400;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffCapsule {
    pub goal: String,
    pub project_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub previous_agent: String,
    pub message_count: usize,
    /// Distinct file paths seen in tool inputs, first-touched order.
    pub files_touched: Vec<String>,
    /// How many distinct files were dropped by the cap.
    pub files_omitted: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_text: Option<String>,
}

impl HandoffCapsule {
    pub fn from_session(session: &Session) -> Self {
        let summary = &session.summary;
        let mut files: Vec<String> = Vec::new();
        for message in &session.messages {
            for block in &message.blocks {
                if let ContentBlock::ToolUse { input, .. } = block {
                    collect_file_paths(input, &mut files);
                }
            }
        }
        let files_omitted = files.len().saturating_sub(MAX_FILES);
        files.truncate(MAX_FILES);
        Self {
            goal: summary.title.clone(),
            project_path: summary.project_path.clone(),
            git_branch: summary.git_branch.clone(),
            previous_agent: summary.provider_id.clone(),
            message_count: summary.message_count,
            files_touched: files,
            files_omitted,
            last_user_prompt: last_text(session, Role::User),
            last_assistant_text: last_text(session, Role::Assistant),
        }
    }

    /// The prompt handed to the next agent. English on purpose: it is agent
    /// input, not UI copy.
    pub fn prompt(&self) -> String {
        let mut out = String::from(
            "You are continuing an existing coding task started in another agent session.\n\n",
        );
        out.push_str(&format!("Goal: {}\n", self.goal));
        out.push_str(&format!("Path: {}\n", self.project_path));
        if let Some(branch) = &self.git_branch {
            out.push_str(&format!("Branch: {branch}\n"));
        }
        out.push_str(&format!(
            "Previous agent: {} ({} messages in the prior session)\n",
            self.previous_agent, self.message_count
        ));
        if !self.files_touched.is_empty() {
            out.push_str("\nFiles touched in the prior session:\n");
            for file in &self.files_touched {
                out.push_str(&format!("- {file}\n"));
            }
            if self.files_omitted > 0 {
                out.push_str(&format!("- … and {} more\n", self.files_omitted));
            }
        }
        if let Some(prompt) = &self.last_user_prompt {
            out.push_str(&format!("\nLast user request:\n{prompt}\n"));
        }
        if let Some(note) = &self.last_assistant_text {
            out.push_str(&format!("\nWhere the previous agent left off:\n{note}\n"));
        }
        out.push_str(
            "\nContinue from this state. Inspect the files above before changing them; \
             do not redo work that is already done.\n",
        );
        out
    }
}

/// Last plain-text block of the given role, excerpt-truncated. Tool blocks
/// are skipped: the capsule wants intent and state, not raw tool output.
fn last_text(session: &Session, role: Role) -> Option<String> {
    session
        .messages
        .iter()
        .rev()
        .filter(|m| m.role == role)
        .flat_map(|m| m.blocks.iter().rev())
        .find_map(|block| match block {
            ContentBlock::Text { text } if !text.trim().is_empty() => Some(excerpt(text.trim())),
            _ => None,
        })
}

fn excerpt(text: &str) -> String {
    if text.chars().count() <= MAX_EXCERPT_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_EXCERPT_CHARS).collect();
    format!("{cut}…")
}

/// Path-shaped values in tool inputs, across providers: the unified ToolUse
/// block keeps the raw input JSON, and file-taking tools consistently use
/// these key names. Nested inputs (batched edits) are walked recursively.
fn collect_file_paths(input: &Value, out: &mut Vec<String>) {
    const PATH_KEYS: [&str; 4] = ["file_path", "path", "notebook_path", "filePath"];
    match input {
        Value::Object(map) => {
            for (key, value) in map {
                if let (true, Some(path)) = (PATH_KEYS.contains(&key.as_str()), value.as_str()) {
                    if !path.is_empty() && !out.iter().any(|seen| seen == path) {
                        out.push(path.to_string());
                    }
                }
                collect_file_paths(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_file_paths(item, out);
            }
        }
        _ => {}
    }
}
