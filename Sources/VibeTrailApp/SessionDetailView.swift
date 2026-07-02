import SwiftUI
import VibeTrailCore

/// F3: message timeline. Prompts and replies render in full; tool calls,
/// tool results, and thinking collapse to a single expandable row.
struct SessionDetailView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        Group {
            if state.detailLoading {
                ProgressView()
            } else if let session = state.detail {
                timeline(session)
            } else {
                ContentUnavailableView("Select a session", systemImage: "text.bubble")
            }
        }
        .toolbar {
            if let session = state.detail, state.canResume(session.summary) {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        state.resume(session.summary)
                    } label: {
                        Label("Resume", systemImage: "play.fill")
                    }
                    .help("Reopen this session in your terminal")
                }
            }
        }
    }

    private func timeline(_ session: Session) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    header(session.summary)
                    ForEach(session.messages, id: \.uuid) { message in
                        MessageRow(message: message)
                            .id(message.uuid)
                    }
                }
                .padding()
            }
            .onChange(of: state.scrollTarget) { _, target in
                if let target {
                    withAnimation { proxy.scrollTo(target, anchor: .top) }
                }
            }
            .onAppear {
                if let target = state.scrollTarget {
                    proxy.scrollTo(target, anchor: .top)
                }
            }
        }
    }

    private func header(_ summary: SessionSummary) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(summary.title).font(.title3).bold()
            HStack(spacing: 6) {
                Text(summary.projectPath)
                Text("·")
                Text("\(summary.messageCount) messages")
                if let branch = summary.gitBranch {
                    Text("·")
                    Text(branch)
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            Divider()
        }
    }
}

struct MessageRow: View {
    let message: Message

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: message.role == .user ? "person.circle" : "sparkles")
                .foregroundStyle(message.role == .user ? Color.accentColor : .secondary)
                .padding(.top, 2)
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(message.blocks.enumerated()), id: \.offset) { _, block in
                    BlockView(block: block)
                }
            }
            Spacer(minLength: 0)
        }
    }
}

struct BlockView: View {
    let block: ContentBlock
    @State private var expanded = false

    var body: some View {
        switch block {
        case .text(let text):
            Text(text)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        case .thinking(let text):
            collapsible(label: "Thinking", icon: "brain", tint: .purple) {
                Text(text)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
        case .toolUse(let name, let input):
            collapsible(label: "Tool: \(name)", icon: "wrench.adjustable", tint: .blue) {
                Text((try? VibeTrailJSON.encode(input)) ?? "")
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
            }
        case .toolResult(let summary, let truncated):
            collapsible(label: "Result" + (truncated ? " (truncated)" : ""),
                        icon: "arrow.turn.down.right", tint: .green) {
                Text(summary)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
            }
        }
    }

    private func collapsible(label: String, icon: String, tint: Color,
                             @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Button {
                expanded.toggle()
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: icon)
                    Text(label)
                    Image(systemName: expanded ? "chevron.down" : "chevron.right")
                        .font(.caption2)
                }
                .font(.caption)
                .foregroundStyle(tint)
            }
            .buttonStyle(.plain)
            if expanded {
                content()
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 6))
            }
        }
    }
}
