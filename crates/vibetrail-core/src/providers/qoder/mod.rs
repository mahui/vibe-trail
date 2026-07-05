use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{ContentBlock, Message, MessageStub, Role, Session, SessionSummary};
use crate::provider::{LaunchMode, Provider, ProviderCapabilities, RawSession, ResumeSpec};
use crate::search::SearchHit;
use crate::store::normalize_path;
use crate::textutil::{make_snippet, sanitize_title};

const PROVIDER_ID: &str = "qoder";
/// Display truncation for tool results; full text re-read on demand.
const RESULT_PREVIEW_CHARS: usize = 2000;
/// quick_title fallback scans at most the head of the transcript.
const BOUNDED_READ: usize = 256 * 1024;

/// Qoder provider (TECH_SPEC §4.5): reads `~/.qoder/projects/**` strictly
/// read-only. The layout mirrors Claude Code's project-encoded directories,
/// but with a friendlier contract: a top-level session is the file pair
/// `<id>-session.json` (metadata: title, working_dir, tokens, chain parent)
/// plus `<id>.jsonl` (linear transcript). working_dir is the authoritative
/// cwd — no lossy directory-name decoding needed.
///
/// Not sessions: `toolu_*.jsonl` files without a `-session.json` companion
/// (tool/subagent by-products) and `<id>/` checkpoint snapshot directories.
pub struct QoderProvider {
    /// `~/.qoder/projects`; injectable for fixture tests.
    root: PathBuf,
}

/// `<id>-session.json` — the discovery-level metadata record.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct QoderSessionMeta {
    title: Option<String>,
    working_dir: Option<String>,
    parent_session_id: Option<String>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

/// One transcript line.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct QoderLine {
    id: Option<String>,
    role: Option<String>,
    parts: Vec<QoderPart>,
    created_at: Option<i64>,
    is_meta: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct QoderPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    data: Option<Value>,
}

#[derive(Debug, Default)]
struct QoderParseStats {
    undecodable_lines: u64,
    meta_lines: u64,
    ignored_roles: BTreeMap<String, u64>,
    ignored_part_types: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
struct QoderParseResult {
    messages: Vec<Message>,
    first_user_prompt: Option<String>,
    stats: QoderParseStats,
}

impl QoderProvider {
    /// Default store root; the settings layer surfaces it and may override
    /// it per provider (TECH_SPEC §12).
    pub fn default_root() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".qoder")
            .join("projects")
    }

    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root: root.unwrap_or_else(Self::default_root),
        }
    }

    fn meta_path_for(jsonl: &Path) -> PathBuf {
        let stem = jsonl.file_stem().unwrap_or_default().to_string_lossy();
        jsonl.with_file_name(format!("{stem}-session.json"))
    }

    fn read_meta(path: &Path) -> Option<QoderSessionMeta> {
        let data = fs::read(path).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// Lossy fallback only ("-Users-x-my-app" cannot distinguish "/" from
    /// "-"); the authoritative cwd is the session.json working_dir.
    fn decode_project_dir_name(name: &str) -> String {
        if name.starts_with('-') {
            name.replace('-', "/")
        } else {
            name.to_string()
        }
    }

    fn run_pipeline(&self, data: &[u8]) -> QoderParseResult {
        self.run_pipeline_with_limit(data, RESULT_PREVIEW_CHARS)
    }

    /// Linear whitelist pipeline (no CC rules here: one line = one message,
    /// ids are unique, no tree): user/assistant/tool lines become messages,
    /// `is_meta` rows and unknown roles/part types are counted, never fatal.
    fn run_pipeline_with_limit(&self, data: &[u8], result_limit: usize) -> QoderParseResult {
        let mut result = QoderParseResult::default();
        for (index, line) in data.split(|&b| b == b'\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            let Ok(entry) = serde_json::from_slice::<QoderLine>(line) else {
                result.stats.undecodable_lines += 1;
                continue;
            };
            if entry.is_meta == Some(true) {
                result.stats.meta_lines += 1;
                continue;
            }
            // role=tool carries the tool_result payloads; it renders on the
            // assistant side like every other provider's tool output.
            let role = match entry.role.as_deref() {
                Some("user") => Role::User,
                Some("assistant") | Some("tool") => Role::Assistant,
                other => {
                    let key = other.unwrap_or("<none>").to_string();
                    *result.stats.ignored_roles.entry(key).or_insert(0) += 1;
                    continue;
                }
            };
            let mut blocks = Vec::new();
            for part in &entry.parts {
                let data = part.data.as_ref().unwrap_or(&Value::Null);
                match part.part_type.as_deref() {
                    Some("text") => {
                        let Some(text) = data.get("text").and_then(Value::as_str) else {
                            continue;
                        };
                        if text.trim().is_empty() {
                            continue;
                        }
                        if role == Role::User && result.first_user_prompt.is_none() {
                            result.first_user_prompt = Some(text.to_string());
                        }
                        blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                    Some("tool_call") => {
                        let name = data
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string();
                        // input is JSON re-encoded as a string.
                        let input = data
                            .get("input")
                            .and_then(Value::as_str)
                            .map(|args| {
                                serde_json::from_str(args)
                                    .unwrap_or(Value::String(args.to_string()))
                            })
                            .unwrap_or(Value::Null);
                        blocks.push(ContentBlock::ToolUse { name, input });
                    }
                    Some("tool_result") => {
                        let mut texts = Vec::new();
                        if let Some(content) = data.get("content") {
                            collect_string_leaves(content, &mut texts);
                        }
                        let content = texts.join("\n");
                        if content.trim().is_empty() {
                            continue;
                        }
                        blocks.push(ContentBlock::ToolResult {
                            summary: content.chars().take(result_limit).collect(),
                            truncated: content.chars().count() > result_limit,
                        });
                    }
                    // Known terminator marker, nothing to display.
                    Some("finish") => {}
                    other => {
                        let key = other.unwrap_or("<none>").to_string();
                        *result.stats.ignored_part_types.entry(key).or_insert(0) += 1;
                    }
                }
            }
            if blocks.is_empty() {
                continue;
            }
            result.messages.push(Message {
                uuid: entry
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("L{}", index + 1)),
                alias_uuids: Vec::new(),
                parent_uuid: None,
                role,
                blocks,
                timestamp: ms_to_datetime(entry.created_at),
            });
        }
        result
    }

    fn read_transcript(&self, raw: &RawSession) -> Result<Vec<u8>> {
        fs::read(&raw.file_path)
            .map_err(|e| Error::Data(format!("Cannot read {}: {e}", raw.file_path.display())))
    }

    fn make_summary(
        &self,
        raw: &RawSession,
        meta: &QoderSessionMeta,
        result: &QoderParseResult,
    ) -> SessionSummary {
        let title_source = meta
            .title
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .or(result.first_user_prompt.as_deref())
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
            None,
            duration,
        )
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
                format!("→ {}", summary.lines().next().unwrap_or(""))
            }
            Some(ContentBlock::Thinking { .. }) => "(thinking)".to_string(),
            _ => String::new(),
        }
    }
}

