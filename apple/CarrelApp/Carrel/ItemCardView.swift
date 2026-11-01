// An item's card in the inspector: its fields, notes, history and related items.

import AppKit
import CarrelKit
import SwiftUI

/// Fields shown in the header rather than the list.
private let headerFields: Set<String> = ["carrel", "format", "id", "kind", "title", "summary"]

struct ItemCardView<Actions: View, Below: View>: View {
    let item: ItemSummary
    let card: Card?
    let root: URL
    /// The archive, to read the related items from.
    var archive: ArchiveModel?
    /// Opens a related item.
    var open: (String) -> Void = { _ in }
    /// Browse's actions on the item, beside Show in Finder.
    @ViewBuilder let actions: Actions
    /// Sections under the card, such as Carrel's questions about the item.
    @ViewBuilder let below: Below

    init(
        item: ItemSummary, card: Card?, root: URL, archive: ArchiveModel? = nil,
        open: @escaping (String) -> Void = { _ in }, @ViewBuilder actions: () -> Actions,
        @ViewBuilder below: () -> Below
    ) {
        self.item = item
        self.card = card
        self.root = root
        self.archive = archive
        self.open = open
        self.actions = actions()
        self.below = below()
    }

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label(item.kind.name, systemImage: item.kind.symbol)
                        .font(.caption).foregroundStyle(.secondary)
                    Text(item.title).font(.title2.bold())
                    if let summary = card?.value("summary"), !summary.isEmpty {
                        Text(summary).foregroundStyle(.secondary)
                    }
                }
                .textSelection(.enabled)
            }
            if let card {
                Section("Fields") {
                    ForEach(card.fields.filter { !headerFields.contains($0.key) }) { field in
                        LabeledContent(field.key.capitalized) {
                            Text(field.value).textSelection(.enabled)
                        }
                    }
                }
                if !card.notes.isEmpty {
                    Section("Notes") {
                        Text(
                            (try? AttributedString(
                                markdown: card.notes,
                                options: .init(
                                    interpretedSyntax: .inlineOnlyPreservingWhitespace)))
                                ?? AttributedString(card.notes)
                        )
                        .textSelection(.enabled)
                    }
                }
                if !card.history.isEmpty {
                    Section("History") {
                        ForEach(card.history, id: \.self) { Text($0) }
                    }
                }
            } else {
                Section {
                    Text("The card at \(item.card) can’t be read.").foregroundStyle(.secondary)
                }
            }
            if let archive {
                ItemRelatedSection(archive: archive, item: item.id, open: open)
            }
            Section {
                Button("Show in Finder", systemImage: "folder") {
                    let path = item.item ?? item.card
                    NSWorkspace.shared.activateFileViewerSelecting([root.appending(path: path)])
                }
                actions
            }
            below
        }
        .formStyle(.grouped)
    }
}

extension ItemCardView where Below == EmptyView {
    init(
        item: ItemSummary, card: Card?, root: URL, archive: ArchiveModel? = nil,
        open: @escaping (String) -> Void = { _ in }, @ViewBuilder actions: () -> Actions
    ) {
        self.init(
            item: item, card: card, root: root, archive: archive, open: open, actions: actions
        ) { EmptyView() }
    }
}
