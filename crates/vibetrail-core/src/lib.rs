//! VibeTrail core: unified session models, the provider trait, project
//! aggregation, full-text search, and resume orchestration.
//!
//! Architecture rules (TECH_SPEC §11): no GUI/terminal-UI dependencies here;
//! agent stores are opened strictly read-only; provider-specific knowledge
//! stays inside `providers::*`.

pub mod config;
pub mod error;
pub mod handoff;
pub mod model;
pub mod provider;
pub mod providers;
pub mod resume;
pub mod search;
pub mod store;
pub(crate) mod textutil;

pub use error::{Error, Result};
pub use handoff::HandoffCapsule;
pub use model::{
    AgentDef, ContentBlock, MemoryDoc, Message, MessageStub, Project, Role, Session, SessionSummary,
};
pub use provider::{LaunchMode, Provider, ProviderCapabilities, RawSession, ResumeSpec};
pub use providers::antigravity::AntigravityProvider;
pub use providers::claude_code::ClaudeCodeProvider;
pub use providers::codex::CodexProvider;
pub use providers::cursor::CursorProvider;
pub use providers::qoder::QoderProvider;
pub use resume::Resumer;
pub use search::{search_store, GrepSearchEngine, Scope, SearchEngine, SearchHit};
pub use store::{normalize_path, SessionStore};
