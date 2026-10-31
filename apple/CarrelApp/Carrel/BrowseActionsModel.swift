// Browse's actions on the archive's structure and its filed items: creating a collection,
// renaming an item and expanding a filed compressed file. Each is asked first and is one
// change undo can reverse, through CarrelKit's ArchiveModel, as `carrel collection new`,
// `carrel rename` and `carrel expand` (and the interactive mode's n, r and z) do.

import CarrelKit
import Foundation
import Observation

/// A tag being written for a new collection.
struct TagDraft: Identifiable, Equatable {
    let id = UUID()
    var name = ""
    var meaning = ""
}

/// A collection about to be created: its path, what belongs there and its tags.
struct CollectionDraft: Identifiable {
    let id = UUID()
    var path: String
    var description = ""
    var tags: [TagDraft] = []
    /// Why the last try didn't create it, shown in the sheet.
    var problem: String?

    /// The path as the core takes it.
    var trimmedPath: String {
        path.trimmingCharacters(in: .whitespaces.union(CharacterSet(charactersIn: "/")))
    }

    /// The tags given, leaving out empty rows; a half-written one goes to the core to say why
    /// it won't do.
    var collectionTags: [CollectionTag] {
        tags.filter { !($0.name.isEmpty && $0.meaning.isEmpty) }
            .map { CollectionTag(name: $0.name, meaning: $0.meaning) }
    }
}

/// An item about to be renamed, starting from the portable name `carrel rename` suggests.
struct RenameDraft: Identifiable {
    let item: String
    let title: String
    /// Its name now.
    let current: String
    var name: String
    /// Why the last try didn't rename it, shown in the sheet.
    var problem: String?
    var id: String { item }

    var canRename: Bool {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        return !trimmed.isEmpty && trimmed != current
    }
}

/// A filed compressed file about to be expanded.
struct ExpandRequest: Identifiable {
    let item: String
    let title: String
    /// Why it's kept compressed, when it would be expanded all the same.
    let keptBecause: String?
    var id: String { item }
}

@Observable
final class BrowseActionsModel {
    let archive: ArchiveModel
    /// Called after a change, so the window's lists follow it.
    private let didChange: () async -> Void
    /// Called with a new collection's path once it's created, to show it.
    var created: (String) -> Void = { _ in }

    /// The collection being set up.
    var creating: CollectionDraft?
    /// The rename being set up.
    var renaming: RenameDraft?
    /// The expansion being confirmed.
    var expanding: ExpandRequest?
    /// What went wrong, or what to know about the last action, for an alert.
    var problem: String?

    init(archive: ArchiveModel, didChange: @escaping () async -> Void = {}) {
        self.archive = archive
        self.didChange = didChange
    }

    var isReadOnly: Bool { archive.handle.isReadOnly() }

    // MARK: - New collection

    /// Starts a new collection inside `parent`, or at the top when it's `nil`.
    func startNewCollection(in parent: String?) {
        creating = CollectionDraft(path: parent.map { $0 + "/" } ?? "")
    }

    /// Creates the collection `draft` describes; on a problem the sheet stays open saying it.
    func create(_ draft: CollectionDraft) async {
        let description = draft.description.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            let new = try await archive.createCollection(
                draft.trimmedPath, description: description.isEmpty ? nil : description,
                tags: draft.collectionTags)
            creating = nil
            await didChange()
            created(new.path)
        } catch {
            var draft = draft
            draft.problem = take(error)
            creating = draft
        }
    }

    // MARK: - Rename

    /// Starts renaming `item`, from the name that works on every system.
    func startRename(of item: ItemSummary) async {
        let id = item.id
        do {
            let suggestion = try await archive.call { try $0.nameSuggestion(item: id) }
            renaming = RenameDraft(
                item: id, title: item.title, current: suggestion.current,
                name: suggestion.suggested)
        } catch {
            problem = take(error)
        }
    }

    /// Renames the item as `draft` says; on a problem the sheet stays open saying it.
    func rename(_ draft: RenameDraft) async {
        do {
            try await archive.rename(
                draft.item, to: draft.name.trimmingCharacters(in: .whitespaces))
            renaming = nil
            await didChange()
        } catch {
            var draft = draft
            draft.problem = take(error)
            renaming = draft
        }
    }

    // MARK: - Expand

    /// Whether `item` is named as a compressed file, so Browse offers to expand it.
    func canExpand(_ item: ItemSummary) -> Bool {
        item.kind == .file && looksCompressed(path: item.item ?? item.card)
    }

    /// Asks before expanding `item`, saying why it's kept compressed when it would be
    /// expanded all the same; or says why it can't be.
    func askToExpand(_ item: ItemSummary) async {
        let id = item.id
        do {
            switch try await archive.call({ try $0.expandable(item: id) }) {
            case .yes:
                expanding = ExpandRequest(item: id, title: item.title, keptBecause: nil)
            case .allTheSame(let reason):
                expanding = ExpandRequest(item: id, title: item.title, keptBecause: reason)
            case .no(let reason):
                problem = "“\(item.title)” can’t be expanded: \(reason)."
            }
        } catch {
            problem = take(error)
        }
    }

    /// Expands the item `request` names, all the same when it's kept compressed.
    func expand(_ request: ExpandRequest) async {
        expanding = nil
        do {
            let expanded = try await archive.expand(
                request.item, anyway: request.keptBecause != nil)
            await didChange()
            if let trouble = expanded.trashProblem {
                problem =
                    "It’s expanded, but the compressed file couldn’t go to the Trash, so it’s still at \(expanded.from): \(trouble)"
            }
        } catch {
            problem = take(error)
        }
    }

    /// The message of `error`, taking it from the archive so the window doesn't show it too.
    private func take(_ error: Error) -> String {
        archive.lastError = nil
        return (error as? CarrelError)?.message ?? error.localizedDescription
    }
}
