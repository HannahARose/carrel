// Flagging from Browse (FORMAT.md §5.9): Flag… with a note, the mark in the lists, the
// sidebar's Flagged, and the offer to clear a flag after an edit. Each test has its own copy
// of the sample archive with a configuration folder of its own.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class FlaggingTests: XCTestCase {
    private var sample: SampleWindow!
    private var browse: BrowseActionsModel { sample.model.browse! }

    override func setUp() async throws {
        sample = try await SampleWindow()
    }

    override func tearDown() async throws {
        sample.remove()
    }

    private func book() throws -> ItemSummary {
        try XCTUnwrap(sample.model.items.first { $0.card == "Reading/On the Origin of Species.md" })
    }

    func testAnItemIsFlaggedWithANoteListedAndUnflagged() async throws {
        let item = try book()
        XCTAssertNil(item.flag)
        browse.startFlag(of: item)
        var draft = try XCTUnwrap(browse.flagging)
        draft.note = "  wrong title "
        await browse.flag(draft)
        XCTAssertNil(browse.flagging, "the sheet closes")
        let flagged = try book()
        XCTAssertEqual(flagged.flag?.note, "wrong title")
        XCTAssertEqual(sample.model.flaggedItems.map(\.id), [item.id])
        sample.model.screen = .flagged
        XCTAssertEqual(sample.model.screenTitle, "Flagged")

        browse.askToUnflag(flagged)
        let request = try XCTUnwrap(browse.unflagging)
        XCTAssertFalse(request.afterEdit)
        await browse.unflag(request)
        XCTAssertNil(try book().flag)
        XCTAssertTrue(sample.model.flaggedItems.isEmpty)
    }

    func testACollectionIsFlaggedAndShowsOnStatus() async throws {
        let projects = try XCTUnwrap(sample.model.collections.first { $0.path == "Projects" })
        browse.startFlag(of: projects)
        await browse.flag(try XCTUnwrap(browse.flagging))
        XCTAssertNotNil(sample.model.collections.first { $0.path == "Projects" }?.flag)
        XCTAssertEqual(sample.model.flaggedCollections.map(\.path), ["Projects"])
        XCTAssertEqual(sample.model.archive?.status?.flagged.first?.path, "Projects")
    }

    func testAnEditOfAFlaggedCardOffersToClearTheFlag() async throws {
        let item = try book()
        try await sample.archive.flag(item: item.id, note: "wrong title")
        await browse.offerToUnflag(item: item.id, title: item.title)
        let offer = try XCTUnwrap(browse.unflagging)
        XCTAssertTrue(offer.afterEdit)
        XCTAssertEqual(offer.flag.note, "wrong title")

        browse.unflagging = nil
        try await sample.archive.unflag(item: item.id)
        await browse.offerToUnflag(item: item.id, title: item.title)
        XCTAssertNil(browse.unflagging, "nothing to offer for a card that isn't flagged")
    }
}
