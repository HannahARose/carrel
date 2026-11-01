// What the History screen shows and does: every change, newest first, a page at a time, or
// only the librarian's (`carrel log --librarian`), and undoing one after saying what undoing
// restores and what it doesn't.

import CarrelKit
import Foundation
import Observation

/// What undoing a change restores and what it doesn't, for the confirmation sheet.
struct UndoExplanation: Equatable {
    /// What a new change puts back.
    var restores: String
    /// What stays as it is now.
    var keeps: String
    /// For a return, how to get the files it replaced: from the backups, not history.
    var command: String?

    init(_ change: Change) {
        restores =
            "A new change reverses this one: items it moved go back, and the cards, "
            + "collections and settings it changed return to how they were, merged with "
            + "later changes."
        switch change.operation {
        case "return":
            keeps =
                "History holds cards, not files: the files this return brought back stay as "
                + "they are now. The earlier files come back only from a backup."
            let item = change.items.first ?? "ITEM"
            command = "carrel backup restore \(item) --at \(change.id.prefix(12))"
        default:
            keeps =
                "History holds cards, not files: no file’s contents change. Earlier contents "
                + "come back only from a backup, with `carrel backup restore`."
            command = nil
        }
    }
}

@Observable
final class HistoryScreenModel {
    let archive: ArchiveModel
    /// Called after an undo, so the window's other lists follow it.
    private let didChange: () async -> Void

    /// The chosen change's identifier.
    var selectedID: String?
    /// The change being confirmed for undoing.
    var undoing: Change?
    /// What went wrong with the last action, for an alert.
    var problem: String?

    init(archive: ArchiveModel, didChange: @escaping () async -> Void = {}) {
        self.archive = archive
        self.didChange = didChange
    }

    /// Whether only the changes the librarian made on its own are listed.
    var librarianOnly = false

    /// How many changes a page holds.
    static let pageSize: UInt32 = 200

    /// Every change read, newest first, or only the librarian's.
    var changes: [Change] {
        librarianOnly ? archive.history.filter { $0.agent == "librarian" } : archive.history
    }

    /// Whether there may be older changes than those read.
    var canLoadMore: Bool {
        archive.historyLimit.map { UInt32(archive.history.count) >= $0 } ?? false
    }

    /// How many changes were read, for the list's footer.
    var readCount: Int { archive.history.count }

    /// Reads the next page of older changes.
    func loadMore() async {
        archive.historyLimit = (archive.historyLimit ?? 0) + Self.pageSize
        await refresh()
    }

    /// Reads every change, however many.
    func loadAll() async {
        archive.historyLimit = nil
        await refresh()
    }

    var selected: Change? {
        selectedID.flatMap { id in changes.first { $0.id == id } }
    }

    /// Reads the history again; an archive without one has no changes.
    func refresh() async {
        do {
            try await archive.refreshStatus()
            if archive.status?.hasHistory == true { try await archive.refreshHistory() }
        } catch {
            problem = (error as? CarrelError)?.message ?? error.localizedDescription
        }
    }

    /// Asks before undoing the chosen change.
    func askToUndo() {
        undoing = selected
    }

    /// Undoes `change`, once the owner has agreed, as a new change, and chooses that one.
    func undo(_ change: Change) async {
        undoing = nil
        do {
            try await archive.undo(change.id)
            selectedID = changes.first?.id
            await didChange()
        } catch {
            problem = (error as? CarrelError)?.message ?? error.localizedDescription
        }
    }
}
