@testable import CarrelAppleProviders
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers

/// A picture of `lines` of text, black on white.
func picture(of lines: [String]) throws -> CGImage {
    let width = 1200, height = 200 * lines.count
    let context = try #require(CGContext(
        data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
    context.setFillColor(CGColor(gray: 1, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let font = CTFontCreateWithName("Helvetica" as CFString, 64, nil)
    for (index, text) in lines.enumerated() {
        let line = CTLineCreateWithAttributedString(NSAttributedString(
            string: text, attributes: [.font: font, .foregroundColor: CGColor(gray: 0, alpha: 1)]))
        context.textPosition = CGPoint(x: 60, y: height - 130 - index * 200)
        CTLineDraw(line, context)
    }
    return try #require(context.makeImage())
}

func temporary(_ ext: String) -> URL {
    FileManager.default.temporaryDirectory.appending(path: "carrel-ocr-\(UUID().uuidString).\(ext)")
}

func writePNG(_ image: CGImage) throws -> URL {
    let url = temporary("png")
    let destination = try #require(
        CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
    CGImageDestinationAddImage(destination, image, nil)
    #expect(CGImageDestinationFinalize(destination))
    return url
}

/// A "scanned" PDF: each page is only a picture, with no text layer.
func scannedPDF(_ pages: [CGImage]) throws -> URL {
    let url = temporary("pdf")
    var box = CGRect(x: 0, y: 0, width: 612, height: 792)
    let context = try #require(CGContext(url as CFURL, mediaBox: &box, nil))
    for image in pages {
        context.beginPDFPage(nil)
        context.draw(image, in: CGRect(x: 36, y: 500, width: 540, height: 540 * CGFloat(image.height) / CGFloat(image.width)))
        context.endPDFPage()
    }
    context.closePDF()
    return url
}

func normalized(_ text: String) -> String {
    text.lowercased().filter { $0.isLetter || $0.isNumber || $0 == "\n" }
}

@Test func visionReadsTextInImagesInReadingOrder() async throws {
    let url = try writePNG(picture(of: ["Coupled oscillators", "Field notes 2025"]))
    defer { try? FileManager.default.removeItem(at: url) }
    let result = try await VisionOCR().extract(url, mediaType: "image/png")
    #expect(result.method == "ocr")
    let lines = normalized(result.text).split(separator: "\n")
    #expect(lines == ["coupledoscillators", "fieldnotes2025"], "\(result.text)")
}

@Test func visionReadsScannedPDFsThatPDFKitCant() async throws {
    let url = try scannedPDF([picture(of: ["Desk lamp parts"]), picture(of: ["Second page"])])
    defer { try? FileManager.default.removeItem(at: url) }
    await #expect(throws: ProviderError.self, "no text layer") {
        try await PDFText().extract(url, mediaType: "application/pdf")
    }
    let result = try await VisionOCR().extract(url, mediaType: "application/pdf")
    #expect(result.pages == 2)
    let pages = result.text.split(separator: "\u{0C}").map { normalized(String($0)) }
    #expect(pages.count == 2)
    #expect(pages.first?.contains("desklampparts") == true, "\(result.text)")
    #expect(pages.last?.contains("secondpage") == true, "\(result.text)")
}

@Test func visionReportsUnreadableFilesAsFailures() async {
    await #expect(throws: ProviderError.failed("can't read missing.png")) {
        try await VisionOCR().extract(URL(filePath: "/nowhere/missing.png"), mediaType: "image/png")
    }
}

@Test func theOfferCoversImagesAndPDFs() {
    let offer = VisionOCR().offer
    #expect(offer.id == "vision")
    #expect(offer.mediaTypes?.contains("image/heic") == true)
    #expect(offer.mediaTypes?.contains("application/pdf") == true)
}
