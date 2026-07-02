//! Fixture parity tests for the Claude Code provider: the four parsing rules
//! each have a counterexample baked into the fixtures, plus discovery,
//! summaries, subagents, store resolution, and search.

use std::path::PathBuf;

use vibetrail_core::{
    search_store, ClaudeCodeProvider, ContentBlock, Error, Provider, RawSession, Role, Scope,
    SessionStore,
};

const SESSION_1: &str = "11111111-1111-1111-1111-111111111111";
const SESSION_2: &str = "22222222-2222-2222-2222-222222222222";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-projects")
}

fn provider() -> ClaudeCodeProvider {
    ClaudeCodeProvider::new(Some(fixture_root()))
}

fn store() -> SessionStore {
    SessionStore::new(vec![Box::new(provider())])
}

fn raw(native_id: &str) -> RawSession {
    provider()
        .discover()
        .unwrap()
        .into_iter()
        .find(|raw| raw.native_id == native_id)
        .expect("fixture session present")
}

#[test]
fn discovers_top_level_sessions_only() {
    let sessions = provider().discover().unwrap();
    // Subagent files under <session>/subagents/ must not surface as sessions.
    assert_eq!(sessions.len(), 4);
    assert!(sessions
        .iter()
        .all(|s| s.project_path == "/Users/tester/demo-app"));
    assert!(sessions.iter().all(|s| s.provider_id == "claude-code"));
}

// Rule 1: streamed lines sharing message.id regroup into one message.
#[test]
fn regroups_streamed_assistant_message() {
    let session = provider().parse(&raw(SESSION_1)).unwrap();
    assert_eq!(session.messages.len(), 4);
    let merged = &session.messages[1];
    assert_eq!(merged.role, Role::Assistant);
    assert_eq!(merged.uuid, "a1y");
    assert_eq!(merged.parent_uuid.as_deref(), Some("u1"));
    assert_eq!(merged.blocks.len(), 2);
    let ContentBlock::Text { text } = &merged.blocks[0] else {
        panic!("expected text block, got {:?}", merged.blocks[0]);
    };
    assert_eq!(text, "Let me look at the login module.");
    let ContentBlock::ToolUse { name, .. } = &merged.blocks[1] else {
        panic!("expected tool_use block, got {:?}", merged.blocks[1]);
    };
    assert_eq!(name, "Read");
}

// Rule 2: duplicated UUIDs (branch/resume rewrites) count once.
#[test]
fn deduplicates_by_uuid() {
    let session = provider().parse(&raw(SESSION_1)).unwrap();
    assert_eq!(
        session.messages.iter().filter(|m| m.uuid == "u1").count(),
        1
    );
    assert_eq!(session.extensions["debug"]["duplicateUuids"], 1);
}

// Rule 3: unknown entry/block types are counted, never fatal.
#[test]
fn tolerates_unknown_entries_and_blocks() {
    let session = provider().parse(&raw(SESSION_1)).unwrap();
    let debug = &session.extensions["debug"];
    assert_eq!(debug["undecodableLines"], 1);
    assert_eq!(debug["ignoredEntryTypes"]["future-unknown-type"], 1);
    assert_eq!(debug["ignoredEntryTypes"]["mode"], 1);
    assert_eq!(debug["unknownBlockTypes"]["server_tool_use"], 1);
}

// Rule 4: parent-child tree ordering beats raw file order.
#[test]
fn reorders_out_of_order_tree() {
    let session = provider().parse(&raw(SESSION_2)).unwrap();
    let uuids: Vec<&str> = session.messages.iter().map(|m| m.uuid.as_str()).collect();
    assert_eq!(uuids, ["u1", "a1", "a2", "u3"]);
}

#[test]
fn filters_meta_and_sidechain_entries() {
    let session = provider().parse(&raw(SESSION_1)).unwrap();
    assert!(!session
        .messages
        .iter()
        .any(|m| m.uuid == "m1" || m.uuid == "s1"));
    assert_eq!(session.extensions["debug"]["sidechainEntries"], 1);
}

#[test]
fn summary_fields() {
    let summary = provider().parse(&raw(SESSION_1)).unwrap().summary;
    assert_eq!(summary.id, format!("claude-code:{SESSION_1}"));
    assert_eq!(summary.title, "Fix login certificate bug");
    assert_eq!(summary.message_count, 4);
    assert_eq!(summary.git_branch.as_deref(), Some("main"));
    assert_eq!(summary.duration, 12.0);
}

#[test]
fn quick_title_prefers_tail_last_prompt() {
    let title = provider().quick_title(&raw(SESSION_1));
    assert_eq!(title.as_deref(), Some("How do I fix the login bug?"));
}

#[test]
fn quick_title_falls_back_to_first_user_prompt() {
    let title = provider().quick_title(&raw(SESSION_2));
    assert_eq!(title.as_deref(), Some("Refactor the config loader"));
}

// P1: token stats accumulate over deduplicated logical messages; per
// streamed message the last chunk's usage wins (final API totals).
#[test]
fn usage_totals_deduplicated() {
    let session = provider().parse(&raw(SESSION_1)).unwrap();
    let usage = &session.extensions["usage"];
    assert_eq!(usage["inputTokens"], 300);
    assert_eq!(usage["outputTokens"], 55); // 25 (msg_A final chunk) + 30
    assert_eq!(usage["cacheReadTokens"], 500);
    assert_eq!(usage["cacheCreationTokens"], 0);
}

