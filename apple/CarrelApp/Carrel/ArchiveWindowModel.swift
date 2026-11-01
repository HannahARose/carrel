// What an archive's window shows: the screens in the sidebar, the collection tree, the chosen
// screen's list and the chosen item's card.
//
// It reads the archive through the bindings CarrelKit has today (collections and items).
// CarrelKit's ArchiveModel (#233) takes over loading and the other screens once it lands.

import CarrelKit
import Foundation
import Observation

/// A screen in the sidebar, in the interactive mode's order.
enum Screen: Hashable {
    case status
    case review
    case questions
    case active
    /// Browse, showing the collection at this path.
    case collection(String)
    case history
}

/// A collection and the collections nested inside it.
struct CollectionNode: Identifiable, Hashable {
    let collection: CollectionSummary
    /// `nil` when it has none, so the sidebar shows no disclosure triangle.
    let children: [CollectionNode]?
    var id: String { collection.path }

    /// The tree of `collections`, from paths such as `Projects/Hardware`.
    static func tree(_ collections: [CollectionSummary]) -> [CollectionNode] {
        let paths = Set(collections.map(\.path))
        func parent(_ path: String) -> String? {
            var components = path.split(separator: "/")
            while components.count > 1 {
                components.removeLast()
                let candidate = components.joined(separator: "/")
                if paths.contains(candidate) { return candidate }
            }
            return nil
        }
        let byParent = Dictionary(grouping: collections) { parent($0.path) }
        func nodes(under path: String?) -> [CollectionNode] {
            (byParent[path] ?? [])
                .sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
                .map { collection in
                    let children = nodes(under: collection.path)
                    return CollectionNode(
                        collection: collection, children: children.isEmpty ? nil : children)
                }
        }
        return nodes(under: nil)
    }
}

/// What Status shows until the core's Status summary reaches CarrelKit (#233).
struct ArchiveSummary: Equatable {
    var itemCount: Int
    var collectionCount: Int
    /// Item counts by kind, most common first.
    var kinds: [(kind: ItemKind, count: Int)]
    var holdingCount: Int

    static func == (a: Self, b: Self) -> Bool {
        a.itemCount == b.itemCount && a.collectionCount == b.collectionCount
            && a.holdingCount == b.holdingCount
            && a.kinds.map(\.kind) == b.kinds.map(\.kind)
            && a.kinds.map(\.count) == b.kinds.map(\.count)
    }
}

@Observable
final class ArchiveWindowModel {
    let url: URL
    /// How the archive is opened; `nil` for the device's own configuration.
    let options: OpenOptions?
    private(set) var handle: ArchiveHandle?
    private(set) var name: String
    private(set) var isReadOnly = false
    private(set) var collections: [CollectionSummary] = []
    private(set) var tree: [CollectionNode] = []
    private(set) var items: [ItemSummary] = []
    private(set) var isLoading = false
    private(set) var problem: String?
    /// The archive's queues and operations, through the core, once it's open.
    private(set) var archive: ArchiveModel?

    // Review, Questions, corrections and editing (#234).
    private(set) var review: ReviewModel?
    private(set) var questions: QuestionsModel?
    /// The correction sheet's, while it's open.
    var correction: CorrectionModel?
    /// The filed item whose card the inspector is editing (⌘E).
    var editing: CardEditModel?

    // Active and History (#235).
    private(set) var activeScreen: ActiveScreenModel?
    private(set) var historyScreen: HistoryScreenModel?

    // Browse's actions: new collections, renaming and expanding (#268).
    private(set) var browse: BrowseActionsModel?

    // Previews and the grid (#256).
    private(set) var previews: PreviewStore?
    /// Browse's list or grid, kept for every window.
    var browseLayout: BrowseLayout {
        didSet { defaults.set(browseLayout.rawValue, forKey: "browseLayout") }
    }
    private let defaults: UserDefaults

    var screen: Screen? = .status {
        didSet { if screen != oldValue { selectedItemID = nil } }
    }
    var selectedItemID: ItemSummary.ID? {
        didSet { if selectedItemID != oldValue { editing = nil } }
    }

