// The link the agent's menu bar list opens an item with, which the app answers by showing the
// archive's window with the item chosen: carrel://open?archive=<root>&item=<id>.

import Foundation

/// An archive, and optionally an item in it, for the app to show.
public struct AgentLink: Sendable, Equatable {
    /// The scheme the app registers.
    public static let scheme = "carrel"

    /// The archive's root folder.
    public var archive: URL
    /// The item's identifier, if one is to be chosen.
    public var item: String?

    public init(archive: URL, item: String? = nil) {
        self.archive = archive
        self.item = item
    }

    /// Reads a link, or `nil` if `url` isn't one.
    public init?(_ url: URL) {
        guard url.scheme == Self.scheme, url.host() == "open",
            let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
            let archive = items.first(where: { $0.name == "archive" })?.value, !archive.isEmpty
        else { return nil }
        self.archive = URL(filePath: archive)
        item = items.first { $0.name == "item" }?.value.flatMap { $0.isEmpty ? nil : $0 }
    }

    /// The link as a URL.
    public var url: URL {
        var components = URLComponents()
        components.scheme = Self.scheme
        components.host = "open"
        components.queryItems =
            [URLQueryItem(name: "archive", value: archive.path(percentEncoded: false))]
            + (item.map { [URLQueryItem(name: "item", value: $0)] } ?? [])
        return components.url!
    }
}
