import Darwin
import Foundation
import VibeTrailCore

/// ADR-4 CLI resume: chdir into the project, then exec the provider's resume
/// command, replacing this process. Never returns on success.
struct CLIResumer: Resumer {
    func resume(_ spec: ResumeSpec) throws {
        guard let executable = spec.command.first else {
            throw VibeTrailError.data("Empty resume command")
        }
        guard FileManager.default.changeCurrentDirectoryPath(spec.projectPath) else {
            throw VibeTrailError.resumePrecondition("Cannot chdir to \(spec.projectPath)")
        }
        var argv: [UnsafeMutablePointer<CChar>?] = spec.command.map { strdup($0) }
        argv.append(nil)
        execvp(executable, argv)
        // Only reached when exec failed.
        throw VibeTrailError.data(
            "Failed to exec \(spec.command.joined(separator: " ")): \(String(cString: strerror(errno)))")
    }
}
