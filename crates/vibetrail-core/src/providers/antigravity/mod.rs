use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{ContentBlock, Message, MessageStub, Role, Session, SessionSummary};
use crate::provider::{Provider, ProviderCapabilities, RawSession, ResumeSpec};
use crate::search::SearchHit;
use crate::store::normalize_path;
use crate::textutil::{make_snippet, sanitize_title};

const PROVIDER_ID: &str = "antigravity";
/// Display truncation for tool results; full text re-read on demand.
const RESULT_PREVIEW_CHARS: usize = 2000;
/// Discovery-time reads stay bounded even though transcripts are small.
const BOUNDED_READ: usize = 1024 * 1024;

/// Antigravity provider (EXPERIMENTAL, TECH_SPEC §4.3): reads only the
/// file-based part of `~/.gemini/antigravity/brain/<conversation-id>/` —
/// `.system_generated/logs/transcript.jsonl` plus the markdown artifacts next
/// to it. The IDE-side .pb transcripts and LanguageServer API are out of
/// scope per ADR-6 (they need a live host process).
///
/// Transcripts carry no cwd; the project is derived heuristically from the
/// `file://` paths the agent touched (longest common directory prefix).
pub struct AntigravityProvider {
    /// `~/.gemini/antigravity/brain`; injectable for fixture tests.
    root: PathBuf,
}

/// One step of a transcript.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AgyStep {
    step_index: Option<u64>,
    source: Option<String>,
    #[serde(rename = "type")]
    step_type: Option<String>,
    created_at: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Default)]
struct AgyParseStats {
    undecodable_lines: u64,
    ignored_step_types: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
struct AgyParseResult {
    messages: Vec<Message>,
    project_path: Option<String>,
    first_user_prompt: Option<String>,
    stats: AgyParseStats,
}

impl AntigravityProvider {
    /// Default store root; the settings layer surfaces it and may override
    /// it per provider (TECH_SPEC §12).
    pub fn default_root() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".gemini/antigravity/brain")
    }

    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root: root.unwrap_or_else(Self::default_root),
        }
    }

    fn transcript_path(&self, conversation_dir: &Path) -> PathBuf {
        conversation_dir.join(".system_generated/logs/transcript.jsonl")
    }

    fn run_pipeline(&self, data: &[u8]) -> AgyParseResult {
        self.run_pipeline_with_limit(data, RESULT_PREVIEW_CHARS)
    }

    fn run_pipeline_with_limit(&self, data: &[u8], result_limit: usize) -> AgyParseResult {
        let mut result = AgyParseResult::default();
        let mut touched_paths: Vec<PathBuf> = Vec::new();
        for line in data.split(|&b| b == b'\n') {
            if line.is_empty() {
                continue;
            }
            let Ok(step) = serde_json::from_slice::<AgyStep>(line) else {
                result.stats.undecodable_lines += 1;
                continue;
            };
            let content = step.content.as_deref().unwrap_or("");
            collect_file_paths(content, &mut touched_paths);
            let timestamp = step
                .created_at
                .as_deref()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
            let uuid = format!("S{}", step.step_index.unwrap_or(u64::MAX));
            // Whitelist by step type (rule analog to CC rule 3).
            let message = match step.step_type.as_deref() {
                Some("USER_INPUT") => {
                    let text = extract_user_request(content);
                    if result.first_user_prompt.is_none() && !text.is_empty() {
                        result.first_user_prompt = Some(text.clone());
                    }
                    Some((Role::User, vec![ContentBlock::Text { text }]))
                }
                Some("PLANNER_RESPONSE") => Some((
                    Role::Assistant,
                    vec![ContentBlock::Text {
                        text: content.to_string(),
                    }],
                )),
                Some(
                    tool @ ("VIEW_FILE" | "GREP_SEARCH" | "RUN_COMMAND" | "CODE_ACTION"
                    | "LIST_DIRECTORY" | "SEARCH_WEB" | "READ_URL_CONTENT"),
                ) => {
                    let summary: String = content.chars().take(result_limit).collect();
                    Some((
                        Role::Assistant,
                        vec![
                            ContentBlock::ToolUse {
                                name: tool.to_lowercase(),
                                input: Value::Null,
                            },
                            ContentBlock::ToolResult {
                                summary,
                                truncated: content.chars().count() > result_limit,
                            },
                        ],
                    ))
                }
                other => {
                    let key = other.unwrap_or("<none>").to_string();
                    *result.stats.ignored_step_types.entry(key).or_insert(0) += 1;
                    None
                }
            };
            if let Some((role, blocks)) = message {
                if blocks
                    .iter()
                    .any(|b| !matches!(b, ContentBlock::Text { text } if text.is_empty()))
                {
                    result.messages.push(Message {
                        uuid,
                        parent_uuid: None,
                        role,
                        blocks,
                        timestamp,
                    });
                }
            }
        }
        result.project_path = derive_project(&touched_paths);
        result
    }

    fn read_transcript(&self, raw: &RawSession) -> Result<Vec<u8>> {
        fs::read(&raw.file_path)
            .map_err(|e| Error::Data(format!("Cannot read {}: {e}", raw.file_path.display())))
    }

    /// Markdown artifacts (plan/task/walkthrough) beside the transcript →
    /// extensions. This is the provider's differentiator (has_artifacts).
    fn collect_artifacts(&self, conversation_dir: &Path) -> Vec<Value> {
        let Ok(entries) = fs::read_dir(conversation_dir) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|ext| ext == "md")
                    && !p.to_string_lossy().ends_with(".metadata.json")
            })
            .collect();
        files.sort();
        files
            .into_iter()
            .map(|file| {
                let mut artifact = json!({
                    "name": file.file_name().unwrap_or_default().to_string_lossy(),
                    "path": file.to_string_lossy(),
                });
                let meta_path = file.with_file_name(format!(
                    "{}.metadata.json",
                    file.file_name().unwrap_or_default().to_string_lossy()
                ));
                if let Ok(meta) = fs::read(&meta_path) {
                    if let Ok(value) = serde_json::from_slice::<Value>(&meta) {
                        if let Some(summary) = value.get("summary") {
                            artifact["summary"] = summary.clone();
                        }
                        if let Some(kind) = value.get("artifactType") {
                            artifact["artifactType"] = kind.clone();
                        }
                    }
                }
                artifact
            })
            .collect()
    }

    fn make_summary(&self, raw: &RawSession, result: &AgyParseResult) -> SessionSummary {
        let title = sanitize_title(result.first_user_prompt.as_deref().unwrap_or(""));
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
        match message.blocks.first() {
            Some(ContentBlock::Text { text }) => text
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect(),
            Some(ContentBlock::ToolUse { name, .. }) => format!("⚙ {name}"),
            Some(ContentBlock::ToolResult { summary, .. }) => {
                format!("→ {}", summary.lines().next().unwrap_or(""))
            }
            Some(ContentBlock::Thinking { .. }) => "(thinking)".to_string(),
            None => String::new(),
        }
    }
}

