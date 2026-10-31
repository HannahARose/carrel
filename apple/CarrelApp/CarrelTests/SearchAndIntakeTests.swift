// Search and adding items, each test against its own temporary copy of the sample archive
// with a configuration folder of its own, so nothing outside the copy is written.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
class SampleCopyTestCase: XCTestCase {
    private(set) var folder: URL!
    private(set) var archive: ArchiveModel!

    override func setUp() async throws {
        folder = FileManager.default.temporaryDirectory
            .appending(path: "carrel-app-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let root = folder.appending(path: "archive")
        try FileManager.default.copyItem(at: sampleArchive, to: root)
        let handle = try ArchiveHandle.open(
            root, options: .isolated(config: folder.appending(path: "config")))
        try handle.startHistory()
        archive = ArchiveModel(handle: handle)
    }

    override func tearDown() async throws {
        archive = nil
        try? FileManager.default.removeItem(at: folder)
    }
}

final class SearchModelTests: SampleCopyTestCase {
    func testWordsComeBackBeforeThePauseAndTheWholeSearchAfter() async throws {
        let search = SearchModel(archive: archive, pause: .seconds(60))
        search.text = "lamp"
        // The words' results arrive while the search by meaning waits for typing to pause.
        for _ in 0..<200 where search.results == nil {
            try await Task.sleep(for: .milliseconds(10))
        }
        let words = try XCTUnwrap(search.results)
        XCTAssertTrue(words.hits.contains { $0.title == "Desk lamp parts list" })
        XCTAssertNil(words.wordsOnly, "by words alone, before meaning is tried")
        XCTAssertEqual(search.heading, "\(words.total) results")
        XCTAssertNil(search.notice)
        XCTAssertTrue(search.isActive)
    }

    func testSaysWhyMeaningWasntUsedAsTheCLIDoes() async throws {
        let search = SearchModel(archive: archive, pause: .zero)
        search.text = "lamp"
        await search.settled()
        XCTAssertFalse(search.isSearchingByMeaning)
        let results = try XCTUnwrap(search.results)
        XCTAssertFalse(results.byMeaning)
        // The sample archive names an embedding model but has no meaning index for it.
        XCTAssertEqual(
            search.notice,
            "There's no meaning index for sample/embedding-model-v1 yet, so this was searched by words only; build it with `carrel index rebuild`."
        )
        let first = try XCTUnwrap(results.hits.first)
        XCTAssertEqual(first.collection, "Projects/Hardware")
    }

    func testListsTheBestFirstWithTheirCollectionAndPassage() async throws {
        let search = SearchModel(archive: archive, pause: .zero)
        search.text = "lamp"
        await search.settled()
        let hit = try XCTUnwrap(search.hits.first { $0.title == "Desk lamp build" })
        XCTAssertEqual(hit.collection, "Projects/Hardware")
        XCTAssertNotNil(hit.itemId)
        let passage = hit.highlightedPassage
        XCTAssertTrue(String(passage.characters).contains("lamp"))
        let marked = passage.runs.filter { $0.inlinePresentationIntent == .stronglyEmphasized }
        XCTAssertEqual(marked.map { String(passage[$0.range].characters) }, ["lamp"])
    }

    func testFiltersNarrowTheResults() async throws {
        let search = SearchModel(archive: archive, pause: .zero)
        search.text = "kind:physical"
        await search.settled()
        XCTAssertFalse(search.hits.isEmpty)
        XCTAssertTrue(search.hits.allSatisfy { $0.kind == .physical })

        search.text = "collection:Reading"
        await search.settled()
        XCTAssertFalse(search.hits.isEmpty)
        XCTAssertTrue(search.hits.allSatisfy { $0.collection == "Reading" })
    }

    func testAFilterThatCantBeUsedIsAProblemNotAnAlert() async throws {
        let search = SearchModel(archive: archive, pause: .zero)
        search.text = "after:someday"
        await search.settled()
        XCTAssertNil(search.results)
        XCTAssertNotNil(search.problem)
        XCTAssertNil(archive.lastError, "typing never raises an alert")
    }

