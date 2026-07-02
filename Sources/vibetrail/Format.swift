import Foundation
import VibeTrailCore

enum Format {
    static func relativeTime(_ date: Date) -> String {
        let interval = Date().timeIntervalSince(date)
        switch interval {
        case ..<60: return "just now"
        case ..<3600: return "\(Int(interval / 60))m ago"
        case ..<86400: return "\(Int(interval / 3600))h ago"
        case ..<(86400 * 30): return "\(Int(interval / 86400))d ago"
        default:
            let formatter = DateFormatter()
            formatter.dateFormat = "yyyy-MM-dd"
            return formatter.string(from: date)
        }
    }

    static func duration(_ interval: TimeInterval) -> String {
        switch interval {
        case ..<1: return "-"
        case ..<60: return "\(Int(interval))s"
        case ..<3600: return "\(Int(interval / 60))m"
        default: return String(format: "%.1fh", interval / 3600)
        }
    }

    static func abbreviatePath(_ path: String) -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        return path.hasPrefix(home) ? "~" + path.dropFirst(home.count) : path
    }

    static func truncate(_ text: String, to length: Int) -> String {
        text.count > length ? String(text.prefix(length - 1)) + "…" : text
    }

    static func roleIcon(_ role: Role) -> String {
        switch role {
        case .user: return "❯"
        case .assistant: return "●"
        case .system: return "◦"
        }
    }

    /// Fixed-width left-aligned column padding (rough; CJK width ignored).
    static func pad(_ text: String, _ width: Int) -> String {
        text.count >= width ? text : text + String(repeating: " ", count: width - text.count)
    }
}