impl Provider for AntigravityProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            resumable: false,
            file_based_only: false, // .pb/LanguageServer parts are not covered
            has_artifacts: true,
            project_native: false,
        }
    }

    fn discover(&self) -> Result<Vec<RawSession>> {
        let Ok(dirs) = fs::read_dir(&self.root) else {
            return Ok(Vec::new()); // no store yet: empty, not an error
        };
        let mut sessions = Vec::new();
        for entry in dirs.filter_map(|e| e.ok()) {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let transcript = self.transcript_path(&dir);
            let Ok(metadata) = fs::metadata(&transcript) else {
                continue;
            };
            // Project derivation needs the file paths the agent touched;
            // transcripts are small, so a bounded scan stays cheap.
            let head = read_head(&transcript, BOUNDED_READ).unwrap_or_default();
            let mut touched: Vec<PathBuf> = Vec::new();
            for line in head.split(|&b| b == b'\n') {
                if let Ok(step) = serde_json::from_slice::<AgyStep>(line) {
                    collect_file_paths(step.content.as_deref().unwrap_or(""), &mut touched);
                }
            }
            let project_path =
                derive_project(&touched).unwrap_or_else(|| "(antigravity)".to_string());
            sessions.push(RawSession {
                provider_id: PROVIDER_ID.to_string(),
                native_id: entry.file_name().to_string_lossy().to_string(),
                project_path: if project_path.starts_with('/') {
                    normalize_path(&project_path)
                } else {
                    project_path
                },
                mtime: metadata
                    .modified()
                    .map(DateTime::<Utc>::from)
                    .unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
                file_size: metadata.len(),
                file_path: transcript,
                parent_native_id: None,
            });
        }
        Ok(sessions)
    }

    fn parse(&self, raw: &RawSession) -> Result<Session> {
        let data = self.read_transcript(raw)?;
        let result = self.run_pipeline(&data);
        let conversation_dir = raw
            .file_path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut extensions = serde_json::Map::new();
        let artifacts = self.collect_artifacts(&conversation_dir);
        if !artifacts.is_empty() {
            extensions.insert("artifacts".to_string(), Value::Array(artifacts));
        }
        extensions.insert("experimental".to_string(), Value::Bool(true));
        extensions.insert(
            "debug".to_string(),
            json!({
                "undecodableLines": result.stats.undecodable_lines,
                "ignoredStepTypes": result.stats.ignored_step_types,
            }),
        );
        Ok(Session {
            summary: self.make_summary(raw, &result),
            messages: result.messages,
            extensions,
        })
    }

    fn message_full(&self, raw: &RawSession, message_uuid: &str) -> Result<Option<Message>> {
        let data = self.read_transcript(raw)?;
        let result = self.run_pipeline_with_limit(&data, usize::MAX);
        Ok(result.messages.into_iter().find(|m| m.uuid == message_uuid))
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

    /// Not resumable: Antigravity has no CLI resume entry point (ADR-6 keeps
    /// the LanguageServer path out of scope).
    fn resume_spec(&self, _raw: &RawSession) -> Option<ResumeSpec> {
        None
    }

    fn quick_title(&self, raw: &RawSession) -> Option<String> {
        let head = read_head(&raw.file_path, BOUNDED_READ)?;
        for line in head.split(|&b| b == b'\n') {
            let Ok(step) = serde_json::from_slice::<AgyStep>(line) else {
                continue;
            };
            if step.step_type.as_deref() == Some("USER_INPUT") {
                let text = extract_user_request(step.content.as_deref().unwrap_or(""));
                if !text.is_empty() {
                    return Some(sanitize_title(&text));
                }
            }
        }
        None
    }

    fn search_roots(&self, _project_path: Option<&str>) -> Vec<PathBuf> {
        // Conversations don't map to project directories; the engine filters
        // resolved hits against the scope.
        vec![self.root.clone()]
    }

    fn resolve_hit(
        &self,
        file: &Path,
        _line_number: u64,
        line: &str,
        query: &str,
    ) -> Option<SearchHit> {
        let step = serde_json::from_str::<AgyStep>(line).ok()?;
        if !matches!(
            step.step_type.as_deref(),
            Some("USER_INPUT")
                | Some("PLANNER_RESPONSE")
                | Some("RUN_COMMAND")
                | Some("CODE_ACTION")
        ) {
            return None;
        }
        let texts = vec![step.content.unwrap_or_default()];
        let snippet = make_snippet(&texts, query)?;
        // <brain>/<conversation-id>/.system_generated/logs/transcript.jsonl
        let conversation_dir = file.parent()?.parent()?.parent()?;
        let native_id = conversation_dir.file_name()?.to_string_lossy().to_string();
        // Small store: re-deriving the project per hit is acceptable.
        let project_path = self
            .discover()
            .ok()?
            .into_iter()
            .find(|s| s.native_id == native_id)
            .map(|s| s.project_path)
            .unwrap_or_else(|| "(antigravity)".to_string());
        Some(SearchHit::new(
            PROVIDER_ID,
            native_id,
            project_path,
            Some(format!("S{}", step.step_index.unwrap_or(u64::MAX))),
            snippet,
        ))
    }
}

