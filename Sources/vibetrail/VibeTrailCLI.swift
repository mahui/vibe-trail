import ArgumentParser
import Foundation
import VibeTrailCore

@main
struct VibeTrailCLI: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "vibetrail",
        abstract: "Session browser & resume for coding agents.",
        version: "0.1.0",
        subcommands: [ProjectsCommand.self, SessionsCommand.self, SearchCommand.self,
                      ShowCommand.self, ResumeCommand.self, OpenCommand.self])
}

enum CLIEnvironment {
    static func store() -> SessionStore {
        SessionStore(providers: [ClaudeCodeProvider()])
    }

    static func searchEngine() throws -> RipgrepSearchEngine {
        guard let rg = RipgrepSearchEngine.locateRipgrep() else {
            throw VibeTrailError.data("ripgrep (rg) not found on PATH; install with `brew install ripgrep`")
        }
        return RipgrepSearchEngine(providers: [ClaudeCodeProvider()], ripgrepURL: rg)
    }

    /// Runs a command body, mapping VibeTrailError onto the documented exit
    /// codes (1 usage / 2 data / 3 resume precondition / 4 unsupported).
    static func run(_ body: () throws -> Void) throws {
        do {
            try body()
        } catch let error as VibeTrailError {
            FileHandle.standardError.write(Data("vibetrail: \(error.description)\n".utf8))
            throw ExitCode(error.exitCode)
        }
    }
}
