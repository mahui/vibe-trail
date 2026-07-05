use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{ContentBlock, Message, MessageStub, Role, Session, SessionSummary};
use crate::provider::{LaunchMode, Provider, ProviderCapabilities, RawSession, ResumeSpec};
use crate::search::SearchHit;
use crate::store::normalize_path;
use crate::textutil::{make_snippet, sanitize_title};

const PROVIDER_ID: &str = "cursor";
/// Display truncation for tool results; full text re-read on demand.
const RESULT_PREVIEW_CHARS: usize = 2000;
/// Degrade-path hit cap, mirroring the engine's own 500-hit fuse.
const SEARCH_HIT_CAP: usize = 500;
/// New-format quick_title fallback reads at most this many user bubbles.
const QUICK_TITLE_BUBBLES: usize = 5;

/// Cursor provider (EXPERIMENTAL, TECH_SPEC §4.4): reads the IDE-side session
/// store — SQLite databases with JSON values — strictly read-only (ADR-7).
///
/// Discovery is a two-level walk that never touches the (potentially huge)
/// global database body: `workspaceStorage/<hash>/workspace.json` names the
/// project, the small per-workspace database lists that workspace's composers
/// with metadata. Transcript bodies live in `globalStorage/state.vscdb` under
/// `composerData:<id>` (header) and `bubbleId:<composerId>:<bubbleId>`
/// (messages) and are only point-queried, never table-scanned.
///
/// Resume opens the Cursor client at the project (LaunchMode::GuiApp) —
/// there is no public deep link to a specific past chat (ADR-4).
pub struct CursorProvider {
    /// `~/Library/Application Support/Cursor/User`; injectable for fixtures.
    root: PathBuf,
}

#[derive(Debug, Default)]
struct CursorParseStats {
    /// Bubbles referenced by the header but absent from the store.
    missing_bubbles: u64,
    /// Bubbles whose type is neither user (1) nor assistant (2).
    unknown_bubble_types: BTreeMap<String, u64>,
    /// Whitelisted bubbles that carried no displayable content.
    empty_bubbles: u64,
    undecodable_bubbles: u64,
}

#[derive(Debug, Default)]
struct CursorParseResult {
    messages: Vec<Message>,
    /// Cursor's own chat title, when the header carries one.
    name: Option<String>,
    first_user_prompt: Option<String>,
    stats: CursorParseStats,
}

