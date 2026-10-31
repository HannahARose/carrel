// carrel-provider-mlx: open models through MLX on Apple silicon (docs/design/mlx.md). Carrel
// starts it and talks to it over standard input and output; standard error is its log.

import CarrelAppleProviders
import CarrelMLXProvider
import Foundation

let folders = ModelFolder.modelsRoot().map(ModelFolder.discover) ?? []
let offered = offerFolders(folders)
let server = Server(
    name: "MLX", version: "0.3.0", models: offered.models, embedders: offered.embedders)
FileHandle.standardError.write(
    Data("carrel-provider-mlx: started with \(folders.count) model(s)\n".utf8))
while let line = readLine(strippingNewline: true) {
    if let reply = await server.reply(to: line) {
        print(reply)
        fflush(stdout)
    }
}
