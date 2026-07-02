use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{ContentBlock, Message, Role};

/// One physical line of a Codex rollout file. All fields optional: unknown
/// shapes are counted, never fatal.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RolloutLine {
    pub timestamp: Option<String>,
    #[serde(rename = "type")]
    pub entry_type: Option<String>,
    pub payload: Option<Value>,
}

#[derive(Debug, Default)]
pub struct CodexParseStats {
    pub undecodable_lines: u64,
    pub ignored_entry_types: BTreeMap<String, u64>,
    pub ignored_payload_types: BTreeMap<String, u64>,
    pub context_user_messages: u64,
    pub developer_messages: u64,
    pub empty_reasoning: u64,
}

#[derive(Debug, Default)]
pub struct CodexParseResult {
    pub messages: Vec<Message>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub first_user_prompt: Option<String>,
    pub stats: CodexParseStats,
}

/// User-message payloads that are injected context, not typed prompts.
pub fn is_context_payload(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<environment_context>") || trimmed.starts_with("<user_instructions>")
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Rollout transcripts are linear: no streaming splits, no duplicate UUIDs,
/// no parent-child tree (those are Claude Code quirks that stay in that
/// provider). The stages here are decode → whitelist-classify → transform.
/// Messages have no intrinsic id, so each gets `L<line-number>` (1-based),
/// which search hit resolution reproduces for jump anchoring.
pub fn run(data: &[u8]) -> CodexParseResult {
    run_with_limit(data, RESULT_PREVIEW_CHARS)
}

/// Display truncation for tool results; full text re-read on demand.
pub const RESULT_PREVIEW_CHARS: usize = 2000;

pub fn run_with_limit(data: &[u8], result_limit: usize) -> CodexParseResult {
    let mut result = CodexParseResult::default();
    for (index, line) in data.split(|&b| b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_number = index as u64 + 1;
        let Ok(entry) = serde_json::from_slice::<RolloutLine>(line) else {
            result.stats.undecodable_lines += 1;
            continue;
        };
        let timestamp =
            parse_timestamp(entry.timestamp.as_deref()).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        match entry.entry_type.as_deref() {
            Some("session_meta") => {
                let payload = entry.payload.unwrap_or(Value::Null);
                result.cwd = payload
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(result.cwd.take());
                result.git_branch = payload
                    .pointer("/git/branch")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(result.git_branch.take());
            }
            Some("response_item") => {
                let payload = entry.payload.unwrap_or(Value::Null);
                if let Some((role, blocks)) =
                    transform_response_item(&payload, result_limit, &mut result.stats)
                {
                    if role == Role::User && result.first_user_prompt.is_none() {
                        if let Some(ContentBlock::Text { text }) = blocks.first() {
                            result.first_user_prompt = Some(text.trim().to_string());
                        }
                    }
                    result.messages.push(Message {
                        uuid: format!("L{line_number}"),
                        parent_uuid: None,
                        role,
                        blocks,
                        timestamp,
                    });
                }
            }
            // event_msg entries mirror response_item content (agent_message,
            // user_message) or carry runtime telemetry; all skipped.
            other => {
                let key = other.unwrap_or("<none>").to_string();
                *result.stats.ignored_entry_types.entry(key).or_insert(0) += 1;
            }
        }
    }
    result
}

/// Whitelist transform of `response_item` payloads to unified blocks.
fn transform_response_item(
    payload: &Value,
    result_limit: usize,
    stats: &mut CodexParseStats,
) -> Option<(Role, Vec<ContentBlock>)> {
    match payload.get("type").and_then(Value::as_str) {
        Some("message") => {
            let role = match payload.get("role").and_then(Value::as_str) {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                _ => {
                    stats.developer_messages += 1;
                    return None;
                }
            };
            let mut texts: Vec<String> = Vec::new();
            for item in payload
                .get("content")
                .and_then(Value::as_array)
                .unwrap_or(&Vec::new())
            {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        texts.push(text.to_string());
                    }
                }
            }
            if role == Role::User && texts.iter().all(|t| is_context_payload(t)) {
                stats.context_user_messages += 1;
                return None;
            }
            let blocks: Vec<ContentBlock> = texts
                .into_iter()
                .map(|text| ContentBlock::Text { text })
                .collect();
            if blocks.is_empty() {
                return None;
            }
            Some((role, blocks))
        }
        Some("reasoning") => {
            let summary: Vec<&str> = payload
                .get("summary")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("text").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default();
            if summary.is_empty() {
                // Encrypted-only reasoning carries nothing displayable.
                stats.empty_reasoning += 1;
                return None;
            }
            Some((
                Role::Assistant,
                vec![ContentBlock::Thinking {
                    text: summary.join("\n"),
                }],
            ))
        }
        Some("function_call") | Some("custom_tool_call") => {
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string();
            // `arguments`/`input` is a JSON-encoded string; decode for display.
            let raw_input = payload
                .get("arguments")
                .or_else(|| payload.get("input"))
                .cloned()
                .unwrap_or(Value::Null);
            let input = match &raw_input {
                Value::String(text) => {
                    serde_json::from_str::<Value>(text).unwrap_or(raw_input.clone())
                }
                _ => raw_input,
            };
            Some((Role::Assistant, vec![ContentBlock::ToolUse { name, input }]))
        }
        Some("function_call_output") | Some("custom_tool_call_output") => {
            let full = payload.get("output").and_then(Value::as_str).unwrap_or("");
            let summary: String = full.chars().take(result_limit).collect();
            let truncated = full.chars().count() > result_limit;
            Some((
                Role::User,
                vec![ContentBlock::ToolResult { summary, truncated }],
            ))
        }
        Some("web_search_call") => Some((
            Role::Assistant,
            vec![ContentBlock::ToolUse {
                name: "web_search".to_string(),
                input: payload.get("action").cloned().unwrap_or(Value::Null),
            }],
        )),
        other => {
            let key = other.unwrap_or("<none>").to_string();
            *stats.ignored_payload_types.entry(key).or_insert(0) += 1;
            None
        }
    }
}

/// Collect every searchable text in one rollout line: used by hit resolution
/// (and the .zst degrade path) so search results match what parse displays.
pub fn searchable_texts(payload: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    match payload.get("type").and_then(Value::as_str) {
        Some("message") => {
            for item in payload
                .get("content")
                .and_then(Value::as_array)
                .unwrap_or(&Vec::new())
            {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !is_context_payload(text) {
                        texts.push(text.to_string());
                    }
                }
            }
        }
        Some("reasoning") => {
            if let Some(items) = payload.get("summary").and_then(Value::as_array) {
                for item in items {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        texts.push(text.to_string());
                    }
                }
            }
        }
        Some("function_call") | Some("custom_tool_call") => {
            for key in ["name", "arguments", "input"] {
                if let Some(text) = payload.get(key).and_then(Value::as_str) {
                    texts.push(text.to_string());
                }
            }
        }
        Some("function_call_output") | Some("custom_tool_call_output") => {
            if let Some(text) = payload.get("output").and_then(Value::as_str) {
                texts.push(text.to_string());
            }
        }
        _ => {}
    }
    texts
}
