// The link the agent's menu bar list opens an item with, which the app answers by showing the
// archive's window with the item chosen: carrel://open?archive=<root>&item=<id>. The Finder
// extension adds `&action=` for an action that needs a decision in the app's own sheet, such as
// a return's preview.

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

    public init(archive: URL, item: String? = nil, action: Action? = nil) {
        self.archive = archive
        self.item = item
        self.action = action
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
        return components.url!
    }
}
