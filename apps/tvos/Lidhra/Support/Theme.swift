import SwiftUI

/// Brand colours (the dark palette of ui/index.html) and 10-foot type sizes.
enum Theme {
    static let accent = Color(hex: 0x18D88F)
    static let accentStrong = Color(hex: 0x08A873)
    static let background = Color(hex: 0x07110E)
    static let surface = Color(hex: 0x10211B)
    static let textPrimary = Color(hex: 0xF3FBF7)
    static let textSecondary = Color(hex: 0x93AAA0)
    static let track = Color(hex: 0x1B2F28)
    static let danger = Color(hex: 0xE5484D)
    static let warning = Color(hex: 0xD89518)

    /// Full-screen backdrop: the brand plate with a soft accent glow, like the app icon.
    static var backdrop: some View {
        ZStack {
            background
            RadialGradient(colors: [accent.opacity(0.16), .clear], center: .topTrailing, startRadius: 40, endRadius: 1300)
        }
        .ignoresSafeArea()
    }

    enum Font {
        static let hero = SwiftUI.Font.system(size: 76, weight: .bold)
        static let code = SwiftUI.Font.system(size: 128, weight: .bold, design: .monospaced)
        static let url = SwiftUI.Font.system(size: 44, weight: .semibold)
        static let body = SwiftUI.Font.system(size: 34)
        static let caption = SwiftUI.Font.system(size: 28)
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: 1
        )
    }
}
