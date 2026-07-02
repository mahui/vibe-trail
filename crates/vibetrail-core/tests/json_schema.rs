//! Pins the `--json` / Tauri IPC schema (TECH_SPEC §6): all shells share this
//! shape. Adding a field is fine — update the snapshot deliberately; renaming
//! or removing one is a breaking change.

use chrono::{DateTime, Utc};
use vibetrail_core::{ContentBlock, Message, Project, Role, SearchHit, SessionSummary};

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
fn message_blocks_snapshot() {
    let message = Message {
        uuid: "a1y".to_string(),
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
