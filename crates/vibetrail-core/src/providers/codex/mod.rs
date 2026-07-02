mod pipeline;

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{ContentBlock, Message, MessageStub, Session, SessionSummary};
use crate::provider::{Provider, ProviderCapabilities, RawSession, ResumeSpec};
use crate::search::SearchHit;
use crate::store::normalize_path;
use crate::textutil::{make_snippet, sanitize_title};

use pipeline::{CodexParseResult, CodexParseStats, RolloutLine};

const PROVIDER_ID: &str = "codex";
/// session_meta is the first line; base_instructions can make it large.
const FIRST_LINE_CAP: usize = 512 * 1024;

/// Codex provider: reads `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl[.zst]`
/// strictly read-only. Projects are derived from the session_meta cwd —
/// the date layout carries no project information.
pub struct CodexProvider {
    /// `~/.codex/sessions`; injectable for fixture tests.
    root: PathBuf,
}

impl CodexProvider {
    pub fn new(root: Option<PathBuf>) -> Self {
        let root = root.unwrap_or_else(|| {
            dirs::home_dir().unwrap_or_default().join(".codex").join("sessions")
        });
        Self { root }
    }

    fn rollout_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .filter(|path| is_rollout_file(path))
            .collect();
        files.sort();
        files
    }

    fn read_all(&self, path: &Path) -> Result<Vec<u8>> {
        let data = fs::read(path)
            .map_err(|e| Error::Data(format!("Cannot read {}: {e}", path.display())))?;
        if is_zst(path) {
            zstd::stream::decode_all(data.as_slice())
                .map_err(|e| Error::Data(format!("Cannot decompress {}: {e}", path.display())))
        } else {
            Ok(data)
        }
    }

    /// Bounded read of the session_meta line (works through .zst too:
    /// streaming decode stops at the first newline).
    fn read_first_line(&self, path: &Path) -> Option<Vec<u8>> {
        let file = fs::File::open(path).ok()?;
        let mut line = Vec::new();
        if is_zst(path) {
            let decoder = zstd::stream::read::Decoder::new(file).ok()?;
            read_line_capped(decoder, &mut line)?;
        } else {
            read_line_capped(file, &mut line)?;
        }
        Some(line)
    }

    fn extract_meta(&self, path: &Path) -> (Option<String>, Option<String>) {
        let Some(line) = self.read_first_line(path) else { return (None, None) };
        let Ok(entry) = serde_json::from_slice::<RolloutLine>(&line) else {
            return (None, None);
        };
        if entry.entry_type.as_deref() != Some("session_meta") {
            return (None, None);
        }
        let payload = entry.payload.unwrap_or(Value::Null);
        let cwd = payload.get("cwd").and_then(Value::as_str).map(str::to_string);
        let branch =
            payload.pointer("/git/branch").and_then(Value::as_str).map(str::to_string);
        (cwd, branch)
    }

    fn make_summary(&self, raw: &RawSession, result: &CodexParseResult) -> SessionSummary {
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
            result.git_branch.clone(),
            duration,
        )
    }

    fn debug_extension(&self, stats: &CodexParseStats) -> Value {
        json!({
            "undecodableLines": stats.undecodable_lines,
            "ignoredEntryTypes": stats.ignored_entry_types,
            "ignoredPayloadTypes": stats.ignored_payload_types,
            "contextUserMessages": stats.context_user_messages,
            "developerMessages": stats.developer_messages,
            "emptyReasoning": stats.empty_reasoning,
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
                let line: String =
                    summary.lines().next().unwrap_or("").chars().take(100).collect();
                format!("→ {line}")
            }
            Some(ContentBlock::Thinking { .. }) => "(thinking)".to_string(),
            _ => String::new(),
        }
    }

    fn hit_for_line(
        &self,
        file: &Path,
        line_number: u64,
        line: &str,
        query: &str,
    ) -> Option<SearchHit> {
        let entry = serde_json::from_str::<RolloutLine>(line).ok()?;
        if entry.entry_type.as_deref() != Some("response_item") {
            return None;
        }
        let payload = entry.payload.unwrap_or(Value::Null);
        let texts = pipeline::searchable_texts(&payload);
        let snippet = make_snippet(&texts, query)?;
        let (cwd, _) = self.extract_meta(file);
        Some(SearchHit::new(
            PROVIDER_ID,
            native_id_of(file)?,
            normalize_path(cwd.as_deref().unwrap_or("")),
            Some(format!("L{line_number}")),
            snippet,
        ))
    }
}

