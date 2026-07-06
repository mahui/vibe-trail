//! Pins the `--json` / Tauri IPC schema (TECH_SPEC §6): all shells share this
//! shape. Adding a field is fine — update the snapshot deliberately; renaming
//! or removing one is a breaking change.

use chrono::{DateTime, Utc};
use vibetrail_core::config::{ConfigReport, ProviderStatus};
use vibetrail_core::{
    AgentDef, ContentBlock, HandoffCapsule, MemoryDoc, Message, Project, Role, SearchHit,
    SessionSummary,
};

fn fixed_time() -> DateTime<Utc> {
    DateTime::from_timestamp(1_750_000_000, 0).unwrap()
}

#[test]
fn session_summary_snapshot() {
    let summary = SessionSummary::new(
        "claude-code",
        "11111111-1111-1111-1111-111111111111",
        "/Users/tester/demo-app".to_string(),
        "Fix login certificate bug".to_string(),
        fixed_time(),
        4,
        Some("main".to_string()),
        12.0,
    );
    let expected = r#"{
  "id": "claude-code:11111111-1111-1111-1111-111111111111",
  "providerId": "claude-code",
  "nativeId": "11111111-1111-1111-1111-111111111111",
  "projectPath": "/Users/tester/demo-app",
  "title": "Fix login certificate bug",
  "mtime": "2025-06-15T15:06:40Z",
  "messageCount": 4,
  "gitBranch": "main",
  "duration": 12.0
}"#;
    assert_eq!(serde_json::to_string_pretty(&summary).unwrap(), expected);
}

#[test]
fn project_snapshot() {
    let project = Project {
        id: "/Users/tester/demo-app".to_string(),
        real_path: "/Users/tester/demo-app".to_string(),
        exists: false,
        session_count: 2,
        last_active: fixed_time(),
        last_prompt: Some("How do I fix the login bug?".to_string()),
        providers: ["claude-code".to_string()].into(),
    };
    let expected = r#"{
  "id": "/Users/tester/demo-app",
  "realPath": "/Users/tester/demo-app",
  "exists": false,
  "sessionCount": 2,
  "lastActive": "2025-06-15T15:06:40Z",
  "lastPrompt": "How do I fix the login bug?",
  "providers": [
    "claude-code"
  ]
}"#;
    assert_eq!(serde_json::to_string_pretty(&project).unwrap(), expected);
}

#[test]
fn search_hit_snapshot() {
    let hit = SearchHit::new(
        "claude-code",
        "11111111-1111-1111-1111-111111111111".to_string(),
        "/Users/tester/demo-app".to_string(),
        Some("u2".to_string()),
        "reads certificate path from env".to_string(),
    );
    let expected = r#"{
  "providerId": "claude-code",
  "sessionId": "claude-code:11111111-1111-1111-1111-111111111111",
  "nativeSessionId": "11111111-1111-1111-1111-111111111111",
  "projectPath": "/Users/tester/demo-app",
  "messageUuid": "u2",
  "snippet": "reads certificate path from env"
}"#;
    assert_eq!(serde_json::to_string_pretty(&hit).unwrap(), expected);
}

#[test]
fn config_report_snapshot() {
    let report = ConfigReport {
        path: "/Users/tester/.config/vibetrail/config.json".to_string(),
        providers: vec![ProviderStatus {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            enabled: false,
            root: "/Users/tester/alt/codex".to_string(),
            default_root: "/Users/tester/.codex/sessions".to_string(),
            root_is_custom: true,
            root_exists: false,
        }],
    };
    let expected = r#"{
  "path": "/Users/tester/.config/vibetrail/config.json",
  "providers": [
    {
      "id": "codex",
      "name": "Codex",
      "enabled": false,
      "root": "/Users/tester/alt/codex",
      "defaultRoot": "/Users/tester/.codex/sessions",
      "rootIsCustom": true,
      "rootExists": false
    }
  ]
}"#;
    assert_eq!(serde_json::to_string_pretty(&report).unwrap(), expected);
}

