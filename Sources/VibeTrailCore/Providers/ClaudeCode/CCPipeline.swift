import Foundation

/// Parse statistics. Unknown input is never fatal (rule 3); it is counted here
/// and surfaced through `Session.extensions["debug"]`.
struct CCParseStats {
    var undecodableLines = 0
    var ignoredEntryTypes: [String: Int] = [:]
    var duplicateUuids = 0
    var sidechainEntries = 0
    var metaEntries = 0
    var unknownBlockTypes: [String: Int] = [:]
    var emptyMessages = 0
}

struct CCParseResult {
    var messages: [Message] = []
    var aiTitle: String?
    var lastPrompt: String?
    var gitBranch: String?
    var firstUserPrompt: String?
    var stats = CCParseStats()
}

/// The five-stage Claude Code transcript pipeline (TECH_SPEC §4.1):
/// entry decode → classify/filter → message regroup → tree rebuild → display
/// transform. Stages are deliberately separate functions; do not fuse them.
enum CCPipeline {
    static func run(data: Data, includeSidechain: Bool = false) -> CCParseResult {
        var result = CCParseResult()
        let entries = decodeEntries(data: data, stats: &result.stats)
        let kept = classifyAndFilter(entries, includeSidechain: includeSidechain, result: &result)
        let logical = regroupByMessageId(kept)
        let ordered = treeOrder(logical)
        result.messages = transform(ordered, stats: &result.stats)
        result.firstUserPrompt = firstUserPrompt(in: result.messages)
        return result
    }

    // MARK: Stage 1 — entry decode

    private static func decodeEntries(data: Data, stats: inout CCParseStats) -> [CCEntry] {
        let decoder = JSONDecoder()
        var entries: [CCEntry] = []
        for lineData in data.split(separator: UInt8(ascii: "\n")) {
            guard !lineData.isEmpty else { continue }
            if let entry = try? decoder.decode(CCEntry.self, from: Data(lineData)) {
                entries.append(entry)
            } else {
                stats.undecodableLines += 1
            }
        }
        return entries
    }

    // MARK: Stage 2 — classify, whitelist-filter, dedup by UUID (rules 2 & 3)

    private static func classifyAndFilter(_ entries: [CCEntry], includeSidechain: Bool,
                                          result: inout CCParseResult) -> [CCEntry] {
        var kept: [CCEntry] = []
        var seenUuids = Set<String>()
        for entry in entries {
            switch entry.type {
            case "user", "assistant":
                if let branch = entry.gitBranch, !branch.isEmpty {
                    result.gitBranch = branch
                }
                if entry.isMeta == true {
                    result.stats.metaEntries += 1
                    continue
                }
                if entry.isSidechain == true && !includeSidechain {
                    result.stats.sidechainEntries += 1
                    continue
                }
                if let uuid = entry.uuid {
                    guard seenUuids.insert(uuid).inserted else {
                        result.stats.duplicateUuids += 1
                        continue
                    }
                }
                kept.append(entry)
            case "ai-title":
                result.aiTitle = entry.aiTitle
            case "last-prompt":
                result.lastPrompt = entry.lastPrompt
            default:
                result.stats.ignoredEntryTypes[entry.type ?? "<none>", default: 0] += 1
            }
        }
        return kept
    }

    // MARK: Stage 3 — regroup streamed lines into logical messages (rule 1)

    struct LogicalMessage {
        /// UUID of the last physical chunk: the next message's parentUuid
        /// points at it, so it is the node identity in the tree.
        var uuid: String
        var parentUuid: String?
        var role: Role
        var timestamp: Date
        var blocks: [CCBlock]
        var plainText: String?
        var apiMessageId: String?
    }

