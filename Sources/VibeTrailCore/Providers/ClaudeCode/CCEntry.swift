import Foundation

/// One physical line of a Claude Code session JSONL file. All fields optional:
/// unknown shapes must decode (and be skipped later), never throw (rule 3).
struct CCEntry: Decodable {
    let type: String?
    let uuid: String?
    let parentUuid: String?
    let timestamp: String?
    let cwd: String?
    let gitBranch: String?
    let isSidechain: Bool?
    let isMeta: Bool?
    let message: CCMessage?
    // Metadata entry payloads (type == "ai-title" / "last-prompt").
    let aiTitle: String?
    let lastPrompt: String?
}

struct CCMessage: Decodable {
    /// API message id ("msg_…"). Streaming splits one logical message across
    /// several lines sharing this id (rule 1).
    let id: String?
    let role: String?
    let content: CCMessageContent?
}

/// `message.content` is either a plain string (typed user prompt) or an array
/// of content blocks.
enum CCMessageContent: Decodable {
    case text(String)
    case blocks([CCBlock])

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let text = try? container.decode(String.self) {
            self = .text(text)
        } else if let blocks = try? container.decode([CCBlock].self) {
            self = .blocks(blocks)
        } else {
            self = .blocks([])
        }
    }
}

struct CCBlock: Decodable {
    let type: String?
    let text: String?
    let thinking: String?
    let name: String?
    let input: JSONValue?
    let content: JSONValue?
}

/// `subagents/agent-<id>.meta.json`.
struct CCSubagentMeta: Decodable {
    let agentType: String?
    let description: String?
}

enum CCTimestamp {
    /// ISO8601 with or without fractional seconds.
    static func parse(_ string: String?) -> Date? {
        guard let string else { return nil }
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = fractional.date(from: string) { return date }
        let plain = ISO8601DateFormatter()
        plain.formatOptions = [.withInternetDateTime]
        return plain.date(from: string)
    }
}
