// Browse's actions in the File menu, the item list's context menu and the item's card: new
// collections, renaming and expanding, for the key archive window.

import CarrelKit
import SwiftUI

struct BrowseCommands: Commands {
    @FocusedValue(ArchiveWindowModel.self) private var window

    var body: some Commands {
        CommandGroup(after: .newItem) {
            Divider()
            NewCollectionButton(model: window)
                .keyboardShortcut("n", modifiers: [.command, .option])
            let item = window?.screen.isCollection == true ? window?.selectedItem : nil
            Button("Rename…") {
                if let item, let browse = window?.browse {
                    Task { await browse.startRename(of: item) }
                }
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])
            .disabled(item == nil || window?.canChange != true)
            Button("Expand…") {
                if let item, let browse = window?.browse {
                    Task { await browse.askToExpand(item) }
                }
            }
            .keyboardShortcut("e", modifiers: [.command, .shift])
            .disabled(item.map { window?.browse?.canExpand($0) != true } ?? true)
            .disabled(window?.canChange != true)
        }
    }
}

/// Renaming and, for a compressed file, expanding `item`.
struct ItemActions: View {
    let model: ArchiveWindowModel
    let item: ItemSummary

    var body: some View {
        AskAboutButton(model: model, item: item)
        Button("Wander from Here", systemImage: "figure.walk") { model.wander(from: item.id) }
            .disabled(model.discover == nil)
        ItemFlagButtons(model: model, item: item)
        ItemDownloadButton(model: model, item: item)
        Button("Report a Missing Feature…", systemImage: "exclamationmark.bubble") {
            model.missingFeature = MissingFeatureModel(item: item)
        }
        if let browse = model.browse {
            Button("Rename…", systemImage: "pencil") {
                Task { await browse.startRename(of: item) }
            }
            .disabled(!model.canChange)
            if browse.canExpand(item) {
                Button("Expand…", systemImage: "arrow.up.left.and.arrow.down.right") {
                    Task { await browse.askToExpand(item) }
                }
                .disabled(!model.canChange)
            }
        }
    }
}

/// Starts a new collection, inside the one Browse shows.
struct NewCollectionButton: View {
    let model: ArchiveWindowModel?

    var body: some View {
        Button("New Collection…") {
            model?.browse?.startNewCollection(in: model?.browsedCollection)
        }
        .disabled(model?.canChange != true)
    }
}

extension ArchiveWindowModel {
    /// Starts a walk through the stacks from the item `id`, on Discover's Wander page.
    func wander(from id: String) {
        guard let discover else { return }
        discover.wander(from: id)
        screen = .discover(.wander)
    }

    /// Whether Browse's actions can change the archive.
    var canChange: Bool { browse != nil && !isReadOnly }

    /// The collection Browse shows, if it's showing one.
    var browsedCollection: String? {
        if case .collection(let path) = screen { path } else { nil }
    }
}

extension Optional where Wrapped == Screen {
    /// Whether it's Browse, showing a collection.
    var isCollection: Bool {
        if case .collection = self { true } else { false }
    }
}
