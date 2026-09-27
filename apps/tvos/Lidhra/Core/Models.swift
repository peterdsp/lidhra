import Foundation

/// `providers` entry.
struct Provider: Decodable, Identifiable, Hashable {
    let id: String
    let label: String
    /// True when the provider supports the code-on-screen device login (Real-Debrid, AllDebrid).
    let deviceLogin: Bool
}

/// The signed-in account (`connect`, `login_poll` and `restore` results, minus the session).
struct Account: Equatable {
    let username: String
    let premium: Bool
    let provider: String
}

/// `login_start` result.
struct DeviceLogin: Decodable, Equatable {
    let userCode: String
    let verifyUrl: String
    /// Seconds between `login_poll` calls.
    let interval: UInt64
    /// Seconds until the code expires (0 when the provider does not say).
    let expiresIn: UInt64
    /// Opaque poll state; never shown.
    let handle: String
}

/// `transfers` entry (the desktop `Tx` shape; only the fields the TV needs are decoded).
struct Transfer: Decodable, Identifiable, Hashable {
    let id: String
    let name: String
    let status: String
    /// 0...1
    let progress: Double
    /// Number of files the provider holds for this transfer.
    let links: Int

    enum State: Int {
        case ready, downloading, queued, error, unknown
    }

    var state: State {
        switch status.lowercased() {
        case "ready": return .ready
        case "downloading": return .downloading
        case "queued": return .queued
        case "error": return .error
        default: return .unknown
        }
    }

    var isReady: Bool { state == .ready }
}

/// `links` entry: a direct HTTPS URL for one file of a ready transfer.
struct FileLink: Decodable, Identifiable, Hashable {
    let url: String
    let filename: String
    let size: UInt64
    let mime: String?

    var id: String { url }

    private var fileExtension: String { (filename as NSString).pathExtension.lowercased() }

    var isVideo: Bool {
        mime?.hasPrefix("video/") == true || Self.videoExtensions.contains(fileExtension)
    }

    var isAudio: Bool {
        mime?.hasPrefix("audio/") == true || Self.audioExtensions.contains(fileExtension)
    }

    /// Containers AVFoundation plays natively on tvOS. Others (MKV, AVI, ...) are still offered,
    /// but may fail to play; the player reports the failure.
    var isLikelyPlayable: Bool { Self.playableExtensions.contains(fileExtension) }

    /// The filename without its extension, for the player's title.
    var displayTitle: String {
        let base = (filename as NSString).deletingPathExtension
        return base.isEmpty ? filename : base
    }

    private static let videoExtensions: Set = ["mp4", "m4v", "mov", "mkv", "avi", "webm", "ts", "m2ts", "mpg", "mpeg", "wmv", "flv", "m3u8"]
    private static let audioExtensions: Set = ["mp3", "m4a", "aac", "flac", "wav", "ogg", "opus", "alac", "aiff"]
    private static let playableExtensions: Set = ["mp4", "m4v", "mov", "ts", "m3u8", "mp3", "m4a", "aac", "wav", "aiff", "flac", "alac"]
}