impl CursorProvider {
    /// Default store root; the settings layer surfaces it and may override
    /// it per provider (TECH_SPEC §12).
    pub fn default_root() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/Application Support/Cursor/User")
    }

    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root: root.unwrap_or_else(Self::default_root),
        }
    }

    fn global_db(&self) -> PathBuf {
        self.root.join("globalStorage/state.vscdb")
    }

    fn open_global(&self) -> Result<Connection> {
        open_read_only(&self.global_db())
            .ok_or_else(|| Error::Data(format!("Cannot open {}", self.global_db().display())))
    }

    /// Full transcript of one composer. Legacy headers embed the bubbles in a
    /// `conversation` array; the current format (`_v` ≥ 3) stores only bubble
    /// references in `fullConversationHeadersOnly` and the bodies are
    /// point-queried per bubble. Both must stay supported (TECH_SPEC §4.4).
    fn run_pipeline(&self, conn: &Connection, raw: &RawSession) -> Result<CursorParseResult> {
        self.run_pipeline_with_limit(conn, raw, RESULT_PREVIEW_CHARS)
    }

    fn run_pipeline_with_limit(
        &self,
        conn: &Connection,
        raw: &RawSession,
        result_limit: usize,
    ) -> Result<CursorParseResult> {
        let header = kv_get(conn, &format!("composerData:{}", raw.native_id))
            .ok_or_else(|| Error::Data(format!("Composer {} not found", raw.native_id)))?;
        let header: Value = serde_json::from_str(&header)
            .map_err(|e| Error::Data(format!("Undecodable composer header: {e}")))?;
        let mut result = CursorParseResult {
            name: header
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
            ..Default::default()
        };
        let fallback_ts = ms_to_datetime(header.get("createdAt").and_then(Value::as_i64));

        let legacy = header
            .get("conversation")
            .and_then(Value::as_array)
            .filter(|conversation| !conversation.is_empty());
        if let Some(conversation) = legacy {
            for bubble in conversation {
                self.push_bubble(bubble, fallback_ts, result_limit, &mut result);
            }
            return Ok(result);
        }
        let headers = header
            .get("fullConversationHeadersOnly")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for head in &headers {
            let Some(bubble_id) = head.get("bubbleId").and_then(Value::as_str) else {
                result.stats.undecodable_bubbles += 1;
                continue;
            };
            let key = format!("bubbleId:{}:{bubble_id}", raw.native_id);
            let Some(body) = kv_get(conn, &key) else {
                result.stats.missing_bubbles += 1;
                continue;
            };
            let Ok(bubble) = serde_json::from_str::<Value>(&body) else {
                result.stats.undecodable_bubbles += 1;
                continue;
            };
            self.push_bubble(&bubble, fallback_ts, result_limit, &mut result);
        }
        Ok(result)
    }

    /// Whitelist conversion of one bubble (rule analog to CC rule 3): user
    /// text, assistant text, thinking and toolFormerData become blocks;
    /// unknown bubble types are counted, never fatal; content-free bubbles
    /// (status/placeholder rows are common) are dropped.
    fn push_bubble(
        &self,
        bubble: &Value,
        fallback_ts: DateTime<Utc>,
        result_limit: usize,
        result: &mut CursorParseResult,
    ) {
        let Some(bubble_id) = bubble.get("bubbleId").and_then(Value::as_str) else {
            result.stats.undecodable_bubbles += 1;
            return;
        };
        let role = match bubble.get("type").and_then(Value::as_i64) {
            Some(1) => Role::User,
            Some(2) => Role::Assistant,
            other => {
                let key = other.map_or("<none>".to_string(), |t| t.to_string());
                *result.stats.unknown_bubble_types.entry(key).or_insert(0) += 1;
                return;
            }
        };
        let mut blocks = Vec::new();
        if let Some(thinking) = bubble
            .pointer("/thinking/text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
        {
            blocks.push(ContentBlock::Thinking {
                text: thinking.to_string(),
            });
        } else if let Some(thinking_blocks) =
            bubble.get("allThinkingBlocks").and_then(Value::as_array)
        {
            for block in thinking_blocks {
                if let Some(text) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                {
                    blocks.push(ContentBlock::Thinking {
                        text: text.to_string(),
                    });
                }
            }
        }
        if let Some(text) = bubble
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
        {
            if role == Role::User && result.first_user_prompt.is_none() {
                result.first_user_prompt = Some(text.to_string());
            }
            blocks.push(ContentBlock::Text {
                text: text.to_string(),
            });
        }
        if let Some(tool) = bubble.get("toolFormerData").filter(|t| t.is_object()) {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "tool_{}",
                        tool.get("tool").and_then(Value::as_i64).unwrap_or(0)
                    )
                });
            // rawArgs/params/result are JSON re-encoded as strings.
            let input = tool
                .get("rawArgs")
                .or_else(|| tool.get("params"))
                .and_then(Value::as_str)
                .map(|args| serde_json::from_str(args).unwrap_or(Value::String(args.to_string())))
                .unwrap_or(Value::Null);
            blocks.push(ContentBlock::ToolUse { name, input });
            if let Some(tool_result) = tool
                .get("result")
                .and_then(Value::as_str)
                .filter(|r| !r.trim().is_empty())
            {
                blocks.push(ContentBlock::ToolResult {
                    summary: tool_result.chars().take(result_limit).collect(),
                    truncated: tool_result.chars().count() > result_limit,
                });
            }
        }
        if blocks.is_empty() {
            result.stats.empty_bubbles += 1;
            return;
        }
        // Bubbles carry no reliable per-message timestamp; the composer's
        // createdAt anchors the whole transcript (duration degrades to 0).
        result.messages.push(Message {
            uuid: bubble_id.to_string(),
            alias_uuids: Vec::new(),
            parent_uuid: None,
            role,
            blocks,
            timestamp: fallback_ts,
        });
    }

    fn make_summary(&self, raw: &RawSession, result: &CursorParseResult) -> SessionSummary {
        let title_source = result
            .name
            .as_deref()
            .or(result.first_user_prompt.as_deref())
            .unwrap_or("");
        let title = sanitize_title(title_source);
        let title = if title.is_empty() {
            raw.native_id.chars().take(8).collect()
        } else {
            title
        };
        SessionSummary::new(
            PROVIDER_ID,
            &raw.native_id,
            raw.project_path.clone(),
            title,
            raw.mtime,
            result.messages.len(),
            None,
            0.0,
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

    /// Bubble texts a search should look at, mirroring the parse whitelist.
    fn searchable_texts(bubble: &Value) -> Vec<String> {
        let mut texts = Vec::new();
        if let Some(text) = bubble.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                texts.push(text.to_string());
            }
        }
        if let Some(thinking) = bubble.pointer("/thinking/text").and_then(Value::as_str) {
            if !thinking.is_empty() {
                texts.push(thinking.to_string());
            }
        }
        if let Some(tool_result) = bubble
            .pointer("/toolFormerData/result")
            .and_then(Value::as_str)
        {
            if !tool_result.is_empty() {
                texts.push(tool_result.to_string());
            }
        }
        texts
    }
}

