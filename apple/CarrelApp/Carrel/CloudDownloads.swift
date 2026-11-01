// iCloud Drive's evicted files in Browse (docs/design/sync-and-mobile.md, "iCloud Drive's
// files"), as `carrel sync download` and `carrel sync keep` offer them: Quick Look's preview
// downloads an evicted item first, an evicted item offers Download from iCloud, and a
// collection's inspector keeps its items downloaded on every device (`sync.keep_downloaded`)
// or downloads them now.

import CarrelKit
import SwiftUI

/// A collection's `sync.keep_downloaded`, and downloading its items now.
@Observable
final class CollectionSyncModel {
    let archive: ArchiveModel
    let path: String
    /// Whether it keeps its items downloaded, by its own setting or the collection above's.
    private(set) var keeps: Bool?
    /// Whether its items are being downloaded.
    private(set) var isDownloading = false
    /// How many files the last Download Now downloaded.
    private(set) var downloaded: UInt64?
    /// Why the setting couldn't be read or changed, or a download failed.
    private(set) var problem: String?

    init(archive: ArchiveModel, path: String) {
        self.archive = archive
        self.path = path
    }

    /// Reads the setting.
    func load() async {
        do {
            keeps = try await archive.keepsDownloaded(path)
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
    }

    /// Sets the collection's own `sync.keep_downloaded`, as one change undo can reverse; when
    /// on, its items are downloaded.
    func setKeeps(_ on: Bool) async {
        do {
            try await archive.setKeepDownloaded(path, to: on)
            problem = nil
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
        await load()
    }

    /// Downloads everything iCloud evicted in the collection.
    func downloadNow() async {
        isDownloading = true
        defer { isDownloading = false }
        do {
            downloaded = try await archive.download(collection: path)
            problem = nil
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
    }
}

/// The collection inspector's iCloud section.
struct CollectionSyncSection: View {
    let window: ArchiveWindowModel
    let collection: CollectionSummary

    @State private var model: CollectionSyncModel?

    var body: some View {
        Section {
            if let model {
                Toggle(
                    "Keep Downloaded",
                    isOn: Binding(
                        get: { model.keeps ?? false },
                        set: { on in Task { await model.setKeeps(on) } })
                )
                .disabled(window.isReadOnly || model.keeps == nil)
                LabeledContent {
                    Button("Download Now") { Task { await model.downloadNow() } }
                        .disabled(model.isDownloading)
                } label: {
                    if model.isDownloading {
                        ProgressView("Downloading…").controlSize(.small)
                    } else if let downloaded = model.downloaded {
                        Text(
                            downloaded == 0
                                ? "It’s all downloaded already."
                                : "Downloaded \(downloaded) files.")
                    } else {
                        Text("Download what iCloud removed from this Mac")
                    }
                }
                if let problem = model.problem {
                    Label(problem, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red).font(.callout)
                }
            }
        } header: {
            Text("iCloud")
        } footer: {
            Text(
                "Keep Downloaded keeps this collection’s items on every device, for using them offline. Otherwise iCloud may remove them to save space, and Carrel downloads each when it’s opened."
            )
            .foregroundStyle(.secondary)
        }
        .task(id: collection.path) {
            guard let archive = window.archive else { return }
            let model = CollectionSyncModel(archive: archive, path: collection.path)
            self.model = model
            await model.load()
        }
    }
}

/// Download from iCloud, for an item whose contents iCloud evicted.
struct ItemDownloadButton: View {
    let model: ArchiveWindowModel
    let item: ItemSummary

    var body: some View {
        if let contents = item.item, let archive = model.archive,
            CloudFiles.isEvicted(model.root.appending(path: contents))
        {
            Button("Download from iCloud", systemImage: "icloud.and.arrow.down") {
                Task {
                    try? await archive.download(item: item.id)
                    await model.refreshItems()
                }
            }
        }
    }
}

extension PreviewStore {
    /// Downloads the item `id`'s contents, off the main thread, if iCloud evicted them.
    func download(_ id: String) async throws {
        let handle = handle
        _ = try await Task.detached { try handle.downloadItem(item: id) }.value
    }
}

/// Quick Look's preview of an item, downloading its contents first if iCloud evicted them.
struct DownloadingQuickLook: View {
    let item: ItemSummary
    let url: URL
    let store: PreviewStore

    /// The item whose contents are here to preview.
    @State private var ready: String?
    @State private var problem: String?

    var body: some View {
        Group {
            if ready == item.id {
                QuickLookPreview(url: url)
            } else if let problem {
                VStack(spacing: 8) {
                    Image(systemName: "icloud.slash").font(.title2).foregroundStyle(.secondary)
                    Text(problem).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    Button("Try Again") { Task { await prepare() } }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView("Downloading from iCloud…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task(id: item.id) { await prepare() }
    }

    private func prepare() async {
        problem = nil
        guard CloudFiles.isEvicted(url) else {
            ready = item.id
            return
        }
        ready = nil
        do {
            try await store.download(item.id)
            ready = item.id
        } catch {
            problem = message(for: error)
        }
    }
}
