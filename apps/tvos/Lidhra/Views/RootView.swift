import SwiftUI

/// Switches between launch, sign-in and the signed-in app.
struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ZStack {
            Theme.backdrop
            switch model.phase {
            case .launching:
                LaunchView()
            case .signedOut:
                ConnectFlow()
            case .restoreFailed(let message):
                RestoreFailedView(message: message)
            case .signedIn:
                MainTabs()
            }
        }
        .task { await model.launch() }
    }
}

private struct LaunchView: View {
    var body: some View {
        VStack(spacing: 40) {
            Image("BrandMark")
            ProgressView()
        }
    }
}

private struct RestoreFailedView: View {
    @Environment(AppModel.self) private var model
    let message: String

    var body: some View {
        ContentUnavailableView {
            Label(Strings.restoreFailedTitle, systemImage: "wifi.exclamationmark")
        } description: {
            Text(message)
        } actions: {
            HStack(spacing: 40) {
                Button(Strings.tryAgain) { Task { await model.launch() } }
                Button(Strings.signOut, role: .destructive) { Task { await model.signOut() } }
            }
        }
    }
}

/// Library and Settings, as top tabs.
private struct MainTabs: View {
    var body: some View {
        TabView {
            NavigationStack {
                LibraryView()
                    .navigationDestination(for: Transfer.self) { FilesView(transfer: $0) }
            }
            .tabItem { Label(Strings.libraryTab, systemImage: "play.rectangle.on.rectangle") }

            SettingsView()
                .tabItem { Label(Strings.settingsTab, systemImage: "gearshape") }
        }
    }
}
