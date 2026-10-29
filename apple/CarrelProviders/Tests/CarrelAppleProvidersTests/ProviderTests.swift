import CarrelAppleProviders
import CoreText
import Foundation
import Testing

/// A one-page PDF with `text` drawn on it, or a blank page when `text` is nil.
func makePDF(_ text: String?) throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appending(path: "carrel-test-\(UUID().uuidString).pdf")
    var box = CGRect(x: 0, y: 0, width: 612, height: 792)
    let context = try #require(CGContext(url as CFURL, mediaBox: &box, nil))
    context.beginPDFPage(nil)
    if let text {
        let font = CTFontCreateWithName("Helvetica" as CFString, 24, nil)
        let line = CTLineCreateWithAttributedString(
            NSAttributedString(string: text, attributes: [.font: font]))
        context.textPosition = CGPoint(x: 72, y: 700)
        CTLineDraw(line, context)
    }
    context.endPDFPage()
    context.closePDF()
    return url
}

/// Sends `request` (a JSON object) to `server` and decodes the reply.
func send(_ server: Server, _ request: [String: Any]) throws -> [String: Any] {
    let line = String(data: try JSONSerialization.data(withJSONObject: request), encoding: .utf8)!
    let reply = try #require(server.reply(to: line))
    #expect(!reply.contains("\n"), "one message per line")
    return try #require(JSONSerialization.jsonObject(with: Data(reply.utf8)) as? [String: Any])
}

let server = Server(name: "Apple", version: "0.1.0", extractors: [PDFText()])

@Test func initializeReportsTheProtocolAndOffers() throws {
    let reply = try send(server, ["jsonrpc": "2.0", "id": 1, "method": "initialize",
                                  "params": ["protocol": 1, "client": "test"]])
    #expect(reply["id"] as? Int == 1)
    let result = try #require(reply["result"] as? [String: Any])
    #expect(result["protocol"] as? Int == 1)
    #expect(result["name"] as? String == "Apple")
    let offers = try #require(result["offers"] as? [[String: Any]])
    #expect(offers.count == 1)
    #expect(offers[0]["capability"] as? String == "extract-text")
    #expect(offers[0]["id"] as? String == "pdfkit")
    #expect(offers[0]["priority"] as? Int == 100)
    #expect(offers[0]["media_types"] as? [String] == ["application/pdf"])
}

@Test func pdfKitReadsEmbeddedText() throws {
    let url = try makePDF("Coupled oscillators")
    defer { try? FileManager.default.removeItem(at: url) }
    let reply = try send(server, ["jsonrpc": "2.0", "id": 2, "method": "text/extract", "params": [
        "offer": "pdfkit", "path": url.path(percentEncoded: false), "media_type": "application/pdf",
    ]])
    let result = try #require(reply["result"] as? [String: Any])
    #expect((result["text"] as? String)?.contains("Coupled oscillators") == true)
    #expect(result["method"] as? String == "embedded")
    #expect(result["pages"] as? Int == 1)
}

@Test func scansWithoutTextAreLeftForOCR() throws {
    let url = try makePDF(nil)
    defer { try? FileManager.default.removeItem(at: url) }
    #expect(throws: ProviderError.self) {
        try PDFText().extract(url, mediaType: "application/pdf")
    }
    let reply = try send(server, ["jsonrpc": "2.0", "id": 3, "method": "text/extract", "params": [
        "offer": "pdfkit", "path": url.path(percentEncoded: false), "media_type": "application/pdf",
    ]])
    let error = try #require(reply["error"] as? [String: Any])
    #expect(error["code"] as? Int == 2, "unsupported, so Carrel tries OCR next")
}

@Test func errorsUseTheProtocolsCodes() throws {
    let missing = try send(server, ["jsonrpc": "2.0", "id": 4, "method": "text/extract", "params": [
        "offer": "pdfkit", "path": "/nowhere/missing.pdf", "media_type": "application/pdf",
    ]])
    #expect((missing["error"] as? [String: Any])?["code"] as? Int == 3)

    let unknownOffer = try send(server, ["jsonrpc": "2.0", "id": 5, "method": "text/extract",
                                         "params": ["offer": "nope", "path": "/x", "media_type": "a/b"]])
    #expect((unknownOffer["error"] as? [String: Any])?["code"] as? Int == 2)

    let badParams = try send(server, ["jsonrpc": "2.0", "id": 6, "method": "text/extract", "params": [:]])
    #expect((badParams["error"] as? [String: Any])?["code"] as? Int == -32602)

    let unknownMethod = try send(server, ["jsonrpc": "2.0", "id": 7, "method": "bogus"])
    #expect((unknownMethod["error"] as? [String: Any])?["code"] as? Int == -32601)

    let noModels = try send(server, ["jsonrpc": "2.0", "id": 8, "method": "text/complete",
                                     "params": ["offer": "x", "prompt": "hi"]])
    #expect((noModels["error"] as? [String: Any])?["code"] as? Int == 2)

    let shutdown = try send(server, ["jsonrpc": "2.0", "id": 9, "method": "shutdown"])
    #expect(shutdown["result"] is NSNull)
}

@Test func notificationsAndNonsenseGetNoReply() {
    #expect(server.reply(to: #"{"jsonrpc":"2.0","method":"initialize"}"#) == nil)
    #expect(server.reply(to: "not json") == nil)
    #expect(server.reply(to: "") == nil)
}

@Test func theSampleArchivesPDFHasText() throws {
    let pdf = URL(filePath: #filePath)
        .deletingLastPathComponent()
        .appending(path: "../../../../fixtures/sample-archive/Reading/Coupled oscillators note.pdf")
        .standardizedFileURL
    let extraction = try PDFText().extract(pdf, mediaType: "application/pdf")
    #expect(!extraction.text.isEmpty)
}
