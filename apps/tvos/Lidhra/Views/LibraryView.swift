import SwiftUI

/// The provider's cloud transfers (`transfers`), Ready first. Refreshes on appear and every
/// `refreshInterval` while visible.
struct LibraryView: View {
    @Environment(AppModel.self) private var model

    @State private var transfers: [Transfer]?
    @State private var error: String?
    @State private var refreshing = false

    private let refreshInterval: Duration = .seconds(10)

    var body: some View {
        Group {
            if let transfers {
                if transfers.isEmpty {
                    empty
                } else {
                    list(transfers)
                }
            } else if let error {
                ContentUnavailableView {
                    Label(Strings.libraryFailed, systemImage: "exclamationmark.triangle")
                } description: {
                    Text(error)
                } actions: {
                    Button(Strings.tryAgain) { Task { await refresh() } }
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle(Strings.libraryTitle)
        .task {
            while !Task.isCancelled {
                await refresh()
                try? await Task.sleep(for: refreshInterval)
            }
        }
    }

    private var empty: some View {
        ContentUnavailableView {
            Label(Strings.libraryEmptyTitle, systemImage: "tray")
        } description: {
            Text(Strings.libraryEmptyDetail)
        } actions: {
            Button(Strings.refresh) { Task { await refresh() } }
        }
    }

    private func list(_ transfers: [Transfer]) -> some View {
        List {
            Section {
                ForEach(transfers) { transfer in
                    NavigationLink(value: transfer) { TransferRow(transfer: transfer) }
                }
            } header: {
                HStack(spacing: 24) {
                    if let account = model.account {
                        Text(Strings.libraryHeader(provider: account.provider, count: transfers.count))
                    }
                    if refreshing { ProgressView().scaleEffect(0.6) }
                    if let error {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(Theme.warning)
                            .lineLimit(1)
                    }
                }
            }
        }
    }

    private func refresh() async {
        guard !refreshing else { return }
        refreshing = true
        defer { refreshing = false }
        do {
            let list = try await model.core.call("transfers", as: [Transfer].self)
            // Ready first, then in progress, queued, failed; provider order within each group.
            transfers = list.enumerated()
                .sorted { ($0.element.state.rawValue, $0.offset) < ($1.element.state.rawValue, $1.offset) }
                .map(\.element)
            error = nil
        } catch is CancellationError {
        } catch {
            self.error = error.localizedDescription
        }
    }
}

private struct TransferRow: View {
    let transfer: Transfer

    var body: some View {
        HStack(spacing: 32) {
            Image(systemName: icon)
                .font(.system(size: 40, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 60)
            VStack(alignment: .leading, spacing: 10) {
                Text(transfer.name)
                    .font(.system(size: 34, weight: .medium))
                    .lineLimit(1)
                HStack(spacing: 20) {
                    Text(Strings.status(transfer.state))
                        .foregroundStyle(tint)
                    if transfer.isReady {
                        Text(Strings.fileCount(transfer.links))
                            .foregroundStyle(.secondary)
                    } else if transfer.state != .error {
                        ProgressView(value: max(0, min(1, transfer.progress)))
                            .frame(width: 320)
                        Text(Format.percent(transfer.progress))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                }
                .font(Theme.Font.caption)
            }
        }
        .padding(.vertical, 8)
    }

    private var icon: String {
        switch transfer.state {
        case .ready: return "play.circle.fill"
        case .downloading: return "arrow.down.circle"
        case .queued: return "clock"
        case .error: return "exclamationmark.triangle"
        case .unknown: return "questionmark.circle"
        }
    }

    private var tint: Color {
        switch transfer.state {
        case .ready: return Theme.accent
        case .error: return Theme.danger
        case .downloading, .queued, .unknown: return Theme.textSecondary
        }
    }
}
