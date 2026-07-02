import Foundation

public struct SearchScope: Sendable {
    public let projectPath: String?
    public let providerId: String?

    public init(projectPath: String? = nil, providerId: String? = nil) {
        self.projectPath = projectPath
        self.providerId = providerId
    }
}

public struct SearchHit: Codable, Equatable, Sendable {
    public let providerId: String
    /// Composite session id ("provider:native-id").
    public let sessionId: String
    public let nativeSessionId: String
    public let projectPath: String
    public let messageUuid: String?
    public let snippet: String

    public init(providerId: String, nativeSessionId: String, projectPath: String,
                messageUuid: String?, snippet: String) {
        self.providerId = providerId
        self.sessionId = "\(providerId):\(nativeSessionId)"
        self.nativeSessionId = nativeSessionId
        self.projectPath = projectPath
        self.messageUuid = messageUuid
        self.snippet = snippet
    }
}

public protocol SearchEngine: Sendable {
    func search(_ query: String, scope: SearchScope) throws -> [SearchHit]
}

/// ADR-3: full-text search shells out to ripgrep, no index. Matched lines are
/// handed back to the owning provider so format knowledge stays out of Core.
public struct RipgrepSearchEngine: SearchEngine {
    public let providers: [any SearchableProvider]
    public let ripgrepURL: URL

    /// Locates `rg` on PATH, falling back to the given bundled binary.
    public static func locateRipgrep(bundled: URL? = nil) -> URL? {
        let fm = FileManager.default
        let pathDirs = (ProcessInfo.processInfo.environment["PATH"] ?? "")
            .split(separator: ":").map(String.init)
        let candidates = pathDirs.map { "\($0)/rg" } + ["/opt/homebrew/bin/rg", "/usr/local/bin/rg"]
        for candidate in candidates where fm.isExecutableFile(atPath: candidate) {
            return URL(fileURLWithPath: candidate)
        }
        if let bundled, fm.isExecutableFile(atPath: bundled.path) { return bundled }
        return nil
    }

    public init(providers: [any SearchableProvider], ripgrepURL: URL) {
        self.providers = providers
        self.ripgrepURL = ripgrepURL
    }

    public func search(_ query: String, scope: SearchScope) throws -> [SearchHit] {
        guard !query.isEmpty else { throw VibeTrailError.usage("Empty search query") }
        var hits: [SearchHit] = []
        for provider in providers where scope.providerId == nil || provider.id == scope.providerId {
            let roots = provider.searchRoots(projectPath: scope.projectPath)
                .filter { FileManager.default.fileExists(atPath: $0.path) }
            guard !roots.isEmpty else { continue }
            for match in try runRipgrep(query: query, roots: roots) {
                if let hit = provider.resolveHit(fileURL: match.fileURL, line: match.line, query: query) {
                    hits.append(hit)
                }
            }
        }
        return hits
    }

    struct RawMatch {
        let fileURL: URL
        let line: String
    }

    private func runRipgrep(query: String, roots: [URL]) throws -> [RawMatch] {
        let process = Process()
        process.executableURL = ripgrepURL
        process.arguments = ["--json", "--ignore-case", "--fixed-strings",
                             "--max-count", "50", "--glob", "*.jsonl",
                             "--", query] + roots.map(\.path)
        let stdout = Pipe()
        process.standardOutput = stdout
        process.standardError = Pipe()
        try process.run()
        let data = stdout.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        // rg exits 1 when nothing matched; only >=2 is a real failure.
        if process.terminationStatus >= 2 {
            throw VibeTrailError.data("ripgrep failed with exit code \(process.terminationStatus)")
        }
        var matches: [RawMatch] = []
        for lineData in data.split(separator: UInt8(ascii: "\n")) {
            guard let event = try? JSONSerialization.jsonObject(with: Data(lineData)) as? [String: Any],
                  event["type"] as? String == "match",
                  let payload = event["data"] as? [String: Any],
                  let path = (payload["path"] as? [String: Any])?["text"] as? String,
                  let lineText = (payload["lines"] as? [String: Any])?["text"] as? String
            else { continue }
            matches.append(RawMatch(fileURL: URL(fileURLWithPath: path), line: lineText))
        }
        return matches
    }
}
