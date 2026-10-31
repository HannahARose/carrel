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

    init(defaults: UserDefaults = .standard) {
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

    /// Creates an archive in the new or empty folder `url`, named after the folder, notes it
    /// as recent, and returns its root.
    func create(at url: URL) throws -> URL {
        let handle = try ArchiveHandle.create(
            path: url.path(percentEncoded: false), name: url.lastPathComponent)
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
            pendingLink = link.item.map { AgentLink(archive: root, item: $0) }
            show(root)
        }
    }

    /// The item a link asked the window for the archive at `root` to choose, once.
    func takePendingItem(for root: URL) -> String? {
        guard let link = pendingLink, link.archive.standardizedFileURL == root.standardizedFileURL
        else { return nil }
        pendingLink = nil
        return link.item
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
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            show(try create(at: url))
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

/// What went wrong, in the core's own words for the core's errors.
func message(for error: any Error) -> String {
    switch error {
    case CarrelError.NotAnArchive(let message), CarrelError.Failed(let message),
        CarrelError.Callback(let message):
        message
    default:
        error.localizedDescription
    }
}
