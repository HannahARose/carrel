// Flags (FORMAT.md §5.9): marking items and collections that need work, with a note, as
// `carrel flag` and `carrel unflag` do. Each is one change undo can reverse, so Status and
// History follow it.

extension ArchiveModel {
    /// Flags the item `id` as needing work, with `note` saying what. Flagging it again
    /// replaces the note.
    @discardableResult
    public func flag(item id: String, note: String?) async throws -> FlagChange {
        try await afterFlagging(try await call { try $0.flagItem(item: id, note: note) })
    }

    /// Clears the item `id`'s flag.
    @discardableResult
    public func unflag(item id: String) async throws -> FlagChange {
        try await afterFlagging(try await call { try $0.unflagItem(item: id) })
    }

    /// Flags the collection at `path` as needing work, with `note` saying what.
    @discardableResult
    public func flag(collection path: String, note: String?) async throws -> FlagChange {
        try await afterFlagging(try await call { try $0.flagCollection(path: path, note: note) })
    }

    /// Clears the collection at `path`'s flag.
    @discardableResult
    public func unflag(collection path: String) async throws -> FlagChange {
        try await afterFlagging(try await call { try $0.unflagCollection(path: path) })
    }

    /// The item `id`'s flag, if it has one: to offer clearing it once an edit or a correction
    /// is saved.
    public func flag(of id: String) async throws -> Flag? {
        try await call { try $0.itemFlag(item: id) }
    }

    /// Status and History show a flag that changed something.
    private func afterFlagging(_ change: FlagChange) async throws -> FlagChange {
        if change.changed {
            try await refreshStatus()
            if status?.hasHistory == true { try await refreshHistory() }
        }
        return change
    }
}