impl Provider for CodexProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            resumable: true,
            file_based_only: true,
            has_artifacts: false,
            project_native: false,
        }
    }

    fn discover(&self) -> Result<Vec<RawSession>> {
        // Tens of thousands of rollout files are normal; the first-line reads
        // are latency-bound, so fan them out (still no index, ADR-2).
        use rayon::prelude::*;
        let sessions = self
            .rollout_files()
            .into_par_iter()
            .filter_map(|file| {
                let native_id = native_id_of(&file)?;
                let metadata = fs::metadata(&file).ok()?;
                let (cwd, _) = self.extract_meta(&file);
                // No usable fallback: without session_meta cwd the session
                // cannot be grouped, so it lands under "/" rather than being
                // dropped.
                let project_path = normalize_path(cwd.as_deref().unwrap_or("/"));
                Some(RawSession {
                    provider_id: PROVIDER_ID.to_string(),
                    native_id,
                    project_path,
                    mtime: metadata
                        .modified()
                        .map(DateTime::<Utc>::from)
                        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
                    file_size: metadata.len(),
                    file_path: file,
                })
            })
            .collect();
        Ok(sessions)
    }

    fn parse(&self, raw: &RawSession) -> Result<Session> {
        let data = self.read_all(&raw.file_path)?;
        let result = pipeline::run(&data);
        let mut extensions = serde_json::Map::new();
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

    fn resume_spec(&self, summary: &SessionSummary) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            project_path: summary.project_path.clone(),
            command: vec![
                "codex".to_string(),
                "resume".to_string(),
                summary.native_id.clone(),
            ],
        })
    }

    /// First typed user prompt from the file head. Codex has no tail metadata
    /// equivalent to Claude Code's ai-title/last-prompt entries.
    fn quick_title(&self, raw: &RawSession) -> Option<String> {
        let file = fs::File::open(&raw.file_path).ok()?;
        let reader: Box<dyn Read> = if is_zst(&raw.file_path) {
            Box::new(zstd::stream::read::Decoder::new(file).ok()?)
        } else {
            Box::new(file)
        };
        let mut reader = BufReader::new(reader.take(FIRST_LINE_CAP as u64 * 2));
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).ok()? > 0 {
            let parsed = serde_json::from_slice::<RolloutLine>(&line);
            line.clear();
            let Ok(entry) = parsed else { continue };
            if entry.entry_type.as_deref() != Some("response_item") {
                continue;
            }
            let payload = entry.payload.unwrap_or(Value::Null);
            if payload.get("type").and_then(Value::as_str) != Some("message")
                || payload.get("role").and_then(Value::as_str) != Some("user")
            {
                continue;
            }
            for item in payload.get("content").and_then(Value::as_array).unwrap_or(&Vec::new()) {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() && !pipeline::is_context_payload(text) {
                        return Some(sanitize_title(text));
                    }
                }
            }
        }
        None
    }

    fn search_roots(&self, _project_path: Option<&str>) -> Vec<PathBuf> {
        // Date-organized storage cannot narrow by project; the engine filters
        // resolved hits against the scope instead.
        vec![self.root.clone()]
    }

    fn resolve_hit(
        &self,
        file: &Path,
        line_number: u64,
        line: &str,
        query: &str,
    ) -> Option<SearchHit> {
        self.hit_for_line(file, line_number, line, query)
    }

    /// ADR-3 degrade path: .jsonl.zst transcripts are decompressed and
    /// scanned inside the provider.
    fn search_compressed(&self, query: &str, _project_path: Option<&str>) -> Vec<SearchHit> {
        let query_lower = query.to_lowercase();
        let mut hits = Vec::new();
        for file in self.rollout_files() {
            if !is_zst(&file) {
                continue;
            }
            let Ok(data) = self.read_all(&file) else { continue };
            let Ok(text) = String::from_utf8(data) else { continue };
            let mut per_file = 0;
            for (index, line) in text.lines().enumerate() {
                if per_file >= 50 {
                    break;
                }
                if !line.to_lowercase().contains(&query_lower) {
                    continue;
                }
                if let Some(hit) = self.hit_for_line(&file, index as u64 + 1, line, query) {
                    hits.push(hit);
                    per_file += 1;
                }
            }
        }
        hits
    }
}

fn is_zst(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "zst")
}

fn is_rollout_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    name.starts_with("rollout-") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
}

/// `rollout-YYYY-MM-DDTHH-MM-SS-<session-id>.jsonl[.zst]` → session id.
fn native_id_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let name = name.strip_suffix(".zst").unwrap_or(name);
    let name = name.strip_suffix(".jsonl")?;
    let rest = name.strip_prefix("rollout-")?;
    // Skip the 19-char timestamp and its trailing dash.
    let id = rest.get(20..)?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn read_line_capped(reader: impl Read, line: &mut Vec<u8>) -> Option<usize> {
    let mut reader = BufReader::new(reader.take(FIRST_LINE_CAP as u64));
    reader.read_until(b'\n', line).ok()
}
