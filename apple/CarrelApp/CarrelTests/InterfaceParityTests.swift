// What the app gained to match the command line and the interactive mode (#343): the intake
// queue's list and folder choice, History's pages and librarian filter, a card's body edited,
// related items, Activity's range and wandering from an item. Each test has its own copy of
// the sample archive with a configuration folder of its own.

import CarrelKit
import XCTest

@testable import Carrel

final class IntakeQueueListTests: SampleCopyTestCase {
    private func inbox(_ names: [String]) throws -> URL {
        let inbox = folder.appending(path: "Inbox")
        try FileManager.default.createDirectory(at: inbox, withIntermediateDirectories: true)
        for name in names {
            try "\(name)\n".write(
                to: inbox.appending(path: name), atomically: true, encoding: .utf8)
        }
        return inbox
    }

    func testAFolderAsksHowItGoesInAndEachEntryCanBeChosen() async throws {
        let intake = IntakeModel(archive: archive)
        let inbox = try inbox(["one.txt", "two.txt"])
        await intake.add([inbox])
        XCTAssertEqual(intake.adding?.urls, [inbox], "asks first")
        XCTAssertEqual(intake.adding?.hasFolders, true)
        XCTAssertTrue(intake.queue.waiting.isEmpty)

        await intake.add([inbox], as: .each)
        XCTAssertNil(intake.adding)
        XCTAssertEqual(intake.queue.waiting.count, 2, "each entry as its own item")
    }

    func testAFolderCanGoInWhole() async throws {
        let intake = IntakeModel(archive: archive)
        let inbox = try inbox(["one.txt", "two.txt"])
        await intake.add([inbox], as: .whole)
        XCTAssertEqual(intake.queue.waiting.count, 1, "the folder as one item")
        XCTAssertEqual(intake.queue.waiting.first?.path.hasSuffix("/Inbox"), true)
    }

    func testEntriesAreMovedToTheFrontAndRemoved() async throws {
        let intake = IntakeModel(archive: archive)
        let inbox = try inbox(["one.txt", "two.txt"])
        let files = [inbox.appending(path: "one.txt"), inbox.appending(path: "two.txt")]
        await intake.add(files)
        XCTAssertEqual(intake.adding?.hasFolders, false, "files ask only for a note")
        await intake.add(files, as: .decide)
        let last = try XCTUnwrap(intake.queue.waiting.last)
        await intake.moveToFront(last)
        XCTAssertEqual(intake.queue.waiting.first?.id, last.id)
        await intake.remove(last)
        XCTAssertFalse(intake.queue.waiting.contains { $0.id == last.id })
        XCTAssertEqual(intake.queue.waiting.count, 1)
        XCTAssertNil(archive.lastError)
    }
}

@MainActor
final class WindowParityTests: XCTestCase {
    private var sample: SampleWindow!

    override func setUp() async throws {
        sample = try await SampleWindow()
    }

    override func tearDown() async throws {
        sample.remove()
    }

    func testHistoryLoadsMoreAndFiltersTheLibrariansChanges() async throws {
        let history = try XCTUnwrap(sample.model.historyScreen)
        _ = try await sample.archive.set("ingest.originals", to: "move")
        _ = try await sample.archive.set("ingest.originals", to: "keep")
        sample.archive.historyLimit = 1
        await history.refresh()
        XCTAssertEqual(history.changes.count, 1)
        XCTAssertTrue(history.canLoadMore)
        await history.loadMore()
        XCTAssertGreaterThan(history.changes.count, 1)
        await history.loadAll()
        XCTAssertFalse(history.canLoadMore)

        history.librarianOnly = true
        XCTAssertTrue(history.changes.allSatisfy { $0.agent == "librarian" })
        XCTAssertLessThan(history.changes.count, history.readCount)
    }

    func testACardsBodyIsEditedWithItsFields() async throws {
        let item = try XCTUnwrap(sample.model.items.first { $0.title == "Desk lamp parts list" })
        let edit = CardEditModel(archive: sample.archive, target: .item(item.id))
        await edit.load()
        let body = try XCTUnwrap(edit.body)
        edit.body = "Bought the strip at the market.\n\n" + body
        XCTAssertTrue(edit.hasChanges)
        let saved = await edit.save()
        XCTAssertTrue(saved)
        XCTAssertFalse(edit.hasChanges)
        XCTAssertTrue(edit.body?.hasPrefix("Bought the strip") == true)
        XCTAssertEqual(sample.archive.history.first?.summary.hasSuffix("notes"), true)
        let text = try String(contentsOf: sample.root.appending(path: item.card), encoding: .utf8)
        XCTAssertTrue(text.contains("Bought the strip at the market."))
    }

    func testABodySavedAnotherWayMeanwhileIsKeptOrChosen() async throws {
        let item = try XCTUnwrap(sample.model.items.first { $0.title == "Desk lamp parts list" })
        let edit = CardEditModel(archive: sample.archive, target: .item(item.id))
        await edit.load()
        let opened = try XCTUnwrap(edit.body)
        // Saved another way while the editor is open, on the same line as the owner's edit.
        let theirs = "Their note.\n\n" + opened
        _ = try await sample.archive.edit(item: item.id, base: opened, body: theirs)
        edit.body = "My note.\n\n" + opened
        let saved = await edit.save()
        XCTAssertFalse(saved, "nothing is overwritten")
        let clash = try XCTUnwrap(edit.bodyClash)
        XCTAssertEqual(clash.current, theirs)
        edit.keepBothBodies()
        XCTAssertTrue(edit.body?.contains("Their note.") == true)
        XCTAssertTrue(edit.body?.contains("My note.") == true)
        let savedBoth = await edit.save()
        XCTAssertTrue(savedBoth)
        let text = try String(contentsOf: sample.root.appending(path: item.card), encoding: .utf8)
        XCTAssertTrue(text.contains("Their note.") && text.contains("My note."))
    }

