// The app's view models, against the sample archive in fixtures/ and temporary archives.

import CarrelKit
import XCTest

@testable import Carrel

/// The sample archive at the repository root's fixtures/.
let sampleArchive = URL(filePath: #filePath)
    .deletingLastPathComponent()
    .appending(path: "../../../fixtures/sample-archive")
    .standardizedFileURL

@MainActor
final class ArchiveWindowModelTests: XCTestCase {
    private var config: URL!

    override func setUp() async throws {
        // Opening a window reads what's active, which needs this device's identity: never the
        // real configuration's.
        config = FileManager.default.temporaryDirectory
            .appending(path: "CarrelTests.\(UUID().uuidString)")
        setenv("CARREL_CONFIG_DIR", config.path(percentEncoded: false), 1)
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: config)
    }

    func testLoadsTheSampleArchive() async throws {
        let model = ArchiveWindowModel(url: sampleArchive.appending(path: "Reading"))
        await model.load()
        XCTAssertNil(model.problem)
        XCTAssertEqual(model.name, "Sample archive")
        XCTAssertFalse(model.isReadOnly)
        XCTAssertEqual(model.items.count, 12)
        XCTAssertEqual(model.root.standardizedFileURL.path, sampleArchive.path)
    }

    func testNestsCollectionsInTheTree() async throws {
        let model = ArchiveWindowModel(url: sampleArchive)
        await model.load()
        let projects = try XCTUnwrap(model.tree.first { $0.collection.path == "Projects" })
        XCTAssertTrue(
            projects.children?.contains { $0.collection.path == "Projects/Hardware" } ?? false)
        XCTAssertFalse(model.tree.contains { $0.collection.path == "Projects/Hardware" })
        let titles = model.tree.map(\.collection.title)
        XCTAssertEqual(
            titles, titles.sorted { $0.localizedStandardCompare($1) == .orderedAscending })
    }

    func testBrowseListsACollectionsOwnItems() async throws {
        let model = ArchiveWindowModel(url: sampleArchive)
        await model.load()
        XCTAssertEqual(model.listedItems, [], "Status lists no items")
        XCTAssertEqual(model.screenTitle, "Status")
        model.screen = .collection("Projects/Hardware")
        XCTAssertEqual(model.screenTitle, "Hardware")
        model.screen = .collection("Reading")
        let cards = model.listedItems.map(\.card)
        XCTAssertTrue(cards.contains("Reading/On the Origin of Species.md"))
        XCTAssertTrue(cards.allSatisfy { $0.hasPrefix("Reading/") })
        // Every item is listed in exactly one collection.
        let listed = model.collections.flatMap { model.items(in: $0.path) }
        XCTAssertEqual(listed.count, model.items.count)
    }

    func testChangingScreenClearsTheSelection() async throws {
        let model = ArchiveWindowModel(url: sampleArchive)
        await model.load()
        model.screen = .collection("Reading")
        model.selectedItemID = model.listedItems.first?.id
        XCTAssertNotNil(model.selectedItem)
        model.screen = .review
        XCTAssertNil(model.selectedItem)
    }

    func testSummaryCountsWhatTheArchiveHolds() async throws {
        let model = ArchiveWindowModel(url: sampleArchive)
        await model.load()
        let summary = model.summary
        XCTAssertEqual(summary.itemCount, 12)
        XCTAssertEqual(summary.collectionCount, model.collections.count)
        XCTAssertEqual(summary.kinds.map(\.count).reduce(0, +), 12)
        XCTAssertTrue(summary.kinds.contains { $0.kind == .physical })
        XCTAssertEqual(summary.holdingCount, model.items(in: "Holding").count)
    }

    func testReadsTheSelectedItemsCard() async throws {
        let model = ArchiveWindowModel(url: sampleArchive)
        await model.load()
        let book = try XCTUnwrap(
            model.items.first { $0.card == "Reading/On the Origin of Species.md" })
        let card = try XCTUnwrap(model.card(for: book))
        XCTAssertEqual(card.value("title"), "On the Origin of Species (1859), paperback")
        XCTAssertEqual(card.value("tags"), "biology, reference")
        XCTAssertEqual(card.value("dates"), "added 2026-10-21, document 1859-11-24")
        XCTAssertEqual(card.notes, "Margin notes in chapter 4.")
    }

    func testReportsFoldersThatArentArchives() async throws {
        let model = ArchiveWindowModel(url: FileManager.default.temporaryDirectory)
        await model.load()
        XCTAssertNotNil(model.problem)
        XCTAssertTrue(model.items.isEmpty)
    }
}

