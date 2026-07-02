import Foundation

public struct ProviderCapabilities: Sendable {
    public let resumable: Bool
    public let fileBasedOnly: Bool
    public let hasArtifacts: Bool
    public let projectNative: Bool

    public init(resumable: Bool, fileBasedOnly: Bool, hasArtifacts: Bool, projectNative: Bool) {
        self.resumable = resumable
        self.fileBasedOnly = fileBasedOnly
        self.hasArtifacts = hasArtifacts
        self.projectNative = projectNative
    }
}

/// Metadata-level handle to a stored session, produced by discovery without
/// parsing the transcript body.
public struct RawSession: Sendable {
    public let providerId: String
    public let nativeId: String
    public let fileURL: URL
    public let projectPath: String
    public let mtime: Date
    public let fileSize: Int64

    public init(providerId: String, nativeId: String, fileURL: URL,
                projectPath: String, mtime: Date, fileSize: Int64) {
        self.providerId = providerId
        self.nativeId = nativeId
        self.fileURL = fileURL
        self.projectPath = projectPath
        self.mtime = mtime
        self.fileSize = fileSize
    }

    public var compositeId: String { "\(providerId):\(nativeId)" }
}

public struct ResumeSpec: Codable, Equatable, Sendable {
    public let projectPath: String
    /// argv of the resume command, e.g. ["claude", "--resume", "<uuid>"].
    public let command: [String]

    public init(projectPath: String, command: [String]) {
        self.projectPath = projectPath
        self.command = command
    }
}

public protocol Provider: Sendable {
    var id: String { get }
    var capabilities: ProviderCapabilities { get }
    /// Enumerate stored sessions. Metadata-level only: directory listing plus
    /// at most the first line/block of each file.
    func discover() throws -> [RawSession]
    func parse(_ raw: RawSession) throws -> Session
    func outline(_ raw: RawSession) throws -> [MessageStub]
    func page(_ raw: RawSession, offset: Int, limit: Int) throws -> [Message]
    /// nil when this provider (or this session) cannot be resumed.
    func resumeSpec(_ summary: SessionSummary) -> ResumeSpec?
    /// Cheap title/prompt extraction for the project overview. Must stay
    /// metadata-level (bounded reads); the default falls back to a full parse.
    func quickTitle(_ raw: RawSession) throws -> String?
}

public extension Provider {
    /// Summary without keeping the full message array around. Providers may
    /// override with a cheaper streaming implementation.
    func summarize(_ raw: RawSession) throws -> SessionSummary {
        try parse(raw).summary
    }

    func quickTitle(_ raw: RawSession) throws -> String? {
        try summarize(raw).title
    }
}

/// Adopted by providers whose stores can be grepped as plain text. The search
/// engine shells out to ripgrep over `searchRoots` and hands raw matches back
/// to the owning provider for resolution, keeping format knowledge inside the
/// provider.
public protocol SearchableProvider: Provider {
    /// Directories to grep; scoped to one project when `projectPath` is given.
    func searchRoots(projectPath: String?) -> [URL]
    /// Map one matched line back to a session/message. Return nil to drop the
    /// hit (e.g. the match landed in structural metadata, not message text).
    func resolveHit(fileURL: URL, line: String, query: String) -> SearchHit?
}
