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
/// parsing the transcript body.
#[derive(Debug, Clone)]
pub struct RawSession {
    pub provider_id: String,
    pub native_id: String,
    pub file_path: PathBuf,
    pub project_path: String,
    pub mtime: DateTime<Utc>,
    pub file_size: u64,
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
    /// None when this provider (or this session) cannot be resumed.
    fn resume_spec(&self, summary: &SessionSummary) -> Option<ResumeSpec>;

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

    /// Directories the search engine may grep, scoped to one project when
    /// `project_path` is given. Empty (the default) opts the provider out of
    /// full-text search.
    fn search_roots(&self, _project_path: Option<&str>) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Map one grep-matched line back to a session/message. Return None to
    /// drop the hit (e.g. the match landed in structural metadata, not
    /// message text). Keeps format knowledge inside the provider.
    fn resolve_hit(&self, _file: &Path, _line: &str, _query: &str) -> Option<SearchHit> {
        None
    }
}
