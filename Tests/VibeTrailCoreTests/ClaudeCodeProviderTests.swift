import Foundation
import Testing
@testable import VibeTrailCore

private func fixtureRoot() -> URL {
    Bundle.module.resourceURL!.appendingPathComponent("Fixtures/claude-projects")
}

private func fixtureProvider() -> ClaudeCodeProvider {
    ClaudeCodeProvider(root: fixtureRoot())
}

private func rawSession(_ nativeId: String) throws -> RawSession {
    let raw = try fixtureProvider().discover().first { $0.nativeId == nativeId }
    return try #require(raw)
}

@Suite("Discovery")
struct DiscoveryTests {
    @Test func discoversTopLevelSessionsOnly() throws {
        let sessions = try fixtureProvider().discover()
        // Subagent files under <session>/subagents/ must not surface as sessions.
        #expect(sessions.count == 2)
        #expect(sessions.allSatisfy { $0.projectPath == "/Users/tester/demo-app" })
        #expect(sessions.allSatisfy { $0.providerId == "claude-code" })
    }
}

@Suite("Parse pipeline — the four rules")
struct PipelineRuleTests {
    // Rule 1: streamed lines sharing message.id regroup into one message.
    @Test func regroupsStreamedAssistantMessage() throws {
        let provider = fixtureProvider()
        let session = try provider.parse(try rawSession("11111111-1111-1111-1111-111111111111"))
        #expect(session.messages.count == 4)
        let merged = session.messages[1]
        #expect(merged.role == .assistant)
        #expect(merged.uuid == "a1y")
        #expect(merged.parentUuid == "u1")
        #expect(merged.blocks.count == 2)
        guard case .text(let text) = merged.blocks[0],
              case .toolUse(let name, _) = merged.blocks[1] else {
            Issue.record("Merged message should contain text + tool_use, got \(merged.blocks)")
            return
        }
        #expect(text == "Let me look at the login module.")
        #expect(name == "Read")
    }

    // Rule 2: duplicated UUIDs (branch/resume rewrites) count once.
    @Test func deduplicatesByUuid() throws {
        let session = try fixtureProvider().parse(try rawSession("11111111-1111-1111-1111-111111111111"))
        #expect(session.messages.filter { $0.uuid == "u1" }.count == 1)
        let debug = try #require(session.extensions["debug"]?.objectValue)
        #expect(debug["duplicateUuids"] == .number(1))
    }

    // Rule 3: unknown entry/block types are counted, never fatal.
    @Test func toleratesUnknownEntriesAndBlocks() throws {
        let session = try fixtureProvider().parse(try rawSession("11111111-1111-1111-1111-111111111111"))
        let debug = try #require(session.extensions["debug"]?.objectValue)
        #expect(debug["undecodableLines"] == .number(1))
        let ignored = try #require(debug["ignoredEntryTypes"]?.objectValue)
        #expect(ignored["future-unknown-type"] == .number(1))
        #expect(ignored["mode"] == .number(1))
        let unknownBlocks = try #require(debug["unknownBlockTypes"]?.objectValue)
        #expect(unknownBlocks["server_tool_use"] == .number(1))
    }

    // Rule 4: parent-child tree ordering beats raw file order.
    @Test func reordersOutOfOrderTree() throws {
        let session = try fixtureProvider().parse(try rawSession("22222222-2222-2222-2222-222222222222"))
        #expect(session.messages.map(\.uuid) == ["u1", "a1", "a2", "u3"])
    }

    @Test func filtersMetaAndSidechainEntries() throws {
        let session = try fixtureProvider().parse(try rawSession("11111111-1111-1111-1111-111111111111"))
        #expect(!session.messages.contains { $0.uuid == "m1" || $0.uuid == "s1" })
        let debug = try #require(session.extensions["debug"]?.objectValue)
        #expect(debug["sidechainEntries"] == .number(1))
    }
}

@Suite("Summary & subagents")
struct SummaryTests {
    @Test func summaryFields() throws {
        let summary = try fixtureProvider().parse(try rawSession("11111111-1111-1111-1111-111111111111")).summary
        #expect(summary.id == "claude-code:11111111-1111-1111-1111-111111111111")
        #expect(summary.title == "Fix login certificate bug")
        #expect(summary.messageCount == 4)
        #expect(summary.gitBranch == "main")
        #expect(summary.duration == 12)
    }

