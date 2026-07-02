import Foundation
import Testing
@testable import VibeTrailCore

/// Pins the `--json` output schema (TECH_SPEC §6): GUI, CLI, and the future
/// MCP shell all share this shape. Adding a field is fine — update the
/// snapshot deliberately; renaming or removing one is a breaking change.
@Suite("--json schema snapshots")
struct JSONSchemaSnapshotTests {
    @Test func sessionSummarySnapshot() throws {
        let summary = SessionSummary(
            providerId: "claude-code",
            nativeId: "11111111-1111-1111-1111-111111111111",
            projectPath: "/Users/tester/demo-app",
            title: "Fix login certificate bug",
            mtime: Date(timeIntervalSince1970: 1_750_000_000),
            messageCount: 4,
            gitBranch: "main",
            duration: 12)
        let expected = """
        {
          "duration" : 12,
          "gitBranch" : "main",
          "id" : "claude-code:11111111-1111-1111-1111-111111111111",
          "messageCount" : 4,
          "mtime" : "2025-06-15T15:06:40Z",
          "nativeId" : "11111111-1111-1111-1111-111111111111",
          "projectPath" : "/Users/tester/demo-app",
          "providerId" : "claude-code",
          "title" : "Fix login certificate bug"
        }
        """
        #expect(try VibeTrailJSON.encode(summary) == expected)
    }

    @Test func projectSnapshot() throws {
        let project = Project(
            id: "/Users/tester/demo-app",
            realPath: "/Users/tester/demo-app",
            exists: false,
            sessionCount: 2,
            lastActive: Date(timeIntervalSince1970: 1_750_000_000),
            lastPrompt: "How do I fix the login bug?",
            providers: ["claude-code"])
        let expected = """
        {
          "exists" : false,
          "id" : "/Users/tester/demo-app",
          "lastActive" : "2025-06-15T15:06:40Z",
          "lastPrompt" : "How do I fix the login bug?",
          "providers" : [
            "claude-code"
          ],
          "realPath" : "/Users/tester/demo-app",
          "sessionCount" : 2
        }
        """
        #expect(try VibeTrailJSON.encode(project) == expected)
    }

    @Test func searchHitSnapshot() throws {
        let hit = SearchHit(
            providerId: "claude-code",
            nativeSessionId: "11111111-1111-1111-1111-111111111111",
            projectPath: "/Users/tester/demo-app",
            messageUuid: "u2",
            snippet: "reads certificate path from env")
        let expected = """
        {
          "messageUuid" : "u2",
          "nativeSessionId" : "11111111-1111-1111-1111-111111111111",
          "projectPath" : "/Users/tester/demo-app",
          "providerId" : "claude-code",
          "sessionId" : "claude-code:11111111-1111-1111-1111-111111111111",
          "snippet" : "reads certificate path from env"
        }
        """
        #expect(try VibeTrailJSON.encode(hit) == expected)
    }

    @Test func messageBlocksSnapshot() throws {
        let message = Message(
            uuid: "a1y",
            parentUuid: "u1",
            role: .assistant,
            blocks: [
                .text("Let me look."),
                .toolUse(name: "Read", input: .object(["file_path": .string("/tmp/x")])),
                .toolResult(summary: "ok", truncated: false),
                .thinking("hmm"),
            ],
            timestamp: Date(timeIntervalSince1970: 1_750_000_000))
        let expected = """
        {
          "blocks" : [
            {
              "kind" : "text",
              "text" : "Let me look."
            },
            {
              "input" : {
                "file_path" : "/tmp/x"
              },
              "kind" : "tool_use",
              "name" : "Read"
            },
            {
              "kind" : "tool_result",
              "summary" : "ok",
              "truncated" : false
            },
            {
              "kind" : "thinking",
              "text" : "hmm"
            }
          ],
          "parentUuid" : "u1",
          "role" : "assistant",
          "timestamp" : "2025-06-15T15:06:40Z",
          "uuid" : "a1y"
        }
        """
        #expect(try VibeTrailJSON.encode(message) == expected)
    }
}
