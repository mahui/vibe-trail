use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{ContentBlock, Message, Role};

use super::entry::{parse_timestamp, CcEntry};

/// Parse statistics. Unknown input is never fatal (rule 3); it is counted
/// here and surfaced through `Session.extensions["debug"]`.
#[derive(Debug, Default)]
pub struct CcParseStats {
    pub undecodable_lines: u64,
    pub ignored_entry_types: BTreeMap<String, u64>,
    pub duplicate_uuids: u64,
    pub sidechain_entries: u64,
    pub meta_entries: u64,
    pub unknown_block_types: BTreeMap<String, u64>,
    pub empty_messages: u64,
}

#[derive(Debug, Default)]
pub struct CcParseResult {
    pub messages: Vec<Message>,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub first_user_prompt: Option<String>,
    pub stats: CcParseStats,
}

/// The five-stage Claude Code transcript pipeline (TECH_SPEC §4.1):
/// entry decode → classify/filter → message regroup → tree rebuild → display
/// transform. Stages are deliberately separate functions; do not fuse them.
pub fn run(data: &[u8], include_sidechain: bool) -> CcParseResult {
    let mut result = CcParseResult::default();
    let entries = decode_entries(data, &mut result.stats);
    let kept = classify_and_filter(entries, include_sidechain, &mut result);
    let logical = regroup_by_message_id(kept);
    let ordered = tree_order(logical);
    result.messages = transform(ordered, &mut result.stats);
    result.first_user_prompt = first_user_prompt(&result.messages);
    result
}

// ---- Stage 1: entry decode -------------------------------------------------

fn decode_entries(data: &[u8], stats: &mut CcParseStats) -> Vec<CcEntry> {
    let mut entries = Vec::new();
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<CcEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => stats.undecodable_lines += 1,
        }
    }
    entries
}

// ---- Stage 2: classify, whitelist-filter, dedup by UUID (rules 2 & 3) ------

fn classify_and_filter(
    entries: Vec<CcEntry>,
    include_sidechain: bool,
    result: &mut CcParseResult,
) -> Vec<CcEntry> {
    let mut kept = Vec::new();
    let mut seen_uuids: HashSet<String> = HashSet::new();
    for entry in entries {
        match entry.entry_type.as_deref() {
            Some("user") | Some("assistant") => {
                if let Some(branch) = entry.git_branch.as_deref() {
                    if !branch.is_empty() {
                        result.git_branch = Some(branch.to_string());
                    }
                }
                if entry.is_meta == Some(true) {
                    result.stats.meta_entries += 1;
                    continue;
                }
                if entry.is_sidechain == Some(true) && !include_sidechain {
                    result.stats.sidechain_entries += 1;
                    continue;
                }
                if let Some(uuid) = &entry.uuid {
                    if !seen_uuids.insert(uuid.clone()) {
                        result.stats.duplicate_uuids += 1;
                        continue;
                    }
                }
                kept.push(entry);
            }
            Some("ai-title") => result.ai_title = entry.ai_title,
            Some("last-prompt") => result.last_prompt = entry.last_prompt,
            other => {
                let key = other.unwrap_or("<none>").to_string();
                *result.stats.ignored_entry_types.entry(key).or_insert(0) += 1;
            }
        }
    }
    kept
}

// ---- Stage 3: regroup streamed lines into logical messages (rule 1) --------

pub struct LogicalMessage {
    /// UUID of the last physical chunk: the next message's parentUuid points
    /// at it, so it is the node identity in the tree.
    uuid: String,
    parent_uuid: Option<String>,
    role: Role,
    timestamp: DateTime<Utc>,
    blocks: Vec<Value>,
    plain_text: Option<String>,
}

