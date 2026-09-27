import AVFoundation
import SwiftUI

@main
struct LidhraApp: App {
    @State private var model = AppModel()

    init() {
        // Play audio like a video app: ignore the silent switch, keep playing with the screen dimmed.
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .moviePlayback)
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
        }
    }
}