impl Provider for QoderProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            resumable: true, // qodercli -r <session-id>
            file_based_only: true,
            has_artifacts: false,
            project_native: true,
        }
    }

    fn discover(&self) -> Result<Vec<RawSession>> {
        let Ok(dirs) = fs::read_dir(&self.root) else {
            return Ok(Vec::new()); // no store yet: empty, not an error
        };
        let mut sessions = Vec::new();
        for dir in dirs.filter_map(|e| e.ok()).map(|e| e.path()) {
            if !dir.is_dir() {
                continue;
            }
            let Ok(files) = fs::read_dir(&dir) else {
                continue;
            };
            for file in files.filter_map(|e| e.ok()).map(|e| e.path()) {
                // A session is the `<id>-session.json` + `<id>.jsonl` pair;
                // toolu_* transcripts and checkpoint dirs have no metadata
                // file and are skipped here by construction.
                let name = file.file_name().unwrap_or_default().to_string_lossy();
                let Some(native_id) = name.strip_suffix("-session.json") else {
                    continue;
                };
                let transcript = dir.join(format!("{native_id}.jsonl"));
                let Ok(metadata) = fs::metadata(&transcript) else {
                    continue; // metadata without a transcript: nothing to show
                };
                let meta = Self::read_meta(&file).unwrap_or_default();
                let cwd = meta
                    .working_dir
                    .as_deref()
                    .filter(|w| !w.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        Self::decode_project_dir_name(
                            &dir.file_name().unwrap_or_default().to_string_lossy(),
                        )
                    });
                sessions.push(RawSession {
                    provider_id: PROVIDER_ID.to_string(),
                    native_id: native_id.to_string(),
                    project_path: normalize_path(&cwd),
                    mtime: metadata
                        .modified()
                        .map(DateTime::<Utc>::from)
                        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
                    file_size: metadata.len(),
                    file_path: transcript,
                    parent_native_id: meta.parent_session_id.filter(|parent| !parent.is_empty()),
                });
            }
        }
        Ok(sessions)
    }

    fn parse(&self, raw: &RawSession) -> Result<Session> {
        let data = self.read_transcript(raw)?;
        let result = self.run_pipeline(&data);
        let meta = Self::read_meta(&Self::meta_path_for(&raw.file_path)).unwrap_or_default();
        let mut extensions = serde_json::Map::new();
        let prompt_tokens = meta.prompt_tokens.unwrap_or(0);
        let completion_tokens = meta.completion_tokens.unwrap_or(0);
        if prompt_tokens + completion_tokens > 0 {
            extensions.insert(
                "usage".to_string(),
                json!({
                    "inputTokens": prompt_tokens,
                    "outputTokens": completion_tokens,
                    "cacheCreationTokens": 0,
                    "cacheReadTokens": 0,
                }),
            );
        }
        extensions.insert(
            "debug".to_string(),
            json!({
                "undecodableLines": result.stats.undecodable_lines,
                "metaLines": result.stats.meta_lines,
                "ignoredRoles": result.stats.ignored_roles,
                "ignoredPartTypes": result.stats.ignored_part_types,
            }),
        );
        Ok(Session {
            summary: self.make_summary(raw, &meta, &result),
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

    /// `qodercli -r <session-id>` (cd into the project first) — a plain
    /// terminal resume like Claude Code and Codex.
    fn resume_spec(&self, raw: &RawSession) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            project_path: raw.project_path.clone(),
            command: vec![
                "qodercli".to_string(),
                "-r".to_string(),
                raw.native_id.clone(),
            ],
            launch: LaunchMode::default(),
        })
    }

    fn message_full(&self, raw: &RawSession, message_uuid: &str) -> Result<Option<Message>> {
        let data = self.read_transcript(raw)?;
        let result = self.run_pipeline_with_limit(&data, usize::MAX);
        Ok(result.messages.into_iter().find(|m| m.uuid == message_uuid))
    }

    /// Metadata-level: the session.json title, else the first user prompt
    /// from a bounded head read of the transcript.
    fn quick_title(&self, raw: &RawSession) -> Option<String> {
        let meta = Self::read_meta(&Self::meta_path_for(&raw.file_path)).unwrap_or_default();
        if let Some(title) = meta.title.filter(|t| !t.trim().is_empty()) {
            return Some(sanitize_title(&title));
        }
        let head = read_head(&raw.file_path, BOUNDED_READ)?;
        for line in head.split(|&b| b == b'\n') {
            let Ok(entry) = serde_json::from_slice::<QoderLine>(line) else {
                continue;
            };
            if entry.is_meta == Some(true) || entry.role.as_deref() != Some("user") {
                continue;
            }
            for part in &entry.parts {
                if part.part_type.as_deref() != Some("text") {
                    continue;
                }
                if let Some(text) = part
                    .data
                    .as_ref()
                    .and_then(|d| d.get("text"))
                    .and_then(Value::as_str)
                {
                    if !text.trim().is_empty() {
                        return Some(sanitize_title(text));
                    }
                }
            }
        }
        None
    }

    /// Plain JSONL: full-text search goes through the grep engine. Project
    /// scope narrows to the matching store directories (working_dir decides,
    /// like the Claude Code provider).
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
                let meta = files
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .find(|p| p.to_string_lossy().ends_with("-session.json"));
                let cwd = meta
                    .and_then(|m| Self::read_meta(&m))
                    .and_then(|m| m.working_dir)
                    .filter(|w| !w.is_empty())
                    .unwrap_or_else(|| {
                        Self::decode_project_dir_name(
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
        line_number: u64,
        line: &str,
        query: &str,
    ) -> Option<SearchHit> {
        // Only paired top-level transcripts resolve: toolu_* by-products and
        // checkpoint snapshots have no -session.json companion.
        if file.extension().is_none_or(|ext| ext != "jsonl") {
            return None;
        }
        let meta_path = Self::meta_path_for(file);
        if !meta_path.is_file() {
            return None;
        }
        let entry = serde_json::from_str::<QoderLine>(line).ok()?;
        if entry.is_meta == Some(true)
            || !matches!(
                entry.role.as_deref(),
                Some("user") | Some("assistant") | Some("tool")
            )
        {
            return None;
        }
        let mut texts: Vec<String> = Vec::new();
        for part in &entry.parts {
            let Some(data) = part.data.as_ref() else {
                continue;
            };
            match part.part_type.as_deref() {
                Some("text") => {
                    if let Some(text) = data.get("text").and_then(Value::as_str) {
                        texts.push(text.to_string());
                    }
                }
                Some("tool_call") => {
                    if let Some(input) = data.get("input").and_then(Value::as_str) {
                        texts.push(input.to_string());
                    }
                }
                Some("tool_result") => {
                    if let Some(content) = data.get("content") {
                        collect_string_leaves(content, &mut texts);
                    }
                }
                _ => {}
            }
        }
        let snippet = make_snippet(&texts, query)?;
        let native_id = file.file_stem().unwrap_or_default().to_string_lossy();
        let cwd = Self::read_meta(&meta_path)
            .and_then(|m| m.working_dir)
            .filter(|w| !w.is_empty())
            .unwrap_or_else(|| {
                Self::decode_project_dir_name(
                    &file
                        .parent()
                        .and_then(|p| p.file_name())
                        .unwrap_or_default()
                        .to_string_lossy(),
                )
            });
        Some(SearchHit::new(
            PROVIDER_ID,
            native_id.to_string(),
            normalize_path(&cwd),
            entry.id.or(Some(format!("L{line_number}"))),
            snippet,
        ))
    }
}

/// Depth-first string leaves of a JSON value — tool_result content can be a
/// plain string or a nested block array depending on the tool.
fn collect_string_leaves(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if !text.is_empty() {
                out.push(text.clone());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_string_leaves(item, out);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_string_leaves(item, out);
            }
        }
        _ => {}
    }
}

fn ms_to_datetime(ms: Option<i64>) -> DateTime<Utc> {
    ms.and_then(DateTime::<Utc>::from_timestamp_millis)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

fn read_head(file: &Path, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
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
