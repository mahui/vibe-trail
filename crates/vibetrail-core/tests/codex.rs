//! Fixture parity tests for the Codex provider, plus the cross-provider
//! aggregation that M4 exists to validate: two providers, one store, one
//! project view.

use std::fs;
use std::path::PathBuf;

use vibetrail_core::{
    search_store, ClaudeCodeProvider, CodexProvider, ContentBlock, Provider, RawSession, Role,
    Scope, SessionStore,
};

const SESSION_ID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-sessions")
}

fn provider() -> CodexProvider {
    CodexProvider::new(Some(fixture_root()))
}

fn raw() -> RawSession {
    provider()
        .discover()
        .unwrap()
        .into_iter()
        .find(|raw| raw.native_id == SESSION_ID)
        .expect("fixture session present")
}

#[test]
fn discovers_rollout_files_with_meta_cwd() {
    let sessions = provider().discover().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].native_id, SESSION_ID);
    assert_eq!(sessions[0].project_path, "/Users/tester/demo-app");
}

#[test]
fn parses_whitelisted_response_items_only() {
    let session = provider().parse(&raw()).unwrap();
    // user prompt, reasoning, function_call, function_call_output, assistant.
    assert_eq!(session.messages.len(), 5);
    let kinds: Vec<(&Role, usize)> =
        session.messages.iter().map(|m| (&m.role, m.blocks.len())).collect();
    assert_eq!(kinds.len(), 5);
    assert!(matches!(&session.messages[0].blocks[0], ContentBlock::Text { text } if text == "Add a retry to the flaky upload"));
    assert!(matches!(&session.messages[1].blocks[0], ContentBlock::Thinking { text } if text == "Look at the uploader first."));
    let ContentBlock::ToolUse { name, input } = &session.messages[2].blocks[0] else {
        panic!("expected tool_use");
    };
    assert_eq!(name, "exec_command");
    // JSON-encoded argument strings are decoded for display.
    assert_eq!(input["cmd"], "ls src");
    assert!(matches!(&session.messages[3].blocks[0], ContentBlock::ToolResult { summary, truncated: false } if summary == "upload.rs\nmain.rs"));
    assert_eq!(session.messages[4].role, Role::Assistant);
}

#[test]
fn line_number_uuids_anchor_messages() {
    let session = provider().parse(&raw()).unwrap();
    let uuids: Vec<&str> = session.messages.iter().map(|m| m.uuid.as_str()).collect();
    assert_eq!(uuids, ["L4", "L5", "L6", "L7", "L8"]);
}

#[test]
fn context_and_duplicate_event_entries_are_filtered() {
    let session = provider().parse(&raw()).unwrap();
    let debug = &session.extensions["debug"];
    assert_eq!(debug["contextUserMessages"], 1);
    assert_eq!(debug["developerMessages"], 1);
    assert_eq!(debug["undecodableLines"], 1);
    assert_eq!(debug["ignoredEntryTypes"]["event_msg"], 1);
    assert_eq!(debug["ignoredEntryTypes"]["future-type"], 1);
    assert_eq!(debug["ignoredPayloadTypes"]["rate_limit"], 1);
}

#[test]
fn summary_fields() {
    let summary = provider().parse(&raw()).unwrap().summary;
    assert_eq!(summary.id, format!("codex:{SESSION_ID}"));
    assert_eq!(summary.title, "Add a retry to the flaky upload");
    assert_eq!(summary.message_count, 5);
    assert_eq!(summary.git_branch.as_deref(), Some("main"));
    // L4 (09:00:00.300) → L8 (09:02:00.000).
    assert_eq!(summary.duration, 119.7);
}

#[test]
fn quick_title_skips_context_payloads() {
    let title = provider().quick_title(&raw());
    assert_eq!(title.as_deref(), Some("Add a retry to the flaky upload"));
}

#[test]
fn resume_command_matches_codex_cli() {
    let summary = provider().parse(&raw()).unwrap().summary;
    let spec = provider().resume_spec(&summary).unwrap();
    assert_eq!(spec.command, ["codex", "resume", SESSION_ID]);
    assert_eq!(spec.project_path, "/Users/tester/demo-app");
}

#[test]
fn search_resolves_line_anchor() {
    let store = SessionStore::new(vec![Box::new(provider())]);
    let hits = search_store(&store, "exponential backoff", &Scope::default()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, format!("codex:{SESSION_ID}"));
    assert_eq!(hits[0].message_uuid.as_deref(), Some("L8"));
}

#[test]
fn search_ignores_context_payload_matches() {
    let store = SessionStore::new(vec![Box::new(provider())]);
    // "environment_context" only appears inside the filtered context payload.
    let hits = search_store(&store, "environment_context", &Scope::default()).unwrap();
    assert!(hits.is_empty());
}

// ADR-3 degrade path: .jsonl.zst sessions must discover, parse, and search.
#[test]
fn zst_sessions_discover_parse_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let day_dir = dir.path().join("2026/06/11");
    fs::create_dir_all(&day_dir).unwrap();
    let plain = fs::read(
        fixture_root().join("2026/06/10/rollout-2026-06-10T12-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl"),
    )
    .unwrap();
    let compressed = zstd::encode_all(plain.as_slice(), 3).unwrap();
    fs::write(
        day_dir.join("rollout-2026-06-11T09-00-00-bbbbbbbb-cccc-dddd-eeee-ffffffffffff.jsonl.zst"),
        compressed,
    )
    .unwrap();

    let provider = CodexProvider::new(Some(dir.path().to_path_buf()));
    let sessions = provider.discover().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].native_id, "bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
    assert_eq!(sessions[0].project_path, "/Users/tester/demo-app");

    let session = provider.parse(&sessions[0]).unwrap();
    assert_eq!(session.messages.len(), 5);

    let store = SessionStore::new(vec![Box::new(provider)]);
    let hits = search_store(&store, "exponential backoff", &Scope::default()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, "codex:bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
    assert_eq!(hits[0].message_uuid.as_deref(), Some("L8"));
}

// The point of M4: both providers behind one store, one merged project view.
#[test]
fn cross_provider_project_aggregation() {
    let cc_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-projects");
    let store = SessionStore::new(vec![
        Box::new(ClaudeCodeProvider::new(Some(cc_root))),
        Box::new(CodexProvider::new(Some(fixture_root()))),
    ]);
    let projects = store.projects().unwrap();
    // Both fixtures use cwd /Users/tester/demo-app → a single merged project.
    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(project.session_count, 3);
    assert!(project.providers.contains("claude-code"));
    assert!(project.providers.contains("codex"));

    let sessions = store.sessions("/Users/tester/demo-app", None, None).unwrap();
    assert_eq!(sessions.len(), 3);

    // Provider-scoped listing still works.
    let codex_only = store.sessions("/Users/tester/demo-app", Some("codex"), None).unwrap();
    assert_eq!(codex_only.len(), 1);
    assert_eq!(codex_only[0].provider_id, "codex");
}