    init(url: URL, options: OpenOptions? = nil, defaults: UserDefaults = .standard) {
        self.url = url
        self.options = options
        self.defaults = defaults
        browseLayout =
            BrowseLayout(rawValue: defaults.string(forKey: "browseLayout") ?? "") ?? .list
        name = url.lastPathComponent
    }

    var root: URL { URL(filePath: handle?.root() ?? url.path(percentEncoded: false)) }

    /// Opens the archive and reads its collections and items, off the main thread.
    func load() async {
        isLoading = true
        defer { isLoading = false }
        let url = url
        let options = options
        do {
            let loaded = try await Task.detached {
                let handle =
                    if let options {
                        try ArchiveHandle.open(url, options: options)
                    } else {
                        try ArchiveHandle.open(url)
                    }
                return (handle, try handle.collections(), try handle.items())
            }.value
            handle = loaded.0
            name = loaded.0.name()
            isReadOnly = loaded.0.isReadOnly()
            collections = loaded.1
            tree = CollectionNode.tree(loaded.1)
            items = loaded.2
            problem = nil
            let archive = ArchiveModel(handle: loaded.0)
            self.archive = archive
            review = ReviewModel(archive: archive)
            review?.filed = { [weak self] in await self?.refreshItems() }
            questions = QuestionsModel(archive: archive)
            activeScreen = ActiveScreenModel(archive: archive) { [weak self] in
                await self?.refreshItems()
            }
            historyScreen = HistoryScreenModel(archive: archive) { [weak self] in
                await self?.refreshItems()
            }
            let browse = BrowseActionsModel(archive: archive) { [weak self] in
                await self?.refreshItems()
            }
            browse.created = { [weak self] path in self?.screen = .collection(path) }
            self.browse = browse
            let previews = PreviewStore(handle: loaded.0)
            self.previews = previews
            await previews.reload()
            try? await archive.refreshAll()
        } catch {
            problem = message(for: error)
        }
    }

    /// Reads the collections and items again, after a change that files or refiles items.
    func refreshItems() async {
        guard let handle else { return }
        guard
            let loaded = try? await Task.detached(operation: {
                (try handle.collections(), try handle.items())
            }).value
        else { return }
        collections = loaded.0
        tree = CollectionNode.tree(loaded.0)
        items = loaded.1
        await previews?.reload()
    }

    /// The collection an item's card is filed in: the deepest collection folder holding it.
    func collectionPath(of item: ItemSummary) -> String? {
        let folder = (item.card as NSString).deletingLastPathComponent
        return collections.map(\.path)
            .filter { folder == $0 || folder.hasPrefix($0 + "/") }
            .max { $0.count < $1.count }
    }

    /// The items filed directly in the collection at `path`, by title.
    func items(in path: String) -> [ItemSummary] {
        items.filter { collectionPath(of: $0) == path }
            .sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
    }

    /// The content column's list for the chosen screen.
    var listedItems: [ItemSummary] {
        if case .collection(let path) = screen { items(in: path) } else { [] }
    }

    /// The chosen screen's name, shown under the archive's in the title bar.
    var screenTitle: String {
        switch screen {
        case .status, nil: "Status"
        case .review: "Review"
        case .questions: "Questions"
        case .active: "Active"
        case .collection(let path): collections.first { $0.path == path }?.title ?? path
        case .history: "History"
        }
    }

    var selectedItem: ItemSummary? {
        selectedItemID.flatMap { id in items.first { $0.id == id } }
    }

    var summary: ArchiveSummary {
        let kinds = Dictionary(grouping: items, by: \.kind).mapValues(\.count)
            .sorted { $0.value > $1.value || ($0.value == $1.value && "\($0.key)" < "\($1.key)") }
            .map { (kind: $0.key, count: $0.value) }
        return ArchiveSummary(
            itemCount: items.count, collectionCount: collections.count, kinds: kinds,
            holdingCount: items(in: "Holding").count)
    }

    /// The card of `item`, read from its file.
    func card(for item: ItemSummary) -> Card? {
        let url = root.appending(path: item.card)
        return (try? String(contentsOf: url, encoding: .utf8)).map(Card.init(text:))
    }
}