impl Provider for CursorProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            // Client-level resume: open Cursor at the project (ADR-4).
            resumable: true,
            file_based_only: true,
            has_artifacts: false,
            project_native: false, // derived from the workspace mapping
        }
    }

    fn discover(&self) -> Result<Vec<RawSession>> {
        let Ok(workspaces) = fs::read_dir(self.root.join("workspaceStorage")) else {
            return Ok(Vec::new()); // no store yet: empty, not an error
        };
        let global_db = self.global_db();
        // Composers without a workspace mapping (deleted workspaces) are
        // ignored by design (TECH_SPEC §4.4); duplicates across workspaces
        // keep the newest sighting.
        let mut by_id: BTreeMap<String, RawSession> = BTreeMap::new();
        for entry in workspaces.filter_map(|e| e.ok()) {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(project_path) = read_workspace_folder(&dir.join("workspace.json")) else {
                continue; // multi-root or empty workspaces carry no folder
            };
            let Some(conn) = open_read_only(&dir.join("state.vscdb")) else {
                continue;
            };
            let Some(payload) = item_get(&conn, "composer.composerData") else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&payload) else {
                continue;
            };
            let Some(composers) = value.get("allComposers").and_then(Value::as_array) else {
                continue;
            };
            for composer in composers {
                let Some(id) = composer.get("composerId").and_then(Value::as_str) else {
                    continue;
                };
                let mtime = ms_to_datetime(
                    composer
                        .get("lastUpdatedAt")
                        .and_then(Value::as_i64)
                        .or_else(|| composer.get("createdAt").and_then(Value::as_i64)),
                );
                if by_id.get(id).is_some_and(|prev| prev.mtime >= mtime) {
                    continue;
                }
                by_id.insert(
                    id.to_string(),
                    RawSession {
                        provider_id: PROVIDER_ID.to_string(),
                        native_id: id.to_string(),
                        file_path: global_db.clone(),
                        project_path: project_path.clone(),
                        mtime,
                        // One shared database file: no per-session size.
                        file_size: 0,
                        parent_native_id: None,
                    },
                );
            }
        }
        Ok(by_id.into_values().collect())
    }

    fn parse(&self, raw: &RawSession) -> Result<Session> {
        let conn = self.open_global()?;
        let result = self.run_pipeline(&conn, raw)?;
        let mut extensions = serde_json::Map::new();
        extensions.insert("experimental".to_string(), Value::Bool(true));
        extensions.insert(
            "debug".to_string(),
            json!({
                "missingBubbles": result.stats.missing_bubbles,
                "unknownBubbleTypes": result.stats.unknown_bubble_types,
                "emptyBubbles": result.stats.empty_bubbles,
                "undecodableBubbles": result.stats.undecodable_bubbles,
            }),
        );
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

    /// Client-level resume (ADR-4 GuiApp): open Cursor at the project. No
    /// public deep link reaches a specific past chat yet; the user picks the
    /// session from Cursor's chat history.
    fn resume_spec(&self, raw: &RawSession) -> Option<ResumeSpec> {
        Some(ResumeSpec {
            project_path: raw.project_path.clone(),
            command: vec![
                "open".to_string(),
                "-a".to_string(),
                "Cursor".to_string(),
                raw.project_path.clone(),
            ],
            launch: LaunchMode::GuiApp,
        })
    }

    fn message_full(&self, raw: &RawSession, message_uuid: &str) -> Result<Option<Message>> {
        let conn = self.open_global()?;
        let result = self.run_pipeline_with_limit(&conn, raw, usize::MAX)?;
        Ok(result.messages.into_iter().find(|m| m.uuid == message_uuid))
    }

    /// Header-level title: Cursor's own chat name when present, else the
    /// first user bubble (legacy: inline; current: at most a handful of
    /// point queries). Stays metadata-bounded either way.
    fn quick_title(&self, raw: &RawSession) -> Option<String> {
        let conn = open_read_only(&self.global_db())?;
        let header = kv_get(&conn, &format!("composerData:{}", raw.native_id))?;
        let header: Value = serde_json::from_str(&header).ok()?;
        if let Some(name) = header
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            return Some(sanitize_title(name));
        }
        if let Some(conversation) = header.get("conversation").and_then(Value::as_array) {
            for bubble in conversation {
                if bubble.get("type").and_then(Value::as_i64) == Some(1) {
                    if let Some(text) = bubble.get("text").and_then(Value::as_str) {
                        if !text.trim().is_empty() {
                            return Some(sanitize_title(text));
                        }
                    }
                }
            }
        }
        let headers = header.get("fullConversationHeadersOnly")?.as_array()?;
        for head in headers
            .iter()
            .filter(|h| h.get("type").and_then(Value::as_i64) == Some(1))
            .take(QUICK_TITLE_BUBBLES)
        {
            let bubble_id = head.get("bubbleId").and_then(Value::as_str)?;
            let key = format!("bubbleId:{}:{bubble_id}", raw.native_id);
            let Some(body) = kv_get(&conn, &key) else {
                continue;
            };
            let Ok(bubble) = serde_json::from_str::<Value>(&body) else {
                continue;
            };
            if let Some(text) = bubble.get("text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    return Some(sanitize_title(text));
                }
            }
        }
        None
    }

    /// The grep engine cannot read SQLite: no roots, everything goes through
    /// the ADR-3 degrade path below.
    fn search_roots(&self, _project_path: Option<&str>) -> Vec<PathBuf> {
        Vec::new()
    }

    /// ADR-3 degrade path over the bubble store. Bubble bodies are reached
    /// with index-friendly point/range queries only — never a table scan of
    /// the multi-GB global database (TECH_SPEC §4.4). Composers are searched
    /// in parallel (rayon, like discovery elsewhere); SQLite is safe under
    /// concurrent read-only connections, so each task opens its own.
    fn search_compressed(&self, query: &str, project_path: Option<&str>) -> Vec<SearchHit> {
        use rayon::prelude::*;
        let scope = project_path.map(normalize_path);
        let Ok(sessions) = self.discover() else {
            return Vec::new();
        };
        let needle = query.to_lowercase();
        let mut hits: Vec<SearchHit> = sessions
            .par_iter()
            .filter(|raw| scope.as_deref().is_none_or(|s| s == raw.project_path))
            .flat_map_iter(|raw| self.search_one_composer(raw, query, &needle))
            .collect();
        hits.truncate(SEARCH_HIT_CAP);
        hits
    }
}

