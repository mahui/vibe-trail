import Foundation

/// Canonical encoder for the `--json` surface. GUI, CLI, and the future MCP
/// shell must all emit this exact shape; snapshot tests pin it.
public enum VibeTrailJSON {
    public static func encoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }

    public static func encode<T: Encodable>(_ value: T) throws -> String {
        String(decoding: try encoder().encode(value), as: UTF8.self)
    }
}
