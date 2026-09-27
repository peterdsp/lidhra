import SwiftUI

/// Where the sign-in screens can go.
enum ConnectRoute: Hashable {
    /// Code on the TV, approval on a phone (`login_start` / `login_poll`).
    case deviceLogin(Provider)
    /// Paste an API key with the tvOS keyboard (`connect`).
    case token(Provider)
}

/// Sign-in navigation: provider picker, then device login or token entry.
struct ConnectFlow: View {
    @State private var path: [ConnectRoute] = []

    var body: some View {
        NavigationStack(path: $path) {
            ConnectView(path: $path)
                .navigationDestination(for: ConnectRoute.self) { route in
                    switch route {
                    case .deviceLogin(let provider): DeviceLoginView(provider: provider)
                    case .token(let provider): TokenEntryView(provider: provider)
                    }
                }
        }
    }
}

/// Provider picker, fed by the `providers` command.
struct ConnectView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [ConnectRoute]
    @State private var providers: [Provider] = []
    @State private var loadError: String?
    @State private var openedLaunchProvider = false

    private let columns = [GridItem(.adaptive(minimum: 420, maximum: 520), spacing: 48)]

    var body: some View {
        HStack(alignment: .top, spacing: 100) {
            VStack(alignment: .leading, spacing: 28) {
                Image("BrandMark")
                Text(Strings.appName)
                    .font(Theme.Font.hero)
                    .foregroundStyle(Theme.textPrimary)
                Text(Strings.connectTagline)
                    .font(.title3)
                    .foregroundStyle(Theme.textPrimary)
                Text(Strings.connectHint)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textSecondary)
            }
            .frame(width: 560, alignment: .leading)
            .padding(.top, 40)

            VStack(alignment: .leading, spacing: 32) {
                Text(Strings.chooseProvider)
                    .font(.headline)
                    .foregroundStyle(Theme.textSecondary)
                if let loadError {
                    ContentUnavailableView {
                        Label(Strings.providersFailed, systemImage: "exclamationmark.triangle")
                    } description: {
                        Text(loadError)
                    } actions: {
                        Button(Strings.tryAgain) { Task { await load() } }
                    }
                } else if providers.isEmpty {
                    ProgressView()
                        .frame(maxWidth: .infinity, minHeight: 300)
                } else {
                    ScrollView {
                        LazyVGrid(columns: columns, alignment: .leading, spacing: 48) {
                            ForEach(providers) { provider in
                                NavigationLink(value: Self.route(for: provider)) {
                                    ProviderTile(provider: provider)
                                }
                                .buttonStyle(.card)
                            }
                        }
                        .padding(40)
                    }
                    .focusSection()
                }
            }
        }
        .padding(.horizontal, 20)
        .task { if providers.isEmpty { await load() } }
    }

    static func route(for provider: Provider) -> ConnectRoute {
        provider.deviceLogin ? .deviceLogin(provider) : .token(provider)
    }

    private func load() async {
        loadError = nil
        do {
            providers = try await model.core.call("providers", as: [Provider].self)
            openLaunchArgumentProvider()
        } catch {
            loadError = error.localizedDescription
        }
    }

    /// Debug builds only: `-LidhraOpenProvider Real-Debrid` on the launch command line opens that
    /// provider's sign-in screen directly (for simulator screenshots without a remote).
    private func openLaunchArgumentProvider() {
        #if DEBUG
        guard !openedLaunchProvider,
              let wanted = UserDefaults.standard.string(forKey: "LidhraOpenProvider"),
              let provider = providers.first(where: { $0.id.caseInsensitiveCompare(wanted) == .orderedSame }) else { return }
        openedLaunchProvider = true
        path = [Self.route(for: provider)]
        #endif
    }
}

private struct ProviderTile: View {
    let provider: Provider

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Image(systemName: provider.deviceLogin ? "qrcode" : "key.horizontal")
                .font(.system(size: 40, weight: .semibold))
                .foregroundStyle(Theme.accent)
            Text(provider.label)
                .font(.system(size: 36, weight: .semibold))
                .lineLimit(1)
            Text(provider.deviceLogin ? Strings.signInWithCode : Strings.signInWithKey)
                .font(Theme.Font.caption)
                .foregroundStyle(.secondary)
        }
        .padding(32)
        .frame(maxWidth: .infinity, minHeight: 220, alignment: .leading)
    }
}