impl CursorProvider {
    fn search_one_composer(&self, raw: &RawSession, query: &str, needle: &str) -> Vec<SearchHit> {
        let Some(conn) = open_read_only(&self.global_db()) else {
            return Vec::new();
        };
        let Some(header) = kv_get(&conn, &format!("composerData:{}", raw.native_id)) else {
            return Vec::new();
        };
        let Ok(header) = serde_json::from_str::<Value>(&header) else {
            return Vec::new();
        };
        let mut bubbles: Vec<Value> = Vec::new();
        let legacy = header
            .get("conversation")
            .and_then(Value::as_array)
            .filter(|conversation| !conversation.is_empty());
        if let Some(conversation) = legacy {
            bubbles.extend(conversation.iter().cloned());
        } else {
            // Range over `bubbleId:<composerId>:*` ( ';' = ':' + 1 ).
            let low = format!("bubbleId:{}:", raw.native_id);
            let high = format!("bubbleId:{};", raw.native_id);
            let Ok(mut stmt) =
                conn.prepare("SELECT value FROM cursorDiskKV WHERE key >= ?1 AND key < ?2")
            else {
                return Vec::new();
            };
            let rows = stmt.query_map([&low, &high], |row| Ok(value_to_string(row.get_ref(0)?)));
            if let Ok(rows) = rows {
                for row in rows.flatten().flatten() {
                    if let Ok(bubble) = serde_json::from_str::<Value>(&row) {
                        bubbles.push(bubble);
                    }
                }
            }
        }
        let mut hits = Vec::new();
        for bubble in &bubbles {
            let texts = Self::searchable_texts(bubble);
            if !texts.iter().any(|t| t.to_lowercase().contains(needle)) {
                continue;
            }
            let Some(snippet) = make_snippet(&texts, query) else {
                continue;
            };
            hits.push(SearchHit::new(
                PROVIDER_ID,
                raw.native_id.clone(),
                raw.project_path.clone(),
                bubble
                    .get("bubbleId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                snippet,
            ));
            if hits.len() >= SEARCH_HIT_CAP {
                break;
            }
        }
        hits
    }
}

/// Read-only SQLite open (ADR-7): Cursor keeps writing while we read, so this
/// is `mode=ro` plus a busy timeout — never immutable, never a write.
fn open_read_only(path: &Path) -> Option<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    conn.busy_timeout(Duration::from_millis(1000)).ok()?;
    Some(conn)
}

