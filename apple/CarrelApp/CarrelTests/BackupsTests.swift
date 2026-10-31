// Backups, each test against its own temporary copy of the sample archive and, with Kopia, a
// backup repository in a temporary folder. The password is kept in memory, never in the
// keychain, as the Rust tests keep it in KOPIA_PASSWORD.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class BackupsTests: XCTestCase {
    func testWithoutBackupsTheScreenSaysHowToSetThemUp() async throws {
        let window = try await SampleWindow()
        defer { window.remove() }
        window.model.screen = .backups
        XCTAssertEqual(window.model.screenTitle, "Backups")
        let backups = BackupsModel(archive: window.archive, passwords: .memory)
        await backups.refresh()
        XCTAssertFalse(backups.isSetUp)
        XCTAssertEqual(backups.snapshots, [])
        if backups.state?.installed == true {
            XCTAssertTrue(backups.notice?.contains("No backups are set up") ?? false)
        }
        await backups.snapshotNow()
        XCTAssertNil(backups.report)
        XCTAssertTrue(window.archive.lastError?.message.contains("aren't set up") ?? false)
    }

    func testANewRepositoryNeedsThePasswordTwice() throws {
        var setup = BackupSetup(folder: URL(filePath: "/Volumes/Backup drive"), isNew: true)
        XCTAssertNotNil(setup.problem)
        setup.password = "correct horse"
        XCTAssertEqual(setup.problem, "The passwords don’t match.")
        setup.confirmation = "correct horse"
        XCTAssertNil(setup.problem)
        let connect = BackupSetup(folder: setup.folder, isNew: false, password: "correct horse")
        XCTAssertNil(connect.problem, "connecting asks once")
    }

    func testBackupsAreSetUpTakenVerifiedAndRestoredAndSayWhenTheDriveIsMissing() async throws {
        let window = try await SampleWindow()
        defer { window.remove() }
        let backups = BackupsModel(archive: window.archive, passwords: .memory)
        await backups.refresh()
        try XCTSkipUnless(
            backups.state?.installed == true
                || ProcessInfo.processInfo.environment["CARREL_REQUIRE_KOPIA"] != nil,
            "Kopia isn't installed")

        let drive = window.folder.appending(path: "Backup drive")
        try FileManager.default.createDirectory(at: drive, withIntermediateDirectories: true)
        backups.choose(folder: drive)
        XCTAssertEqual(backups.setup?.isNew, true)
        backups.setup?.password = "test-\(UUID().uuidString)"
        backups.setup?.confirmation = backups.setup?.password ?? ""
        await backups.setUp()
        XCTAssertNil(window.archive.lastError?.message)
        XCTAssertNil(backups.setup, "the sheet closes")
        XCTAssertTrue(backups.report?.contains("Created") ?? false, backups.report ?? "")
        XCTAssertTrue(backups.report?.contains("only while Carrel is open") ?? false)
        XCTAssertTrue(backups.isSetUp)
        XCTAssertNil(backups.notice)
        XCTAssertEqual(backups.state?.description, "Backup drive")

        backups.choose(folder: drive)
        XCTAssertEqual(backups.setup?.isNew, false, "a repository is there now")
        backups.setup = nil

        // History lists a snapshot with a change, so there's one to record it with.
        let card = window.root.appending(path: "Projects/Hardware/lamp-build.md")
        try (String(contentsOf: card, encoding: .utf8) + "\nSanded the diffuser.\n")
            .write(to: card, atomically: true, encoding: .utf8)
        await backups.snapshotNow()
        XCTAssertTrue(backups.report?.hasPrefix("Snapshot") ?? false, backups.report ?? "")
        XCTAssertEqual(backups.snapshots.count, 1)
        XCTAssertEqual(backups.selection, backups.snapshots.first?.id)
        XCTAssertEqual(backups.snapshots.first?.summary, "Snapshot on request")

        await backups.verify()
        XCTAssertTrue(backups.report?.contains("check out") ?? false, backups.report ?? "")

        let item = try XCTUnwrap(
            window.model.items.first { $0.card == "Projects/Hardware/parts-list.csv.md" })
        await backups.restore(item: item.id)
        let restored = try XCTUnwrap(backups.restored, backups.report ?? "")
        XCTAssertTrue(FileManager.default.fileExists(atPath: restored.path(percentEncoded: false)))
        XCTAssertEqual(
            restored.deletingLastPathComponent().resolvingSymlinksInPath().path,
            window.folder.resolvingSymlinksInPath().path, "beside the archive")
        XCTAssertTrue(restored.lastPathComponent.hasPrefix("parts-list.csv (restored "))

        try FileManager.default.moveItem(at: drive, to: window.folder.appending(path: "unplugged"))
        await backups.refresh()
        XCTAssertEqual(backups.state?.driveMissing, true)
        XCTAssertTrue(backups.notice?.contains("isn’t connected") ?? false, backups.notice ?? "")
        XCTAssertEqual(backups.snapshots.count, 1, "listed from history without the drive")
        window.archive.lastError = nil
        await backups.snapshotNow()
        XCTAssertTrue(
            window.archive.lastError?.message.contains("is its drive connected") ?? false,
            window.archive.lastError?.message ?? "no error")
    }
}
