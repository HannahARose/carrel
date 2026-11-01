// What the Active screen shows and does: what's checked out and on this Mac's desk, each with
// its pending history entries, and returning, cancelling, checking out and deciding entries,
// all through CarrelKit's ArchiveModel, as the interactive mode's Active screen does.

import CarrelKit
import Foundation
import Observation

/// A row of the Active list: an item, or one of its pending entries.
enum ActiveRow: Hashable {
    /// The active item with this identifier.
    case item(String)
    /// That item's pending entry with this number, from 1.
    case entry(String, UInt32)

    var itemID: String {
        switch self {
        case .item(let id), .entry(let id, _): id
        }
    }
}

/// A pending entry being edited.
struct EntryDraft: Identifiable {
    let item: String
    let number: UInt32
    var text: String
    var id: String { "\(item)#\(number)" }
}

/// An item about to be checked out: where to, and when it's due back.
struct CheckoutDraft: Identifiable {
    let item: String
    let title: String
    var folder: URL
    var due: Date?
    var id: String { item }

    /// The due date as the core takes it, `YYYY-MM-DD`.
    var dueString: String? {
        due.map { $0.formatted(.iso8601.year().month().day().dateSeparator(.dash)) }
    }
}

@Observable
final class ActiveScreenModel {
    let archive: ArchiveModel
    /// Called after a change, so the window's other lists follow it.
    private let didChange: () async -> Void

    /// The chosen rows: one item, or some of its entries.
    var selection: Set<ActiveRow> = []
    /// The return being confirmed, with what it brings back.
    var returning: ReturnPreview?
    /// The checkout being confirmed for cancelling.
    var cancelling: ActiveItem?
    /// The entry being edited.
    var editing: EntryDraft?
    /// The checkout being set up.
    var checkingOut: CheckoutDraft?
    /// What went wrong with the last action, for an alert.
    var problem: String?

    /// Where a checkout goes unless changed: the Desktop, or the folder tests give.
    let checkoutFolder: URL

    init(
        archive: ArchiveModel,
        checkoutFolder: URL = FileManager.default.urls(
            for: .desktopDirectory, in: .userDomainMask)[0],
        didChange: @escaping () async -> Void = {}
    ) {
        self.archive = archive
        self.checkoutFolder = checkoutFolder
        self.didChange = didChange
    }

    var items: [ActiveItem] { archive.active }

    /// How many entries wait for approval, for the sidebar's badge.
    var pendingCount: Int { items.reduce(0) { $0 + $1.pending.count } }

    /// The item the chosen rows belong to, when they all belong to one.
    var selectedItem: ActiveItem? {
        let ids = Set(selection.map(\.itemID))
        guard ids.count == 1, let id = ids.first else { return nil }
        return items.first { $0.id == id }
    }

    /// The chosen entries' numbers, in order: empty when the item itself is chosen, which
    /// means all of its entries.
    var selectedEntries: [UInt32] {
        selection.compactMap {
            if case .entry(_, let number) = $0 { number } else { nil }
        }.sorted()
    }

    /// The entries an approve, combine or drop would decide.
    var decidedEntries: [UInt32] {
        guard let item = selectedItem else { return [] }
        let chosen = selectedEntries
        return chosen.isEmpty ? Array(1...UInt32(max(item.pending.count, 1))) : chosen
    }

    var canReturn: Bool { selectedItem?.kind == .checkedOut }
    var canDecide: Bool { !(selectedItem?.pending.isEmpty ?? true) }
    /// Combining needs two entries.
    var canCombine: Bool { canDecide && decidedEntries.count > 1 }
    /// Editing needs one entry: the chosen one, or the item's only one.
    var canEdit: Bool { entryToEdit != nil }

    private var entryToEdit: UInt32? {
        guard let item = selectedItem else { return nil }
        let chosen = selectedEntries
        if chosen.count == 1 { return chosen[0] }
        return chosen.isEmpty && item.pending.count == 1 ? 1 : nil
    }

