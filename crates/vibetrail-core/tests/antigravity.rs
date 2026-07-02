//! Fixture tests for the experimental Antigravity provider: transcript step
//! whitelist, heuristic project derivation, artifacts, and the capability
//! degradations (no resume).

use std::path::PathBuf;

use vibetrail_core::{
    search_store, AntigravityProvider, ContentBlock, Provider, RawSession, Role, Scope,
    SessionStore,
};

const CONV_ID: &str = "cccccccc-1111-2222-3333-444444444444";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agy-brain")
}

fn provider() -> AntigravityProvider {
    AntigravityProvider::new(Some(fixture_root()))
}

fn raw() -> RawSession {
    provider()
        .discover()
        .unwrap()
        .into_iter()
        .next()
        .expect("fixture conversation present")
}

#[test]
fn discovers_conversations_with_derived_project() {
    let sessions = provider().discover().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].native_id, CONV_ID);
    // Longest common prefix of the two touched file:// paths, with the
    // brain's own artifact write under ~/.gemini excluded.
    assert_eq!(sessions[0].project_path, "/Users/tester/demo-app");
}

#[test]
fn parses_whitelisted_steps_only() {
    let session = provider().parse(&raw()).unwrap();
    // USER_INPUT, PLANNER_RESPONSE, 2×VIEW_FILE, CODE_ACTION, RUN_COMMAND.
    assert_eq!(session.messages.len(), 6);
    assert_eq!(session.messages[0].role, Role::User);
    assert!(matches!(&session.messages[0].blocks[0],
        ContentBlock::Text { text } if text == "Fix the subtitle auto-scroll jitter"));
    assert!(matches!(&session.messages[2].blocks[0],
        ContentBlock::ToolUse { name, .. } if name == "view_file"));
    let debug = &session.extensions["debug"];
    assert_eq!(debug["undecodableLines"], 1);
    assert_eq!(debug["ignoredStepTypes"]["CONVERSATION_HISTORY"], 1);
    assert_eq!(debug["ignoredStepTypes"]["FUTURE_STEP_KIND"], 1);
}

#[test]
fn artifacts_land_in_extensions() {
    let session = provider().parse(&raw()).unwrap();
    assert_eq!(session.extensions["experimental"], true);
    let artifacts = session.extensions["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0]["name"], "implementation_plan.md");
    assert_eq!(artifacts[1]["name"], "task.md");
    assert_eq!(
        artifacts[1]["summary"],
        "Task list for fixing subtitle scroll jitter."
    );
    assert_eq!(artifacts[1]["artifactType"], "ARTIFACT_TYPE_TASK");
}

#[test]
fn summary_and_quick_title() {
    let summary = provider().parse(&raw()).unwrap().summary;
    assert_eq!(summary.title, "Fix the subtitle auto-scroll jitter");
    assert_eq!(summary.message_count, 6);
    assert_eq!(
        provider().quick_title(&raw()).as_deref(),
        Some("Fix the subtitle auto-scroll jitter")
    );
}

#[test]
fn not_resumable() {
    let store = SessionStore::new(vec![Box::new(provider())]);
    let error = store.resume_spec_for(CONV_ID).unwrap_err();
    // Provider declares resumable=false → exit-code-4 unsupported.
    assert_eq!(error.exit_code(), 4);
}

#[test]
fn search_resolves_step_anchor() {
    let store = SessionStore::new(vec![Box::new(provider())]);
    let hits = search_store(&store, "jitter regression", &Scope::default()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, format!("antigravity:{CONV_ID}"));
    assert_eq!(hits[0].message_uuid.as_deref(), Some("S6"));
    assert_eq!(hits[0].project_path, "/Users/tester/demo-app");
}
