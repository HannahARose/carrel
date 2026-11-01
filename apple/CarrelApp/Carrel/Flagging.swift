// Flags (FORMAT.md §5.9): marking items and collections that need work while browsing, with a
// note saying what, as `carrel flag` and the interactive mode's f do. Flag… and Unflag are in
// Browse's context menus and the inspector, flagged things carry a flag in the lists and the
// grid, the sidebar's Flagged lists them, and Status does too. Once an edit or a correction is
// saved on a flagged card, the app offers to clear its flag. Each flag is one change undo can
// reverse.

import CarrelKit
import SwiftUI

/// What a flag is on: an item, by its identifier, or a collection, by its folder.
enum FlagTarget: Hashable {
    case item(String)
    case collection(String)
}

/// A flag about to be set: what needs work, in the owner's words.
struct FlagDraft: Identifiable {
    let target: FlagTarget
    let title: String
    var note: String
    /// Why the last try didn't flag it, shown in the sheet.
    var problem: String?
    var id: FlagTarget { target }
}

/// A flag the owner may clear: chosen with Unflag, or offered once an edit or a correction is
/// saved on a flagged card.
struct UnflagRequest: Identifiable {
    let target: FlagTarget
    let title: String
    let flag: Flag
    /// Whether it's offered after an edit, rather than asked for.
    let afterEdit: Bool
    var id: FlagTarget { target }
}

extension BrowseActionsModel {
    /// Starts flagging `item`, with a note to write.
    func startFlag(of item: ItemSummary) {
        flagging = FlagDraft(target: .item(item.id), title: item.title, note: item.flag?.note ?? "")
    }

    /// Starts flagging `collection`, with a note to write.
    func startFlag(of collection: CollectionSummary) {
        flagging = FlagDraft(
            target: .collection(collection.path), title: collection.title,
            note: collection.flag?.note ?? "")
    }

    /// Flags what `draft` names; on a problem the sheet stays open saying it.
    func flag(_ draft: FlagDraft) async {
        let trimmed = draft.note.trimmingCharacters(in: .whitespacesAndNewlines)
        let note = trimmed.isEmpty ? nil : trimmed
        do {
            switch draft.target {
            case .item(let id): try await archive.flag(item: id, note: note)
            case .collection(let path): try await archive.flag(collection: path, note: note)
            }
            flagging = nil
            await didChange()
        } catch {
            var draft = draft
            draft.problem = flagProblem(error)
            flagging = draft
        }
    }

    /// Clears the flag `request` names.
    func unflag(_ request: UnflagRequest) async {
        unflagging = nil
        do {
            switch request.target {
            case .item(let id): try await archive.unflag(item: id)
            case .collection(let path): try await archive.unflag(collection: path)
            }
            await didChange()
        } catch {
            problem = flagProblem(error)
        }
    }

    /// Asks before clearing `item`'s flag.
    func askToUnflag(_ item: ItemSummary) {
        guard let flag = item.flag else { return }
        unflagging = UnflagRequest(
            target: .item(item.id), title: item.title, flag: flag, afterEdit: false)
    }

    /// Asks before clearing `collection`'s flag.
    func askToUnflag(_ collection: CollectionSummary) {
        guard let flag = collection.flag else { return }
        unflagging = UnflagRequest(
            target: .collection(collection.path), title: collection.title, flag: flag,
            afterEdit: false)
    }

    /// After an edit of the item `id`, titled `title`, was saved: offers to clear its flag,
    /// when it has one.
    func offerToUnflag(item id: String, title: String) async {
        guard let flag = try? await archive.flag(of: id) else { return }
        unflagging = UnflagRequest(target: .item(id), title: title, flag: flag, afterEdit: true)
    }

    /// The message of `error`, taking it from the archive so the window doesn't show it too.
    private func flagProblem(_ error: Error) -> String {
        archive.lastError = nil
        return (error as? CarrelError)?.message ?? error.localizedDescription
    }
}

extension Flag {
    /// The flag in words: its note, and the day it was set.
    var described: String {
        var text = note ?? "Needs work"
        if let at, at.count >= 10 { text += " (flagged \(at.prefix(10)))" }
        return text
    }
}

/// The flag beside a flagged item or collection in a list.
struct FlagMark: View {
    let flag: Flag?

    var body: some View {
        if let flag {
            Image(systemName: "flag.fill")
                .foregroundStyle(.orange)
                .imageScale(.small)
                .help(flag.described)
                .accessibilityLabel("Flagged: \(flag.described)")
        }
    }
}

/// Flag… or Unflag for an item, in its context menu and its card.
struct ItemFlagButtons: View {
    let model: ArchiveWindowModel
    let item: ItemSummary

    var body: some View {
        if let browse = model.browse {
            if item.flag == nil {
                Button("Flag…", systemImage: "flag") { browse.startFlag(of: item) }
                    .disabled(!model.canChange)
            } else {
                Button("Change Flag…", systemImage: "flag") { browse.startFlag(of: item) }
                    .disabled(!model.canChange)
                Button("Unflag", systemImage: "flag.slash") { browse.askToUnflag(item) }
                    .disabled(!model.canChange)
            }
        }
    }
}

/// Flag… or Unflag for a collection, in the sidebar's context menu and the inspector.
struct CollectionFlagButtons: View {
    let model: ArchiveWindowModel
    let collection: CollectionSummary

