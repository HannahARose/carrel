// Carrel in Finder (docs/design/macos-integration.md, "Finder"): badges over every archive Carrel
// Agent follows and every working copy's and desk item's folder, and the archive's actions in the
// context menu. It's sandboxed and never writes the archive: it reads the agent's badge state
// file, sends requests over XPC, and opens Carrel for any action that needs a decision.

import AppKit
import CarrelKit
import FinderSync

/// Finder calls it from threads of its own, so what it read last is kept behind a lock.
nonisolated final class FinderSync: FIFinderSync, @unchecked Sendable {
    private let lock = NSLock()
    private var read = BadgeState()
    private var watcher: DispatchSourceFileSystemObject?

    private var state: BadgeState { lock.withLock { read } }

    override init() {
        super.init()
        let controller = FIFinderSyncController.default()
        for badge in FinderBadge.allCases {
            controller.setBadgeImage(badge.image, label: badge.label, forBadgeIdentifier: badge.id)
        }
        reload()
        watchState()
    }

    // MARK: - The state file

    /// Reads the agent's state again and watches the folders it names.
    private func reload() {
        let state = BadgeState.read(from: AgentService.badgeStateFile) ?? BadgeState()
        lock.withLock { read = state }
        FIFinderSyncController.default().directoryURLs = Set(
            state.folders.map { URL(filePath: $0, directoryHint: .isDirectory) })
    }

    /// Reloads whenever the agent writes the state file, which it replaces atomically, so the
    /// container folder is what's watched.
    private func watchState() {
        let folder = AgentService.container
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let descriptor = open(folder.path(percentEncoded: false), O_EVTONLY)
        guard descriptor >= 0 else { return }
        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: descriptor, eventMask: .write, queue: .main)
        source.setEventHandler { [weak self] in self?.reload() }
        source.setCancelHandler { close(descriptor) }
        source.resume()
        watcher = source
    }

    // MARK: - Badges

    override func requestBadgeIdentifier(for url: URL) {
        let path = url.resolvingSymlinksInPath().path(percentEncoded: false)
        let badge = state.badge(for: path).map { FinderBadge($0.badge) }
        FIFinderSyncController.default().setBadgeIdentifier(badge?.id ?? "", for: url)
    }

    // MARK: - The context menu

    override func menu(for menuKind: FIMenuKind) -> NSMenu? {
        guard menuKind == .contextualMenuForItems || menuKind == .contextualMenuForContainer
        else { return nil }
        let controller = FIFinderSyncController.default()
        let urls = controller.selectedItemURLs() ?? controller.targetedURL().map { [$0] } ?? []
        guard let url = urls.first?.resolvingSymlinksInPath(), urls.count == 1 else { return nil }
        let path = url.path(percentEncoded: false)
        guard let archive = state.archive(containing: path) else { return nil }
        let menu = NSMenu(title: "Carrel")
        for (index, action) in FinderAction.allCases.enumerated()
        where FinderAction.offered(for: state.badge(for: path)?.badge).contains(action) {
            let item = NSMenuItem(
                title: action.title, action: #selector(performCarrelAction(_:)), keyEquivalent: "")
            item.tag = index
            menu.addItem(item)
        }
        return menu
    }

    /// Runs the chosen action on the selection. Finder hands over a copy of the menu item, so
    /// the action travels in its tag and the item is read from the selection again.
    @objc private func performCarrelAction(_ sender: NSMenuItem) {
        let controller = FIFinderSyncController.default()
        guard FinderAction.allCases.indices.contains(sender.tag),
            let url = (controller.selectedItemURLs() ?? controller.targetedURL().map { [$0] })?
                .first?.resolvingSymlinksInPath()
        else { return }
        let path = url.path(percentEncoded: false)
        guard let archive = state.archive(containing: path) else { return }
        FinderAction.allCases[sender.tag].run(on: path, in: archive.root)
    }
}

/// A badge, and how Finder draws it.
nonisolated enum FinderBadge: String, CaseIterable {
    case sealed, checkedOut, workingCopy, tracked

    init(_ badge: BadgeState.Badge) {
        switch badge {
        case .sealed: self = .sealed
        case .checkedOut: self = .checkedOut
        case .workingCopy: self = .workingCopy
        case .tracked: self = .tracked
        }
    }

    var id: String { rawValue }

    var label: String {
        switch self {
        case .sealed: "Sealed"
        case .checkedOut: "Checked Out"
        case .workingCopy: "Working Copy"
        case .tracked: "On the Desk"
        }
    }

    var image: NSImage {
        let symbol =
            switch self {
            case .sealed: "lock.fill"
            case .checkedOut: "arrow.up.forward.circle.fill"
            case .workingCopy: "pencil.circle.fill"
            case .tracked: "eye.circle.fill"
            }
        return NSImage(systemSymbolName: symbol, accessibilityDescription: label) ?? NSImage()
    }
}

/// An action in Finder's context menu.
nonisolated enum FinderAction: CaseIterable {
    case checkOut, checkOutChoosing, returnItem, cancelCheckout, show, ask

    var title: String {
        switch self {
        case .checkOut: "Check Out to Desktop"
        case .checkOutChoosing: "Check Out…"
        case .returnItem: "Return…"
        case .cancelCheckout: "Cancel Checkout…"
        case .show: "Show in Carrel"
        case .ask: "Ask About This"
        }
    }

    /// The actions for the item at `path`, by its badge: a checked-out item or its working copy
    /// can be returned or its checkout cancelled, anything else in the archive checked out.
    static func offered(for badge: BadgeState.Badge?) -> [FinderAction] {
        switch badge {
        case .checkedOut, .workingCopy: [.returnItem, .cancelCheckout, .show, .ask]
        case .tracked: [.show, .ask]
        case .sealed, nil: [.checkOut, .checkOutChoosing, .show, .ask]
        }
    }
}

nonisolated extension FinderAction {
    /// Asks the agent to do it to the item at `path`, or to open Carrel on the item with the
    /// sheet it needs when it needs a decision. A refusal is shown with its reason.
    func run(on path: String, in archive: String) {
        let request: AgentRequest =
            switch self {
            case .checkOut: .checkOut(archive: archive, item: path, folder: nil, due: nil)
            case .show: .open(archive: archive, item: path)
            case .checkOutChoosing: .open(archive: archive, item: path, action: .checkOut)
            case .returnItem: .open(archive: archive, item: path, action: .returnItem)
            case .cancelCheckout: .open(archive: archive, item: path, action: .cancelCheckout)
            case .ask: .open(archive: archive, item: path, action: .ask)
            }
        Task { @MainActor in
            let reply = await AgentClient().send(request)
            guard reply.outcome != .done else { return }
            let alert = NSAlert()
            alert.messageText = "Carrel couldn’t do that"
            alert.informativeText = reply.message
            alert.runModal()
        }
    }
}
