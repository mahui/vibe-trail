import SwiftUI
import VibeTrailCore

@main
struct VibeTrailApp: App {
    @State private var state = AppState()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(state)
                .task { await state.loadProjects() }
        }
    }
}

@MainActor
@Observable
final class AppState {
    let store = SessionStore(providers: [ClaudeCodeProvider()])

    var projects: [Project] = []
    var selectedProjectId: String?
    var sessions: [SessionSummary] = []
    var selectedSessionId: String?
    var detail: Session?
    var detailLoading = false

    var searchQuery = ""
    var searchHits: [SearchHit] = []
    var searchActive = false
    /// Message uuid the detail view should scroll to after a search jump.
    var scrollTarget: String?

    var errorMessage: String?

    func loadProjects() async {
        let store = store
        do {
            projects = try await Task.detached { try store.projects() }.value
        } catch {
            errorMessage = "\(error)"
        }
    }

    func loadSessions(projectId: String) async {
        let store = store
        do {
            sessions = try await Task.detached {
                try store.sessions(inProject: projectId)
            }.value
        } catch {
            sessions = []
            errorMessage = "\(error)"
        }
    }

    func loadDetail(sessionId: String) async {
        let store = store
        detailLoading = true
        defer { detailLoading = false }
        do {
            detail = try await Task.detached { () -> Session in
                let (provider, raw) = try store.resolveSession(sessionId)
                return try provider.parse(raw)
            }.value
        } catch {
            detail = nil
            errorMessage = "\(error)"
        }
    }

    func performSearch() async {
        let query = searchQuery.trimmingCharacters(in: .whitespaces)
        guard !query.isEmpty else {
            searchActive = false
            searchHits = []
            return
        }
        guard let rg = RipgrepSearchEngine.locateRipgrep() else {
            errorMessage = "ripgrep (rg) not found; install with `brew install ripgrep`"
            return
        }
        let engine = RipgrepSearchEngine(
            providers: store.providers.compactMap { $0 as? any SearchableProvider },
            ripgrepURL: rg)
        let scope = SearchScope(projectPath: selectedProjectId)
        do {
            searchHits = try await Task.detached { try engine.search(query, scope: scope) }.value
            searchActive = true
        } catch {
            errorMessage = "\(error)"
        }
    }

    /// F4: jump from a search hit to its message in the session timeline.
    func open(hit: SearchHit) async {
        searchActive = false
        selectedProjectId = hit.projectPath
        await loadSessions(projectId: hit.projectPath)
        selectedSessionId = hit.sessionId
        await loadDetail(sessionId: hit.sessionId)
        scrollTarget = hit.messageUuid
    }

    func resume(_ summary: SessionSummary) {
        do {
            let spec = try store.resumeSpec(for: summary.id)
            try TerminalResumer().resume(spec)
        } catch {
            errorMessage = "\(error)"
        }
    }

    func canResume(_ summary: SessionSummary) -> Bool {
        guard let provider = store.provider(id: summary.providerId) else { return false }
        return provider.capabilities.resumable
            && FileManager.default.fileExists(atPath: summary.projectPath)
    }
}
