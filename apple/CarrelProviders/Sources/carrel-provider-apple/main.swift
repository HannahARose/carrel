// carrel-provider-apple: Carrel's provider for Apple's frameworks (docs/providers.md).
// Carrel starts it and talks to it over standard input and output; standard error is its log.

import CarrelAppleProviders
import Foundation

let server = Server(
    name: "Apple", version: "0.3.0", models: [FoundationModel()],
    // PDFKit first: a PDF's text layer is exact, and OCR is only needed when there isn't one.
    // The image reader before OCR: it reads an image's text too, and what the image is.
    // The model reader reads only 3D models, so its place doesn't matter.
    extractors: [PDFText(), ImageReader(), VisionOCR(), ModelReader()],
    // Offered on every Mac, but used only by archives that chose it.
    embedders: [SentenceEmbedding()])
FileHandle.standardError.write(Data("carrel-provider-apple: started\n".utf8))
while let line = readLine(strippingNewline: true) {
    if let reply = await server.reply(to: line) {
        print(reply)
        fflush(stdout)
    }
}