@MainActor
final class CardTests: XCTestCase {
    func testSplitsFieldsNotesAndHistory() {
        let card = Card(
            text: """
                ---
                title: "A letter"
                tags: [family]
                dates:
                  added: "2026-10-01"
                ---

                Written from Lisbon.

                <!-- carrel:history -->
                ## History
                - 2026-10-01 Added from Holding
                <!-- /carrel:history -->
                """)
        XCTAssertEqual(card.fields.map(\.key), ["title", "tags", "dates"])
        XCTAssertEqual(card.value("title"), "A letter")
        XCTAssertEqual(card.value("tags"), "family")
        XCTAssertEqual(card.value("dates"), "added 2026-10-01")
        XCTAssertEqual(card.notes, "Written from Lisbon.")
        XCTAssertEqual(card.history, ["- 2026-10-01 Added from Holding"])
    }

    func testACardWithoutFrontMatterIsAllNotes() {
        let card = Card(text: "Just notes.\n")
        XCTAssertTrue(card.fields.isEmpty)
        XCTAssertEqual(card.notes, "Just notes.")
        XCTAssertTrue(card.history.isEmpty)
    }
}

@MainActor
final class AppModelTests: XCTestCase {
    private var defaults: UserDefaults!
    private var suite: String!
    private var folder: URL!

    override func setUp() async throws {
        suite = "CarrelTests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)
        folder = FileManager.default.temporaryDirectory.appending(path: suite)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        // Creating an archive records who made it in the configuration folder; never the real one.
        setenv("CARREL_CONFIG_DIR", folder.appending(path: "config").path(percentEncoded: false), 1)
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: folder)
    }

    func testRecentsAreNewestFirstWithoutDuplicates() {
        let recents = RecentArchives(defaults: defaults)
        for name in ["a", "b", "a"] { recents.note(URL(filePath: "/tmp/\(name)")) }
        XCTAssertEqual(recents.urls.map(\.lastPathComponent), ["a", "b"])
        XCTAssertEqual(RecentArchives(defaults: defaults).urls, recents.urls, "kept in defaults")
        for index in 0..<20 { recents.note(URL(filePath: "/tmp/\(index)")) }
        XCTAssertEqual(recents.urls.count, RecentArchives.limit)
        recents.clear()
        XCTAssertTrue(RecentArchives(defaults: defaults).urls.isEmpty)
    }

    func testOpeningFindsTheArchiveRootAndNotesIt() throws {
        let app = AppModel(recents: RecentArchives(defaults: defaults))
        let root = try app.resolve(sampleArchive.appending(path: "Projects/Hardware"))
        XCTAssertEqual(root.path, sampleArchive.path)
        XCTAssertEqual(app.recents.urls, [root])
        XCTAssertThrowsError(try app.resolve(folder))
    }

    func testCreatesAnArchiveNamedAfterItsFolder() async throws {
        let app = AppModel(recents: RecentArchives(defaults: defaults))
        let root = try app.create(at: folder.appending(path: "Letters"))
        XCTAssertEqual(app.recents.urls, [root])
        let model = ArchiveWindowModel(url: root)
        await model.load()
        XCTAssertEqual(model.name, "Letters")
        XCTAssertTrue(model.collections.contains { $0.path == "Holding" })
        XCTAssertThrowsError(try app.create(at: root), "already an archive")
    }

    func testSettingsReadTheArchiveAndThisMac() throws {
        let config = folder.appending(path: "config")
        try FileManager.default.createDirectory(
            at: config.appending(path: "models/qwen"), withIntermediateDirectories: true)
        try Data(#"{"model": "llama/qwen3.5-4b"}"#.utf8).write(
            to: config.appending(path: "cli.json"))
        let settings = LocalSettings.read(
            archive: sampleArchive,
            environment: ["CARREL_CONFIG_DIR": config.path(percentEncoded: false), "PATH": ""])
        XCTAssertEqual(settings.installedModels, ["qwen"])
        XCTAssertEqual(settings.draftingModel, "llama/qwen3.5-4b")
        XCTAssertNil(settings.secondModel)
        XCTAssertEqual(settings.archive?.name, "Sample archive")
        XCTAssertEqual(settings.archive?.embeddingModel, "sample/embedding-model-v1")
        XCTAssertEqual(settings.archive?.backupsSetUp, false)
    }
}
