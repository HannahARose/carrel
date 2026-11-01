// Carrel's Quick Look extension (docs/design/macos-integration.md, "Quick Look"): a card laid
// out as the app's inspector shows it, and Carrel's own drawings of STEP, IGES and 3MF files.
// It's sandboxed: it reads the file it previews, and the drawings and archive roots the agent
// keeps in its QuickLookStore, which its entitlements let it read. It never opens the archive.

import CarrelKit
import Foundation
import QuickLookUI
import UniformTypeIdentifiers

final class CardPreviewProvider: QLPreviewProvider, QLPreviewingController {
    /// The page's size; Quick Look scales the window to it.
    private static let pageSize = CGSize(width: 640, height: 760)

    func providePreview(for request: QLFilePreviewRequest) async throws -> QLPreviewReply {
        let file = request.fileURL
        let store = QuickLookStore.shared
        if QuickLookStore.isDrawn(file.path(percentEncoded: false)) {
            guard
                let drawing = quickLookDrawing(
                    store: store.drawings.path(percentEncoded: false),
                    path: file.path(percentEncoded: false)),
                FileManager.default.fileExists(atPath: drawing)
            else {
                // Not drawn yet, or not in an archive the agent follows: Quick Look shows the
                // file's icon instead.
                throw CocoaError(.fileReadNoSuchFile)
            }
            return QLPreviewReply(fileURL: URL(filePath: drawing))
        }
        let text = try String(contentsOf: file, encoding: .utf8)
        let html: String
        do {
            let card = try quickLookCard(
                text: text, path: file.path(percentEncoded: false), roots: store.roots())
            html = QuickLookCardPage.html(card)
        } catch {
            // Markdown that isn't a card is shown as its text, as Quick Look would.
            html = QuickLookCardPage.plain(text, title: file.lastPathComponent)
        }
        return QLPreviewReply(dataOfContentType: .html, contentSize: Self.pageSize) { _ in
            Data(html.utf8)
        }
    }
}