fn regroup_by_message_id(entries: Vec<CcEntry>) -> Vec<LogicalMessage> {
    let mut messages: Vec<LogicalMessage> = Vec::new();
    let mut index_by_api_id: HashMap<String, usize> = HashMap::new();
    for (fallback_counter, entry) in entries.into_iter().enumerate() {
        let role = if entry.entry_type.as_deref() == Some("assistant") {
            Role::Assistant
        } else {
            Role::User
        };
        let mut blocks: Vec<Value> = Vec::new();
        let mut plain_text: Option<String> = None;
        match entry.message.as_ref().and_then(|m| m.content.as_ref()) {
            Some(Value::String(text)) => plain_text = Some(text.clone()),
            Some(Value::Array(items)) => blocks = items.clone(),
            _ => {}
        }
        let api_id = entry.message.as_ref().and_then(|m| m.id.clone());
        if role == Role::Assistant {
            if let Some(api_id) = &api_id {
                if let Some(&index) = index_by_api_id.get(api_id) {
                    // Continuation chunk of a streamed message: merge blocks,
                    // advance the node uuid to the newest chunk.
                    messages[index].blocks.append(&mut blocks);
                    if let Some(uuid) = entry.uuid {
                        messages[index].uuid = uuid;
                    }
                    continue;
                }
            }
        }
        let message = LogicalMessage {
            uuid: entry.uuid.unwrap_or_else(|| format!("<no-uuid-{fallback_counter}>")),
            parent_uuid: entry.parent_uuid,
            role,
            timestamp: parse_timestamp(entry.timestamp.as_deref())
                .unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
            blocks,
            plain_text,
        };
        messages.push(message);
        if role == Role::Assistant {
            if let Some(api_id) = api_id {
                index_by_api_id.insert(api_id, messages.len() - 1);
            }
        }
    }
    messages
}

// ---- Stage 4: parent-child tree rebuild (rule 4) ----------------------------

fn tree_order(messages: Vec<LogicalMessage>) -> Vec<LogicalMessage> {
    if messages.is_empty() {
        return messages;
    }
    let known: HashSet<&str> = messages.iter().map(|m| m.uuid.as_str()).collect();
    let mut children_of: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        match message.parent_uuid.as_deref() {
            Some(parent) if known.contains(parent) && parent != message.uuid => {
                children_of.entry(parent).or_default().push(index);
            }
            _ => roots.push(index),
        }
    }
    let mut order: Vec<usize> = Vec::with_capacity(messages.len());
    let mut visited: HashSet<usize> = HashSet::new();
    // Iterative DFS; children keep encounter (chronological) order, so
    // resume/branch duplicates that survived dedup stay in file order.
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(index) = stack.pop() {
        if !visited.insert(index) {
            continue;
        }
        order.push(index);
        if let Some(children) = children_of.get(messages[index].uuid.as_str()) {
            stack.extend(children.iter().rev());
        }
    }
    // Cycles or orphaned nodes: append leftovers in file order, never drop.
    if order.len() < messages.len() {
        for index in 0..messages.len() {
            if !visited.contains(&index) {
                order.push(index);
            }
        }
    }
    let mut slots: Vec<Option<LogicalMessage>> = messages.into_iter().map(Some).collect();
    order.into_iter().map(|index| slots[index].take().unwrap()).collect()
}

// ---- Stage 5: display transform ---------------------------------------------

fn transform(messages: Vec<LogicalMessage>, stats: &mut CcParseStats) -> Vec<Message> {
    let mut transformed = Vec::with_capacity(messages.len());
    for message in messages {
        let mut blocks: Vec<ContentBlock> = Vec::new();
        if let Some(text) = message.plain_text {
            if !text.is_empty() {
                blocks.push(ContentBlock::Text { text });
            }
        }
        for block in &message.blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            blocks.push(ContentBlock::Text { text: text.to_string() });
                        }
                    }
                }
                Some("thinking") => {
                    if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                        if !text.is_empty() {
                            blocks.push(ContentBlock::Thinking { text: text.to_string() });
                        }
                    }
                }
                Some("tool_use") => blocks.push(ContentBlock::ToolUse {
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string(),
                    input: block.get("input").cloned().unwrap_or(Value::Null),
                }),
                Some("tool_result") => {
                    let full = flatten_tool_result(block.get("content"));
                    let summary: String = full.chars().take(200).collect();
                    let truncated = full.chars().count() > 200;
                    blocks.push(ContentBlock::ToolResult { summary, truncated });
                }
                other => {
                    let key = other.unwrap_or("<none>").to_string();
                    *stats.unknown_block_types.entry(key).or_insert(0) += 1;
                }
            }
        }
        if blocks.is_empty() {
            stats.empty_messages += 1;
            continue;
        }
        transformed.push(Message {
            uuid: message.uuid,
            parent_uuid: message.parent_uuid,
            role: message.role,
            blocks,
            timestamp: message.timestamp,
        });
    }
    transformed
}

fn flatten_tool_result(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// First real human prompt: used as the session title when no ai-title entry
/// exists. Skips command/attachment XML payloads.
fn first_user_prompt(messages: &[Message]) -> Option<String> {
    for message in messages {
        if message.role != Role::User {
            continue;
        }
        for block in &message.blocks {
            if let ContentBlock::Text { text } = block {
                let trimmed = text.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('<') {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}
