// The chosen item's preview, above its card in the inspector: Quick Look's own preview,
// Carrel's drawing of what Quick Look can't show, or a folder's contents.

import CarrelKit
import QuickLookUI
import SwiftUI

struct ItemPreviewView: View {
    let item: ItemSummary
    let store: PreviewStore

    var body: some View {
        Group {
            switch store.content(for: item) {
            case .none:
                EmptyView()
            case .hidden:
                VStack(spacing: 8) {
                    Image(systemName: "lock").font(.title2).foregroundStyle(.secondary)
                    Text("This item is restricted.").foregroundStyle(.secondary)
                    Button("Show Preview") { store.reveal(item.id) }
                }
                .frame(maxWidth: .infinity, minHeight: 120)
            case .quickLook(let url):
                QuickLookPreview(url: url).frame(height: 280)
            case .drawing:
                DrawingPreview(item: item, store: store).frame(height: 280)
            case .folder:
                FolderPreview(item: item, store: store).frame(maxHeight: 220)
            }
        }
        .onChange(of: item.id, initial: true) { store.chose(item.id) }
    }
}

/// The item's kind icon, standing in until its picture is ready, or when it has none.
struct KindIcon: View {
    let kind: ItemKind

    var body: some View {
        Image(systemName: kind.symbol)
            .font(.system(size: 40, weight: .light))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct DrawingPreview: View {
    let item: ItemSummary
    let store: PreviewStore

    var body: some View {
        Group {
            if let drawing = store.drawings[item.id] {
                Image(decorative: drawing, scale: 2).resizable().scaledToFit().padding(8)
            } else {
                KindIcon(kind: item.kind)
            }
        }
        .task(id: item.id) { await store.loadDrawing(item.id) }
    }
}

private struct FolderPreview: View {
    let item: ItemSummary
    let store: PreviewStore

    var body: some View {
        Group {
            if let entries = store.folders[item.id] {
                if entries.isEmpty {
                    Text("Empty").foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, minHeight: 60)
                } else {
                    List(entries) { entry in
                        Label(entry.name, systemImage: entry.isFolder ? "folder" : "doc")
                    }
                    .listStyle(.inset)
                }
            } else {
                KindIcon(kind: item.kind).frame(height: 120)
            }
        }
        .task(id: item.id) { await store.loadFolder(item.id) }
    }
}

/// Quick Look's preview of a file, which loads it in the background.
private struct QuickLookPreview: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> QLPreviewView {
        let view = QLPreviewView(frame: .zero, style: .compact) ?? QLPreviewView()
        view.shouldCloseWithWindow = false
        view.previewItem = url as NSURL
        return view
    }

    func updateNSView(_ view: QLPreviewView, context: Context) {
        if (view.previewItem as? NSURL) as URL? != url { view.previewItem = url as NSURL }
    }

    static func dismantleNSView(_ view: QLPreviewView, coordinator: ()) {
        view.close()
    }
}
