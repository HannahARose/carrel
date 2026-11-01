// An archive shared through iCloud Drive: Status's Sync section, a collection's Keep
// Downloaded, settling a conflicted copy, and Move to iCloud Drive…. Each test has its own copy
// of the sample archive with a configuration folder of its own, and a Trash of its own.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class SyncTests: XCTestCase {
    private var sample: SampleWindow!

    override func setUp() async throws {
        sample = try await SampleWindow()
        setenv("CARREL_TRASH_DIR", sample.folder.appending(path: "Trash").path(), 1)
    }

    override func tearDown() async throws {
        unsetenv("CARREL_TRASH_DIR")
        sample.remove()
    }

    func testStatusOfAnArchiveOutsideICloudDrive() async throws {
        let sync = SyncStatusModel(archive: sample.archive)
        await sync.load()
        XCTAssertNil(sync.problem)
        let status = try XCTUnwrap(sync.status)
        XCTAssertFalse(status.synced)
        XCTAssertNil(status.writer, "nobody is writing")
        XCTAssertTrue(status.notDownloaded.isEmpty)
        XCTAssertTrue(status.conflicts.isEmpty)
        XCTAssertNil(status.movedTo)
        XCTAssertEqual(syncWriterLockText(nil), "Free")
    }

    func testKeepDownloadedIsSetInheritedAndCleared() async throws {
        let projects = CollectionSyncModel(archive: sample.archive, path: "Projects")
        await projects.load()
        XCTAssertEqual(projects.keeps, false, "off by default")

        await projects.setKeeps(true)
        XCTAssertNil(projects.problem)
        XCTAssertEqual(projects.keeps, true)
        let hardware = CollectionSyncModel(archive: sample.archive, path: "Projects/Hardware")
        await hardware.load()
        XCTAssertEqual(hardware.keeps, true, "the collection inside follows")
        let status = try await sample.archive.syncStatus()
        XCTAssertTrue(status.keptDownloaded.contains("Projects"))
        XCTAssertEqual(
            sample.archive.history.first?.summary,
            "Set sync.keep_downloaded for Projects to on", "one change History shows")

        await projects.setKeeps(false)
        XCTAssertEqual(projects.keeps, false)
        await hardware.load()
        XCTAssertEqual(hardware.keeps, false)

        await projects.downloadNow()
        XCTAssertNil(projects.problem)
        XCTAssertEqual(projects.downloaded, 0, "it's all here already")
    }

    func testAConflictedCopyIsSettledEitherWay() async throws {
        let reading = sample.root.appending(path: "Reading")
        let card = reading.appending(path: "On the Origin of Species.md")
        let copy = reading.appending(path: "On the Origin of Species 2.md")
        try FileManager.default.copyItem(at: card, to: copy)
        let sync = SyncStatusModel(archive: sample.archive)
        await sync.load()
        let conflict = try XCTUnwrap(sync.status?.conflicts.first)
        XCTAssertEqual(conflict.copy, "Reading/On the Origin of Species 2.md")
        XCTAssertEqual(conflict.original, "Reading/On the Origin of Species.md")

        await sync.settle(conflict, keepCopy: false)
        XCTAssertNil(sync.problem)
        XCTAssertEqual(sync.status?.conflicts, [])
        XCTAssertFalse(FileManager.default.fileExists(atPath: copy.path(percentEncoded: false)))
        XCTAssertTrue(FileManager.default.fileExists(atPath: card.path(percentEncoded: false)))

        let edited = try String(contentsOf: card, encoding: .utf8) + "\nEdited on the other Mac.\n"
        try edited.write(to: copy, atomically: true, encoding: .utf8)
        await sync.load()
        let again = try XCTUnwrap(sync.status?.conflicts.first)
        await sync.settle(again, keepCopy: true)
        XCTAssertNil(sync.problem)
        XCTAssertEqual(try String(contentsOf: card, encoding: .utf8), edited)
        XCTAssertFalse(FileManager.default.fileExists(atPath: copy.path(percentEncoded: false)))
    }

    func testAnArchiveMovesAndTheOldCopySaysWhere() async throws {
        sample.model.startICloudMove()
        let move = try XCTUnwrap(sample.model.icloudMove)
        let parent = sample.folder.appending(path: "Moved", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        await move.choose(parent: parent)
        XCTAssertNil(move.problem)
        let plan = try XCTUnwrap(move.plan)
        XCTAssertGreaterThan(plan.files, 0)
        XCTAssertTrue(move.canMove)

        await move.move()
        XCTAssertNil(move.problem)
        let moved = try XCTUnwrap(move.moved)
        XCTAssertEqual(
            moved.standardizedFileURL.resolvingSymlinksInPath().path,
            parent.appending(path: "archive").standardizedFileURL.resolvingSymlinksInPath().path)
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: moved.appending(path: "ARCHIVE.md").path(percentEncoded: false)))
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: sample.root.appending(path: "ARCHIVE.md").path(percentEncoded: false)),
            "the old copy stays")
        XCTAssertFalse(move.canMove)

        let old = try await sample.archive.syncStatus()
        XCTAssertNotNil(old.movedTo, "the old copy says where it went")

        await move.choose(parent: parent)
        XCTAssertNotNil(move.problem, "something is there now")
    }
}
