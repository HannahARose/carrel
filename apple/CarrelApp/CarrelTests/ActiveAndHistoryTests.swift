// Active and History's view models, each test against its own temporary copy of the sample
// archive with a configuration folder of its own, so nothing outside the copy is written.

import CarrelKit
import XCTest

@testable import Carrel

/// A temporary copy of the sample archive with its history started.
@MainActor
final class SampleCopy {
    let folder: URL
    let archive: ArchiveModel

    var root: URL { folder.appending(path: "archive") }

    init() throws {
        folder = FileManager.default.temporaryDirectory
            .appending(path: "carrel-app-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: sampleArchive, to: folder.appending(path: "archive"))
        let handle = try ArchiveHandle.open(
            folder.appending(path: "archive"),
            options: .isolated(config: folder.appending(path: "config")))
        try handle.startHistory()
        archive = ArchiveModel(handle: handle)
    }

    func remove() {
        try? FileManager.default.removeItem(at: folder)
    }

    func item(titled title: String) throws -> String {
        try XCTUnwrap(try archive.handle.items().first { $0.title == title }).id
    }

    /// A folder inside the copy.
    func makeFolder(_ name: String) throws -> URL {
        let url = folder.appending(path: name)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}

@MainActor
final class ActiveScreenModelTests: XCTestCase {
    private var sample: SampleCopy!
    private var model: ActiveScreenModel!
    private var changes = 0

    override func setUp() async throws {
        sample = try SampleCopy()
        changes = 0
        model = ActiveScreenModel(
            archive: sample.archive, checkoutFolder: sample.folder.appending(path: "Desktop")
        ) { [weak self] in self?.changes += 1 }
        await model.refresh()
    }

    override func tearDown() async throws {
        sample.remove()
    }

    /// A desk item with two pending entries, newest first.
    private func deskItemWithEntries() async throws -> ActiveItem {
        let plans = try sample.makeFolder("Plans")
        await model.addToDesk(plans)
        XCTAssertNil(model.problem)
        let item = try XCTUnwrap(model.selectedItem)
        let path =
            item.card.hasPrefix("/")
            ? URL(filePath: item.card) : sample.root.appending(path: item.card)
        var text = try String(contentsOf: path, encoding: .utf8)
        let entries = """
            > [!pending] 2099-01-03 · 1h 10m · auto-generated
            > Drew the second floor.

            > [!pending] 2099-01-02 · 30m · auto-generated
            > Measured the rooms.

            """
        if let end = text.range(of: "<!-- /carrel:history -->") {
            text.insert(contentsOf: entries, at: end.lowerBound)
        } else {
            text += "\n<!-- carrel:history -->\n## History\n\n\(entries)<!-- /carrel:history -->\n"
        }
        try text.write(to: path, atomically: true, encoding: .utf8)
        await model.refresh()
        return try XCTUnwrap(model.items.first { $0.id == item.id })
    }

    func testListsTheSamplesCheckoutWithItsEntry() throws {
        let item = try XCTUnwrap(model.items.first { $0.title.contains("scillator") })
        XCTAssertEqual(item.kind, .checkedOut)
        XCTAssertEqual(item.pending.count, 1)
        XCTAssertEqual(item.pending.first?.heading, "2026-10-28 · 2h 10m")
        XCTAssertEqual(model.pendingCount, 1)
        model.selection = [.item(item.id)]
        XCTAssertTrue(model.canReturn)
        XCTAssertTrue(model.canEdit)
        XCTAssertEqual(model.decidedEntries, [1])
    }

    func testCheckingOutAsksWhereAndWhenItsDue() async throws {
        let id = try sample.item(titled: "Desk lamp parts list")
        model.startCheckout(of: id, titled: "Desk lamp parts list")
        var draft = try XCTUnwrap(model.checkingOut)
        XCTAssertEqual(
            draft.folder, sample.folder.appending(path: "Desktop"), "the Desktop unless changed")
        draft.folder = try sample.makeFolder("Work")
        draft.due = try XCTUnwrap(
            Calendar.current.date(from: DateComponents(year: 2026, month: 12, day: 1)))
        XCTAssertEqual(draft.dueString, "2026-12-01")

        await model.checkOut(draft)
        XCTAssertNil(model.problem)
        XCTAssertNil(model.checkingOut)
        XCTAssertEqual(changes, 1)
        let item = try XCTUnwrap(model.selectedItem)
        XCTAssertEqual(item.id, id)
        XCTAssertEqual(item.kind, .checkedOut)
        XCTAssertEqual(item.due, "2026-12-01")
        XCTAssertTrue(item.working.hasPrefix(draft.folder.path(percentEncoded: false)))
        XCTAssertTrue(model.canReturn)
        XCTAssertFalse(model.canDecide, "no pending entries yet")
        XCTAssertEqual(model.workingCopy(of: item).path(percentEncoded: false), item.working)
    }

    func testReturningShowsWhatChangedFirst() async throws {
        let id = try sample.item(titled: "Desk lamp parts list")
        await model.checkOut(
            CheckoutDraft(item: id, title: "", folder: try sample.makeFolder("Work"), due: nil))
        let item = try XCTUnwrap(model.selectedItem)
        try "part,source\nLED strip,shop\n".write(
            toFile: item.working, atomically: true, encoding: .utf8)

        await model.askToReturn()
        let preview = try XCTUnwrap(model.returning, "asks first")
        XCTAssertEqual(preview.changes.changed.count, 1)
        XCTAssertFalse(preview.entry.isEmpty, "shows the entry it adds")
        XCTAssertTrue(model.items.contains { $0.id == id }, "nothing returned yet")

        await model.confirmReturn(preview)
        XCTAssertNil(model.problem)
        XCTAssertNil(model.returning)
        XCTAssertFalse(model.items.contains { $0.id == id })
        XCTAssertTrue(model.selection.isEmpty)
        XCTAssertEqual(sample.archive.history.first?.operation, "return")
    }

    func testCancellingAsksAndLeavesTheArchiveCopy() async throws {
        let id = try sample.item(titled: "Desk lamp parts list")
        await model.checkOut(
            CheckoutDraft(item: id, title: "", folder: try sample.makeFolder("Work"), due: nil))
        model.askToCancel()
        let item = try XCTUnwrap(model.cancelling)
        XCTAssertTrue(model.items.contains { $0.id == id }, "nothing cancelled yet")
        await model.confirmCancel(item)
        XCTAssertNil(model.problem)
        XCTAssertNil(model.cancelling)
        XCTAssertFalse(model.items.contains { $0.id == id })
    }

    func testTheDeskTakesAFolderAndCantBeReturned() async throws {
        let plans = try sample.makeFolder("Plans")
        await model.addToDesk(plans)
        XCTAssertNil(model.problem)
        let item = try XCTUnwrap(model.selectedItem)
        XCTAssertEqual(item.kind, .desk)
        XCTAssertFalse(model.canReturn)
        await model.askToReturn()
        XCTAssertNil(model.returning)
        model.askToCancel()
        XCTAssertNil(model.cancelling)

        await model.removeFromDesk()
        XCTAssertNil(model.problem)
        XCTAssertFalse(model.items.contains { $0.id == item.id })
    }

    func testEntriesAreCountedForTheBadgeAndApproved() async throws {
        let before = model.pendingCount
        let item = try await deskItemWithEntries()
        XCTAssertEqual(item.pending.count, 2)
        XCTAssertEqual(model.pendingCount, before + 2)

        model.selection = [.entry(item.id, 2)]
        XCTAssertEqual(model.selectedItem?.id, item.id)
        XCTAssertEqual(model.decidedEntries, [2])
        XCTAssertFalse(model.canCombine, "combining needs two")
        await model.decide(.approve)
        XCTAssertNil(model.problem)
        XCTAssertEqual(model.pendingCount, before + 1)
        XCTAssertEqual(model.selection, [.item(item.id)])
        XCTAssertEqual(model.selectedItem?.pending.first?.lines, ["Drew the second floor."])
    }

    func testChoosingTheItemDecidesAllItsEntries() async throws {
        let item = try await deskItemWithEntries()
        model.selection = [.item(item.id)]
        XCTAssertEqual(model.decidedEntries, [1, 2])
        XCTAssertTrue(model.canCombine)
        XCTAssertFalse(model.canEdit, "which one to edit isn't clear")
        await model.decide(.combine)
        XCTAssertNil(model.problem)
        XCTAssertEqual(model.selectedItem?.pending.count, 1)
        XCTAssertTrue(model.canEdit, "with only one, there's no doubt which")

        await model.decide(.drop)
        XCTAssertNil(model.problem)
        XCTAssertEqual(model.selectedItem?.pending.count, 0)
        XCTAssertFalse(model.canDecide)
    }

    func testEditingAnEntryKeepsItPending() async throws {
        let item = try await deskItemWithEntries()
        model.selection = [.entry(item.id, 1)]
        model.startEditing()
        var draft = try XCTUnwrap(model.editing)
        XCTAssertEqual(draft.text, "Drew the second floor.")
        draft.text = "Drew the second floor.\nMy notes: check the stairs."
        await model.save(draft)
        XCTAssertNil(model.problem)
        XCTAssertNil(model.editing)
        let edited = try XCTUnwrap(model.items.first { $0.id == item.id })
        XCTAssertEqual(
            edited.pending.first?.lines, ["Drew the second floor.", "My notes: check the stairs."])
        XCTAssertEqual(edited.pending.count, 2)
    }

    func testChoosingRowsOfTwoItemsChoosesNeither() async throws {
        let item = try await deskItemWithEntries()
        model.selection = [.item(item.id), .item("someone-else")]
        XCTAssertNil(model.selectedItem)
        XCTAssertFalse(model.canDecide)
        await model.refresh()
        XCTAssertEqual(model.selection, [.item(item.id)], "rows that went away are let go")
    }

    func testTheCoresRefusalIsShown() async throws {
        await model.checkOut(
            CheckoutDraft(item: "nothing-like-it", title: "", folder: sample.folder, due: nil))
        XCTAssertNotNil(model.problem)
        XCTAssertEqual(changes, 0)
    }

    func testEntryHeadingsShowTheirLength() {
        func entry(_ minutes: UInt64?) -> CardHistoryEntry {
            CardHistoryEntry(date: "2026-10-29", minutes: minutes, pending: true, lines: [])
        }
        XCTAssertEqual(entry(nil).heading, "2026-10-29")
        XCTAssertEqual(entry(45).heading, "2026-10-29 · 45m")
        XCTAssertEqual(entry(120).heading, "2026-10-29 · 2h")
        XCTAssertEqual(entry(130).heading, "2026-10-29 · 2h 10m")
    }
}

@MainActor
final class HistoryScreenModelTests: XCTestCase {
    private var sample: SampleCopy!
    private var model: HistoryScreenModel!
    private var changes = 0

    override func setUp() async throws {
        sample = try SampleCopy()
        changes = 0
        model = HistoryScreenModel(archive: sample.archive) { [weak self] in self?.changes += 1 }
        await model.refresh()
    }

    override func tearDown() async throws {
        sample.remove()
    }

    func testListsChangesNewestFirst() async throws {
        XCTAssertEqual(model.changes.count, 1, "starting the history")
        try await sample.archive.set("ingest.originals", to: "keep")
        XCTAssertEqual(model.changes.count, 2)
        XCTAssertTrue(model.changes[0].summary.contains("ingest.originals"))
        XCTAssertGreaterThanOrEqual(model.changes[0].time, model.changes[1].time)
    }

    func testUndoAsksFirstThenUndoesAsANewChange() async throws {
        try await sample.archive.set("ingest.originals", to: "keep")
        model.selectedID = model.changes.first?.id
        model.askToUndo()
        let change = try XCTUnwrap(model.undoing, "asks first")
        XCTAssertEqual(
            sample.archive.settings.first { $0.key == "ingest.originals" }?.value, "keep",
            "nothing undone yet")

        await model.undo(change)
        XCTAssertNil(model.problem)
        XCTAssertNil(model.undoing)
        XCTAssertEqual(changes, 1)
        XCTAssertEqual(model.changes.count, 3)
        XCTAssertEqual(model.selected?.reverts, change.id)
        XCTAssertEqual(
            sample.archive.settings.first { $0.key == "ingest.originals" }?.value, "move")
    }

    func testTheFirstChangeCantBeUndone() async throws {
        let first = try XCTUnwrap(model.changes.last)
        await model.undo(first)
        XCTAssertNotNil(model.problem)
        XCTAssertEqual(changes, 0)
    }

    func testTheSheetSaysAReturnsFilesComeBackOnlyFromBackups() async throws {
        let id = try sample.item(titled: "Desk lamp parts list")
        let out = try await sample.archive.checkOut(id, to: try sample.makeFolder("Work"))
        try "changed\n".write(toFile: out.working, atomically: true, encoding: .utf8)
        try await sample.archive.returnItem(id)
        let returned = try XCTUnwrap(model.changes.first)
        XCTAssertEqual(returned.operation, "return")

        let explanation = UndoExplanation(returned)
        XCTAssertTrue(explanation.keeps.contains("cards, not files"))
        XCTAssertTrue(explanation.keeps.contains("backup"))
        let command = try XCTUnwrap(explanation.command)
        XCTAssertTrue(command.hasPrefix("carrel backup restore "))
        XCTAssertTrue(command.contains("--at \(returned.id.prefix(12))"))

        let checkout = try XCTUnwrap(model.changes.first { $0.operation == "checkout" })
        XCTAssertNil(UndoExplanation(checkout).command)
        XCTAssertTrue(UndoExplanation(checkout).restores.contains("new change"))
    }
}
