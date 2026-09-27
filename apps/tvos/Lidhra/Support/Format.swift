import Foundation

enum Format {
    /// "1.4 GB"
    static func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: count), countStyle: .file)
    }

    /// "42%"
    static func percent(_ fraction: Double) -> String {
        max(0, min(1, fraction)).formatted(.percent.precision(.fractionLength(0)))
    }
}