#[test]
fn message_blocks_snapshot() {
    let message = Message {
        uuid: "a1y".to_string(),
        alias_uuids: vec!["a1x".to_string()],
        parent_uuid: Some("u1".to_string()),
        role: Role::Assistant,
        blocks: vec![
            ContentBlock::Text {
                text: "Let me look.".to_string(),
            },
            ContentBlock::ToolUse {
                name: "Read".to_string(),
                input: serde_json::json!({ "file_path": "/tmp/x" }),
            },
            ContentBlock::ToolResult {
                summary: "ok".to_string(),
                truncated: false,
            },
            ContentBlock::Thinking {
                text: "hmm".to_string(),
            },
        ],
        timestamp: fixed_time(),
    };
    let expected = r#"{
  "uuid": "a1y",
  "aliasUuids": [
    "a1x"
  ],
  "parentUuid": "u1",
  "role": "assistant",
  "blocks": [
    {
      "kind": "text",
      "text": "Let me look."
    },
    {
      "kind": "tool_use",
      "name": "Read",
      "input": {
        "file_path": "/tmp/x"
      }
    },
    {
      "kind": "tool_result",
      "summary": "ok",
      "truncated": false
    },
    {
      "kind": "thinking",
      "text": "hmm"
    }
  ],
  "timestamp": "2025-06-15T15:06:40Z"
}"#;
    assert_eq!(serde_json::to_string_pretty(&message).unwrap(), expected);
}

#[test]
fn memory_doc_snapshot() {
    let doc = MemoryDoc {
        provider_id: "claude-code".to_string(),
        name: "login-flow".to_string(),
        description: Some("Token refresh happens in middleware".to_string()),
        doc_type: Some("project".to_string()),
        content: "The login module refreshes tokens.".to_string(),
        file_path: "/Users/tester/.claude/projects/-x/memory/login-flow.md".into(),
        mtime: fixed_time(),
    };
    let expected = r#"{
  "providerId": "claude-code",
  "name": "login-flow",
  "description": "Token refresh happens in middleware",
  "docType": "project",
  "content": "The login module refreshes tokens.",
  "filePath": "/Users/tester/.claude/projects/-x/memory/login-flow.md",
  "mtime": "2025-06-15T15:06:40Z"
}"#;
    assert_eq!(serde_json::to_string_pretty(&doc).unwrap(), expected);
}

#[test]
fn handoff_capsule_snapshot() {
    let capsule = HandoffCapsule {
        goal: "Fix the auth middleware bug".to_string(),
        project_path: "/Users/tester/backend".to_string(),
        git_branch: Some("fix-auth".to_string()),
        previous_agent: "claude-code".to_string(),
        message_count: 12,
        files_touched: vec!["src/auth/middleware.ts".to_string()],
        files_omitted: 0,
        last_user_prompt: Some("run the tests".to_string()),
        last_assistant_text: Some("Tests not run yet.".to_string()),
    };
    let expected = r#"{
  "goal": "Fix the auth middleware bug",
  "projectPath": "/Users/tester/backend",
  "gitBranch": "fix-auth",
  "previousAgent": "claude-code",
  "messageCount": 12,
  "filesTouched": [
    "src/auth/middleware.ts"
  ],
  "filesOmitted": 0,
  "lastUserPrompt": "run the tests",
  "lastAssistantText": "Tests not run yet."
}"#;
    assert_eq!(serde_json::to_string_pretty(&capsule).unwrap(), expected);
}

#[test]
fn agent_def_snapshot() {
    let def = AgentDef {
        provider_id: "claude-code".to_string(),
        name: "reviewer".to_string(),
        description: Some("Reviews diffs before merge".to_string()),
        model: Some("opus".to_string()),
        tools: Some("Read, Grep".to_string()),
        scope: "project".to_string(),
        content: "You are a reviewer.".to_string(),
        file_path: "/Users/tester/demo-app/.claude/agents/reviewer.md".into(),
        mtime: fixed_time(),
    };
    let expected = r#"{
  "providerId": "claude-code",
  "name": "reviewer",
  "description": "Reviews diffs before merge",
  "model": "opus",
  "tools": "Read, Grep",
  "scope": "project",
  "content": "You are a reviewer.",
  "filePath": "/Users/tester/demo-app/.claude/agents/reviewer.md",
  "mtime": "2025-06-15T15:06:40Z"
}"#;
    assert_eq!(serde_json::to_string_pretty(&def).unwrap(), expected);
}
