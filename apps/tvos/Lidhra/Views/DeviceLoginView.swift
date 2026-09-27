import SwiftUI

/// Code-on-screen sign-in: `login_start` shows a code and a URL (plus a QR of it), the user
/// approves on a phone, and `login_poll` runs every `interval` seconds until done or expired.
struct DeviceLoginView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let provider: Provider

    private enum Stage: Equatable {
        case starting
        case waiting(DeviceLogin, expires: Date?)
        case expired
        case failed(String)
    }

    @State private var stage: Stage = .starting
    /// Bumped by "Try again" to restart the login task.
    @State private var attempt = 0

    /// Consecutive poll failures tolerated before giving up (a flaky network should not end the login).
    private let maxPollFailures = 3

    var body: some View {
        VStack(alignment: .leading, spacing: 40) {
            Text(Strings.connectProvider(provider.label))
                .font(.title2)
                .foregroundStyle(Theme.textPrimary)

            switch stage {
            case .starting:
                HStack(spacing: 24) {
                    ProgressView()
                    Text(Strings.gettingCode).font(Theme.Font.body).foregroundStyle(Theme.textSecondary)
                }
                .frame(maxWidth: .infinity, minHeight: 500)
                actions(primary: nil)
            case .waiting(let login, let expires):
                waiting(login, expires: expires)
            case .expired:
                message(Strings.codeExpiredTitle, detail: Strings.codeExpiredDetail, systemImage: "clock.badge.exclamationmark")
            case .failed(let error):
                message(Strings.loginFailedTitle, detail: error, systemImage: "exclamationmark.triangle")
            }
        }
        .padding(.horizontal, 20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .task(id: attempt) { await run() }
    }

    private func waiting(_ login: DeviceLogin, expires: Date?) -> some View {
        HStack(alignment: .center, spacing: 100) {
            VStack(alignment: .leading, spacing: 28) {
                Text(Strings.deviceStep1).font(Theme.Font.body).foregroundStyle(Theme.textSecondary)
                Text(login.verifyUrl)
                    .font(Theme.Font.url)
                    .foregroundStyle(Theme.accent)
                    .lineLimit(2)
                Text(Strings.deviceStep2).font(Theme.Font.body).foregroundStyle(Theme.textSecondary)
                Text(login.userCode)
                    .font(Theme.Font.code)
                    .kerning(14)
                    .foregroundStyle(Theme.textPrimary)
                    .minimumScaleFactor(0.5)
                    .lineLimit(1)
                    .accessibilityLabel(Strings.codeAccessibility(login.userCode))
                HStack(spacing: 20) {
                    ProgressView()
                    Text(Strings.waitingForApproval).font(Theme.Font.caption).foregroundStyle(Theme.textSecondary)
                    if let expires, expires > .now {
                        Text("\(Strings.codeExpiresIn) \(Text(timerInterval: Date.now...expires, countsDown: true))")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textSecondary)
                            .monospacedDigit()
                    }
                }
                actions(primary: (Strings.newCode, { attempt += 1 }))
                    .padding(.top, 20)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            VStack(spacing: 20) {
                QRCodeView(text: login.verifyUrl)
                    .frame(width: 440, height: 440)
                Text(Strings.scanToOpen).font(Theme.Font.caption).foregroundStyle(Theme.textSecondary)
            }
        }
    }

    private func message(_ title: String, detail: String, systemImage: String) -> some View {
        VStack(alignment: .leading, spacing: 28) {
            Label(title, systemImage: systemImage)
                .font(.title3)
                .foregroundStyle(Theme.textPrimary)
            Text(detail).font(Theme.Font.body).foregroundStyle(Theme.textSecondary)
            actions(primary: (Strings.tryAgain, { attempt += 1 }))
                .padding(.top, 20)
        }
        .frame(maxWidth: .infinity, minHeight: 500, alignment: .leading)
    }

    /// Primary action (if any), Cancel, and a way to paste a key instead.
    private func actions(primary: (String, () -> Void)?) -> some View {
        HStack(spacing: 40) {
            if let primary {
                Button(primary.0, action: primary.1)
            }
            Button(Strings.cancel) { dismiss() }
            NavigationLink(Strings.useApiKeyInstead, value: ConnectRoute.token(provider))
        }
        .focusSection()
    }

    private func run() async {
        stage = .starting
        do {
            let login = try await model.core.call("login_start", ["provider": provider.id], as: DeviceLogin.self)
            let expires = login.expiresIn > 0 ? Date.now.addingTimeInterval(TimeInterval(login.expiresIn)) : nil
            stage = .waiting(login, expires: expires)

            var failures = 0
            while true {
                try await Task.sleep(for: .seconds(max(1, login.interval)))
                if let expires, Date.now >= expires {
                    stage = .expired
                    return
                }
                do {
                    let result = try await model.core.call("login_poll", ["provider": provider.id, "handle": login.handle])
                    failures = 0
                    if let object = result as? [String: Any], object["pending"] as? Bool == false {
                        // The bridge is now connected; store the session and leave the sign-in flow.
                        try model.adopt(result)
                        return
                    }
                } catch {
                    try Task.checkCancellation()
                    failures += 1
                    if failures >= maxPollFailures { throw error }
                }
            }
        } catch is CancellationError {
            // Left the screen or asked for a new code.
        } catch {
            stage = .failed(error.localizedDescription)
        }
    }
}
