import SwiftUI

/// Account details and sign out (`disconnect` + clear the Keychain session).
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var confirmSignOut = false

    private var appVersion: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(version) (\(build))"
    }

    var body: some View {
        HStack(alignment: .top, spacing: 100) {
            VStack(alignment: .leading, spacing: 28) {
                Image("BrandMark")
                Text(Strings.appName).font(Theme.Font.hero).foregroundStyle(Theme.textPrimary)
                Text(Strings.connectTagline).font(Theme.Font.caption).foregroundStyle(Theme.textSecondary)
            }
            .frame(width: 520, alignment: .leading)

            List {
                Section(Strings.accountSection) {
                    if let account = model.account {
                        LabeledContent(Strings.providerLabel, value: account.provider)
                        LabeledContent(Strings.usernameLabel, value: account.username)
                        LabeledContent(Strings.planLabel, value: account.premium ? Strings.planPremium : Strings.planFree)
                    }
                    Button(Strings.signOut, role: .destructive) { confirmSignOut = true }
                }
                Section(Strings.aboutSection) {
                    LabeledContent(Strings.appVersionLabel, value: appVersion)
                    LabeledContent(Strings.coreVersionLabel, value: LidhraCore.version)
                }
            }
        }
        .padding(.horizontal, 20)
        .confirmationDialog(Strings.signOutConfirm, isPresented: $confirmSignOut) {
            Button(Strings.signOut, role: .destructive) { Task { await model.signOut() } }
            Button(Strings.cancel, role: .cancel) {}
        }
    }
}
