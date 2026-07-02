import Foundation

/// Normalizes cwd strings coming out of provider metadata so that sessions
/// from different providers referring to the same directory group together.
public enum PathNormalizer {
    public static func normalize(_ path: String) -> String {
        var expanded = (path as NSString).expandingTildeInPath
        expanded = (expanded as NSString).standardizingPath
        let resolved = URL(fileURLWithPath: expanded).resolvingSymlinksInPath().path
        return resolved.count > 1 && resolved.hasSuffix("/") ? String(resolved.dropLast()) : resolved
    }
}

/// Aggregation façade shared by CLI and GUI. Owns no state: every call re-reads
/// provider stores (ADR-2, live reads, no index).
public struct SessionStore: Sendable {
    public let providers: [any Provider]

    public init(providers: [any Provider]) {
        self.providers = providers
    }

    public func provider(id: String) -> (any Provider)? {
        providers.first { $0.id == id }
    }

    public func discoverAll(providerId: String? = nil) throws -> [RawSession] {
        var all: [RawSession] = []
        for provider in providers where providerId == nil || provider.id == providerId {
            all.append(contentsOf: try provider.discover())
        }
        return all
    }

    /// F1: cross-provider project overview, grouped by normalized cwd.
    public func projects() throws -> [Project] {
        let sessions = try discoverAll()
        let grouped = Dictionary(grouping: sessions, by: \.projectPath)
        let fm = FileManager.default
        var projects: [Project] = []
        for (path, group) in grouped {
            let newest = group.max { $0.mtime < $1.mtime }!
            let lastPrompt = provider(id: newest.providerId)
                .flatMap { try? $0.quickTitle(newest) }
                .flatMap { $0.isEmpty ? nil : $0 }
            projects.append(Project(
                id: path,
                realPath: path,
                exists: fm.fileExists(atPath: path),
                sessionCount: group.count,
                lastActive: newest.mtime,
                lastPrompt: lastPrompt,
                providers: Set(group.map(\.providerId))
            ))
        }
        return projects.sorted { $0.lastActive > $1.lastActive }
    }

    /// F2: sessions of one project, newest first.
    public func sessions(inProject projectPath: String, providerId: String? = nil,
                         limit: Int? = nil) throws -> [SessionSummary] {
        let normalized = PathNormalizer.normalize(projectPath)
        let raws = try discoverAll(providerId: providerId)
            .filter { $0.projectPath == normalized }
            .sorted { $0.mtime > $1.mtime }
        let sliced = limit.map { Array(raws.prefix($0)) } ?? raws
        return try sliced.compactMap { raw in
            guard let provider = provider(id: raw.providerId) else { return nil }
            return try provider.summarize(raw)
        }
    }

    /// Resolve a user-supplied project reference: absolute/relative path or a
    /// unique suffix of a known project path.
    public func resolveProject(_ reference: String) throws -> String {
        let normalized = PathNormalizer.normalize(reference)
        let known = Set(try discoverAll().map(\.projectPath))
        if known.contains(normalized) { return normalized }
        let suffixMatches = known.filter {
            $0.hasSuffix("/" + reference) || ($0 as NSString).lastPathComponent == reference
        }
        switch suffixMatches.count {
        case 1: return suffixMatches.first!
        case 0: throw VibeTrailError.data("No project matches \"\(reference)\"")
        default:
            throw VibeTrailError.usage(
                "Ambiguous project \"\(reference)\": \(suffixMatches.sorted().joined(separator: ", "))")
        }
    }

    /// Resolve a session by composite id ("provider:native-id"), full native
    /// id, or unique native-id prefix.
    public func resolveSession(_ reference: String) throws -> (provider: any Provider, raw: RawSession) {
        let all = try discoverAll()
        let matches = all.filter {
            $0.compositeId == reference || $0.nativeId == reference || $0.nativeId.hasPrefix(reference)
        }
        switch matches.count {
        case 1:
            let raw = matches[0]
            return (provider(id: raw.providerId)!, raw)
        case 0:
            throw VibeTrailError.data("No session matches \"\(reference)\"")
        default:
            let ids = matches.prefix(5).map(\.compositeId).joined(separator: ", ")
            throw VibeTrailError.usage("Ambiguous session id \"\(reference)\": \(ids)…")
        }
    }

    /// F5 precondition: a session can only be resumed into an existing project
    /// directory, and only by a provider that declares the capability.
    public func resumeSpec(for reference: String) throws -> ResumeSpec {
        let (provider, raw) = try resolveSession(reference)
        guard provider.capabilities.resumable else {
            throw VibeTrailError.unsupported("Provider \(provider.id) does not support resume")
        }
        let summary = try provider.summarize(raw)
        guard let spec = provider.resumeSpec(summary) else {
            throw VibeTrailError.unsupported("Session \(summary.id) cannot be resumed")
        }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: spec.projectPath, isDirectory: &isDirectory),
              isDirectory.boolValue else {
            throw VibeTrailError.resumePrecondition(
                "Project path no longer exists: \(spec.projectPath)")
        }
        return spec
    }
}