    private static func regroupByMessageId(_ entries: [CCEntry]) -> [LogicalMessage] {
        var messages: [LogicalMessage] = []
        var indexByApiId: [String: Int] = [:]
        for entry in entries {
            let role: Role = entry.type == "assistant" ? .assistant : .user
            var blocks: [CCBlock] = []
            var plainText: String?
            switch entry.message?.content {
            case .text(let text): plainText = text
            case .blocks(let entryBlocks): blocks = entryBlocks
            case nil: break
            }
            let apiId = entry.message?.id
            if role == .assistant, let apiId, let index = indexByApiId[apiId] {
                // Continuation chunk of a streamed message: merge, advance uuid.
                messages[index].blocks.append(contentsOf: blocks)
                if let uuid = entry.uuid { messages[index].uuid = uuid }
                continue
            }
            let message = LogicalMessage(
                uuid: entry.uuid ?? UUID().uuidString,
                parentUuid: entry.parentUuid,
                role: role,
                timestamp: CCTimestamp.parse(entry.timestamp) ?? Date(timeIntervalSince1970: 0),
                blocks: blocks,
                plainText: plainText,
                apiMessageId: apiId
            )
            messages.append(message)
            if role == .assistant, let apiId { indexByApiId[apiId] = messages.count - 1 }
        }
        return messages
    }

    // MARK: Stage 4 — parent-child tree rebuild (rule 4)

    private static func treeOrder(_ messages: [LogicalMessage]) -> [LogicalMessage] {
        guard !messages.isEmpty else { return [] }
        let knownUuids = Set(messages.map(\.uuid))
        var childrenOf: [String: [Int]] = [:]
        var roots: [Int] = []
        for (index, message) in messages.enumerated() {
            if let parent = message.parentUuid, knownUuids.contains(parent), parent != message.uuid {
                childrenOf[parent, default: []].append(index)
            } else {
                roots.append(index)
            }
        }
        var ordered: [LogicalMessage] = []
        ordered.reserveCapacity(messages.count)
        var visited = Set<Int>()
        // Iterative DFS; children keep encounter (chronological) order, so
        // resume/branch duplicates that survived dedup stay in file order.
        var stack = roots.reversed().map { $0 }
        while let index = stack.popLast() {
            guard visited.insert(index).inserted else { continue }
            ordered.append(messages[index])
            let children = childrenOf[messages[index].uuid] ?? []
            stack.append(contentsOf: children.reversed())
        }
        // Cycles or orphaned nodes: append leftovers in file order, never drop.
        if ordered.count < messages.count {
            for (index, message) in messages.enumerated() where !visited.contains(index) {
                ordered.append(message)
            }
        }
        return ordered
    }

    // MARK: Stage 5 — display transform

    private static func transform(_ messages: [LogicalMessage], stats: inout CCParseStats) -> [Message] {
        var transformed: [Message] = []
        transformed.reserveCapacity(messages.count)
        for message in messages {
            var blocks: [ContentBlock] = []
            if let text = message.plainText, !text.isEmpty {
                blocks.append(.text(text))
            }
            for block in message.blocks {
                switch block.type {
                case "text":
                    if let text = block.text, !text.isEmpty { blocks.append(.text(text)) }
                case "thinking":
                    if let thinking = block.thinking, !thinking.isEmpty { blocks.append(.thinking(thinking)) }
                case "tool_use":
                    blocks.append(.toolUse(name: block.name ?? "?", input: block.input ?? .null))
                case "tool_result":
                    let full = flattenToolResult(block.content)
                    let truncated = full.count > 200
                    blocks.append(.toolResult(summary: String(full.prefix(200)), truncated: truncated))
                default:
                    stats.unknownBlockTypes[block.type ?? "<none>", default: 0] += 1
                }
            }
            guard !blocks.isEmpty else {
                stats.emptyMessages += 1
                continue
            }
            transformed.append(Message(uuid: message.uuid, parentUuid: message.parentUuid,
                                       role: message.role, blocks: blocks,
                                       timestamp: message.timestamp))
        }
        return transformed
    }

    private static func flattenToolResult(_ content: JSONValue?) -> String {
        switch content {
        case .string(let text):
            return text
        case .array(let items):
            return items.compactMap { $0.objectValue?["text"]?.stringValue }.joined(separator: "\n")
        default:
            return ""
        }
    }

    /// First real human prompt: used as the session title when no ai-title
    /// entry exists. Skips command/attachment XML payloads.
    private static func firstUserPrompt(in messages: [Message]) -> String? {
        for message in messages where message.role == .user {
            for block in message.blocks {
                if case .text(let text) = block {
                    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
                    if !trimmed.isEmpty && !trimmed.hasPrefix("<") {
                        return trimmed
                    }
                }
            }
        }
        return nil
    }
}
