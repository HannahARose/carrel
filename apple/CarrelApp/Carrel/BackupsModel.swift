// What the Backups screen shows and does, as the interactive mode's Backups screen and
// `carrel backup` do: whether backups are set up, the snapshots taken with each change (from
// history, so they're listed without the drive), and setting up, taking, checking and
// restoring from them through ArchiveModel.

import CarrelKit
import Foundation
import Observation

/// Where the repository's password is kept once setup has it.
enum BackupPasswords {
    /// In this Mac's keychain, by Kopia, as the CLI does: the app's.
    case keychain
    /// In memory only, passed to each call, as `KOPIA_PASSWORD` is for the CLI: the tests',
    /// so they never write the keychain.
    case memory
}

/// Setting up a repository: the folder chosen, then its password.
struct BackupSetup: Equatable {
    var folder: URL
    /// Whether the folder holds no repository yet, so the password is asked for twice.
    var isNew: Bool
    var description = ""
    var password = ""
    var confirmation = ""

    /// Why the password can't be used yet, if it can't.
    var problem: String? {
        if password.isEmpty { return "Choose a password." }
        if isNew, password != confirmation { return "The passwords don’t match." }
        return nil
    }
}

@MainActor
@Observable
final class BackupsModel {
    let archive: ArchiveModel
    let passwords: BackupPasswords

    /// Where backups stand, once read.
    private(set) var state: BackupsState?
    /// The snapshot the inspector shows: its change's identifier.
    var selection: Change.ID?
    /// The setup sheet's, while it's open.
    var setup: BackupSetup?
    /// What's under way, such as "Taking a snapshot…".
    private(set) var working: String?
    /// What the last operation did, in a sentence.
    private(set) var report: String?
    /// Where the last restore put the item, to show in Finder.
    private(set) var restored: URL?
    /// The password, when it's kept in memory rather than the keychain.
    private var password: String?

    /// How much of the files' contents Verify reads back unless the owner chooses, as
    /// `carrel backup verify` does without `--percent`.
    static let defaultVerifyPercent: UInt8 = 1
    /// Whether the owner is choosing how much Verify reads back.
    var choosingVerify = false

    init(archive: ArchiveModel, passwords: BackupPasswords = .keychain) {
        self.archive = archive
        self.passwords = passwords
    }

    /// The snapshots, newest first, each with the change it was taken with.
    var snapshots: [Change] { state?.snapshots ?? [] }

    var selected: Change? {
        selection.flatMap { id in snapshots.first { $0.id == id } }
    }

    var isSetUp: Bool { state?.setUpHere == true }

    /// What the screen says about where backups stand, when there's something to say.
    var notice: String? {
        guard let state else { return nil }
        if !state.installed {
            return
                "Kopia, which Carrel backs up with, isn’t installed. Install it with “brew install kopia”, then try again."
        }
        if state.driveMissing {
            let path = state.repository ?? "the backup repository"
            return
                "The backup drive isn’t connected (\(path)). The snapshots below are read from history; connect the drive to take, check or restore from them."
        }
        if !state.setUpHere, let description = state.description {
            return
                "This archive backs up to “\(description)”, but this Mac isn’t connected to it yet. Set up backups and choose its folder to connect."
        }
        if !state.setUpHere {
            return
                "No backups are set up. Choose a folder, ideally on an external drive, to keep snapshots of the archive’s contents."
        }
        return nil
    }

    /// Reads where backups stand again.
    func refresh() async {
        guard let state = try? await archive.backups() else { return }
        self.state = state
        if let selection, !state.snapshots.contains(where: { $0.id == selection }) {
            self.selection = nil
        }
    }

    // MARK: - Setting up

    /// Starts setting up with `folder`: a new repository there, or the one already there.
    func choose(folder: URL) {
        setup = BackupSetup(folder: folder, isNew: !isBackupRepository(folder))
    }

    /// Sets up the repository the sheet describes, keeping the password as `passwords` says.
    func setUp() async {
        guard let setup, setup.problem == nil else { return }
        let description = setup.description.trimmingCharacters(in: .whitespacesAndNewlines)
        let kept = passwords == .keychain
        let done = await run("Setting up backups…") {
            try await $0.setUpBackups(
                in: setup.folder, password: setup.password,
                description: description.isEmpty ? nil : description, keepPassword: kept)
        }
        guard let done else { return }
        if !kept { password = setup.password }
        self.setup = nil
        report = Self.describe(done, folder: setup.folder)
    }

    // MARK: - Snapshots

    /// Takes a snapshot of the whole archive now.
    func snapshotNow() async {
        let password = password
        guard
            let taken = await run(
                "Taking a snapshot…", { try await $0.snapshotNow(password: password) })
        else { return }
        report = Self.describe(taken)
        if taken.commit != nil { selection = snapshots.first?.id }
    }

    /// Checks every snapshot's structure, reading back `percent`% of the files (from 1 to
    /// 100), as `carrel backup verify --percent` does.
    func verify(percent: UInt8 = defaultVerifyPercent) async {
        choosingVerify = false
        let percent = min(max(percent, 1), 100)
        let password = password
        guard
            let found = await run(
                "Checking the backups…",
                { try await $0.verifyBackups(percent: percent, password: password) })
        else { return }
        report = "The backups check out (reading back \(percent)% of files): \(found)"
    }

    /// Restores the item `id` as it was in the chosen snapshot, or the latest, as `place`,
    /// which mustn't exist yet, or into a new folder beside the archive when it's `nil`, as
    /// `carrel backup restore --to` does.
    func restore(item id: String, to place: URL? = nil) async {
        let password = password
        let at = selection
        guard
            let done = await run(
                "Restoring…",
                { try await $0.restore(item: id, at: at, to: place, password: password) })
        else { return }
        restored = URL(filePath: done.to)
        report =
            "Restored \(done.path) as of “\(done.change.summary)” (\(done.change.when)) to \(done.to). Nothing in the archive changed; compare, then move it in yourself if you want it."
    }

    /// Runs `work` with `title` showing, then reads where backups stand again. Errors reach
    /// ArchiveModel's `lastError`, with the core's message, for the window's alert.
    private func run<T>(_ title: String, _ work: (ArchiveModel) async throws -> T) async -> T? {
        working = title
        report = nil
        restored = nil
        defer { working = nil }
        let result = try? await work(archive)
        await refresh()
        return result
    }

    // MARK: - Words

    static func describe(_ setUp: BackupsSetUp, folder: URL) -> String {
        var text =
            setUp.created
            ? "Created the backup repository “\(setUp.description)” at \(folder.path(percentEncoded: false))."
            : "Connected to the backup repository “\(setUp.description)”."
        text +=
            setUp.keptInKeychain
            ? " Kopia keeps the password in this Mac’s keychain."
            : " The password is kept only while Carrel is open."
        if setUp.sameDisk {
            text +=
                " It’s on the same disk as the archive: that guards against mistakes, but not against losing the disk; an external drive is safer."
        }
        return text + " Each approval batch now takes a snapshot."
    }

    static func describe(_ taken: SnapshotTaken) -> String {
        var text = "Snapshot \(taken.snapshot) taken"
        if let files = taken.files, let size = taken.size {
            let bytes = ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)
            text += ": \(files) files, \(bytes)"
        }
        text += "."
        if taken.commit == nil {
            text +=
                " Nothing had changed since the last change, so history doesn’t list it separately."
        }
        return text
    }
}
