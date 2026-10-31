// Browse's actions (new collections, renaming and expanding), each test against its own
// temporary copy of the sample archive with a configuration folder of its own, so nothing
// outside the copy is written. Expanding puts the compressed file in a Trash folder inside the
// copy, never the real Trash.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class BrowseActionsTests: XCTestCase {
    private var sample: SampleWindow!
    private var browse: BrowseActionsModel { sample.model.browse! }

    override func setUp() async throws {
        sample = try await SampleWindow()
        setenv("CARREL_TRASH_DIR", sample.folder.appending(path: "Trash").path(), 1)
    }

    override func tearDown() async throws {
        unsetenv("CARREL_TRASH_DIR")
        sample.remove()
    }

    private func item(titled title: String) throws -> ItemSummary {
        try XCTUnwrap(sample.model.items.first { $0.title == title })
    }

    private func item(id: String) throws -> ItemSummary {
        try XCTUnwrap(sample.model.items.first { $0.id == id })
    }

    /// Runs `program` with `arguments` in `folder`, expecting it to succeed.
    private func run(_ program: String, _ arguments: [String], in folder: URL) throws {
        let process = Process()
        process.executableURL = URL(filePath: program)
        process.currentDirectoryURL = folder
        process.arguments = arguments
        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "\(program) \(arguments)")
    }

    /// Files a zip holding `files` at `Projects/Hardware/NAME`, with its card, recorded in the
    /// history as filing records it, so undo has something to go back to.
    private func fileZip(_ name: String, holding files: [String: String]) async throws
        -> ItemSummary
    {
        let work = sample.folder.appending(path: "zip-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        for (file, text) in files {
            try Data(text.utf8).write(to: work.appending(path: file))
        }
        let folder = sample.root.appending(path: "Projects/Hardware")
        try run(
            "/usr/bin/zip", ["-q", folder.appending(path: name).path()] + files.keys.sorted(),
            in: work)
        let card = """
            ---
            carrel: card
            format: "1.0"
            id: \(UUID().uuidString.lowercased())
            kind: file
            title: "\(name)"
            item: "\(name)"
            dates:
              added: "2026-10-31"
            sensitivity: personal
            ---

            """
        try Data(card.utf8).write(to: folder.appending(path: "\(name).md"))
        let git = [
            "--git-dir=_System/history.git", "--work-tree=.", "-c", "user.name=Test",
            "-c", "user.email=test@example.com",
        ]
        try run(
            "/usr/bin/git", git + ["add", "-f", "Projects/Hardware/\(name).md"], in: sample.root)
        try run("/usr/bin/git", git + ["commit", "-q", "-m", "File \(name)"], in: sample.root)
        await sample.model.refreshItems()
        return try item(titled: name)
    }

    func testANewCollectionIsCreatedInTheOneShownAndShown() async throws {
        sample.model.screen = .collection("Projects")
        browse.startNewCollection(in: sample.model.browsedCollection)
        var draft = try XCTUnwrap(browse.creating)
        XCTAssertEqual(draft.path, "Projects/")
        draft.path = "Projects/Optics/"
        draft.description = "Lenses and spectrometers"
        draft.tags = [TagDraft(name: "lab", meaning: "made in the lab"), TagDraft()]
        XCTAssertEqual(draft.collectionTags.count, 1, "an empty row is left out")
        await browse.create(draft)

        XCTAssertNil(browse.creating)
        XCTAssertEqual(sample.model.screen, .collection("Projects/Optics"))
        let created = try XCTUnwrap(
            sample.model.collections.first { $0.path == "Projects/Optics" })
        XCTAssertEqual(created.description, "Lenses and spectrometers")
        XCTAssertEqual(sample.archive.history.first?.operation, "restructure")
    }

    func testANewCollectionThatCantBeMadeKeepsTheSheetOpenSayingWhy() async throws {
        browse.startNewCollection(in: "Projects")
        var draft = try XCTUnwrap(browse.creating)
        draft.path = "Projects"
        await browse.create(draft)

        let shown = try XCTUnwrap(browse.creating)
        XCTAssertEqual(shown.path, "Projects")
        XCTAssertNotNil(shown.problem)
        XCTAssertNil(sample.archive.lastError, "the sheet says it, not the window's alert")

        var tagged = shown
        tagged.path = "Projects/Optics"
        tagged.tags = [TagDraft(name: "Not A Tag", meaning: "spaces")]
        await browse.create(tagged)
        XCTAssertNotNil(browse.creating?.problem)
        XCTAssertFalse(sample.model.collections.contains { $0.path == "Projects/Optics" })
    }

    func testAnItemIsRenamedFromTheSuggestedNameAndUndoBringsItBack() async throws {
        let parts = try item(titled: "Desk lamp parts list")
        sample.model.screen = .collection("Projects/Hardware")
        sample.model.selectedItemID = parts.id
        await browse.startRename(of: parts)
        var draft = try XCTUnwrap(browse.renaming)
        XCTAssertEqual(draft.current, "parts-list.csv")
        XCTAssertEqual(draft.name, "parts-list.csv", "already portable")
        XCTAssertFalse(draft.canRename, "the same name isn't a rename")
        draft.name = "lamp-parts.csv"
        XCTAssertTrue(draft.canRename)
        await browse.rename(draft)

        XCTAssertNil(browse.renaming)
        let renamed = try item(titled: "Desk lamp parts list")
        XCTAssertEqual(renamed.id, parts.id)
        XCTAssertEqual(renamed.item, "Projects/Hardware/lamp-parts.csv")
        XCTAssertEqual(sample.model.selectedItem?.id, parts.id, "it stays chosen")
        let change = try XCTUnwrap(sample.archive.history.first)
        XCTAssertEqual(change.operation, "move")

        try await sample.archive.undo(change.id)
        await sample.model.refreshItems()
        XCTAssertEqual(try item(titled: "Desk lamp parts list").item, parts.item)
    }

    func testARenameToATakenNameKeepsTheSheetOpenSayingWhy() async throws {
        let parts = try item(titled: "Desk lamp parts list")
        await browse.startRename(of: parts)
        var draft = try XCTUnwrap(browse.renaming)
        draft.name = "notes.md"
        await browse.rename(draft)
        XCTAssertNotNil(browse.renaming?.problem)
        XCTAssertNil(sample.archive.lastError)
        XCTAssertEqual(try item(titled: "Desk lamp parts list").item, parts.item)
    }

    func testOnlyACompressedFileOffersToExpand() async throws {
        let parts = try item(titled: "Desk lamp parts list")
        XCTAssertFalse(browse.canExpand(parts))
        await browse.askToExpand(parts)
        XCTAssertNil(browse.expanding)
        XCTAssertNotNil(browse.problem)
    }

    func testAFiledZipIsExpandedAfterAskingAndUndone() async throws {
        let zip = try await fileZip("Spectra.zip", holding: ["a.txt": "a", "b.txt": "b"])
        XCTAssertTrue(browse.canExpand(zip))
        await browse.askToExpand(zip)
        let request = try XCTUnwrap(browse.expanding)
        XCTAssertNil(request.keptBecause)
        await browse.expand(request)

        XCTAssertNil(browse.expanding)
        XCTAssertNil(browse.problem)
        let expanded = try item(id: zip.id)
        XCTAssertEqual(expanded.kind, .folder)
        let folder = sample.root.appending(path: try XCTUnwrap(expanded.item))
        XCTAssertTrue(FileManager.default.fileExists(atPath: folder.appending(path: "a.txt").path))
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: sample.folder.appending(path: "Trash/Spectra.zip").path))
        let change = try XCTUnwrap(sample.archive.history.first)
        try await sample.archive.undo(change.id)
        await sample.model.refreshItems()
        XCTAssertEqual(try item(id: zip.id).item, zip.item)
    }

    func testSoftwareIsExpandedOnlyAllTheSame() async throws {
        let zip = try await fileZip("Utility.zip", holding: ["Setup.exe": "MZ"])
        await browse.askToExpand(zip)
        let request = try XCTUnwrap(browse.expanding)
        XCTAssertNotNil(request.keptBecause)
        await browse.expand(request)
        XCTAssertEqual(try item(id: zip.id).item, "Projects/Hardware/Setup.exe", "unwrapped")
    }

    func testAReadOnlyWindowOffersNoChanges() async throws {
        XCTAssertTrue(sample.model.canChange)
        XCTAssertFalse(ArchiveWindowModel(url: sample.root).canChange, "nothing loaded yet")
    }
}
