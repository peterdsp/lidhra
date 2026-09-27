import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

/// A crisp QR code for `text`, drawn with CoreImage's CIQRCodeGenerator.
struct QRCodeView: View {
    let text: String

    var body: some View {
        if let image = QRCode.image(for: text) {
            Image(decorative: image, scale: 1)
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .padding(28)
                .background(.white, in: RoundedRectangle(cornerRadius: 24))
                .accessibilityLabel(Strings.qrAccessibility)
        }
    }
}

enum QRCode {
    private static let context = CIContext()

    /// One pixel per module; scale it up with `.interpolation(.none)`.
    static func image(for text: String) -> CGImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return nil }
        return context.createCGImage(output, from: output.extent)
    }
}
