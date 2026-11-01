// Adding from the window itself (#439): the add sheet's note, File → Add Web Address… and Add
// Note…, pasting onto the window, and saying so when another device is the writer.

import AppKit
import CarrelKit
import XCTest

@testable import Carrel

final class AddFromWindowTests: SampleCopyTestCase {
    private func file(_ name: String) throws -> URL {
        let url = folder.appending(path: name)
        try "A file.".write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    /// A pasteboard of the test's own, so the owner's clipboard is never touched.
    private func pasteboard() -> NSPasteboard {
        let board = NSPasteboard(name: .init("carrel-test-\(UUID().uuidString)"))
        board.clearContents()
        addTeardownBlock { board.releaseGlobally() }
        return board
    }

    func testFilesAreAddedWithTheOwnersNote() async throws {
        let intake = IntakeModel(archive: archive)
        let list = try file("shopping list.txt")
        await intake.add([list])
        XCTAssertEqual(intake.adding?.urls, [list], "the sheet asks for a note first")
        await intake.add([list], as: .decide, note: "For the lamp")
        XCTAssertNil(intake.adding)
        XCTAssertEqual(intake.queue.waiting.count, 1)
        XCTAssertEqual(intake.report?.title, "Added “shopping list.txt” to the Intake Queue")
    }

    func testAWebAddressIsAddedByItsTitle() async throws {
        let intake = IntakeModel(archive: archive)
        intake.writing = Writing(kind: .webAddress)
        await intake.addWebAddress(
            " https://example.org/prc ", title: "Phase response curves", note: "")
        XCTAssertNil(intake.writing, "the sheet closes")
        XCTAssertEqual(intake.report?.title, "Added “Phase response curves” to the Intake Queue")
        XCTAssertEqual(intake.queue.waiting.first?.path, "https://example.org/prc")

        await intake.addWebAddress("example.org", title: "", note: "")
        XCTAssertEqual(intake.report?.title, "Couldn’t Add to the Archive")
    }

    func testANoteIsAddedAsAnItemOfItsOwn() async throws {
        let intake = IntakeModel(archive: archive)
        await intake.addNote("Ring the framer about the print.", title: "", note: "")
        XCTAssertEqual(
            intake.report?.title,
            "Added “Ring the framer about the print.md” to the Intake Queue")
        let path = try XCTUnwrap(intake.queue.waiting.first?.path)
        XCTAssertTrue(path.contains("/config/notes/"), path)
        XCTAssertEqual(
            try String(contentsOfFile: path, encoding: .utf8), "Ring the framer about the print.\n")
    }

    func testPastingFilesAWebAddressOrTextOpensWhatAddsIt() async throws {
        let intake = IntakeModel(archive: archive)
        let board = pasteboard()
        let empty = await intake.paste(from: board)
        XCTAssertFalse(empty)

        let list = try file("list.txt")
        board.writeObjects([list as NSURL])
        await intake.paste(from: board)
        XCTAssertEqual(intake.adding?.urls.map(\.lastPathComponent), ["list.txt"])

        board.clearContents()
        board.setString("https://example.org/a", forType: .string)
        await intake.paste(from: board)
        XCTAssertEqual(intake.writing?.kind, .webAddress)
        XCTAssertEqual(intake.writing?.text, "https://example.org/a")

        board.clearContents()
        board.setString("Ideas for the lamp\nA frosted diffuser", forType: .string)
        await intake.paste(from: board)
        XCTAssertEqual(intake.writing?.kind, .note)
        XCTAssertTrue(intake.queue.waiting.isEmpty, "nothing is added until the owner says")

        XCTAssertTrue(IntakeModel.isWebAddress("HTTPS://example.org"))
        XCTAssertFalse(IntakeModel.isWebAddress("https://"))
        XCTAssertFalse(IntakeModel.isWebAddress("see https://example.org"))
    }

    func testAnotherWriterSendsWhatsAddedToThisMacsInbox() async throws {
        let lock: [String: Any] = [
            "token": "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b",
            "person": "01928f39-eeee-7fff-8000-000000000005",
            "device": "01928f3a-0000-7000-8000-00000000d002",
            "purpose": "ingest", "program": "carrel 0.1.0",
            "acquired": "2026-10-29T14:02:11-07:00", "expires": "2999-01-01T00:00:00Z",
        ]
        try JSONSerialization.data(withJSONObject: lock).write(
            to: folder.appending(path: "archive/_System/writer.lock"))
        let intake = IntakeModel(archive: archive)
        await intake.addNote("Elsewhere", title: "", note: "")
        let report = try XCTUnwrap(intake.report)
        XCTAssertEqual(report.title, "Sent to This Mac’s Inbox")
        XCTAssertTrue(report.details?.contains("01928f3a") == true, report.details ?? "")
        XCTAssertTrue(intake.queue.waiting.isEmpty)
    }
}