/// TEXT or BLOB → UTF-8 string; the value column is declared BLOB but Cursor
/// stores JSON text in either affinity depending on version.
fn value_to_string(value: rusqlite::types::ValueRef<'_>) -> Option<String> {
    match value {
        rusqlite::types::ValueRef::Text(bytes) | rusqlite::types::ValueRef::Blob(bytes) => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => None,
    }
}

/// Point query against the global kv table.
fn kv_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM cursorDiskKV WHERE key = ?1",
        [key],
        |row| Ok(value_to_string(row.get_ref(0)?)),
    )
    .ok()
    .flatten()
}

/// Point query against a workspace ItemTable.
fn item_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
        Ok(value_to_string(row.get_ref(0)?))
    })
    .ok()
    .flatten()
}

/// `workspace.json` → normalized project path. Only single-folder workspaces
/// (`{"folder": "file:///…"}`) map to a project; multi-root ones are skipped.
fn read_workspace_folder(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&data).ok()?;
    let folder = value.get("folder")?.as_str()?;
    let raw_path = folder.strip_prefix("file://")?;
    let decoded = percent_decode(raw_path);
    if decoded.is_empty() {
        return None;
    }
    Some(normalize_path(&decoded))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn ms_to_datetime(ms: Option<i64>) -> DateTime<Utc> {
    ms.and_then(DateTime::<Utc>::from_timestamp_millis)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}
