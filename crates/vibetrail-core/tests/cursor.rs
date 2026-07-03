//! Fixture tests for the experimental Cursor provider (TECH_SPEC §4.4): the
//! two-level workspace discovery, both composer generations (legacy inline
//! `conversation` vs current bubble point queries), the bubble whitelist with
//! its tolerance counters, client-level resume (LaunchMode::GuiApp) and the
//! ADR-3 search degrade path.
//!
//! The store is SQLite, so the fixture is materialized into a tempdir from
//! the JSON sources in `tests/fixtures/cursor-user/` — the repo stays free of
//! binary blobs while the provider still reads a byte-real database.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tempfile::TempDir;
use vibetrail_core::{
    search_store, ContentBlock, CursorProvider, LaunchMode, Provider, RawSession, Role, Scope,
    SessionStore,
};

const CURRENT_ID: &str = "aaaa1111-0000-4000-8000-000000000001";
const LEGACY_ID: &str = "bbbb2222-0000-4000-8000-000000000002";
const ORPHAN_ID: &str = "cccc3333-0000-4000-8000-000000000003";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cursor-user")
}

/// Build a byte-real Cursor user store in a tempdir: a project directory the
/// workspace points at (resume checks it exists), the per-workspace database
/// with the composer list, and the global database with headers and bubbles.
fn materialize() -> (TempDir, CursorProvider, String) {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("demo-app");
    fs::create_dir_all(&project).unwrap();

    let user = dir.path().join("User");
    let ws = user.join("workspaceStorage/ws-demo");
    fs::create_dir_all(user.join("globalStorage")).unwrap();
    fs::create_dir_all(&ws).unwrap();

    fs::write(
        ws.join("workspace.json"),
        format!("{{\"folder\": \"file://{}\"}}", project.display()),
    )
    .unwrap();

    let ws_db = rusqlite::Connection::open(ws.join("state.vscdb")).unwrap();
    ws_db
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)")
        .unwrap();
    let composers = fs::read_to_string(fixture_dir().join("workspace-composers.json")).unwrap();
    ws_db
        .execute(
            "INSERT INTO ItemTable (key, value) VALUES ('composer.composerData', ?1)",
            [composers],
        )
        .unwrap();

    let global_db = rusqlite::Connection::open(user.join("globalStorage/state.vscdb")).unwrap();
    global_db
        .execute_batch(
            "CREATE TABLE cursorDiskKV (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
        )
        .unwrap();
    let kv: Value =
        serde_json::from_str(&fs::read_to_string(fixture_dir().join("global-kv.json")).unwrap())
            .unwrap();
    for (key, value) in kv.as_object().unwrap() {
        global_db
            .execute(
                "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                (key, serde_json::to_string(value).unwrap()),
            )
            .unwrap();
    }

    let expected_project = vibetrail_core::normalize_path(&project.to_string_lossy());
    (dir, CursorProvider::new(Some(user)), expected_project)
}

fn raw_for(provider: &CursorProvider, native_id: &str) -> RawSession {
    provider
        .discover()
        .unwrap()
        .into_iter()
        .find(|raw| raw.native_id == native_id)
        .expect("fixture composer present")
}

#[test]
fn discovers_workspace_mapped_composers_only() {
    let (_dir, provider, expected_project) = materialize();
    let mut sessions = provider.discover().unwrap();
    sessions.sort_by(|a, b| a.native_id.cmp(&b.native_id));
    // The orphan composer exists in the global store but belongs to no
    // workspace — ignored by design.
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].native_id, CURRENT_ID);
    assert_eq!(sessions[1].native_id, LEGACY_ID);
    assert!(!sessions.iter().any(|s| s.native_id == ORPHAN_ID));
    for session in &sessions {
        assert_eq!(session.project_path, expected_project);
    }
    // mtime prefers lastUpdatedAt over createdAt.
    assert_eq!(sessions[0].mtime.timestamp_millis(), 1_751_500_600_000);
    assert_eq!(sessions[1].mtime.timestamp_millis(), 1_751_400_000_000);
}