    /// Reads what's active again.
    func refresh() async {
        await perform { try await $0.refreshActive() }
        selection = selection.filter { row in
            guard let item = items.first(where: { $0.id == row.itemID }) else { return false }
            if case .entry(_, let number) = row { return number <= item.pending.count }
            return true
        }
    }

    // MARK: - Returning and cancelling

    /// Works out what returning the chosen checkout brings back, for the confirmation.
    func askToReturn() async {
        guard let item = selectedItem, item.kind == .checkedOut else { return }
        let id = item.id
        returning = await perform { model in
            try await model.call { try $0.returnPreview(item: id, from: nil) }
        }
    }

    /// Returns the checkout `preview` describes, once the owner has seen it.
    func confirmReturn(_ preview: ReturnPreview) async {
        returning = nil
        if await perform({ try await $0.returnItem(preview.item) }) != nil {
            selection = []
            await didChange()
        }
    }

    /// Asks before cancelling the chosen checkout.
    func askToCancel() {
        guard let item = selectedItem, item.kind == .checkedOut else { return }
        cancelling = item
    }

    /// Cancels the checkout of `item`, once the owner has agreed; its working copy's changes
    /// aren't brought back.
    func confirmCancel(_ item: ActiveItem) async {
        cancelling = nil
        if await perform({ try await $0.cancelCheckout(item.id) }) != nil {
            selection = []
            await didChange()
        }
    }

    /// The working copy, or the desk item itself.
    func workingCopy(of item: ActiveItem) -> URL {
        URL(filePath: item.working)
    }

    // MARK: - Pending entries

    /// Approves, combines or drops the chosen entries, or all of the chosen item's.
    func decide(_ decision: EntryDecision) async {
        guard let item = selectedItem, canDecide else { return }
        if decision == .combine, !canCombine { return }
        let numbers = selectedEntries
        if await perform({ try await $0.decide(entries: numbers, of: item.id, decision) }) != nil {
            selection = [.item(item.id)]
        }
    }

    /// Opens the chosen entry for editing.
    func startEditing() {
        guard let item = selectedItem, let number = entryToEdit else { return }
        let entry = item.pending[Int(number) - 1]
        editing = EntryDraft(
            item: item.id, number: number, text: entry.lines.joined(separator: "\n"))
    }

    /// Saves an edited entry, still pending.
    func save(_ draft: EntryDraft) async {
        editing = nil
        await perform { try await $0.edit(entry: draft.number, of: draft.item, text: draft.text) }
    }

    // MARK: - Checking out and the desk

    /// Starts checking out the item `id`, to the checkout folder with no due date unless
    /// changed.
    func startCheckout(of id: String, titled title: String) {
        checkingOut = CheckoutDraft(item: id, title: title, folder: checkoutFolder, due: nil)
    }

    /// Checks out the item as `draft` sets it up.
    func checkOut(_ draft: CheckoutDraft) async {
        checkingOut = nil
        let due = draft.dueString
        let out = await perform { try await $0.checkOut(draft.item, to: draft.folder, due: due) }
        if let out {
            selection = [.item(out.item)]
            await didChange()
        }
    }

    /// Puts the file or folder at `url` on this Mac's desk.
    func addToDesk(_ url: URL) async {
        if let desk = await perform({ try await $0.addToDesk(url) }) {
            selection = [.item(desk.id)]
        }
    }

    /// Takes the chosen desk item off the desk, keeping its card.
    func removeFromDesk() async {
        guard let item = selectedItem, item.kind == .desk else { return }
        if await perform({ try await $0.removeFromDesk(URL(filePath: item.working)) }) != nil {
            selection = []
        }
    }

    /// Runs `work` on the archive, keeping its error as `problem`; `nil` if it failed.
    @discardableResult
    private func perform<T>(_ work: (ArchiveModel) async throws -> T) async -> T? {
        do {
            return try await work(archive)
        } catch let error as CarrelError {
            problem = error.message
        } catch {
            problem = error.localizedDescription
        }
        return nil
    }
}
