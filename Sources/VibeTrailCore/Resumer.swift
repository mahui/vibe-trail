import Foundation

/// ADR-4: one implementation per shell — the CLI chdir+execs, the GUI drives a
/// terminal via AppleScript. Core only defines the contract and the
/// precondition check (SessionStore.resumeSpec).
public protocol Resumer: Sendable {
    func resume(_ spec: ResumeSpec) throws
}
