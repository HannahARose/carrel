// Browse's grid: a collection's items as thumbnails, beside the list.

import CarrelKit
import SwiftUI

/// How Browse lays out a collection's items.
enum BrowseLayout: String, CaseIterable {
    case list
    case grid
}

struct BrowseGrid: View {
    @Bindable var model: ArchiveWindowModel

    var body: some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 128, maximum: 180), spacing: 12)]) {
                ForEach(model.listedItems) { item in
                    GridCell(
                        item: item, store: model.previews,
                        isSelected: model.selectedItemID == item.id
                    )
                    .onTapGesture { model.selectedItemID = item.id }
                    .contextMenu { BrowseActions(model: model, ids: [item.id]) }
                }
            }
            .padding(12)
        }
        .onKeyPress(.leftArrow) { move(by: -1) }
        .onKeyPress(.rightArrow) { move(by: 1) }
        .focusable()
        .focusEffectDisabled()
    }

    /// Chooses the item `offset` places from the chosen one.
    private func move(by offset: Int) -> KeyPress.Result {
        let items = model.listedItems
        guard !items.isEmpty else { return .ignored }
        let at = items.firstIndex { $0.id == model.selectedItemID } ?? (offset > 0 ? -1 : 0)
        model.selectedItemID = items[min(max(at + offset, 0), items.count - 1)].id
        return .handled
    }
}

private struct GridCell: View {
    let item: ItemSummary
    let store: PreviewStore?
    let isSelected: Bool

    var body: some View {
        VStack(spacing: 6) {
            Group {
                if let picture = store?.picture(for: item.id) {
                    Image(decorative: picture, scale: 2).resizable().scaledToFit()
                } else {
                    KindIcon(kind: item.kind)
                }
            }
            .frame(width: 112, height: 112)
            .overlay(alignment: .topTrailing) { FlagMark(flag: item.flag) }
            .padding(6)
            .background(
                isSelected ? AnyShapeStyle(.selection) : AnyShapeStyle(.clear),
                in: .rect(cornerRadius: 8))
            Text(item.title)
                .font(.caption)
                .lineLimit(2)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 140)
        }
        .contentShape(.rect)
        .help(item.title)
        .task(id: item.id) { await store?.loadThumbnail(item.id) }
    }
}
