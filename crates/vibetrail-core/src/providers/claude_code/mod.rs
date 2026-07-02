mod entry;
mod pipeline;

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{ContentBlock, Message, MessageStub, Session, SessionSummary};
use crate::provider::{Provider, ProviderCapabilities, RawSession, ResumeSpec};
use crate::search::SearchHit;
use crate::store::normalize_path;
use crate::textutil::{make_snippet, sanitize_title};

use entry::{CcEntry, CcSubagentMeta};
use pipeline::{CcParseResult, CcParseStats};

const PROVIDER_ID: &str = "claude-code";
/// Bound for metadata-level head/tail reads (discovery, quick_title).
const BOUNDED_READ: usize = 128 * 1024;

/// Claude Code provider: reads `~/.claude/projects/**` strictly read-only.
pub struct ClaudeCodeProvider {
    /// `~/.claude/projects`; injectable for fixture tests.
    root: PathBuf,
}

impl ClaudeCodeProvider {
    pub fn new(root: Option<PathBuf>) -> Self {
        let root = root.unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".claude")
                .join("projects")
        });
        Self { root }
    }

    /// Metadata-level cwd extraction: scan only the head of the file for the
    /// first entry carrying a `cwd` field (discovery must not full-parse).
    fn extract_cwd(&self, file: &Path) -> Option<String> {
        self.extract_head_meta(file, None).0
    }

    /// One bounded head read yields both the cwd and the resume-chain parent:
    /// a resume-fork copies the parent's history into the new file, and those
    /// copied lines keep their original sessionId — the first sessionId that
    /// differs from the file's own id names the parent.
    fn extract_head_meta(
        &self,
        file: &Path,
        own_id: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let Some(head) = read_head(file, BOUNDED_READ) else {
            return (None, None);
        };
        let mut cwd = None;
        let mut parent = None;
        for line in head.split(|&b| b == b'\n').take(40) {
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            if cwd.is_none() {
                if let Some(found) = value.get("cwd").and_then(Value::as_str) {
                    if !found.is_empty() {
                        cwd = Some(found.to_string());
                    }
                }
            }
            if parent.is_none() {
                if let (Some(own_id), Some(sid)) =
                    (own_id, value.get("sessionId").and_then(Value::as_str))
                {
                    if !sid.is_empty() && sid != own_id {
                        parent = Some(sid.to_string());
                    }
                }
            }
            if cwd.is_some() && (own_id.is_none() || parent.is_some()) {
                break;
            }
        }
        (cwd, parent)
    }

    /// Lossy fallback only ("-Users-x-my-app" cannot distinguish "/" from
    /// "-" or "."); real resolution comes from the cwd field inside the file.
    fn decode_project_dir_name(&self, name: &str) -> String {
        if name.starts_with('-') {
            name.replace('-', "/")
        } else {
            name.to_string()
        }
    }

    fn make_summary(&self, raw: &RawSession, result: &CcParseResult) -> SessionSummary {
        let title_source = result
            .ai_title
            .as_deref()
            .or(result.first_user_prompt.as_deref())
            .or(result.last_prompt.as_deref())
            .unwrap_or("");
        let title = sanitize_title(title_source);
        let title = if title.is_empty() {
            raw.native_id.chars().take(8).collect()
        } else {
            title
        };
        let timestamps: Vec<DateTime<Utc>> = result
            .messages
            .iter()
            .map(|m| m.timestamp)
            .filter(|t| *t != DateTime::<Utc>::UNIX_EPOCH)
            .collect();
        let duration = match (timestamps.iter().min(), timestamps.iter().max()) {
            (Some(first), Some(last)) => (*last - *first).num_milliseconds() as f64 / 1000.0,
            _ => 0.0,
        };
        SessionSummary::new(
            PROVIDER_ID,
            &raw.native_id,
            raw.project_path.clone(),
            title,
            raw.mtime,
            result.messages.len(),
            result.git_branch.clone(),
            duration,
        )
    }

    /// Rule 4: subagent transcripts are separate files merged in a fixed,
    /// deterministic order (sorted by file name), each through the full
    /// pipeline — never fused into the main pass.
    fn parse_subagents(&self, raw: &RawSession) -> Vec<Value> {
        let dir = raw.file_path.with_extension("").join("subagents");
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        files.sort();
        let mut subagents = Vec::new();
        for file in files {
            let Ok(data) = fs::read(&file) else { continue };
            // Subagent transcripts are sidechains by definition.
            let result = pipeline::run(&data, true);
            let previews: Vec<Value> = result
                .messages
                .iter()
                .take(100)
                .map(|message| {
                    json!({
                        "role": message.role,
                        "preview": self.preview(message),
                    })
                })
                .collect();
            let mut object = json!({
                "agentId": file.file_stem().unwrap_or_default().to_string_lossy(),
                "messageCount": result.messages.len(),
                "messages": previews,
            });
            let meta_path = file.with_extension("meta.json");
            if let Ok(meta_data) = fs::read(&meta_path) {
                if let Ok(meta) = serde_json::from_slice::<CcSubagentMeta>(&meta_data) {
                    if let Some(agent_type) = meta.agent_type {
                        object["agentType"] = json!(agent_type);
                    }
                    if let Some(description) = meta.description {
                        object["description"] = json!(description);
                    }
                }
            }
            subagents.push(object);
        }
        subagents
    }

    fn debug_extension(&self, stats: &CcParseStats) -> Value {
        json!({
            "undecodableLines": stats.undecodable_lines,
            "duplicateUuids": stats.duplicate_uuids,
            "sidechainEntries": stats.sidechain_entries,
            "ignoredEntryTypes": stats.ignored_entry_types,
            "unknownBlockTypes": stats.unknown_block_types,
        })
    }

    fn preview(&self, message: &Message) -> String {
        for block in &message.blocks {
            if let ContentBlock::Text { text } = block {
                let line = text.lines().next().unwrap_or("");
                return line.chars().take(120).collect();
            }
        }
        match message.blocks.first() {
            Some(ContentBlock::ToolUse { name, .. }) => format!("⚙ {name}"),
            Some(ContentBlock::ToolResult { summary, .. }) => {
                let line: String = summary
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(100)
                    .collect();
                format!("→ {line}")
            }
            Some(ContentBlock::Thinking { .. }) => "(thinking)".to_string(),
            _ => String::new(),
        }
    }
}

