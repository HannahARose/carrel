// The link the agent's menu bar list opens an item with, which the app answers by showing the
// archive's window with the item chosen: carrel://open?archive=<root>&item=<id>. The Finder
// extension adds `&action=` for an action that needs a decision in the app's own sheet, such as
// a return's preview, and `carrel gui` adds `&screen=` for the screen to show, named as
// `carrel tui` names them.

import Foundation

/// An archive, and optionally an item in it, for the app to show.
public struct AgentLink: Sendable, Equatable {
    /// The scheme the app registers.
    public static let scheme = "carrel"

    /// The archive's root folder.
    public var archive: URL
    /// The item's identifier, if one is to be chosen.
    public var item: String?
    /// What to start on the item, if anything.
    public var action: Action?
    /// The screen to show, if one is asked for.
    public var screen: Screen?

    /// What a link can start on its item, each in the app's sheet for it.
    public enum Action: String, Codable, Sendable {
        /// Check Out…: where to, and a due date.
        case checkOut = "checkout"
        /// Return…: the return's preview.
        case returnItem = "return"
        /// Cancel Checkout…: the confirmation.
        case cancelCheckout = "cancel"
        /// Ask About This: the librarian, with the item in the question.
        case ask
    }

    /// A screen a link can ask for, with the names `carrel gui` and `carrel tui` use.
    public enum Screen: Sendable, Equatable, Hashable {
        case status, review, active, questions, browse, history, ingest, settings, backups
        case librarian
        /// Discover, on a view when one is named.
        case discover(DiscoverView?)

        /// Discover's views.
        public enum DiscoverView: String, Sendable, CaseIterable {
            case today, wander, timeline, map, activity, graph
        }

        /// Reads a name such as `review` or `discover:wander`, or `nil` for one it doesn't know.
        public init?(name: String) {
            if name == "discover" {
                self = .discover(nil)
                return
            }
            if name.hasPrefix("discover:") {
                guard let view = DiscoverView(rawValue: String(name.dropFirst("discover:".count)))
                else { return nil }
                self = .discover(view)
                return
            }
            let screens: [String: Self] = [
                "status": .status, "review": .review, "active": .active,
                "questions": .questions, "browse": .browse, "history": .history,
                "ingest": .ingest, "settings": .settings, "backups": .backups,
                "librarian": .librarian,
            ]
            guard let screen = screens[name] else { return nil }
            self = screen
        }

        /// Its name, as the link carries it.
        public var name: String {
            switch self {
            case .status: "status"
            case .review: "review"
            case .active: "active"
            case .questions: "questions"
            case .browse: "browse"
            case .history: "history"
            case .ingest: "ingest"
            case .settings: "settings"
            case .backups: "backups"
            case .librarian: "librarian"
            case .discover(nil): "discover"
            case .discover(let view?): "discover:\(view.rawValue)"
            }
        }
    }

    public init(
        archive: URL, item: String? = nil, action: Action? = nil, screen: Screen? = nil
    ) {
        self.archive = archive
        self.item = item
        self.action = action
        self.screen = screen
    }

    /// Reads a link, or `nil` if `url` isn't one.
    public init?(_ url: URL) {
        guard url.scheme == Self.scheme, url.host() == "open",
            let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
            let archive = items.first(where: { $0.name == "archive" })?.value, !archive.isEmpty
        else { return nil }
        self.archive = URL(filePath: archive)
        item = items.first { $0.name == "item" }?.value.flatMap { $0.isEmpty ? nil : $0 }
        action = items.first { $0.name == "action" }?.value.flatMap(Action.init(rawValue:))
        screen = items.first { $0.name == "screen" }?.value.flatMap(Screen.init(name:))
    }

    /// The link as a URL.
    public var url: URL {
        var components = URLComponents()
        components.scheme = Self.scheme
        components.host = "open"
        components.queryItems =
            [URLQueryItem(name: "archive", value: archive.path(percentEncoded: false))]
            + (item.map { [URLQueryItem(name: "item", value: $0)] } ?? [])
            + (action.map { [URLQueryItem(name: "action", value: $0.rawValue)] } ?? [])
            + (screen.map { [URLQueryItem(name: "screen", value: $0.name)] } ?? [])
        return components.url!
    }
}