    @Test func quickTitlePrefersTailLastPrompt() throws {
        let title = try fixtureProvider().quickTitle(try rawSession("11111111-1111-1111-1111-111111111111"))
        #expect(title == "How do I fix the login bug?")
    }

    @Test func quickTitleFallsBackToFirstUserPrompt() throws {
        let title = try fixtureProvider().quickTitle(try rawSession("22222222-2222-2222-2222-222222222222"))
        #expect(title == "Refactor the config loader")
    }

    @Test func subagentsMergedInFixedOrderIntoExtensions() throws {
        let session = try fixtureProvider().parse(try rawSession("11111111-1111-1111-1111-111111111111"))
        let subagents = try #require(session.extensions["subagents"]?.arrayValue)
        #expect(subagents.count == 1)
        let subagent = try #require(subagents[0].objectValue)
        #expect(subagent["agentId"] == .string("agent-abc123"))
        #expect(subagent["agentType"] == .string("explore"))
        #expect(subagent["messageCount"] == .number(2))
    }
}

@Suite("Outline & paging")
struct OutlineTests {
    @Test func outlineMatchesMessages() throws {
        let provider = fixtureProvider()
        let raw = try rawSession("11111111-1111-1111-1111-111111111111")
        let stubs = try provider.outline(raw)
        #expect(stubs.count == 4)
        #expect(stubs[0].preview == "How do I fix the login bug?")
        #expect(stubs.map(\.index) == [0, 1, 2, 3])
    }

    @Test func pageSlices() throws {
        let provider = fixtureProvider()
        let raw = try rawSession("11111111-1111-1111-1111-111111111111")
        let page = try provider.page(raw, offset: 1, limit: 2)
        #expect(page.map(\.uuid) == ["a1y", "u2"])
        #expect(try provider.page(raw, offset: 10, limit: 2).isEmpty)
    }
}

@Suite("SessionStore")
struct SessionStoreTests {
    private var store: SessionStore { SessionStore(providers: [fixtureProvider()]) }

    @Test func projectsAggregation() throws {
        let projects = try store.projects()
        #expect(projects.count == 1)
        let project = projects[0]
        #expect(project.realPath == "/Users/tester/demo-app")
        #expect(project.exists == false)
        #expect(project.sessionCount == 2)
        #expect(project.providers == ["claude-code"])
    }

    @Test func resolveSessionByPrefix() throws {
        let (_, raw) = try store.resolveSession("22222222")
        #expect(raw.nativeId == "22222222-2222-2222-2222-222222222222")
    }

    @Test func resolveSessionUnknownThrowsDataError() throws {
        #expect(throws: VibeTrailError.self) {
            try store.resolveSession("ffffffff")
        }
    }

    @Test func resumeRequiresExistingProjectPath() throws {
        // Fixture project path does not exist on disk → exit-code-3 error.
        do {
            _ = try store.resumeSpec(for: "11111111")
            Issue.record("Expected resumePrecondition error")
        } catch let error as VibeTrailError {
            #expect(error.exitCode == 3)
        }
    }
}

@Suite("Search")
struct SearchTests {
    @Test func ripgrepSearchResolvesMessageUuids() throws {
        let rg = try #require(RipgrepSearchEngine.locateRipgrep())
        let engine = RipgrepSearchEngine(providers: [fixtureProvider()], ripgrepURL: rg)
        let hits = try engine.search("certificate", scope: SearchScope())
        #expect(hits.count == 2)
        #expect(hits.allSatisfy { $0.sessionId == "claude-code:11111111-1111-1111-1111-111111111111" })
        #expect(hits.map(\.messageUuid) == ["u2", "a2"])
        #expect(hits[0].snippet.localizedCaseInsensitiveContains("certificate"))
    }

    @Test func searchScopedToUnknownProjectReturnsNothing() throws {
        let rg = try #require(RipgrepSearchEngine.locateRipgrep())
        let engine = RipgrepSearchEngine(providers: [fixtureProvider()], ripgrepURL: rg)
        let hits = try engine.search("certificate", scope: SearchScope(projectPath: "/nonexistent/elsewhere"))
        #expect(hits.isEmpty)
    }
}