impl Provider for ClaudeCodeProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            resumable: true,
            file_based_only: true,
            has_artifacts: false,
            project_native: true,
        }
    }

    fn discover(&self) -> Result<Vec<RawSession>> {
        let mut sessions = Vec::new();
        let Ok(project_dirs) = fs::read_dir(&self.root) else {
            return Ok(sessions); // no store yet: empty, not an error
        };
        for dir_entry in project_dirs.filter_map(|e| e.ok()) {
            let dir = dir_entry.path();
            if !dir.is_dir() || dir_entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            // Only top-level <session-uuid>.jsonl files; subagent transcripts
            // live under <session-uuid>/subagents/ and are not sessions.
            let Ok(files) = fs::read_dir(&dir) else {
                continue;
            };
            let mut jsonl_files: Vec<PathBuf> = files
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "jsonl"))
                .collect();
            jsonl_files.sort();
            let mut dir_fallback_cwd: Option<String> = None;
            for file in jsonl_files {
                let metadata = fs::metadata(&file)
                    .map_err(|e| Error::Data(format!("stat {}: {e}", file.display())))?;
                let native_id = file
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let (found_cwd, parent_native_id) = self.extract_head_meta(&file, Some(&native_id));
                let cwd = found_cwd
                    .or_else(|| dir_fallback_cwd.clone())
                    .unwrap_or_else(|| {
                        self.decode_project_dir_name(&dir_entry.file_name().to_string_lossy())
                    });
                dir_fallback_cwd.get_or_insert_with(|| cwd.clone());
                sessions.push(RawSession {
                    provider_id: PROVIDER_ID.to_string(),
                    native_id,
                    project_path: normalize_path(&cwd),
                    mtime: metadata
                        .modified()
                        .map(DateTime::<Utc>::from)
                        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
                    file_size: metadata.len(),
                    file_path: file,
                    parent_native_id,
                });
            }
        }
        Ok(sessions)
    }

    fn parse(&self, raw: &RawSession) -> Result<Session> {
        let data = fs::read(&raw.file_path)
            .map_err(|e| Error::Data(format!("Cannot read {}: {e}", raw.file_path.display())))?;
        let result = pipeline::run(&data, false);
        let mut extensions = serde_json::Map::new();
        let subagents = self.parse_subagents(raw);
        if !subagents.is_empty() {
            extensions.insert("subagents".to_string(), Value::Array(subagents));
        }
        if !result.usage.is_zero() {
            extensions.insert(
                "usage".to_string(),
                json!({
                    "inputTokens": result.usage.input_tokens,
                    "outputTokens": result.usage.output_tokens,
                    "cacheCreationTokens": result.usage.cache_creation_tokens,
                    "cacheReadTokens": result.usage.cache_read_tokens,
                }),
            );
        }
        extensions.insert("debug".to_string(), self.debug_extension(&result.stats));
        Ok(Session {
            summary: self.make_summary(raw, &result),
            messages: result.messages,
            extensions,
        })
    }

    fn outline(&self, raw: &RawSession) -> Result<Vec<MessageStub>> {
        Ok(self
            .parse(raw)?
            .messages
            .iter()
            .enumerate()
            .map(|(index, message)| MessageStub {
                index,
                uuid: message.uuid.clone(),
                role: message.role,
                preview: self.preview(message),
                timestamp: message.timestamp,
            })
            .collect())
    }

    fn page(&self, raw: &RawSession, offset: usize, limit: usize) -> Result<Vec<Message>> {
        let messages = self.parse(raw)?.messages;
        Ok(messages.into_iter().skip(offset).take(limit).collect())
    }

    fn resume_spec(&self, raw: &RawSession) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            project_path: raw.project_path.clone(),
            command: vec![
                "claude".to_string(),
                "--resume".to_string(),
                raw.native_id.clone(),
            ],
        })
    }

    /// Bounded-read title: newest `last-prompt`/`ai-title` entry from the
    /// file tail, else the first human prompt from the head. Never
    /// full-parses, so the project overview stays within its cold-start
    /// budget.
    fn quick_title(&self, raw: &RawSession) -> Option<String> {
        if let Some(tail) = read_tail(&raw.file_path, BOUNDED_READ) {
            // First chunk may be a partial line; its decode just fails and is
            // skipped.
            for line in tail.split(|&b| b == b'\n').rev() {
                let Ok(entry) = serde_json::from_slice::<CcEntry>(line) else {
                    continue;
                };
                match entry.entry_type.as_deref() {
                    Some("last-prompt") => {
                        if let Some(prompt) = entry.last_prompt {
                            return Some(sanitize_title(&prompt));
                        }
                    }
                    Some("ai-title") => {
                        if let Some(title) = entry.ai_title {
                            return Some(sanitize_title(&title));
                        }
                    }
                    _ => {}
                }
            }
        }
        let head = read_head(&raw.file_path, BOUNDED_READ)?;
        for line in head.split(|&b| b == b'\n') {
            let Ok(entry) = serde_json::from_slice::<CcEntry>(line) else {
                continue;
            };
            if entry.entry_type.as_deref() != Some("user") || entry.is_meta == Some(true) {
                continue;
            }
            let Some(Value::String(text)) = entry.message.and_then(|m| m.content) else {
                continue;
            };
            let trimmed = text.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('<') {
                return Some(sanitize_title(trimmed));
            }
        }
        None
    }

    fn search_roots(&self, project_path: Option<&str>) -> Vec<PathBuf> {
        let Some(project_path) = project_path else {
            return vec![self.root.clone()];
        };
        let normalized = normalize_path(project_path);
        let Ok(dirs) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        dirs.filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|dir| dir.is_dir())
            .filter(|dir| {
                let Ok(files) = fs::read_dir(dir) else {
                    return false;
                };
                let first = files
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .find(|p| p.extension().is_some_and(|ext| ext == "jsonl"));
                let Some(first) = first else { return false };
                let cwd = self.extract_cwd(&first).unwrap_or_else(|| {
                    self.decode_project_dir_name(
                        &dir.file_name().unwrap_or_default().to_string_lossy(),
                    )
                });
                normalize_path(&cwd) == normalized
            })
            .collect()
    }

    fn resolve_hit(
        &self,
        file: &Path,
        _line_number: u64,
        line: &str,
        query: &str,
    ) -> Option<SearchHit> {
        let entry = serde_json::from_str::<CcEntry>(line).ok()?;
        if !matches!(
            entry.entry_type.as_deref(),
            Some("user") | Some("assistant")
        ) || entry.is_meta == Some(true)
        {
            return None;
        }
        let mut texts: Vec<String> = Vec::new();
        match entry.message.as_ref().and_then(|m| m.content.as_ref()) {
            Some(Value::String(text)) => texts.push(text.clone()),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    for key in ["text", "thinking"] {
                        if let Some(text) = block.get(key).and_then(Value::as_str) {
                            texts.push(text.to_string());
                        }
                    }
                    for key in ["content", "input"] {
                        if let Some(value) = block.get(key) {
                            collect_string_leaves(value, &mut texts);
                        }
                    }
                }
            }
            _ => {}
        }
        let snippet = make_snippet(&texts, query)?;
        // <proj>/<session-uuid>/subagents/agent-x.jsonl → session = grandparent.
        let components: Vec<&str> = file
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        let native_session_id = match components.iter().rposition(|&c| c == "subagents") {
            Some(index) if index > 0 => components[index - 1].to_string(),
            _ => file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        };
        let cwd = entry
            .cwd
            .or_else(|| self.extract_cwd(file))
            .unwrap_or_else(|| {
                self.decode_project_dir_name(
                    &file
                        .parent()
                        .and_then(|p| p.file_name())
                        .unwrap_or_default()
                        .to_string_lossy(),
                )
            });
        Some(SearchHit::new(
            PROVIDER_ID,
            native_session_id,
            normalize_path(&cwd),
            entry.uuid,
            snippet,
        ))
    }
}

fn read_head(file: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut handle = fs::File::open(file).ok()?;
    let mut buffer = vec![0u8; limit];
    let mut filled = 0;
    while filled < limit {
        match handle.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    buffer.truncate(filled);
    Some(buffer)
}

fn read_tail(file: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut handle = fs::File::open(file).ok()?;
    let size = handle.seek(SeekFrom::End(0)).ok()?;
    let length = size.min(limit as u64);
    handle.seek(SeekFrom::Start(size - length)).ok()?;
    let mut buffer = Vec::with_capacity(length as usize);
    handle.read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

fn collect_string_leaves(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_string_leaves(item, out)),
        Value::Object(object) => object
            .values()
            .for_each(|item| collect_string_leaves(item, out)),
        _ => {}
    }
}
