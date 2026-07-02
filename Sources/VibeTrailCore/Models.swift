import Foundation

/// A project group derived from normalized session cwd values. Projects are
/// never stored; they are aggregated across providers at query time.
public struct Project: Codable, Equatable, Sendable {
    public let id: String
    public let realPath: String
    public let exists: Bool
    public let sessionCount: Int
    public let lastActive: Date
    public let lastPrompt: String?
    public let providers: Set<String>

    public init(id: String, realPath: String, exists: Bool, sessionCount: Int,
                lastActive: Date, lastPrompt: String?, providers: Set<String>) {
        self.id = id
        self.realPath = realPath
        self.exists = exists
        self.sessionCount = sessionCount
        self.lastActive = lastActive
        self.lastPrompt = lastPrompt
        self.providers = providers
    }
}

public struct SessionSummary: Codable, Equatable, Sendable {
    /// Globally unique key: "<providerId>:<nativeId>".
    public let id: String
    public let providerId: String
    public let nativeId: String
    public let projectPath: String
    public let title: String
    public let mtime: Date
    public let messageCount: Int
    public let gitBranch: String?
    public let duration: TimeInterval

    public init(providerId: String, nativeId: String, projectPath: String, title: String,
                mtime: Date, messageCount: Int, gitBranch: String?, duration: TimeInterval) {
        self.id = "\(providerId):\(nativeId)"
        self.providerId = providerId
        self.nativeId = nativeId
        self.projectPath = projectPath
        self.title = title
        self.mtime = mtime
        self.messageCount = messageCount
        self.gitBranch = gitBranch
        self.duration = duration
    }
}

public enum Role: String, Codable, Sendable {
    case user
    case assistant
    case system
}

public enum ContentBlock: Codable, Equatable, Sendable {
    case text(String)
    case toolUse(name: String, input: JSONValue)
    case toolResult(summary: String, truncated: Bool)
    case thinking(String)

    private enum CodingKeys: String, CodingKey {
        case kind, text, name, input, summary, truncated
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let kind = try container.decode(String.self, forKey: .kind)
        switch kind {
        case "text":
            self = .text(try container.decode(String.self, forKey: .text))
        case "tool_use":
            self = .toolUse(name: try container.decode(String.self, forKey: .name),
                            input: try container.decode(JSONValue.self, forKey: .input))
        case "tool_result":
            self = .toolResult(summary: try container.decode(String.self, forKey: .summary),
                               truncated: try container.decode(Bool.self, forKey: .truncated))
        case "thinking":
            self = .thinking(try container.decode(String.self, forKey: .text))
        default:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: container,
                                                   debugDescription: "Unknown block kind \(kind)")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .text(let text):
            try container.encode("text", forKey: .kind)
            try container.encode(text, forKey: .text)
        case .toolUse(let name, let input):
            try container.encode("tool_use", forKey: .kind)
            try container.encode(name, forKey: .name)
            try container.encode(input, forKey: .input)
        case .toolResult(let summary, let truncated):
            try container.encode("tool_result", forKey: .kind)
            try container.encode(summary, forKey: .summary)
            try container.encode(truncated, forKey: .truncated)
        case .thinking(let text):
            try container.encode("thinking", forKey: .kind)
            try container.encode(text, forKey: .text)
        }
    }
}

public struct Message: Codable, Equatable, Sendable {
    public let uuid: String
    public let parentUuid: String?
    public let role: Role
    public let blocks: [ContentBlock]
    public let timestamp: Date

    public init(uuid: String, parentUuid: String?, role: Role, blocks: [ContentBlock], timestamp: Date) {
        self.uuid = uuid
        self.parentUuid = parentUuid
        self.role = role
        self.blocks = blocks
        self.timestamp = timestamp
    }
}

public struct Session: Codable, Sendable {
    public let summary: SessionSummary
    public let messages: [Message]
    public let extensions: [String: JSONValue]

    public init(summary: SessionSummary, messages: [Message], extensions: [String: JSONValue]) {
        self.summary = summary
        self.messages = messages
        self.extensions = extensions
    }
}

/// Lightweight per-message stub used to render a timeline before paging in
/// full message bodies.
public struct MessageStub: Codable, Equatable, Sendable {
    public let index: Int
    public let uuid: String
    public let role: Role
    public let preview: String
    public let timestamp: Date

    public init(index: Int, uuid: String, role: Role, preview: String, timestamp: Date) {
        self.index = index
        self.uuid = uuid
        self.role = role
        self.preview = preview
        self.timestamp = timestamp
    }
}
