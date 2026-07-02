import Foundation

/// Claude Code provider: reads `~/.claude/projects/**` strictly read-only.
public struct ClaudeCodeProvider: Provider, SearchableProvider {
    public let id = "claude-code"
    public let capabilities = ProviderCapabilities(
        resumable: true, fileBasedOnly: true, hasArtifacts: false, projectNative: true)

    /// `~/.claude/projects`; injectable for fixture tests.
    public let root: URL

    public init(root: URL? = nil) {
        self.root = root ?? FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".claude/projects")
    }

    // MARK: - Discovery

    public func discover() throws -> [RawSession] {
        let fm = FileManager.default
        guard fm.fileExists(atPath: root.path) else { return [] }
        let projectDirs = (try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: [.isDirectoryKey],
                                                       options: .skipsHiddenFiles)) ?? []
        var sessions: [RawSession] = []
        for dir in projectDirs {
            guard (try? dir.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true else { continue }
            let files = (try? fm.contentsOfDirectory(
                at: dir, includingPropertiesForKeys: [.contentModificationDateKey, .fileSizeKey],
                options: .skipsHiddenFiles)) ?? []
            // Only top-level <session-uuid>.jsonl files; subagent transcripts
            // live under <session-uuid>/subagents/ and are not sessions.
            var dirFallbackCwd: String?
            for file in files.sorted(by: { $0.lastPathComponent < $1.lastPathComponent })
            where file.pathExtension == "jsonl" {
                let values = try? file.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey])
                let cwd = extractCwd(fileURL: file) ?? dirFallbackCwd ?? decodeProjectDirName(dir.lastPathComponent)
                if dirFallbackCwd == nil { dirFallbackCwd = cwd }
                sessions.append(RawSession(
                    providerId: id,
                    nativeId: file.deletingPathExtension().lastPathComponent,
                    fileURL: file,
                    projectPath: PathNormalizer.normalize(cwd),
                    mtime: values?.contentModificationDate ?? .distantPast,
                    fileSize: Int64(values?.fileSize ?? 0)))
            }
        }
        return sessions
    }

    /// Metadata-level cwd extraction: scan only the head of the file for the
    /// first entry carrying a `cwd` field (discovery must not full-parse).
    func extractCwd(fileURL: URL) -> String? {
        guard let handle = try? FileHandle(forReadingFrom: fileURL) else { return nil }
        defer { try? handle.close() }
        let head = (try? handle.read(upToCount: 128 * 1024)) ?? Data()
        for lineData in head.split(separator: UInt8(ascii: "\n")).prefix(40) {
            guard let object = try? JSONSerialization.jsonObject(with: Data(lineData)) as? [String: Any],
                  let cwd = object["cwd"] as? String, !cwd.isEmpty else { continue }
            return cwd
        }
        return nil
    }

    /// Lossy fallback only ("-Users-x-my-app" cannot distinguish "/" from "-"
    /// or "."); real resolution comes from the cwd field inside the file.
    func decodeProjectDirName(_ name: String) -> String {
        name.hasPrefix("-") ? name.replacingOccurrences(of: "-", with: "/") : name
    }

    // MARK: - Parsing

    public func parse(_ raw: RawSession) throws -> Session {
        guard let data = FileManager.default.contents(atPath: raw.fileURL.path) else {
            throw VibeTrailError.data("Cannot read session file: \(raw.fileURL.path)")
        }
        let result = CCPipeline.run(data: data)
        var extensions: [String: JSONValue] = [:]
        let subagents = parseSubagents(for: raw)
        if !subagents.isEmpty {
            extensions["subagents"] = .array(subagents)
        }
        extensions["debug"] = debugExtension(result.stats)
        return Session(summary: makeSummary(raw: raw, result: result),
                       messages: result.messages,
                       extensions: extensions)
    }

    private func makeSummary(raw: RawSession, result: CCParseResult) -> SessionSummary {
        let title = sanitizeTitle(result.aiTitle ?? result.firstUserPrompt ?? result.lastPrompt ?? "")
        let timestamps = result.messages.map(\.timestamp).filter { $0 != Date(timeIntervalSince1970: 0) }
        let duration = timestamps.isEmpty ? 0 : timestamps.max()!.timeIntervalSince(timestamps.min()!)
        return SessionSummary(
            providerId: id,
            nativeId: raw.nativeId,
            projectPath: raw.projectPath,
            title: title.isEmpty ? String(raw.nativeId.prefix(8)) : title,
            mtime: raw.mtime,
            messageCount: result.messages.count,
            gitBranch: result.gitBranch,
            duration: duration)
    }

    private func sanitizeTitle(_ text: String) -> String {
        let collapsed = text.split(whereSeparator: \.isNewline).joined(separator: " ")
            .trimmingCharacters(in: .whitespaces)
        return collapsed.count > 80 ? String(collapsed.prefix(79)) + "…" : collapsed
    }

    /// Rule 4: subagent transcripts are separate files merged in a fixed,
    /// deterministic order (sorted by file name), each through the full
    /// pipeline — never fused into the main pass.
    private func parseSubagents(for raw: RawSession) -> [JSONValue] {
        let fm = FileManager.default
        let dir = raw.fileURL.deletingPathExtension().appendingPathComponent("subagents")
        guard let files = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else {
            return []
        }
        var subagents: [JSONValue] = []
        for file in files.sorted(by: { $0.lastPathComponent < $1.lastPathComponent })
        where file.pathExtension == "jsonl" {
            guard let data = fm.contents(atPath: file.path) else { continue }
            // Subagent transcripts are sidechains by definition.
            let result = CCPipeline.run(data: data, includeSidechain: true)
            var object: [String: JSONValue] = [
                "agentId": .string(file.deletingPathExtension().lastPathComponent),
                "messageCount": .number(Double(result.messages.count)),
            ]
            let metaURL = file.deletingPathExtension().appendingPathExtension("meta.json")
            if let metaData = fm.contents(atPath: metaURL.path),
               let meta = try? JSONDecoder().decode(CCSubagentMeta.self, from: metaData) {
                if let type = meta.agentType { object["agentType"] = .string(type) }
                if let description = meta.description { object["description"] = .string(description) }
            }
            subagents.append(.object(object))
        }
        return subagents
    }

    private func debugExtension(_ stats: CCParseStats) -> JSONValue {
        .object([
            "undecodableLines": .number(Double(stats.undecodableLines)),
            "duplicateUuids": .number(Double(stats.duplicateUuids)),
            "sidechainEntries": .number(Double(stats.sidechainEntries)),
            "ignoredEntryTypes": .object(stats.ignoredEntryTypes.mapValues { .number(Double($0)) }),
            "unknownBlockTypes": .object(stats.unknownBlockTypes.mapValues { .number(Double($0)) }),
        ])
    }

    /// Bounded-read title: newest `last-prompt`/`ai-title` entry from the file
    /// tail, else the first human prompt from the head. Never full-parses, so
    /// the project overview stays within its cold-start budget.
    public func quickTitle(_ raw: RawSession) throws -> String? {
        let decoder = JSONDecoder()
        if let handle = try? FileHandle(forReadingFrom: raw.fileURL) {
            defer { try? handle.close() }
            let size = (try? handle.seekToEnd()) ?? 0
            let tailLength = min(size, 128 * 1024)
            try? handle.seek(toOffset: size - tailLength)
            let tail = (try? handle.readToEnd()) ?? Data()
            // First chunk may be a partial line; its decode just fails and is skipped.
            for lineData in tail.split(separator: UInt8(ascii: "\n")).reversed() {
                guard let entry = try? decoder.decode(CCEntry.self, from: Data(lineData)) else { continue }
                if entry.type == "last-prompt", let prompt = entry.lastPrompt {
                    return sanitizeTitle(prompt)
                }
                if entry.type == "ai-title", let title = entry.aiTitle {
                    return sanitizeTitle(title)
                }
            }
        }
        guard let handle = try? FileHandle(forReadingFrom: raw.fileURL) else { return nil }
        defer { try? handle.close() }
        let head = (try? handle.read(upToCount: 128 * 1024)) ?? Data()
        for lineData in head.split(separator: UInt8(ascii: "\n")) {
            guard let entry = try? decoder.decode(CCEntry.self, from: Data(lineData)),
                  entry.type == "user", entry.isMeta != true,
                  case .text(let text)? = entry.message?.content else { continue }
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty && !trimmed.hasPrefix("<") {
                return sanitizeTitle(trimmed)
            }
        }
        return nil
    }

    // MARK: - Outline / paging

    public func outline(_ raw: RawSession) throws -> [MessageStub] {
        try parse(raw).messages.enumerated().map { index, message in
            MessageStub(index: index, uuid: message.uuid, role: message.role,
                        preview: preview(of: message), timestamp: message.timestamp)
        }
    }

    public func page(_ raw: RawSession, offset: Int, limit: Int) throws -> [Message] {
        let messages = try parse(raw).messages
        guard offset < messages.count, offset >= 0, limit > 0 else { return [] }
        return Array(messages[offset..<min(offset + limit, messages.count)])
    }

    private func preview(of message: Message) -> String {
        for block in message.blocks {
            if case .text(let text) = block {
                let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
                return String(line.prefix(120))
            }
        }
        switch message.blocks.first {
        case .toolUse(let name, _): return "⚙ \(name)"
        case .toolResult(let summary, _):
            return "→ " + String(summary.split(whereSeparator: \.isNewline).first ?? "").prefix(100)
        case .thinking: return "(thinking)"
        default: return ""
        }
    }

    // MARK: - Resume

    public func resumeSpec(_ summary: SessionSummary) -> ResumeSpec? {
        ResumeSpec(projectPath: summary.projectPath,
                   command: ["claude", "--resume", summary.nativeId])
    }

    // MARK: - Search

    public func searchRoots(projectPath: String?) -> [URL] {
        guard let projectPath else { return [root] }
        let normalized = PathNormalizer.normalize(projectPath)
        let fm = FileManager.default
        let dirs = (try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: [.isDirectoryKey],
                                                options: .skipsHiddenFiles)) ?? []
        return dirs.filter { dir in
            guard (try? dir.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true else { return false }
            guard let firstFile = (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil))?
                .first(where: { $0.pathExtension == "jsonl" }) else { return false }
            let cwd = extractCwd(fileURL: firstFile) ?? decodeProjectDirName(dir.lastPathComponent)
            return PathNormalizer.normalize(cwd) == normalized
        }
    }

    public func resolveHit(fileURL: URL, line: String, query: String) -> SearchHit? {
        guard let entry = try? JSONDecoder().decode(CCEntry.self, from: Data(line.utf8)),
              entry.type == "user" || entry.type == "assistant",
              entry.isMeta != true else { return nil }
        var texts: [String] = []
        switch entry.message?.content {
        case .text(let text): texts.append(text)
        case .blocks(let blocks):
            for block in blocks {
                if let text = block.text { texts.append(text) }
                if let thinking = block.thinking { texts.append(thinking) }
                if let content = block.content { texts.append(contentsOf: stringLeaves(of: content)) }
                if let input = block.input { texts.append(contentsOf: stringLeaves(of: input)) }
            }
        case nil: break
        }
        guard let snippet = makeSnippet(texts: texts, query: query) else { return nil }
        let components = fileURL.pathComponents
        let nativeSessionId: String
        if let subagentIndex = components.lastIndex(of: "subagents"), subagentIndex > 0 {
            nativeSessionId = components[subagentIndex - 1]
        } else {
            nativeSessionId = fileURL.deletingPathExtension().lastPathComponent
        }
        let cwd = entry.cwd
            ?? extractCwd(fileURL: fileURL)
            ?? decodeProjectDirName(fileURL.deletingLastPathComponent().lastPathComponent)
        return SearchHit(providerId: id,
                         nativeSessionId: nativeSessionId,
                         projectPath: PathNormalizer.normalize(cwd),
                         messageUuid: entry.uuid,
                         snippet: snippet)
    }

    private func stringLeaves(of value: JSONValue) -> [String] {
        switch value {
        case .string(let text): return [text]
        case .array(let items): return items.flatMap(stringLeaves)
        case .object(let object): return object.values.flatMap(stringLeaves)
        default: return []
        }
    }

    private func makeSnippet(texts: [String], query: String) -> String? {
        for text in texts {
            guard let range = text.range(of: query, options: [.caseInsensitive, .diacriticInsensitive]) else {
                continue
            }
            let start = text.index(range.lowerBound, offsetBy: -60, limitedBy: text.startIndex) ?? text.startIndex
            let end = text.index(range.upperBound, offsetBy: 100, limitedBy: text.endIndex) ?? text.endIndex
            let snippet = text[start..<end].split(whereSeparator: \.isNewline).joined(separator: " ")
            return (start > text.startIndex ? "…" : "") + snippet + (end < text.endIndex ? "…" : "")
        }
        return nil
    }
}