    var body: some View {
        if let browse = model.browse {
            if collection.flag == nil {
                Button("Flag…", systemImage: "flag") { browse.startFlag(of: collection) }
                    .disabled(!model.canChange)
            } else {
                Button("Change Flag…", systemImage: "flag") { browse.startFlag(of: collection) }
                    .disabled(!model.canChange)
                Button("Unflag", systemImage: "flag.slash") { browse.askToUnflag(collection) }
                    .disabled(!model.canChange)
            }
        }
    }
}

/// A collection's flag in the inspector, with what to do about it.
struct CollectionFlagSection: View {
    let model: ArchiveWindowModel
    let collection: CollectionSummary

    var body: some View {
        Section("Needs Work") {
            if let flag = collection.flag {
                Label(flag.described, systemImage: "flag.fill")
                    .foregroundStyle(.orange)
                    .textSelection(.enabled)
            }
            HStack { CollectionFlagButtons(model: model, collection: collection) }
        }
    }
}

/// The note for a flag: what needs work, in a few words.
struct FlagSheet: View {
    @State var draft: FlagDraft
    let confirm: (FlagDraft) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Flag “\(draft.title)”").font(.headline).padding([.horizontal, .top], 20)
            Form {
                TextField("What needs work", text: $draft.note, prompt: Text("wrong title"))
                Text(
                    "A few words saying what needs doing, or nothing. The crawler looks at flagged things first, and Status lists them. Undo it from History."
                )
                .font(.caption).foregroundStyle(.secondary)
                if let problem = draft.problem {
                    Label(problem, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                }
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Flag") { confirm(draft) }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(20)
        }
        .frame(width: 440)
    }
}

extension View {
    /// Adds the sheet for a flag's note, and the question before clearing one.
    func flagDialogs(_ model: BrowseActionsModel) -> some View {
        modifier(FlagDialogs(model: model))
    }
}

private struct FlagDialogs: ViewModifier {
    @Bindable var model: BrowseActionsModel

    func body(content: Content) -> some View {
        content
            .sheet(item: $model.flagging) { draft in
                FlagSheet(draft: draft) { draft in Task { await model.flag(draft) } }
                    .id(draft.problem)
            }
            .confirmationDialog(
                "Clear the flag on “\(model.unflagging?.title ?? "")”?",
                isPresented: Binding(
                    get: { model.unflagging != nil }, set: { if !$0 { model.unflagging = nil } }),
                presenting: model.unflagging
            ) { request in
                Button("Clear the Flag") { Task { await model.unflag(request) } }
                Button(request.afterEdit ? "Keep It" : "Cancel", role: .cancel) {
                    model.unflagging = nil
                }
            } message: { request in
                Text(
                    request.afterEdit
                        ? "It’s flagged as needing work: \(request.flag.described). If what you saved settles it, clear the flag. Undo it from History."
                        : "It’s flagged as needing work: \(request.flag.described). Undo it from History."
                )
            }
    }
}

/// The sidebar's Flagged screen: the collections and items flagged as needing work.
struct FlaggedView: View {
    @Bindable var model: ArchiveWindowModel

    var body: some View {
        List(selection: $model.selectedItemID) {
            if !model.flaggedCollections.isEmpty {
                Section("Collections") {
                    ForEach(model.flaggedCollections, id: \.path) { collection in
                        Button {
                            model.screen = .collection(collection.path)
                        } label: {
                            FlaggedRow(
                                title: collection.title, symbol: "folder", flag: collection.flag)
                        }
                        .buttonStyle(.plain)
                        .contextMenu { CollectionFlagButtons(model: model, collection: collection) }
                    }
                }
            }
            if !model.flaggedItems.isEmpty {
                Section("Items") {
                    ForEach(model.flaggedItems) { item in
                        FlaggedRow(title: item.title, symbol: item.kind.symbol, flag: item.flag)
                            .tag(item.id)
                    }
                }
            }
        }
        .contextMenu(forSelectionType: ItemSummary.ID.self) { ids in
            BrowseActions(model: model, ids: ids)
        }
        .overlay {
            if model.flaggedCollections.isEmpty, model.flaggedItems.isEmpty {
                ContentUnavailableView(
                    "Nothing Flagged", systemImage: "flag",
                    description: Text(
                        "Flag an item or a collection that needs work from its context menu or the inspector."
                    ))
            }
        }
    }
}

private struct FlaggedRow: View {
    let title: String
    let symbol: String
    let flag: Flag?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Label(title, systemImage: symbol)
            if let flag {
                Text(flag.described).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// Status's list of what's flagged.
struct StatusFlaggedSection: View {
    let model: ArchiveWindowModel

    var body: some View {
        let flagged = model.archive?.status?.flagged ?? []
        if !flagged.isEmpty {
            Section("Flagged as Needing Work") {
                ForEach(flagged, id: \.path) { thing in
                    Button {
                        if thing.isCollection {
                            model.screen = .collection(thing.path)
                        } else if let id = thing.id {
                            model.open(item: id)
                        }
                    } label: {
                        FlaggedRow(
                            title: thing.title, symbol: thing.isCollection ? "folder" : "doc",
                            flag: thing.flag)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

extension ArchiveWindowModel {
    /// The collections flagged as needing work, by path.
    var flaggedCollections: [CollectionSummary] {
        collections.filter { $0.flag != nil }.sorted { $0.path < $1.path }
    }

    /// The items flagged as needing work, by title.
    var flaggedItems: [ItemSummary] {
        items.filter { $0.flag != nil }
            .sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
    }
}
