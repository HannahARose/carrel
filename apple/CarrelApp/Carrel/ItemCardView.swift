// An item's card in the inspector: its fields, notes and history.

import AppKit
import CarrelKit
import SwiftUI

struct ItemCardView: View {
    let item: ItemSummary
    let card: Card?
    let root: URL

    /// Fields shown in the header rather than the list.
    private static let header: Set<String> = ["carrel", "format", "id", "kind", "title", "summary"]

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
                    ForEach(card.fields.filter { !Self.header.contains($0.key) }) { field in
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
            Section {
                Button("Show in Finder", systemImage: "folder") {
                    let path = item.item ?? item.card
                    NSWorkspace.shared.activateFileViewerSelecting([root.appending(path: path)])
                }
            }
        }
        .formStyle(.grouped)
    }
}
