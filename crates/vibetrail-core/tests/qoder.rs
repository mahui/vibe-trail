//! Fixture tests for the Qoder provider (TECH_SPEC §4.5): pair-file
//! discovery (`<id>-session.json` + `<id>.jsonl`), the linear parts
//! whitelist with tolerance counters, metadata-driven titles/usage/chains,
//! terminal resume via qodercli, and grep-engine search with the
//! by-product/checkpoint exclusions.

use std::path::PathBuf;

use vibetrail_core::{
    search_store, ContentBlock, LaunchMode, Provider, QoderProvider, RawSession, Role, Scope,
    SessionStore,
};

const MAIN_ID: &str = "dddd4444-0000-4000-8000-000000000004";
const CHILD_ID: &str = "eeee5555-0000-4000-8000-000000000005";

fn provider() -> QoderProvider {
    QoderProvider::new(Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/qoder-projects"),
    ))
}

fn raw_for(native_id: &str) -> RawSession {
    provider()
        .discover()
        .unwrap()
        .into_iter()
        .find(|raw| raw.native_id == native_id)
        .expect("fixture session present")
}

#[test]
fn discovers_paired_sessions_only() {
    let mut sessions = provider().discover().unwrap();
    sessions.sort_by(|a, b| a.native_id.cmp(&b.native_id));
    // toolu_*.jsonl (no metadata pair) and the checkpoint directory are not
    // sessions.
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].native_id, MAIN_ID);
    assert_eq!(sessions[1].native_id, CHILD_ID);
    for session in &sessions {
        // working_dir from session.json is authoritative.
        assert_eq!(session.project_path, "/Users/tester/demo-app");
    }
    // parent_session_id carries the resume/fork chain.
    assert_eq!(sessions[0].parent_native_id, None);
    assert_eq!(sessions[1].parent_native_id.as_deref(), Some(MAIN_ID));
}

#[test]
fn parses_parts_whitelist_with_tolerance() {
    let session = provider().parse(&raw_for(MAIN_ID)).unwrap();
    // m1 user text, m2 assistant text+tool_call, m3 tool_result, m4 text;
    // m0 is_meta, m5 unknown role, m6 unknown part type, one undecodable line.
    assert_eq!(session.messages.len(), 4);
    assert_eq!(session.messages[0].role, Role::User);
    assert!(matches!(&session.messages[0].blocks[0],
        ContentBlock::Text { text } if text.starts_with("Fix the subtitle")));
    let m2 = &session.messages[1];
    assert_eq!(m2.role, Role::Assistant);
    assert!(
        matches!(&m2.blocks[1], ContentBlock::ToolUse { name, input }
        if name == "Read" && input["file_path"] == "player/SubtitleView.swift")
    );
    // role=tool renders as assistant-side tool output.
    let m3 = &session.messages[2];
    assert_eq!(m3.role, Role::Assistant);
    assert!(
        matches!(&m3.blocks[0], ContentBlock::ToolResult { summary, truncated: false }
        if summary.contains("withAnimation"))
    );
    let debug = &session.extensions["debug"];
    assert_eq!(debug["metaLines"], 1);
    assert_eq!(debug["ignoredRoles"]["future_role"], 1);
    assert_eq!(debug["ignoredPartTypes"]["hologram"], 1);
    assert_eq!(debug["undecodableLines"], 1);
    // Token usage comes from session.json, same extension shape as CC.
    assert_eq!(session.extensions["usage"]["inputTokens"], 32060);
    assert_eq!(session.extensions["usage"]["outputTokens"], 261);
}

#[test]
fn titles_prefer_metadata_then_first_prompt() {
    let main = provider().parse(&raw_for(MAIN_ID)).unwrap();
    assert_eq!(main.summary.title, "Fix subtitle scroll jitter");
    // is_meta user lines never become the title source.
    let child = provider().parse(&raw_for(CHILD_ID)).unwrap();
    assert!(child.summary.title.starts_with("Continue: also debounce"));
    assert_eq!(
        provider().quick_title(&raw_for(MAIN_ID)).as_deref(),
        Some("Fix subtitle scroll jitter")
    );
    assert!(provider()
        .quick_title(&raw_for(CHILD_ID))
        .unwrap()
        .starts_with("Continue: also debounce"));
}

#[test]
fn resume_is_a_plain_terminal_command() {
    let spec = provider().resume_spec(&raw_for(MAIN_ID)).unwrap();
    assert_eq!(spec.launch, LaunchMode::Terminal);
    assert_eq!(spec.command, vec!["qodercli", "-r", MAIN_ID]);
    // Fixture project path does not exist on disk → precondition exit 3.
    let store = SessionStore::new(vec![Box::new(provider())]);
    let error = store.resume_spec_for("dddd4444").unwrap_err();
    assert_eq!(error.exit_code(), 3);
}

#[test]
fn search_resolves_lines_and_excludes_by_products() {
    let store = SessionStore::new(vec![Box::new(provider())]);
    let hits = search_store(&store, "exponential backoff", &Scope::default()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, format!("qoder:{CHILD_ID}"));
    assert_eq!(hits[0].message_uuid.as_deref(), Some("n1"));
    assert_eq!(hits[0].project_path, "/Users/tester/demo-app");
    // toolu_* transcripts and checkpoint snapshots never resolve to hits.
    let hits = search_store(&store, "must not be listed", &Scope::default()).unwrap();
    assert!(hits.is_empty());
    let hits = search_store(&store, "checkpoint contents", &Scope::default()).unwrap();
    assert!(hits.is_empty());
}
