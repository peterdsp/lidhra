import SwiftUI

public enum LidhraColor {
    public static let lidhra_teal = Color(hex: "15C3B6")
    public static let lidhra_green = Color(hex: "2FD191")
    public static let lidhra_lime = Color(hex: "54E06A")
    public static let ink_950 = Color(hex: "07110F")
    public static let ink_900 = Color(hex: "0A1512")
    public static let ink_800 = Color(hex: "0C1A16")
    public static let ink_700 = Color(hex: "183027")
    public static let mist_50 = Color(hex: "F7FBF9")
    public static let mist_100 = Color(hex: "F2F8F5")
    public static let mist_200 = Color(hex: "E6EFE9")
    public static let mist_300 = Color(hex: "D8E4DD")
    public static let text_secondary = Color(hex: "62736B")
    public static let download = Color(hex: "22BD7A")
    public static let seeding = Color(hex: "11A594")
    public static let paused = Color(hex: "8A94A8")
    public static let danger = Color(hex: "E75555")
    public static let warning = Color(hex: "F2B84B")

    public static let brandGradient = LinearGradient(colors: [lidhra_teal, lidhra_green, lidhra_lime], startPoint: .topLeading, endPoint: .bottomTrailing)
}

private extension Color {
    init(hex: String) {
        let value = UInt64(hex, radix: 16) ?? 0
        self.init(.sRGB, red: Double((value >> 16) & 0xff) / 255, green: Double((value >> 8) & 0xff) / 255, blue: Double(value & 0xff) / 255, opacity: 1)
    }
}
