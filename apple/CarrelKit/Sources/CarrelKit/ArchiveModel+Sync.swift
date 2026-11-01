// An archive shared through iCloud Drive (docs/design/sync-and-mobile.md), as `carrel sync`
// offers it: the sync status, downloading evicted contents before they're opened, a
// collection's `sync.keep_downloaded`, settling conflicted copies, and moving the archive into
// iCloud Drive.

import Foundation

extension ArchiveModel {
    /// The sync status, as `carrel sync status` shows it.
    public func syncStatus() async throws -> SyncStatus {
        try await call { try $0.syncStatus() }
    }

    /// Downloads the item `id`'s contents if iCloud evicted any, returning where they are.
    @discardableResult
    public func download(item id: String) async throws -> URL {
        URL(filePath: try await call { try $0.downloadItem(item: id) })
    }

    /// Downloads everything iCloud evicted in the collection at `path`; returns how many
    /// files it downloaded.
    @discardableResult
    public func download(collection path: String) async throws -> UInt64 {
        try await call { try $0.downloadCollection(path: path) }
    }

    /// Whether the collection at `path` keeps its items downloaded, its own setting or the
    /// collection above's.
    public func keepsDownloaded(_ path: String) async throws -> Bool {
        try await call { try $0.keepsDownloaded(path: path) }
    }

    /// Sets the collection at `path`'s `sync.keep_downloaded`, or clears it with `nil`, as
    /// one change undo can reverse.
    public func setKeepDownloaded(_ path: String, to on: Bool?) async throws {
        _ = try await call { try $0.setKeepDownloaded(path: path, on: on) }
        try await refreshSettled()
    }

    /// Settles the conflicted copy `copy` by keeping it (`keepCopy`) or the original; the
    /// other goes to the Trash.
    public func resolveConflict(_ copy: String, keepCopy: Bool) async throws {
        _ = try await call { try $0.resolveConflict(copy: copy, keepCopy: keepCopy) }
        try await refreshSettled()
    }

    /// Checks that the archive can move to `destination`, and how much it moves.
    public func planICloudMove(to destination: URL) async throws -> ArchiveMove {
        let to = destination.path(percentEncoded: false)
        return try await call { try $0.planIcloudMove(to: to) }
    }

    /// Moves the archive to `destination`, publishing its progress, and returns the new root.
    /// The old copy stays; open the new root in place of this archive.
    public func moveIntoICloud(to destination: URL) async throws -> URL {
        let to = destination.path(percentEncoded: false)
        let root = try await call(reporting: "Copying the archive") {
            try $0.moveIntoIcloud(to: to, progress: $1)
        }
        return URL(filePath: root, directoryHint: .isDirectory)
    }

    /// The folder this archive moves to by default: one of its name in iCloud Drive, if
    /// iCloud Drive is turned on.
    public nonisolated var defaultICloudDestination: URL? {
        defaultIcloudDestination(archiveRoot: handle.root()).map {
            URL(filePath: $0, directoryHint: .isDirectory)
        }
    }

    /// Status and History show a change that settled something.
    private func refreshSettled() async throws {
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
    }
}
