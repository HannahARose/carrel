// Previews and Browse's grid, each test against its own temporary copy of the sample archive,
// whose `_System/cache.nosync/previews/` is the temporary cache.

import CarrelKit
import CoreGraphics
import XCTest

@testable import Carrel

/// Makes a plain thumbnail of any file, counting what it was asked for.
final class CountingThumbnails: ThumbnailMaker, @unchecked Sendable {
    private let lock = NSLock()
    private var asked: [URL] = []

    var files: [URL] { lock.withLock { asked } }

    func thumbnail(of file: URL, pixels: Int) async -> CGImage? {
        lock.withLock { asked.append(file) }
        let context = CGContext(
            data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        context?.setFillColor(gray: 0.5, alpha: 1)
        context?.fill(CGRect(x: 0, y: 0, width: 8, height: 8))
        return context?.makeImage()
    }
}

/// The sample's items used here.
private let note = "01a18701-0500-776a-9867-0805d715b667"
private let fieldNotes = "01a18701-0500-776a-9867-080ae1375736"
private let lampBuild = "01a18701-0500-776a-9867-080c8c427e79"
private let book = "01a18701-0500-776a-9867-08063e3d5d47"
private let bracket = "01a18701-0500-776a-9867-08f0000000f1"

@MainActor
final class PreviewStoreTests: XCTestCase {
    private var sample: SampleWindow!

    override func setUp() async throws {
        sample = try await SampleWindow()
    }

    override func tearDown() async throws {
        sample.remove()
    }

    private var cache: URL { sample.root.appending(path: "_System/cache.nosync/previews") }

    private func store(_ maker: CountingThumbnails = CountingThumbnails()) async throws
        -> PreviewStore
    {
        let store = PreviewStore(handle: try XCTUnwrap(sample.model.handle), maker: maker)
        await store.reload()
        return store
    }

    private func item(_ id: String) throws -> ItemSummary {
        try XCTUnwrap(sample.model.items.first { $0.id == id })
    }