    func testEveryFieldTheOwnerSetsIsEdited() async throws {
        let book = try XCTUnwrap(
            sample.model.items.first { $0.title.hasPrefix("On the Origin of Species") })
        let lamp = try XCTUnwrap(sample.model.items.first { $0.title == "Desk lamp parts list" })
        let edit = CardEditModel(archive: sample.archive, target: .item(lamp.id))
        await edit.load()
        XCTAssertTrue(edit.takesSnapshots)
        XCTAssertFalse(edit.needsLocation)
        XCTAssertTrue(edit.linkTypes.contains("supersedes"))
        edit.fields?.links.append(CardLink(linkType: "nonsense", item: book.id, note: nil))
        let refused = await edit.save()
        XCTAssertFalse(refused)
        XCTAssertFalse(edit.problems(about: "link").isEmpty)
        edit.fields?.links[1].linkType = "related"
        edit.fields?.snapshotOnReturnOnly = true
        let saved = await edit.save()
        XCTAssertTrue(saved)
        XCTAssertEqual(edit.fields?.links.last?.item, book.id)
        XCTAssertEqual(edit.fields?.snapshotOnReturnOnly, true)

        let place = CardEditModel(archive: sample.archive, target: .item(book.id))
        await place.load()
        XCTAssertTrue(place.needsLocation)
        place.fields?.location = nil
        let empty = await place.save()
        XCTAssertFalse(empty)
        XCTAssertFalse(place.problems(about: "location").isEmpty)
        place.fields?.location = "Study, top shelf"
        let moved = await place.save()
        XCTAssertTrue(moved)
        let text = try String(contentsOf: sample.root.appending(path: book.card), encoding: .utf8)
        XCTAssertTrue(text.contains("Study, top shelf"))
    }

    func testAFieldSavedAnotherWayMeanwhileIsKeptOrChosen() async throws {
        let item = try XCTUnwrap(sample.model.items.first { $0.title == "Desk lamp parts list" })
        let edit = CardEditModel(archive: sample.archive, target: .item(item.id))
        await edit.load()
        let opened = try XCTUnwrap(edit.original)
        // Saved another way while the editor is open: a new title.
        var theirs = opened
        theirs.title = "Lamp parts"
        _ = try await sample.archive.edit(item: item.id, base: opened, fields: theirs)
        // A summary of the owner's is saved beside it.
        edit.fields?.summary = "Every part, with where it came from."
        let savedSummary = await edit.save()
        XCTAssertTrue(savedSummary)
        XCTAssertEqual(edit.fields?.title, "Lamp parts")
        // A title of the owner's, against the card as it was opened, is theirs to choose.
        let other = CardEditModel(archive: sample.archive, target: .item(item.id))
        await other.load()
        let reopened = try XCTUnwrap(other.original)
        var parts = reopened
        parts.title = "Parts"
        _ = try await sample.archive.edit(item: item.id, base: reopened, fields: parts)
        other.fields?.title = "Desk lamp bill of materials"
        let clashed = await other.save()
        XCTAssertFalse(clashed, "nothing is overwritten")
        XCTAssertEqual(other.fieldsClash?.fields, ["title"])
        XCTAssertEqual(other.fieldsClash?.current.title, "Parts")
        other.keepMyFields()
        let kept = await other.save()
        XCTAssertTrue(kept)
        XCTAssertEqual(other.fields?.title, "Desk lamp bill of materials")
    }

    func testAProposalHasNoBodyToEdit() async throws {
        let edit = CardEditModel(archive: sample.archive, target: .proposal(sampleProposal))
        await edit.load()
        XCTAssertNotNil(edit.fields)
        XCTAssertNil(edit.body)
    }

    func testRelatedItemsSayWhyNoneAreNearInMeaning() async throws {
        let related = await RelatedItems.read(sampleNote, from: sample.archive)
        let found = try XCTUnwrap(related)
        XCTAssertTrue(found.near.isEmpty, "the sample has no meaning index")
        XCTAssertNotNil(found.whyNone)
    }

    func testActivityTakesADateRange() async throws {
        let discover = try XCTUnwrap(sample.model.discover)
        discover.activityFrom = try XCTUnwrap(
            Calendar.current.date(from: DateComponents(year: 2026, month: 10, day: 1, hour: 12)))
        discover.activityTo = try XCTUnwrap(
            Calendar.current.date(from: DateComponents(year: 2026, month: 10, day: 31, hour: 12)))
        await discover.refresh(.activity)
        XCTAssertEqual(discover.activity?.from, "2026-10-01")
        XCTAssertEqual(discover.activity?.to, "2026-10-31")
    }

    func testWanderingStartsFromTheItemChosen() async throws {
        let discover = try XCTUnwrap(sample.model.discover)
        sample.model.wander(from: sampleNote)
        XCTAssertEqual(sample.model.screen, .discover(.wander))
        XCTAssertTrue(discover.isStartingWalk)
        await discover.refresh(.wander)
        for _ in 0..<100 where discover.currentStep == nil {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertNil(discover.problem)
        XCTAssertEqual(discover.steps.count, 1, "showing the page started no other walk")
        XCTAssertNotEqual(discover.currentStep?.item.id, sampleNote, "it moves on from there")
    }

    func testStatusHasRecentAndTheDesksSuggestions() async throws {
        _ = try await sample.archive.set("ingest.originals", to: "move")
        let status = try XCTUnwrap(sample.archive.status)
        XCTAssertTrue(status.hasHistory)
        XCTAssertFalse(status.recent.isEmpty, "the change is recent")
    }
}
