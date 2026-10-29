// PDF text through PDFKit.

import Foundation
import PDFKit

/// Reads the text stored in PDFs. Scans without a text layer are left to OCR: they're
/// reported as unsupported, so Carrel tries the next extractor.
public struct PDFText: TextExtractor {
    public init() {}

    public var offer: Offer {
        Offer(
            capability: .extractText, id: "pdfkit", label: "PDFKit",
            mediaTypes: ["application/pdf"])
    }

    public func extract(_ url: URL, mediaType: String) async throws(ProviderError) -> Extraction {
        guard let document = PDFDocument(url: url) else {
            throw .failed("\(url.path(percentEncoded: false)) isn't a readable PDF")
        }
        if document.isLocked {
            throw .failed("\(url.lastPathComponent) is password-protected")
        }
        let text = document.string ?? ""
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            throw .unsupported("\(url.lastPathComponent) has no embedded text; it needs OCR")
        }
        return Extraction(text: text, method: "embedded", pages: document.pageCount)
    }
}
