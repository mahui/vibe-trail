import Foundation
import VibeTrailCore

/// ADR-4 GUI resume: drive Terminal.app via AppleScript to cd into the
/// project and run the provider's resume command. First use triggers the
/// system Automation permission prompt.
struct TerminalResumer: Resumer {
    func resume(_ spec: ResumeSpec) throws {
        let shellCommand = "cd \(shellQuote(spec.projectPath)) && "
            + spec.command.map(shellQuote).joined(separator: " ")
        let source = """
        tell application "Terminal"
            activate
            do script "\(appleScriptEscape(shellCommand))"
        end tell
        """
        var errorInfo: NSDictionary?
        guard let script = NSAppleScript(source: source) else {
            throw VibeTrailError.data("Failed to build resume AppleScript")
        }
        script.executeAndReturnError(&errorInfo)
        if let errorInfo {
            let message = errorInfo[NSAppleScript.errorMessage] as? String ?? "\(errorInfo)"
            throw VibeTrailError.data("Terminal automation failed: \(message)")
        }
    }

    private func shellQuote(_ text: String) -> String {
        "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    private func appleScriptEscape(_ text: String) -> String {
        text.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
    }
}