    func testSensitiveAndRestrictedItemsAreMarked() async throws {
        let search = SearchModel(archive: archive, pause: .zero)
        search.text = "sensitivity:restricted"
        await search.settled()
        let restricted = search.hits.filter { $0.sensitivity == .restricted }
        XCTAssertEqual(restricted.count, search.hits.count)
        XCTAssertTrue(restricted.allSatisfy { $0.sensitivityMark == "Restricted" })

        search.text = "sensitivity:personal"
        await search.settled()
        XCTAssertFalse(search.hits.isEmpty)
        XCTAssertTrue(search.hits.allSatisfy { $0.sensitivityMark == nil })
    }

    func testANewerSearchReplacesAnOlderOneAndClearingEmptiesIt() async throws {
        let search = SearchModel(archive: archive, pause: .zero)
        search.text = "lamp"
        search.text = "species"
        await search.settled()
        XCTAssertTrue(search.hits.contains { $0.title.contains("Species") })
        XCTAssertFalse(search.hits.contains { $0.title.contains("lamp") })

        search.clear()
        XCTAssertFalse(search.isActive)
        XCTAssertNil(search.results)
        XCTAssertNil(search.heading)
    }

    func testChoosingAResultShowsItInTheInspector() async throws {
        // The window only reads the copy here; the search runs on the isolated handle.
        let window = ArchiveWindowModel(url: folder.appending(path: "archive"))
        await window.load()
        let search = SearchModel(archive: archive, pause: .zero)
        search.text = "species"
        await search.settled()
        let hit = try XCTUnwrap(search.hits.first { $0.itemId != nil })
        window.selectedItemID = hit.itemId
        XCTAssertEqual(window.selectedItem?.title, hit.title)
    }
}

final class IntakeModelTests: SampleCopyTestCase {
    private func file(_ name: String, _ text: String) throws -> URL {
        let inbox = folder.appending(path: "Inbox")
        try FileManager.default.createDirectory(at: inbox, withIntermediateDirectories: true)
        let url = inbox.appending(path: name)
        try text.write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    func testAddedFilesJoinTheIntakeQueue() async throws {
        let intake = IntakeModel(archive: archive)
        let list = try file("shopping list.txt", "LED strip, diffuser, screws")
        let notes = try file("notes.txt", "Print the diffuser in PETG.")
        await intake.add([list, notes])
        XCTAssertEqual(intake.queue.waiting.count, 2)
        let report = try XCTUnwrap(intake.report)
        XCTAssertEqual(report.title, "Added 2 Items to the Intake Queue")
        XCTAssertNil(report.details)
        XCTAssertNil(intake.worker, "no worker is running")
        XCTAssertTrue(intake.canDraft)
    }

    func testWhatsLeftOutSaysWhy() async throws {
        let intake = IntakeModel(archive: archive)
        let list = try file("shopping list.txt", "LED strip, diffuser, screws")
        await intake.add([list])
        await intake.add([list])
        let report = try XCTUnwrap(intake.report)
        XCTAssertEqual(report.title, "Nothing to Add")
        XCTAssertEqual(report.leftOut.map(\.name), ["shopping list.txt"])
        XCTAssertNotNil(report.details)
        XCTAssertEqual(intake.queue.waiting.count, 1)
    }

    func testAMissingFileIsAProblem() async throws {
        let intake = IntakeModel(archive: archive)
        await intake.add([folder.appending(path: "Inbox/not here.txt")])
        let report = try XCTUnwrap(intake.report)
        XCTAssertEqual(report.title, "Couldn’t Add to the Archive")
        XCTAssertNotNil(report.problem)
    }

    func testTheWorkerLineIsTheInteractiveModes() {
        XCTAssertNil(IntakeModel(archive: archive).workerLine, "no worker is running")
        let status = WorkerStatus(
            item: "shopping list.txt", stage: "Drafting cards", done: 2, waiting: 3,
            updated: "2026-10-31T12:00:00Z")
        XCTAssertEqual(
            IntakeModel.line(for: status), "Drafting shopping list.txt (Drafting cards), 3 waiting")
        XCTAssertEqual(
            IntakeModel.line(
                for: WorkerStatus(
                    item: nil, stage: nil, done: 0, waiting: 1, updated: "")),
            "Drafting …, 1 waiting")
    }
}
