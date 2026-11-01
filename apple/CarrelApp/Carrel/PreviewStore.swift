// Previews of items for Browse's grid and the inspector: thumbnails from Quick Look, Carrel's
// own drawings of STEP, IGES and 3MF files, and folders' contents, all made in the background.
//
// Thumbnails and drawings are kept in this device's preview cache, the archive's
// `_System/cache.nosync/previews/`, where the core says (`carrel_core::previews` documents the
// layout, which the interactive mode reads too). Nothing is kept for a restricted item.

import CarrelKit
import Foundation
import ImageIO
import Observation
import QuickLookThumbnailing
import UniformTypeIdentifiers

/// Makes a thumbnail of a file.
nonisolated protocol ThumbnailMaker: Sendable {
    /// A thumbnail of `file`, at most `pixels` on its longer side, or `nil` if there's none.
    func thumbnail(of file: URL, pixels: Int) async -> CGImage?
}

/// Thumbnails from Quick Look, as Finder shows them.
nonisolated struct QuickLookThumbnails: ThumbnailMaker {
    func thumbnail(of file: URL, pixels: Int) async -> CGImage? {
        let side = CGFloat(pixels) / 2
        let request = QLThumbnailGenerator.Request(
            fileAt: file, size: CGSize(width: side, height: side), scale: 2,
            representationTypes: .thumbnail)
        return try? await QLThumbnailGenerator.shared.generateBestRepresentation(for: request)
            .cgImage
    }
}

/// What the inspector shows above an item's card.
enum PreviewContent: Equatable {
    /// Nothing to show: a card-only item, or one whose file isn't there.
    case none
    /// A restricted item: its card only, with the preview a click away.
    case hidden
    /// Quick Look's preview of this file.
    case quickLook(URL)
    /// Carrel's drawing, for what Quick Look can't show.
    case drawing
    /// A folder's or bundle's contents.
    case folder(URL)
}

/// Something in a folder, as the inspector lists it.
struct FolderEntry: Hashable, Identifiable {
    let name: String
    let isFolder: Bool
    var id: String { name }
}

@Observable
final class PreviewStore {
    /// The longest side of a thumbnail, in pixels, as the core keeps them.
    static let pixels = Int(thumbnailPixels())

    let handle: ArchiveHandle
    private let maker: any ThumbnailMaker

    /// How to preview each item, by identifier.
    private(set) var previews: [String: ItemPreview] = [:]
    /// Thumbnails made or read so far, by item.
    private(set) var thumbnails: [String: CGImage] = [:]
    /// Carrel's drawings made or read so far, by item.
    private(set) var drawings: [String: CGImage] = [:]
    /// Folders' contents read so far, by item.
    private(set) var folders: [String: [FolderEntry]] = [:]
    /// The restricted item whose preview was asked for; choosing another hides it again.
    var revealedID: String?

    /// Items with nothing to show, or already being made, so each is tried once.
    private var tried: Set<String> = []

    init(handle: ArchiveHandle, maker: any ThumbnailMaker = QuickLookThumbnails()) {
        self.handle = handle
        self.maker = maker
    }

    /// Reads how to preview each item again, after items are filed, changed or moved. Pictures
    /// of items whose files changed are dropped, to be made again.
    func reload() async {
        let handle = handle
        guard let loaded = try? await Task.detached(operation: { try handle.previews() }).value
        else { return }
        let fresh = Dictionary(loaded.map { ($0.id, $0) }) { first, _ in first }
        for (id, old) in previews where fresh[id] != old {
            thumbnails[id] = nil
            drawings[id] = nil
            folders[id] = nil
            tried.remove(id)
        }
        previews = fresh
    }

    /// What the inspector shows above `item`'s card.
    func content(for item: ItemSummary) -> PreviewContent {
        guard let preview = previews[item.id], let file = preview.file else { return .none }
        if preview.restricted, revealedID != item.id { return .hidden }
        if item.kind == .folder || item.kind == .bundle { return .folder(URL(filePath: file)) }
        return preview.drawn ? .drawing : .quickLook(URL(filePath: file))
    }

