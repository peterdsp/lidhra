import Foundation
import Observation

/// App-wide sign-in state. Owns the stored session and keeps it in step with the bridge.
@MainActor
@Observable
final class AppModel {
    enum Phase: Equatable {
        /// Restoring a stored session (or deciding there is none).
        case launching
        case signedOut
        /// A stored session exists but could not be restored (offline, revoked token, ...).
        case restoreFailed(String)
        case signedIn(Account)
    }

    private(set) var phase: Phase = .launching

    let core = LidhraCore.shared

    var account: Account? {
        if case .signedIn(let account) = phase { return account }
        return nil
    }

    /// On launch: `restore` the Keychain session, then re-save what comes back (a Real-Debrid
    /// session may carry a refreshed access token).
    func launch() async {
        guard let stored = SessionStore.load() else {
            phase = .signedOut
            return
        }
        phase = .launching
        do {
            let session = try JSONSerialization.jsonObject(with: stored)
            let result = try await core.call("restore", ["session": session])
            if result is NSNull {
                SessionStore.clear()
                phase = .signedOut
            } else {
                try adopt(result)
            }
        } catch {
            phase = .restoreFailed(error.localizedDescription)
        }
    }

    /// Take a `connect` / `login_poll` / `restore` result: store its session, switch to signed in.
    func adopt(_ result: Any) throws {
        guard let object = result as? [String: Any],
              let session = object["session"], !(session is NSNull),
              JSONSerialization.isValidJSONObject(session) else {
            throw LidhraError.badData(command: "session")
        }
        let data = try JSONSerialization.data(withJSONObject: session, options: [.sortedKeys])
        try SessionStore.save(data)
        phase = .signedIn(Account(
            username: object["username"] as? String ?? "",
            premium: object["premium"] as? Bool ?? false,
            provider: object["provider"] as? String ?? ""
        ))
    }

    /// Forget the provider in the bridge and the session in the Keychain.
    func signOut() async {
        _ = try? await core.call("disconnect")
        SessionStore.clear()
        phase = .signedOut
    }
}
