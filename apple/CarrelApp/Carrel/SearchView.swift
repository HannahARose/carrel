// The search results in the content column: best first, each with its collection, a mark when
// it's sensitive or restricted, and the matched passage with its words highlighted.

import CarrelKit
import SwiftUI

struct SearchResultsView: View {
    @Bindable var model: ArchiveWindowModel
    let search: SearchModel

    var body: some View {
        List(selection: $model.selectedItemID) {
            Section {
                ForEach(search.hits) { hit in
                    SearchHitRow(hit: hit) {
                        if hit.found == .collection { model.screen = .collection(hit.path) }
                    }
                    .tag(hit.itemId)
                    .selectionDisabled(hit.itemId == nil)
                }
            } header: {
                if let heading = search.heading {
                    HStack {
                        Text(heading)
                        if search.isSearchingByMeaning {
                            ProgressView().controlSize(.mini)
                                .help("Searching by meaning")
                        }
                    }
                }
            } footer: {
                if let notice = search.notice {
                    Label(notice, systemImage: "info.circle")
                        .foregroundStyle(.secondary)
                }
            }
        }
        .overlay {
            if let problem = search.problem {
                ContentUnavailableView(
                    "Can’t Search", systemImage: "magnifyingglass",
                    description: Text(problem))
            } else if search.results?.hits.isEmpty == true, !search.isSearchingByMeaning {
                ContentUnavailableView.search(text: search.text)
            }
        }
    }
}

private struct SearchHitRow: View {
    let hit: SearchHit
    let showCollection: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline) {
                Label(hit.title, systemImage: symbol)
                    .font(.headline)
                    .lineLimit(1)
                if let mark = hit.sensitivityMark {
                    Text(mark)
                        .font(.caption.weight(.medium))
                        .padding(.horizontal, 5)
                        .background(.orange.opacity(0.2), in: .capsule)
                        .foregroundStyle(.orange)
                }
                Spacer()
                if hit.found == .collection {
                    Button("Show", action: showCollection)
                        .buttonStyle(.link)
                }
            }
            if let place {
                Text(place)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            if !hit.passage.isEmpty {
                Text(hit.highlightedPassage)
                    .font(.callout)
                    .lineLimit(3)
            }
        }
        .padding(.vertical, 2)
    }

    private var symbol: String {
        switch hit.found {
        case .item: hit.kind?.symbol ?? "doc"
        case .collection: "folder"
        case .glossary: "character.book.closed"
        }
    }

    /// Where it is: an item's collection, a collection's folder or the glossary.
    private var place: String? {
        switch hit.found {
        case .item: [hit.collection, hit.date].compactMap(\.self).joined(separator: " · ")
        case .collection: "Collection · \(hit.path)"
        case .glossary: "Glossary"
        }
    }
}
