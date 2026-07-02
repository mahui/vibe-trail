use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{Message, MessageStub, Session, SessionSummary};
use crate::search::SearchHit;

#[derive(Debug, Clone, Copy)]
pub struct ProviderCapabilities {
    pub resumable: bool,
    pub file_based_only: bool,
    pub has_artifacts: bool,
    pub project_native: bool,
}

/// Metadata-level handle to a stored session, produced by discovery without
/// parsing the transcript body. Serializable so shells can hold a page of
/// handles and trade them back for summaries without re-discovering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSession {
    pub provider_id: String,
    pub native_id: String,
    pub file_path: PathBuf,
    pub project_path: String,
    pub mtime: DateTime<Utc>,
    pub file_size: u64,
    /// Native id of the session this one continues (resume-fork copies in
    /// Claude Code, forked_from_id / subagent thread spawns in Codex).
    /// Extracted from bytes discovery already reads — no extra I/O.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_native_id: Option<String>,
}

impl RawSession {
    pub fn composite_id(&self) -> String {
        format!("{}:{}", self.provider_id, self.native_id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSpec {
    pub project_path: String,
    /// argv of the resume command, e.g. ["claude", "--resume", "<uuid>"].
    pub command: Vec<String>,
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> ProviderCapabilities;
    /// Enumerate stored sessions. Metadata-level only: directory listing plus
    /// at most the first line/block of each file.
    fn discover(&self) -> Result<Vec<RawSession>>;
    fn parse(&self, raw: &RawSession) -> Result<Session>;
    fn outline(&self, raw: &RawSession) -> Result<Vec<MessageStub>>;
    fn page(&self, raw: &RawSession, offset: usize, limit: usize) -> Result<Vec<Message>>;
    /// None when this provider (or this session) cannot be resumed. Takes the
    /// discovery handle: resume needs only metadata and must never trigger a
    /// full transcript parse.
    fn resume_spec(&self, raw: &RawSession) -> Option<ResumeSpec>;

    /// Summary without keeping the full message array around. Providers may
    /// override with a cheaper streaming implementation.
    fn summarize(&self, raw: &RawSession) -> Result<SessionSummary> {
        Ok(self.parse(raw)?.summary)
    }

    /// Cheap title/prompt extraction for the project overview. Must stay
    /// metadata-level (bounded reads); the default falls back to a full parse.
    fn quick_title(&self, raw: &RawSession) -> Option<String> {
        self.summarize(raw).ok().map(|summary| summary.title)
    }

    /// Untruncated version of one message, re-read from disk on demand —
    /// parse() truncates tool results for display, and bulk-loading full
    /// outputs would defeat that. None when the message is unknown.
    fn message_full(&self, raw: &RawSession, message_uuid: &str) -> Result<Option<Message>> {
        let _ = (raw, message_uuid);
        Ok(None)
    }

    /// Locate sessions whose native id equals or starts with `reference`.
    /// The default scans the full discovery; providers whose ids are encoded
    /// in file names (Codex) override with a listing-only lookup so that
    /// resolving one session does not pay a whole-store metadata read.
    fn find(&self, reference: &str) -> Result<Vec<RawSession>> {
        Ok(self
            .discover()?
            .into_iter()
            .filter(|raw| raw.native_id == reference || raw.native_id.starts_with(reference))
            .collect())
    }

    /// Directories the search engine may grep, scoped to one project when
    /// `project_path` is given. Empty (the default) opts the provider out of
    /// full-text search.
    fn search_roots(&self, _project_path: Option<&str>) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Map one grep-matched line back to a session/message. Return None to
    /// drop the hit (e.g. the match landed in structural metadata, not
    /// message text). Keeps format knowledge inside the provider.
    /// `line_number` is 1-based; providers whose messages carry no intrinsic
    /// id (Codex) anchor on it instead.
    fn resolve_hit(
        &self,
        _file: &Path,
        _line_number: u64,
        _line: &str,
        _query: &str,
    ) -> Option<SearchHit> {
        None
    }

    /// ADR-3 degrade path: sessions the grep engine cannot read as plain text
    /// (e.g. Codex `.jsonl.zst`) are searched inside the provider. Results
    /// are appended to the engine's own hits.
    fn search_compressed(&self, _query: &str, _project_path: Option<&str>) -> Vec<SearchHit> {
        Vec::new()
    }
}
