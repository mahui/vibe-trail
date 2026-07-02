import ArgumentParser
import Foundation
import VibeTrailCore

struct ProjectsCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "projects", abstract: "List projects across all providers (F1).")

    @Flag(name: .long, help: "Emit JSON.") var json = false

    func run() throws {
        try CLIEnvironment.run {
            let projects = try CLIEnvironment.store().projects()
            if json {
                print(try VibeTrailJSON.encode(projects))
                return
            }
            if projects.isEmpty {
                print("No sessions found.")
                return
            }
            for project in projects {
                let marker = project.exists ? " " : "!"
                let path = Format.abbreviatePath(project.realPath)
                let providers = project.providers.sorted().joined(separator: ",")
                var line = "\(marker) \(Format.pad(path, 52)) \(Format.pad("\(project.sessionCount)", 5)) "
                line += "\(Format.pad(Format.relativeTime(project.lastActive), 12)) \(providers)"
                if let prompt = project.lastPrompt {
                    line += "  \(Format.truncate(prompt, to: 60))"
                }
                print(line)
            }
        }
    }
}

struct SessionsCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "sessions", abstract: "List sessions of a project, newest first (F2).")

    @Argument(help: "Project path, or a unique directory-name suffix.") var project: String
    @Option(name: .customShort("n"), help: "Maximum sessions to list.") var limit: Int = 20
    @Option(name: .long, help: "Restrict to one provider id.") var provider: String?
    @Flag(name: .long, help: "Emit JSON.") var json = false

    func run() throws {
        try CLIEnvironment.run {
            let store = CLIEnvironment.store()
            let projectPath = try store.resolveProject(project)
            let sessions = try store.sessions(inProject: projectPath, providerId: provider, limit: limit)
            if json {
                print(try VibeTrailJSON.encode(sessions))
                return
            }
            if sessions.isEmpty {
                print("No sessions in \(Format.abbreviatePath(projectPath)).")
                return
            }
            for session in sessions {
                var line = "\(session.nativeId.prefix(8))  \(Format.pad(Format.relativeTime(session.mtime), 12)) "
                line += "\(Format.pad("\(session.messageCount) msg", 9)) \(Format.pad(Format.duration(session.duration), 6)) "
                if let branch = session.gitBranch {
                    line += "\(Format.pad(branch, 14)) "
                }
                line += Format.truncate(session.title, to: 70)
                print(line)
            }
        }
    }
}

struct SearchCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "search", abstract: "Full-text search across providers and projects (F4).")

    @Argument(help: "Literal text to search for.") var query: String
    @Option(name: [.customShort("p"), .long], help: "Restrict to one project.") var project: String?
    @Option(name: .long, help: "Restrict to one provider id.") var provider: String?
    @Flag(name: .long, help: "Emit JSON.") var json = false

    func run() throws {
        try CLIEnvironment.run {
            let store = CLIEnvironment.store()
            let projectPath = try project.map { try store.resolveProject($0) }
            let engine = try CLIEnvironment.searchEngine()
            let hits = try engine.search(query, scope: SearchScope(projectPath: projectPath,
                                                                   providerId: provider))
            if json {
                print(try VibeTrailJSON.encode(hits))
                return
            }
            if hits.isEmpty {
                print("No matches for \"\(query)\".")
                return
            }
            // F4: results aggregated per session.
            var order: [String] = []
            var grouped: [String: [SearchHit]] = [:]
            for hit in hits {
                if grouped[hit.sessionId] == nil { order.append(hit.sessionId) }
                grouped[hit.sessionId, default: []].append(hit)
            }
            for sessionId in order {
                let sessionHits = grouped[sessionId]!
                let first = sessionHits[0]
                print("\(first.nativeSessionId.prefix(8))  \(Format.abbreviatePath(first.projectPath))  [\(first.providerId)]")
                for hit in sessionHits.prefix(5) {
                    print("    \(Format.truncate(hit.snippet, to: 140))")
                }
                if sessionHits.count > 5 {
                    print("    … \(sessionHits.count - 5) more matches")
                }
            }
        }
    }
}

struct ShowCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "show", abstract: "Show one session as outline (default) or full transcript (F3).")

    @Argument(help: "Session id (or unique prefix).") var sessionId: String
    @Flag(name: .long, help: "Outline view (default).") var outline = false
    @Flag(name: .long, help: "Full transcript.") var full = false
    @Flag(name: .long, help: "Emit JSON.") var json = false

    func run() throws {
        try CLIEnvironment.run {
            if outline && full {
                throw VibeTrailError.usage("--outline and --full are mutually exclusive")
            }
            let store = CLIEnvironment.store()
            let (provider, raw) = try store.resolveSession(sessionId)
            if full {
                let session = try provider.parse(raw)
                if json {
                    print(try VibeTrailJSON.encode(session))
                    return
                }
                printHeader(session.summary)
                for message in session.messages {
                    printFull(message)
                }
            } else {
                let stubs = try provider.outline(raw)
                if json {
                    print(try VibeTrailJSON.encode(stubs))
                    return
                }
                printHeader(try provider.summarize(raw))
                for stub in stubs {
                    let index = Format.pad("\(stub.index)", 4)
                    print("\(index) \(Format.roleIcon(stub.role)) \(Format.truncate(stub.preview, to: 110))")
                }
            }
        }
    }

    private func printHeader(_ summary: SessionSummary) {
        print("\(summary.title)")
        var meta = "\(summary.id) · \(Format.abbreviatePath(summary.projectPath)) · "
        meta += "\(summary.messageCount) messages · \(Format.relativeTime(summary.mtime))"
        if let branch = summary.gitBranch { meta += " · \(branch)" }
        print(meta)
        print(String(repeating: "─", count: 80))
    }

    private func printFull(_ message: Message) {
        print("\(Format.roleIcon(message.role)) [\(message.role.rawValue)]")
        for block in message.blocks {
            switch block {
            case .text(let text):
                print(text)
            case .toolUse(let name, _):
                print("  ⚙ tool_use: \(name)")
            case .toolResult(let summary, let truncated):
                let firstLine = summary.split(whereSeparator: \.isNewline).first ?? ""
                print("  → tool_result: \(Format.truncate(String(firstLine), to: 100))\(truncated ? " …" : "")")
            case .thinking:
                print("  ✳ thinking (collapsed)")
            }
        }
        print("")
    }
}

struct ResumeCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "resume", abstract: "Resume a session in its project directory (F5).")

    @Argument(help: "Session id (or unique prefix).") var sessionId: String

    func run() throws {
        try CLIEnvironment.run {
            let spec = try CLIEnvironment.store().resumeSpec(for: sessionId)
            try CLIResumer().resume(spec)
        }
    }
}

struct OpenCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "open", abstract: "Open the VibeTrail GUI app.")

    @Argument(help: "Optional project to open at.") var project: String?

    func run() throws {
        try CLIEnvironment.run {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/open")
            process.arguments = ["-a", "VibeTrail"] + (project.map { [$0] } ?? [])
            try? process.run()
            process.waitUntilExit()
            if process.terminationStatus != 0 {
                throw VibeTrailError.data("VibeTrail.app not found; build it from the VibeTrailApp target")
            }
        }
    }
}