#[test]
fn subagents_carry_message_previews() {
    let session = provider().parse(&raw(SESSION_1)).unwrap();
    let subagents = session.extensions["subagents"].as_array().unwrap();
    let messages = subagents[0]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(
        messages[0]["preview"],
        "Explore the codebase for login handlers"
    );
}

#[test]
fn subagents_merged_in_fixed_order_into_extensions() {
    let session = provider().parse(&raw(SESSION_1)).unwrap();
    let subagents = session.extensions["subagents"].as_array().unwrap();
    assert_eq!(subagents.len(), 1);
    assert_eq!(subagents[0]["agentId"], "agent-abc123");
    assert_eq!(subagents[0]["agentType"], "explore");
    assert_eq!(subagents[0]["messageCount"], 2);
}

#[test]
fn outline_matches_messages() {
    let stubs = provider().outline(&raw(SESSION_1)).unwrap();
    assert_eq!(stubs.len(), 4);
    assert_eq!(stubs[0].preview, "How do I fix the login bug?");
    assert_eq!(
        stubs.iter().map(|s| s.index).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
}

#[test]
fn page_slices() {
    let page = provider().page(&raw(SESSION_1), 1, 2).unwrap();
    assert_eq!(
        page.iter().map(|m| m.uuid.as_str()).collect::<Vec<_>>(),
        ["a1y", "u2"]
    );
    assert!(provider().page(&raw(SESSION_1), 10, 2).unwrap().is_empty());
}

#[test]
fn projects_aggregation() {
    let projects = store().projects().unwrap();
    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(project.real_path, "/Users/tester/demo-app");
    assert!(!project.exists);
    assert_eq!(project.session_count, 4);
    assert!(project.providers.contains("claude-code"));
}

// Tool results truncate for display at 2000 chars; message_full re-reads
// the untruncated version from disk on demand.
#[test]
fn tool_results_truncate_with_full_on_demand() {
    let provider = provider();
    let raw = raw("44444444-4444-4444-4444-444444444444");
    let session = provider.parse(&raw).unwrap();
    let ContentBlock::ToolResult { summary, truncated } = &session.messages[1].blocks[0] else {
        panic!("expected tool_result");
    };
    assert!(truncated);
    assert_eq!(summary.chars().count(), 2000);
    assert!(!summary.ends_with("END-MARKER"));

    let full = provider
        .message_full(&raw, "u2")
        .unwrap()
        .expect("message present");
    let ContentBlock::ToolResult { summary, truncated } = &full.blocks[0] else {
        panic!("expected tool_result");
    };
    assert!(!truncated);
    assert_eq!(summary.chars().count(), 3000);
    assert!(summary.ends_with("END-MARKER"));

    assert!(provider
        .message_full(&raw, "nonexistent")
        .unwrap()
        .is_none());
}

// Resume-fork files start with history copied from the parent; those lines
// keep the parent's sessionId, which names the chain parent.
#[test]
fn resume_chain_parent_extracted() {
    let sessions = provider().discover().unwrap();
    let resumed = sessions
        .iter()
        .find(|s| s.native_id == "33333333-3333-3333-3333-333333333333")
        .unwrap();
    assert_eq!(resumed.parent_native_id.as_deref(), Some(SESSION_1));
    let root = sessions.iter().find(|s| s.native_id == SESSION_1).unwrap();
    assert_eq!(root.parent_native_id, None);
}

#[test]
fn resolve_session_by_prefix() {
    let (_, raw) = store().resolve_session("22222222").unwrap();
    assert_eq!(raw.native_id, SESSION_2);
}

#[test]
fn resolve_session_unknown_is_data_error() {
    let error = store().resolve_session("ffffffff").map(|_| ()).unwrap_err();
    assert_eq!(error.exit_code(), 2);
}

#[test]
fn ambiguous_session_prefix_is_usage_error() {
    // Both fixture ids match the empty-ish shared prefix "".
    let error = store().resolve_session("").map(|_| ()).unwrap_err();
    assert_eq!(error.exit_code(), 1);
}

#[test]
fn resume_requires_existing_project_path() {
    // Fixture project path does not exist on disk → exit-code-3 error.
    let error = store().resume_spec_for("11111111").unwrap_err();
    assert!(matches!(error, Error::ResumePrecondition(_)));
    assert_eq!(error.exit_code(), 3);
}

#[test]
fn grep_search_resolves_message_uuids() {
    let store = store();
    let hits = search_store(&store, "certificate", &Scope::default()).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits
        .iter()
        .all(|h| h.session_id == format!("claude-code:{SESSION_1}")));
    let uuids: Vec<_> = hits.iter().map(|h| h.message_uuid.as_deref()).collect();
    assert_eq!(uuids, [Some("u2"), Some("a2")]);
    assert!(hits[0].snippet.to_lowercase().contains("certificate"));
}

#[test]
fn search_scoped_to_unknown_project_returns_nothing() {
    let store = store();
    let scope = Scope {
        project_path: Some("/nonexistent/elsewhere".into()),
        provider_id: None,
    };
    assert!(search_store(&store, "certificate", &scope)
        .unwrap()
        .is_empty());
}