    /// The picture Browse's grid shows for an item, once it's made.
    func picture(for id: String) -> CGImage? {
        thumbnails[id] ?? drawings[id]
    }

    /// Makes the grid's picture of item `id` in the background, or reads the kept one.
    /// Restricted items get none: the grid shows their kind's icon.
    func loadThumbnail(_ id: String) async {
        guard let preview = previews[id], !preview.restricted, preview.file != nil else { return }
        if preview.drawn { return await loadDrawing(id) }
        guard let file = preview.file, let kept = preview.thumbnail, thumbnails[id] == nil,
            tried.insert(id).inserted
        else { return }
        let maker = maker
        let pixels = Self.pixels
        let image = await Task.detached { () -> CGImage? in
            let keptURL = URL(filePath: kept)
            if let image = readImage(keptURL) { return image }
            guard let image = await maker.thumbnail(of: URL(filePath: file), pixels: pixels)
            else { return nil }
            try? writePNG(image, to: keptURL)
            return image
        }.value
        thumbnails[id] = image
    }

    /// Carrel's drawing of item `id`, from the cache or drawn by this device's providers.
    func loadDrawing(_ id: String) async {
        guard let preview = previews[id], preview.drawn, drawings[id] == nil,
            tried.insert(id).inserted
        else { return }
        let handle = handle
        let restricted = preview.restricted
        let image = await Task.detached { () -> CGImage? in
            guard let path = try? handle.drawPreview(id: id) else { return nil }
            let url = URL(filePath: path)
            // A restricted item's drawing is a temporary file: shown, never kept.
            defer { if restricted { try? FileManager.default.removeItem(at: url) } }
            return readImage(url)
        }.value
        drawings[id] = image
        if restricted { tried.remove(id) }
    }

    /// Reads the contents of folder or bundle item `id` in the background.
    func loadFolder(_ id: String) async {
        guard let file = previews[id]?.file, folders[id] == nil else { return }
        folders[id] = await Task.detached { folderEntries(URL(filePath: file)) }.value
    }

    /// Shows the preview of restricted item `id`.
    func reveal(_ id: String) {
        revealedID = id
    }

    /// Forgets a restricted item's preview once another item is chosen.
    func chose(_ id: String?) {
        guard let revealed = revealedID, revealed != id else { return }
        revealedID = nil
        drawings[revealed] = nil
    }
}

/// What's directly in `folder`, folders first, then by name, leaving out hidden files.
nonisolated func folderEntries(_ folder: URL) -> [FolderEntry] {
    let urls =
        (try? FileManager.default.contentsOfDirectory(
            at: folder, includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles])) ?? []
    return urls.map { url in
        FolderEntry(
            name: url.lastPathComponent,
            isFolder: (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true)
    }
    .sorted {
        $0.isFolder != $1.isFolder
            ? $0.isFolder : $0.name.localizedStandardCompare($1.name) == .orderedAscending
    }
}

/// The image in the file at `url`, if it's there and can be read.
nonisolated func readImage(_ url: URL) -> CGImage? {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
    return CGImageSourceCreateImageAtIndex(source, 0, nil)
}

/// Writes `image` to `url` as a PNG, all at once, so a reader never sees half of one.
nonisolated func writePNG(_ image: CGImage, to url: URL) throws {
    let folder = url.deletingLastPathComponent()
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let temporary = folder.appending(path: ".\(UUID().uuidString).png")
    guard
        let destination = CGImageDestinationCreateWithURL(
            temporary as CFURL, UTType.png.identifier as CFString, 1, nil)
    else { throw CocoaError(.fileWriteUnknown) }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else {
        try? FileManager.default.removeItem(at: temporary)
        throw CocoaError(.fileWriteUnknown)
    }
    try? FileManager.default.removeItem(at: url)
    try FileManager.default.moveItem(at: temporary, to: url)
}
