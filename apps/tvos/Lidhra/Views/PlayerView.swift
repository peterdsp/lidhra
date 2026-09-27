import AVKit
import SwiftUI

/// Full-screen playback of one file. Menu on the remote closes it.
struct PlayerScreen: View {
    @Environment(\.dismiss) private var dismiss
    let file: FileLink
    @State private var failure: String?

    var body: some View {
        Group {
            if let url = URL(string: file.url) {
                PlayerView(url: url, title: file.displayTitle) { failure = $0 }
            } else {
                Color.black.onAppear { failure = Strings.badLink }
            }
        }
        .ignoresSafeArea()
        .alert(Strings.playbackFailedTitle, isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button(Strings.ok) { dismiss() }
        } message: {
            Text(failure ?? "")
        }
    }
}

/// AVPlayerViewController for a direct HTTPS URL, with the title in the info panel.
struct PlayerView: UIViewControllerRepresentable {
    let url: URL
    let title: String
    let onFailure: (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onFailure: onFailure) }

    func makeUIViewController(context: Context) -> AVPlayerViewController {
        let item = AVPlayerItem(url: url)
        let titleItem = AVMutableMetadataItem()
        titleItem.identifier = .commonIdentifierTitle
        titleItem.value = title as NSString
        titleItem.extendedLanguageTag = "und"
        item.externalMetadata = [titleItem]
        context.coordinator.watch(item)

        let player = AVPlayer(playerItem: item)
        let controller = AVPlayerViewController()
        controller.player = player
        player.play()
        return controller
    }

    func updateUIViewController(_ controller: AVPlayerViewController, context: Context) {}

    static func dismantleUIViewController(_ controller: AVPlayerViewController, coordinator: Coordinator) {
        controller.player?.pause()
        controller.player?.replaceCurrentItem(with: nil)
        coordinator.stop()
    }

    final class Coordinator {
        private let onFailure: (String) -> Void
        private var observation: NSKeyValueObservation?

        init(onFailure: @escaping (String) -> Void) {
            self.onFailure = onFailure
        }

        func watch(_ item: AVPlayerItem) {
            observation = item.observe(\.status) { [weak self] item, _ in
                guard item.status == .failed else { return }
                let message = item.error?.localizedDescription ?? Strings.playbackFailedDetail
                DispatchQueue.main.async { self?.onFailure(message) }
            }
        }

        func stop() {
            observation = nil
        }
    }
}
