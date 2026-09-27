import SwiftUI

/// API-key sign-in for providers without a device login (or by choice), via `connect`.
struct TokenEntryView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let provider: Provider

    @State private var token = ""
    @State private var busy = false
    @State private var error: String?

    private var trimmed: String { token.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 36) {
            Text(Strings.connectProvider(provider.label))
                .font(.title2)
                .foregroundStyle(Theme.textPrimary)
            Text(Strings.tokenHint(provider.label))
                .font(Theme.Font.body)
                .foregroundStyle(Theme.textSecondary)
                .frame(maxWidth: 1200, alignment: .leading)

            SecureField(Strings.tokenPlaceholder, text: $token)
                .frame(maxWidth: 1000)
                .disabled(busy)
                .onSubmit { Task { await connect() } }

            HStack(spacing: 40) {
                Button(Strings.connect) { Task { await connect() } }
                    .disabled(trimmed.isEmpty || busy)
                Button(Strings.cancel) { dismiss() }
                if busy { ProgressView() }
            }
            .focusSection()

            if let error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.danger)
            }
        }
        .padding(.horizontal, 20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func connect() async {
        guard !trimmed.isEmpty, !busy else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            let result = try await model.core.call("connect", ["provider": provider.id, "token": trimmed])
            try model.adopt(result)
        } catch {
            self.error = error.localizedDescription
        }
    }
}