/// `<USER_REQUEST>…</USER_REQUEST>` inner text; falls back to the raw content
/// minus metadata tags.
fn extract_user_request(content: &str) -> String {
    if let Some(start) = content.find("<USER_REQUEST>") {
        let rest = &content[start + "<USER_REQUEST>".len()..];
        let end = rest.find("</USER_REQUEST>").unwrap_or(rest.len());
        return rest[..end].trim().to_string();
    }
    content.trim().to_string()
}

/// Pull `file:///…` paths out of step content, ignoring the brain's own
/// artifact writes under ~/.gemini.
fn collect_file_paths(content: &str, out: &mut Vec<PathBuf>) {
    for (index, _) in content.match_indices("file:///") {
        let rest = &content[index + "file://".len()..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '`' || c == '"' || c == '\'' || c == ')')
            .unwrap_or(rest.len());
        let path = &rest[..end];
        if !path.contains("/.gemini/") && path.len() > 1 {
            out.push(PathBuf::from(path));
        }
    }
}

/// Longest common directory prefix of the touched files — the best available
/// stand-in for a cwd that the format does not record.
fn derive_project(paths: &[PathBuf]) -> Option<String> {
    let dirs: Vec<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
    let first = dirs.first()?;
    let mut common: Vec<std::path::Component> = first.components().collect();
    for dir in &dirs[1..] {
        let components: Vec<std::path::Component> = dir.components().collect();
        let shared = common
            .iter()
            .zip(&components)
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(shared);
    }
    if common.len() <= 1 {
        return None; // only "/" in common: no meaningful project
    }
    let mut path = PathBuf::new();
    for component in common {
        path.push(component.as_os_str());
    }
    Some(path.to_string_lossy().to_string())
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
