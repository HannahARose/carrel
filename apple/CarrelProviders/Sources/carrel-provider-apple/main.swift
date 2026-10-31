// carrel-provider-apple: Carrel's provider for Apple's frameworks (docs/providers.md).
// Carrel starts it and talks to it over standard input and output; standard error is its log.

import CarrelAppleProviders
import Foundation

let server = Server(
    name: "Apple", version: "0.1.0", models: [FoundationModel()],
    // PDFKit first: a PDF's text layer is exact, and OCR is only needed when there isn't one.
    // The image reader before OCR: it reads an image's text too, and what the image is.
    extractors: [PDFText(), ImageReader(), VisionOCR()])
FileHandle.standardError.write(Data("carrel-provider-apple: started\n".utf8))
while let line = readLine(strippingNewline: true) {
    if let reply = await server.reply(to: line) {
        print(reply)
        fflush(stdout)
    }
}
