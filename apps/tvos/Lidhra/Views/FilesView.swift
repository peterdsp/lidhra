import SwiftUI

/// The files of one transfer (`links {id}`); choosing one plays it.
struct FilesView: View {
    @Environment(AppModel.self) private var model
    let transfer: Transfer

    @State private var files: [FileLink]?
    @State private var error: String?
    @State private var playing: FileLink?

    var body: some View {
        Group {
            if !transfer.isReady {
                ContentUnavailableView {
                    Label(Strings.notReadyTitle, systemImage: "hourglass")
                } description: {
                    Text(Strings.notReadyDetail(status: Strings.status(transfer.state), progress: Format.percent(transfer.progress)))
                }
            } else if let files {
                if files.isEmpty {
                    ContentUnavailableView(Strings.noFiles, systemImage: "doc")
                } else {
                    List(files) { file in
                        Button { playing = file } label: { FileRow(file: file) }
                    }
                }
            } else if let error {
                ContentUnavailableView {
                    Label(Strings.filesFailed, systemImage: "exclamationmark.triangle")
                } description: {
                    Text(error)
                } actions: {
                    Button(Strings.tryAgain) { Task { await load() } }
                }
            } else {
                VStack(spacing: 24) {
                    ProgressView()
                    Text(Strings.resolvingLinks).font(Theme.Font.caption).foregroundStyle(Theme.textSecondary)
                }
            }
        }
        .navigationTitle(transfer.name)
        .task { if transfer.isReady && files == nil { await load() } }
        .fullScreenCover(item: $playing) { PlayerScreen(file: $0) }
    }

    private func load() async {
        error = nil
        do {
            files = try await model.core.call("links", ["id": transfer.id], as: [FileLink].self)
        } catch is CancellationError {
        } catch {
            self.error = error.localizedDescription
        }
    }
}

private struct FileRow: View {
    let file: FileLink

    var body: some View {
        HStack(spacing: 32) {
            Image(systemName: file.isVideo ? "film" : file.isAudio ? "music.note" : "doc")
                .font(.system(size: 38, weight: .semibold))
                .foregroundStyle(Theme.accent)
                .frame(width: 60)
            VStack(alignment: .leading, spacing: 8) {
                Text(file.filename)
                    .font(.system(size: 34, weight: .medium))
                    .lineLimit(1)
                HStack(spacing: 20) {
                    Text(Format.bytes(file.size))
                    if !file.isLikelyPlayable {
                        Text(Strings.mayNotPlay).foregroundStyle(Theme.warning)
                    }
                }
                .font(Theme.Font.caption)
                .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 8)
    }
}
