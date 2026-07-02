import Foundation

/// Core error taxonomy. `exitCode` matches the CLI contract in TECH_SPEC §6:
/// 1 usage, 2 data, 3 resume precondition, 4 unsupported by provider.
public enum VibeTrailError: Error, CustomStringConvertible, Sendable {
    case usage(String)
    case data(String)
    case resumePrecondition(String)
    case unsupported(String)

    public var exitCode: Int32 {
        switch self {
        case .usage: return 1
        case .data: return 2
        case .resumePrecondition: return 3
        case .unsupported: return 4
        }
    }

    public var description: String {
        switch self {
        case .usage(let message),
             .data(let message),
             .resumePrecondition(let message),
             .unsupported(let message):
            return message
        }
    }
}
