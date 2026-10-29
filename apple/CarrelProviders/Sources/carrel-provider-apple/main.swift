// carrel-provider-apple: Carrel's provider for Apple's frameworks (docs/providers.md).
// Carrel starts it and talks to it over standard input and output; standard error is its log.

import CarrelAppleProviders
import Foundation

let server = Server(name: "Apple", version: "0.1.0", extractors: [PDFText()])
FileHandle.standardError.write(Data("carrel-provider-apple: started\n".utf8))
while let line = readLine(strippingNewline: true) {
    if let reply = server.reply(to: line) {
        print(reply)
        fflush(stdout)
    }
}
