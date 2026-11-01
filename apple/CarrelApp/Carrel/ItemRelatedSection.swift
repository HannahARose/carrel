// The inspector's Related section, as `carrel show` and the interactive mode's item view list
// them: the items an item links to, and the items nearest it in meaning from the embedding
// index, or why there are none. Nothing is embedded, so no model starts.

import CarrelKit
import SwiftUI

/// What the Related section shows for one item.
struct RelatedItems: Equatable {
    /// The items it links to.
    var links: [LinkedItem]
    /// The items nearest it in meaning, nearest first.
    var near: [RelatedItem]
    /// Why none are listed by meaning, when the index can't say.
    var whyNone: String?

    /// Reads them for the item `id`.
    static func read(_ id: String, from archive: ArchiveModel) async -> RelatedItems? {
        try? await archive.call { handle in
            let detail = try handle.itemDetail(id: id)
            let related = try handle.related(id: id)
            return RelatedItems(
                links: detail.links, near: related.items, whyNone: related.whyNone)
        }
    }
}

struct ItemRelatedSection: View {
    let archive: ArchiveModel
    let item: String
    /// Opens a related item in Browse.
    let open: (String) -> Void
    @State private var related: RelatedItems?

    var body: some View {
        Section("Related") {
            if let related {
                ForEach(related.links, id: \.id) { link in
                    Button {
                        open(link.id)
                    } label: {
                        LabeledContent(link.title ?? link.id, value: link.linkType)
                    }
                    .buttonStyle(.plain)
                    .disabled(link.card == nil)
                }
                ForEach(related.near, id: \.id) { near in
                    Button {
                        open(near.id)
                    } label: {
                        LabeledContent {
                            Text(near.score, format: .percent.precision(.fractionLength(0)))
                        } label: {
                            Label(
                                near.title,
                                systemImage: near.sensitivity == .public ? "sparkle" : "lock")
                        }
                    }
                    .buttonStyle(.plain)
                    .help("Near in meaning")
                }
                if related.links.isEmpty, related.near.isEmpty {
                    Text(related.whyNone ?? "Nothing is linked to it or near it in meaning.")
                        .foregroundStyle(.secondary)
                } else if let whyNone = related.whyNone {
                    Text(whyNone).font(.caption).foregroundStyle(.secondary)
                }
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
        }
        .task(id: item) {
            related = nil
            related =
                await RelatedItems.read(item, from: archive)
                ?? RelatedItems(links: [], near: [], whyNone: "Its related items can’t be read.")
        }
    }
}