#[test]
fn parses_current_format_via_bubble_point_queries() {
    let (_dir, provider, _) = materialize();
    let session = provider.parse(&raw_for(&provider, CURRENT_ID)).unwrap();
    // b1 (user text), b2 (thinking + tool call/result), b3 (assistant text);
    // b4 has an unknown type, b5 is content-free, b-missing has no body.
    assert_eq!(session.messages.len(), 3);
    assert_eq!(session.messages[0].role, Role::User);
    assert!(matches!(&session.messages[0].blocks[0],
        ContentBlock::Text { text } if text == "Why does the CORS preflight fail on /api/upload?"));
    let b2 = &session.messages[1];
    assert_eq!(b2.role, Role::Assistant);
    assert!(matches!(&b2.blocks[0], ContentBlock::Thinking { text }
        if text.starts_with("allowCredentials(true)")));
    assert!(
        matches!(&b2.blocks[1], ContentBlock::ToolUse { name, input }
        if name == "read_file" && input["path"] == "server/cors.ts")
    );
    assert!(
        matches!(&b2.blocks[2], ContentBlock::ToolResult { summary, truncated: false }
        if summary.contains("credentials: true"))
    );
    // Cursor's own chat name wins over the first prompt.
    assert_eq!(session.summary.title, "Fix CORS preflight failure");
    let debug = &session.extensions["debug"];
    assert_eq!(session.extensions["experimental"], true);
    assert_eq!(debug["missingBubbles"], 1);
    assert_eq!(debug["unknownBubbleTypes"]["7"], 1);
    assert_eq!(debug["emptyBubbles"], 1);
    assert_eq!(debug["undecodableBubbles"], 0);
}

#[test]
fn parses_legacy_inline_conversation() {
    let (_dir, provider, _) = materialize();
    let session = provider.parse(&raw_for(&provider, LEGACY_ID)).unwrap();
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[0].role, Role::User);
    assert_eq!(session.messages[1].role, Role::Assistant);
    // No name on legacy headers: the first user prompt titles the session.
    assert_eq!(session.summary.title, "Add retry logic to the sync job");
}

#[test]
fn quick_title_stays_metadata_level() {
    let (_dir, provider, _) = materialize();
    assert_eq!(
        provider
            .quick_title(&raw_for(&provider, CURRENT_ID))
            .as_deref(),
        Some("Fix CORS preflight failure")
    );
    assert_eq!(
        provider
            .quick_title(&raw_for(&provider, LEGACY_ID))
            .as_deref(),
        Some("Add retry logic to the sync job")
    );
}

#[test]
fn resume_opens_the_cursor_client() {
    let (_dir, provider, expected_project) = materialize();
    let raw = raw_for(&provider, CURRENT_ID);
    let spec = provider.resume_spec(&raw).unwrap();
    assert_eq!(spec.launch, LaunchMode::GuiApp);
    assert_eq!(
        spec.command,
        vec!["open", "-a", "Cursor", &expected_project]
    );
    // Full precondition path: capability + existing project directory.
    let store = SessionStore::new(vec![Box::new(provider)]);
    let spec = store.resume_spec_for("aaaa1111").unwrap();
    assert_eq!(spec.project_path, expected_project);
    assert_eq!(spec.launch, LaunchMode::GuiApp);
}

#[test]
fn search_degrade_path_resolves_bubble_anchor() {
    let (_dir, provider, expected_project) = materialize();
    let store = SessionStore::new(vec![Box::new(provider)]);
    let hits = search_store(&store, "exponential backoff", &Scope::default()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, format!("cursor:{LEGACY_ID}"));
    assert_eq!(hits[0].message_uuid.as_deref(), Some("lb2"));
    assert_eq!(hits[0].project_path, expected_project);
    // Current-format bubbles are reached through the key-range query.
    let hits = search_store(&store, "allow-list", &Scope::default()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, format!("cursor:{CURRENT_ID}"));
    assert_eq!(hits[0].message_uuid.as_deref(), Some("b3"));
}
