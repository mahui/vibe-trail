import SwiftUI
import VibeTrailCore

struct ContentView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        @Bindable var state = state
        NavigationSplitView {
            ProjectListView()
                .navigationSplitViewColumnWidth(min: 220, ideal: 280)
        } content: {
            SessionListView()
                .navigationSplitViewColumnWidth(min: 300, ideal: 380)
        } detail: {
            SessionDetailView()
        }
        .searchable(text: $state.searchQuery, placement: .toolbar,
                    prompt: "Search all sessions")
        .onSubmit(of: .search) {
            Task { await state.performSearch() }
        }
        .onChange(of: state.searchQuery) { _, newValue in
            if newValue.isEmpty { state.searchActive = false }
        }
        .alert("Error", isPresented: .init(
            get: { state.errorMessage != nil },
            set: { if !$0 { state.errorMessage = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(state.errorMessage ?? "")
        }
    }
}

// MARK: - Sidebar (F1)

struct ProjectListView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        @Bindable var state = state
        List(state.projects, id: \.id, selection: $state.selectedProjectId) { project in
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 4) {
                    Text(displayName(project.realPath))
                        .fontWeight(.medium)
                        .foregroundStyle(project.exists ? .primary : .tertiary)
                    if !project.exists {
                        Image(systemName: "exclamationmark.triangle")
                            .font(.caption2)
                            .foregroundStyle(.orange)
                            .help("Project path no longer exists")
                    }
                }
                HStack(spacing: 6) {
                    Text("\(project.sessionCount) sessions")
                    Text(project.lastActive, format: .relative(presentation: .named))
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                if let prompt = project.lastPrompt {
                    Text(prompt)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            .padding(.vertical, 2)
        }
        .navigationTitle("Projects")
        .onChange(of: state.selectedProjectId) { _, projectId in
            guard let projectId else { return }
            Task { await state.loadSessions(projectId: projectId) }
        }
        .overlay {
            if state.projects.isEmpty {
                ContentUnavailableView("No sessions found", systemImage: "clock",
                                       description: Text("No agent session history discovered yet."))
            }
        }
    }

    private func displayName(_ path: String) -> String {
        let name = (path as NSString).lastPathComponent
        return name.isEmpty ? path : name
    }
}

// MARK: - Session list + search results overlay (F2 / F4)

struct SessionListView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        Group {
            if state.searchActive {
                SearchResultsView()
            } else {
                sessionList
            }
        }
        .navigationTitle(state.selectedProjectId.map { ($0 as NSString).lastPathComponent } ?? "Sessions")
    }

    private var sessionList: some View {
        @Bindable var state = state
        return List(state.sessions, id: \.id, selection: $state.selectedSessionId) { session in
            VStack(alignment: .leading, spacing: 2) {
                Text(session.title)
                    .fontWeight(.medium)
                    .lineLimit(2)
                HStack(spacing: 6) {
                    Text(session.mtime, format: .relative(presentation: .named))
                    Text("·")
                    Text("\(session.messageCount) msg")
                    if let branch = session.gitBranch {
                        Text("·")
                        Image(systemName: "arrow.triangle.branch").font(.caption2)
                        Text(branch)
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            .padding(.vertical, 2)
        }
        .onChange(of: state.selectedSessionId) { _, sessionId in
            guard let sessionId else { return }
            state.scrollTarget = nil
            Task { await state.loadDetail(sessionId: sessionId) }
        }
        .overlay {
            if state.selectedProjectId == nil {
                ContentUnavailableView("Select a project", systemImage: "folder")
            } else if state.sessions.isEmpty {
                ContentUnavailableView("No sessions", systemImage: "clock")
            }
        }
    }
}

struct SearchResultsView: View {
    @Environment(AppState.self) private var state

    /// Hits aggregated per session, preserving hit order.
    private var groups: [(sessionId: String, hits: [SearchHit])] {
        var order: [String] = []
        var grouped: [String: [SearchHit]] = [:]
        for hit in state.searchHits {
            if grouped[hit.sessionId] == nil { order.append(hit.sessionId) }
            grouped[hit.sessionId, default: []].append(hit)
        }
        return order.map { ($0, grouped[$0]!) }
    }

    var body: some View {
        List {
            ForEach(groups, id: \.sessionId) { group in
                Section {
                    ForEach(Array(group.hits.enumerated()), id: \.offset) { _, hit in
                        Text(hit.snippet)
                            .font(.callout)
                            .lineLimit(2)
                            .contentShape(Rectangle())
                            .onTapGesture {
                                Task { await state.open(hit: hit) }
                            }
                    }
                } header: {
                    Text("\(group.hits[0].projectPath) · \(group.hits[0].nativeSessionId.prefix(8)) · \(group.hits[0].providerId)")
                        .font(.caption)
                }
            }
        }
        .overlay {
            if state.searchHits.isEmpty {
                ContentUnavailableView.search(text: state.searchQuery)
            }
        }
    }
}
