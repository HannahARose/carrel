// Text recognition in images and scanned PDFs through Vision.

import CoreGraphics
import Foundation
import PDFKit
import Vision

/// Recognizes text in images, and in PDFs by rendering each page. It's listed after `PDFText`,
/// so a PDF's own text layer is used when it has one.
public struct VisionOCR: TextExtractor {
    /// The image types it reads, besides PDF.
    public static let imageTypes = [
        "image/png", "image/jpeg", "image/heic", "image/heif", "image/tiff", "image/gif",
        "image/bmp", "image/webp",
    ]

    /// How much larger than its own size a PDF page is rendered, for legible small print.
    static let pdfScale: CGFloat = 2

    public init() {}

    public var offer: Offer {
        Offer(
            capability: .extractText, id: "vision", label: "Vision OCR",
            mediaTypes: Self.imageTypes + ["application/pdf"])
    }

    public func extract(_ url: URL, mediaType: String) async throws(ProviderError) -> Extraction {
        if mediaType.lowercased() == "application/pdf" {
            return try await recognizePDF(url)
        }
        guard FileManager.default.isReadableFile(atPath: url.path(percentEncoded: false)) else {
            throw .failed("can't read \(url.lastPathComponent)")
        }
        do {
            let lines = try await Self.request.perform(on: url)
            return Extraction(text: Self.text(of: lines), method: "ocr", pages: 1)
        } catch {
            throw .failed("\(url.lastPathComponent): \(error.localizedDescription)")
        }
    }

    func recognizePDF(_ url: URL) async throws(ProviderError) -> Extraction {
        guard let document = PDFDocument(url: url) else {
            throw .failed("\(url.lastPathComponent) isn't a readable PDF")
        }
        if document.isLocked {
            throw .failed("\(url.lastPathComponent) is password-protected")
        }
        var pages: [String] = []
        for index in 0..<document.pageCount {
            guard let page = document.page(at: index), let image = Self.render(page) else {
                throw .failed("can't render page \(index + 1) of \(url.lastPathComponent)")
            }
            do {
                pages.append(Self.text(of: try await Self.request.perform(on: image)))
            } catch {
                throw .failed("page \(index + 1) of \(url.lastPathComponent): \(error.localizedDescription)")
            }
        }
        // A form feed between pages, as plain-text PDF tools do.
        return Extraction(text: pages.joined(separator: "\n\u{0C}"), method: "ocr", pages: pages.count)
    }

    static var request: RecognizeTextRequest {
        var request = RecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        request.automaticallyDetectsLanguage = true
        return request
    }

    /// Lines in reading order: top to bottom, then left to right. Vision's coordinates start
    /// at the bottom left.
    static func text(of observations: [RecognizedTextObservation]) -> String {
        observations
            .sorted {
                let a = $0.boundingBox.cgRect, b = $1.boundingBox.cgRect
                if abs(a.midY - b.midY) > min(a.height, b.height) / 2 {
                    return a.midY > b.midY
                }
                return a.minX < b.minX
            }
            .compactMap { $0.topCandidates(1).first?.string }
            .joined(separator: "\n")
    }

    /// The page drawn on white at `pdfScale` times its size.
    static func render(_ page: PDFPage) -> CGImage? {
        let bounds = page.bounds(for: .mediaBox)
        let width = Int(bounds.width * pdfScale), height = Int(bounds.height * pdfScale)
        guard width > 0, height > 0,
            let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { return nil }
        context.setFillColor(CGColor(gray: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.scaleBy(x: pdfScale, y: pdfScale)
        context.translateBy(x: -bounds.minX, y: -bounds.minY)
        page.draw(with: .mediaBox, to: context)
        return context.makeImage()
    }
}
