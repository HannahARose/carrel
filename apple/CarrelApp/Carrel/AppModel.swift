// What the app keeps between windows: recent archives, the archive last in front (for
// Settings), and opening and creating archives.

import AppKit
import CarrelKit
import Observation

/// The archives opened most recently, newest first, kept in the app's defaults.
@Observable
final class RecentArchives {
    static let limit = 10
    private static let key = "recentArchives"

    private let defaults: UserDefaults
    private(set) var urls: [URL]

    init(defaults: UserDefaults = .app) {
        self.defaults = defaults
        urls = (defaults.stringArray(forKey: Self.key) ?? []).map { URL(filePath: $0) }
    }

    /// Puts `url` first.
    func note(_ url: URL) {
        let url = url.standardizedFileURL
        urls = Array(([url] + urls.filter { $0 != url }).prefix(Self.limit))
        save()
    }

    /// Forgets `url`, such as when it can no longer be opened.
    func remove(_ url: URL) {
        urls.removeAll { $0 == url.standardizedFileURL }
        save()
    }

    func clear() {
        urls = []
        save()
    }

    private func save() {
        defaults.set(urls.map { $0.path(percentEncoded: false) }, forKey: Self.key)
    }
}

@Observable
final class AppModel {
    let recents: RecentArchives
    /// The root of the archive whose window was last in front, which Settings shows.
    var activeArchive: URL?
    /// An item for its archive's window to choose, from a link the agent's menu opened.
    var pendingLink: AgentLink?
    /// A screen for an archive's window to show, such as Backups from Settings.
    var pendingScreen: PendingScreen?

    init(recents: RecentArchives = RecentArchives()) {
        self.recents = recents
    }

    /// Opens the archive containing `url`, notes it as recent, and returns its root.
    func resolve(_ url: URL) throws -> URL {
        let root = URL(filePath: try ArchiveHandle.open(url).root(), directoryHint: .isDirectory)
            .standardizedFileURL
        recents.note(root)
        return root
    }

    /// Creates an archive in the new or empty folder `url`, named after the folder, with
    /// `embeddingModel` as its embedding model (the default one when `nil`), notes it as
    /// recent, and returns its root.
    func create(at url: URL, embeddingModel: String? = nil) throws -> URL {
        let handle = try ArchiveHandle.createWithEmbeddingModel(
            path: url.path(percentEncoded: false), name: url.lastPathComponent,
            embeddingModel: embeddingModel ?? defaultEmbeddingModel())
        let root = URL(filePath: handle.root(), directoryHint: .isDirectory).standardizedFileURL
        recents.note(root)
        return root
    }

    // MARK: Panels and alerts

    func open(_ url: URL, then show: (URL) -> Void) {
        do {
            show(try resolve(url))
        } catch {
            if !FileManager.default.fileExists(atPath: url.path(percentEncoded: false)) {
                recents.remove(url)
            }
            Self.report(error, opening: url)
        }
    }

    /// Opens the archive a link from the agent names, then has its window choose the item.
    func open(link url: URL, then show: (URL) -> Void) {
        guard let link = AgentLink(url) else { return }
        open(link.archive) { root in
            pendingLink = link.item.map { AgentLink(archive: root, item: $0, action: link.action) }
            show(root)
        }
    }

    /// Has the window for the archive at `root` show `screen`, once it's in front.
    func show(_ screen: Screen, in root: URL) {
        pendingScreen = PendingScreen(archive: root.standardizedFileURL, screen: screen)
    }

    /// The screen the window for the archive at `root` was asked to show, once.
    func takePendingScreen(for root: URL) -> Screen? {
        guard let pending = pendingScreen, pending.archive == root.standardizedFileURL
        else { return nil }
        pendingScreen = nil
        return pending.screen
    }

    /// The item a link asked the window for the archive at `root` to choose, once.
    func takePendingLink(for root: URL) -> AgentLink? {
        guard let link = pendingLink, link.archive.standardizedFileURL == root.standardizedFileURL
        else { return nil }
        pendingLink = nil
        return link
    }

    func chooseArchive(then show: (URL) -> Void) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.message = "Choose an archive’s folder, or any folder inside it."
        panel.prompt = "Open"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        open(url, then: show)
    }

    func chooseNewArchive(then show: (URL) -> Void) {
        let panel = NSSavePanel()
        panel.title = "New Archive"
        panel.message = "The archive is a new folder, named after the archive."
        panel.nameFieldLabel = "Archive:"
        panel.nameFieldStringValue = "Archive"
        panel.prompt = "Create"
        panel.canCreateDirectories = true
        let models = EmbeddingModelPicker()
        panel.accessoryView = models.view
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            show(try create(at: url, embeddingModel: models.chosen))
        } catch {
            Self.report(error, opening: url)
        }
    }

    private static func report(_ error: any Error, opening url: URL) {
        let alert = NSAlert()
        alert.messageText = "Carrel can’t open “\(url.lastPathComponent)”"
        alert.informativeText = message(for: error)
        alert.runModal()
    }
}

/// New Archive's choice of embedding model, as `carrel init --embedding-model` takes it, for
/// the save panel's accessory view.
@MainActor
final class EmbeddingModelPicker {
    let view: NSView
    private let popUp: NSPopUpButton

    init() {
        popUp = NSPopUpButton(frame: .zero, pullsDown: false)
        popUp.addItems(withTitles: embeddingModelChoices())
        popUp.selectItem(withTitle: defaultEmbeddingModel())
        popUp.toolTip = "The model that finds items by meaning. It can be changed later."
        let label = NSTextField(labelWithString: "Embedding model:")
        let stack = NSStackView(views: [label, popUp])
        stack.edgeInsets = NSEdgeInsets(top: 8, left: 16, bottom: 8, right: 16)
        view = stack
    }

    /// The model chosen.
    var chosen: String? { popUp.titleOfSelectedItem }
}

/// What went wrong, in the core's own words for the core's errors.
func message(for error: any Error) -> String {
    (error as? CarrelError)?.message ?? error.localizedDescription
}

/// A screen an archive's window is asked to show.
struct PendingScreen: Equatable {
    var archive: URL
    var screen: Screen
}
