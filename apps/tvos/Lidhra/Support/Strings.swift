import Foundation

/// Every user-facing string, in one place. English only for now; each goes through
/// `String(localized:)`, so adding a String Catalog (Localizable.xcstrings) later picks them up.
enum Strings {
    // MARK: App

    static let appName = String(localized: "Lidhra")
    static let libraryTab = String(localized: "Library")
    static let settingsTab = String(localized: "Settings")
    static let ok = String(localized: "OK")
    static let cancel = String(localized: "Cancel")
    static let tryAgain = String(localized: "Try Again")
    static let refresh = String(localized: "Refresh")
    static let restoreFailedTitle = String(localized: "Could not sign back in")

    // MARK: Connect

    static let connectTagline = String(localized: "Stream your cloud on the big screen.")
    static let connectHint = String(localized: "Connect your debrid account to play what is already in your cloud. Nothing is downloaded to this Apple TV.")
    static let chooseProvider = String(localized: "Choose your provider")
    static let providersFailed = String(localized: "Could not load providers")
    static let signInWithCode = String(localized: "Sign in with a code")
    static let signInWithKey = String(localized: "Sign in with an API key")
    static let connect = String(localized: "Connect")
    static func connectProvider(_ provider: String) -> String { String(localized: "Connect \(provider)") }

    // MARK: Device login

    static let gettingCode = String(localized: "Getting a sign-in code...")
    static let deviceStep1 = String(localized: "On your phone or computer, open")
    static let deviceStep2 = String(localized: "and enter this code:")
    static let waitingForApproval = String(localized: "Waiting for approval")
    static let codeExpiresIn = String(localized: "Code expires in")
    static let newCode = String(localized: "New Code")
    static let scanToOpen = String(localized: "Scan to open the page")
    static let useApiKeyInstead = String(localized: "Use an API Key")
    static let codeExpiredTitle = String(localized: "The code expired")
    static let codeExpiredDetail = String(localized: "Get a new code and approve it within the time shown.")
    static let loginFailedTitle = String(localized: "Sign-in failed")
    static let qrAccessibility = String(localized: "QR code for the sign-in page")
    static func codeAccessibility(_ code: String) -> String { String(localized: "Sign-in code \(code)") }

    // MARK: Token entry

    static let tokenPlaceholder = String(localized: "API key")
    static func tokenHint(_ provider: String) -> String {
        String(localized: "Copy the API key from your \(provider) account page, then enter it here.")
    }

    // MARK: Library

    static let libraryTitle = String(localized: "Library")
    static let libraryFailed = String(localized: "Could not load your library")
    static let libraryEmptyTitle = String(localized: "Nothing in your cloud yet")
    static let libraryEmptyDetail = String(localized: "Add a magnet or link with Lidhra on your phone or computer. It shows up here once your provider has it.")
    static func libraryHeader(provider: String, count: Int) -> String { String(localized: "\(provider): \(count) transfers") }
    static func fileCount(_ count: Int) -> String { String(localized: "\(count) files") }

    static func status(_ state: Transfer.State) -> String {
        switch state {
        case .ready: return String(localized: "Ready")
        case .downloading: return String(localized: "Downloading")
        case .queued: return String(localized: "Queued")
        case .error: return String(localized: "Failed")
        case .unknown: return String(localized: "Unknown")
        }
    }

    // MARK: Files

    static let notReadyTitle = String(localized: "Not ready yet")
    static func notReadyDetail(status: String, progress: String) -> String {
        String(localized: "\(status), \(progress). You can play it once your provider has finished.")
    }
    static let noFiles = String(localized: "No files in this transfer")
    static let filesFailed = String(localized: "Could not load the files")
    static let resolvingLinks = String(localized: "Getting stream links...")
    static let mayNotPlay = String(localized: "May not play on Apple TV")

    // MARK: Player

    static let playbackFailedTitle = String(localized: "Cannot play this file")
    static let playbackFailedDetail = String(localized: "Apple TV could not play this file. MKV and AVI files are not supported yet.")
    static let badLink = String(localized: "The provider returned an invalid link.")

    // MARK: Settings

    static let accountSection = String(localized: "Account")
    static let aboutSection = String(localized: "About")
    static let providerLabel = String(localized: "Provider")
    static let usernameLabel = String(localized: "Username")
    static let planLabel = String(localized: "Plan")
    static let planPremium = String(localized: "Premium")
    static let planFree = String(localized: "Free")
    static let appVersionLabel = String(localized: "App version")
    static let coreVersionLabel = String(localized: "Core version")
    static let signOut = String(localized: "Sign Out")
    static let signOutConfirm = String(localized: "Sign out of your provider on this Apple TV?")

    // MARK: Errors

    static let errorTimedOut = String(localized: "The request took too long. Check your connection and try again.")
    static func errorBadData(_ command: String) -> String { String(localized: "Unexpected response from the core (\(command)).") }
    static func errorKeychain(_ status: Int32) -> String { String(localized: "Could not save the sign-in (Keychain error \(status)).") }
}