    /// Every file in the archive outside the preview cache, with its contents.
    private func archiveFiles() throws -> [String: Data] {
        let root = sample.root.standardizedFileURL.path(percentEncoded: false)
        var files: [String: Data] = [:]
        let walk = FileManager.default.enumerator(atPath: root)
        while let path = walk?.nextObject() as? String {
            let url = URL(filePath: root).appending(path: path)
            guard !path.hasPrefix("_System/cache.nosync"),
                (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
            else { continue }
            files[path] = try Data(contentsOf: url)
        }
        return files
    }

    /// Restricts the sample item whose card is at `card`.
    private func restrict(_ card: String) throws {
        let url = sample.root.appending(path: card)
        let text = try String(contentsOf: url, encoding: .utf8)
        try text.replacingOccurrences(of: "sensitivity: public", with: "sensitivity: restricted")
            .replacingOccurrences(of: "sensitivity: personal", with: "sensitivity: restricted")
            .write(to: url, atomically: true, encoding: .utf8)
    }

    func testTheInspectorPreviewsEachKindOfItem() async throws {
        let store = try await store()
        let file = sample.root.appending(path: "Reading/Coupled oscillators note.pdf")
        guard case .quickLook(let url) = store.content(for: try item(note)) else {
            return XCTFail("a PDF shows Quick Look's preview")
        }
        XCTAssertEqual(url.resolvingSymlinksInPath(), file.resolvingSymlinksInPath())
        guard case .folder = store.content(for: try item(lampBuild)) else {
            return XCTFail("a folder shows its contents")
        }
        guard case .folder = store.content(for: try item(fieldNotes)) else {
            return XCTFail("a bundle shows its contents")
        }
        XCTAssertEqual(store.content(for: try item(book)), .none, "a book has only its card")

        await store.loadFolder(lampBuild)
        let entries = try XCTUnwrap(store.folders[lampBuild])
        XCTAssertFalse(entries.isEmpty)
        XCTAssertFalse(entries.contains { $0.name.hasPrefix(".") })
        XCTAssertEqual(
            entries.map(\.isFolder), entries.map(\.isFolder).sorted { $0 && !$1 },
            "folders come first")
    }

    func testThumbnailsAreMadeOnceAndKeptOnlyInTheDeviceCache() async throws {
        let before = try archiveFiles()
        let maker = CountingThumbnails()
        let store = try await store(maker)
        XCTAssertNil(store.picture(for: note), "the kind icon shows until it's made")
        await store.loadThumbnail(note)
        XCTAssertNotNil(store.picture(for: note))
        XCTAssertEqual(maker.files.map(\.lastPathComponent), ["Coupled oscillators note.pdf"])
        let kept = try FileManager.default.contentsOfDirectory(
            atPath: cache.appending(path: "thumbnails").path(percentEncoded: false))
        XCTAssertEqual(kept.count, 1)
        XCTAssertTrue(kept[0].hasSuffix(".png"))
        XCTAssertEqual(try archiveFiles(), before, "nothing is written into the archive")

        // Another window, or the next launch, reads the kept one.
        let again = CountingThumbnails()
        let other = try await self.store(again)
        await other.loadThumbnail(note)
        XCTAssertNotNil(other.picture(for: note))
        XCTAssertEqual(again.files, [])

        // A changed file gets a new thumbnail.
        try Data("%PDF-1.4 changed".utf8).write(
            to: sample.root.appending(path: "Reading/Coupled oscillators note.pdf"))
        await other.reload()
        XCTAssertNil(other.picture(for: note))
        await other.loadThumbnail(note)
        XCTAssertEqual(again.files.count, 1)
    }

    func testRestrictedItemsShowTheirCardUntilThePreviewIsAskedFor() async throws {
        try restrict("Reading/Coupled oscillators note.pdf.md")
        let maker = CountingThumbnails()
        let store = try await store(maker)
        let item = try item(note)
        XCTAssertEqual(store.content(for: item), .hidden)
        await store.loadThumbnail(note)
        XCTAssertNil(store.picture(for: note), "the grid shows its kind icon")
        XCTAssertEqual(maker.files, [])
        XCTAssertFalse(FileManager.default.fileExists(atPath: cache.path(percentEncoded: false)))

        store.chose(note)
        store.reveal(note)
        guard case .quickLook = store.content(for: item) else {
            return XCTFail("one click shows the preview")
        }
        store.chose(book)
        XCTAssertEqual(store.content(for: item), .hidden, "choosing another hides it again")
    }

    func testFormatsQuickLookCantShowAreDrawnOrShowTheirKindIcon() async throws {
        // A STEP file, filed beside the note.
        let card = try String(
            contentsOf: sample.root.appending(path: "Reading/Coupled oscillators note.pdf.md"),
            encoding: .utf8
        )
        .replacingOccurrences(of: note, with: bracket)
        .replacingOccurrences(of: "A note on coupled oscillators", with: "Shelf bracket")
        .replacingOccurrences(of: "Coupled oscillators note.pdf", with: "bracket.step")
        try card.write(
            to: sample.root.appending(path: "Reading/bracket.step.md"), atomically: true,
            encoding: .utf8)
        try Data("ISO-10303-21;\nEND-ISO-10303-21;\n".utf8).write(
            to: sample.root.appending(path: "Reading/bracket.step"))
        await sample.model.refreshItems()

        let maker = CountingThumbnails()
        let store = try await store(maker)
        XCTAssertEqual(store.content(for: try item(bracket)), .drawing)
        XCTAssertEqual(store.previews[bracket]?.drawn, true)
        // Only the built-in providers here, so nothing draws it: the kind icon stays.
        await store.loadThumbnail(bracket)
        await store.loadDrawing(bracket)
        XCTAssertNil(store.picture(for: bracket))
        XCTAssertEqual(maker.files, [], "Quick Look isn't asked")
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: cache.appending(path: "drawings").path(percentEncoded: false)))
    }

    func testTheWindowKeepsItsPreviewsAndBrowseLayout() async throws {
        XCTAssertNotNil(sample.model.previews?.previews[note])
        let suite = "carrel-tests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = ArchiveWindowModel(url: sample.root, defaults: defaults)
        XCTAssertEqual(model.browseLayout, .list)
        model.browseLayout = .grid
        XCTAssertEqual(ArchiveWindowModel(url: sample.root, defaults: defaults).browseLayout, .grid)
    }
}
