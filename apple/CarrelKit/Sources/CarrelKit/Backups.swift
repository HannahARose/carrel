// Backups through ArchiveModel, as `carrel backup` does them: setting up a repository, the
// snapshots from history, taking one, verifying, and restoring an item beside the archive.
//
// Kopia keeps the repository's password in this Mac's keychain once setup has it, so the calls
// after setup take none. A caller that keeps the password itself, as the tests do so they never
// write the keychain, sets up with `keepPassword` off and passes it to each call.

import Foundation

extension ArchiveModel {
    /// Where backups stand on this Mac, with the snapshots from history, newest first. Needs no
    /// backup drive.
    public func backups() async throws -> BackupsState {
        try await call { try $0.backups() }
    }

    /// Creates a backup repository in `folder`, or connects to the one there, and records it as
    /// one change.
    @discardableResult
    public func setUpBackups(
        in folder: URL, password: String, description: String? = nil, keepPassword: Bool = true
    ) async throws -> BackupsSetUp {
        let path = folder.path(percentEncoded: false)
        let setUp = try await call(reporting: "Setting up backups") { handle, _ in
            try handle.setUpBackups(
                folder: path, password: password, description: description,
                keepPassword: keepPassword)
        }
        try await refreshAfterBackup()
        return setUp
    }

    /// Snapshots the whole archive now, recorded in history.
    @discardableResult
    public func snapshotNow(password: String? = nil) async throws -> SnapshotTaken {
        let taken = try await call(reporting: "Taking a snapshot") { handle, _ in
            try handle.snapshotNow(password: password)
        }
        try await refreshAfterBackup()
        return taken
    }

    /// Checks every snapshot and reads back `percent`% of the files. Returns what was found.
    public func verifyBackups(percent: UInt8 = 1, password: String? = nil) async throws -> String {
        try await call(reporting: "Checking the backups") { handle, _ in
            try handle.verifyBackups(percent: percent, password: password)
        }
    }

    /// Restores the item `id`'s contents as of the change `change` (its snapshot, or the latest
    /// before it) into `folder`, or a new folder beside the archive. Nothing in the archive
    /// changes.
    public func restore(
        item id: String, at change: String? = nil, to folder: URL? = nil, password: String? = nil
    ) async throws -> Restored {
        let to = folder?.path(percentEncoded: false)
        return try await call(reporting: "Restoring") { handle, _ in
            try handle.restoreItem(item: id, at: change, to: to, password: password)
        }
    }

    /// Status and History, which a snapshot or setup changes.
    private func refreshAfterBackup() async throws {
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
    }
}

extension BackupsState {
    /// Whether this Mac is set up but the repository's drive isn't connected.
    public var driveMissing: Bool { setUpHere && !reachable }
}

/// Whether `folder` holds a backup repository already, so setup asks for its password once.
public func isBackupRepository(_ folder: URL) -> Bool {
    isBackupRepository(folder: folder.path(percentEncoded: false))
}
