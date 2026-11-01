// The badge state file (docs/design/macos-integration.md, "One rule for every extension"): what
// the Finder extension needs to badge files without opening the archive. The agent keeps it in
// the app group container and writes it again whenever what it lists changes. Every path is
// absolute, with symbolic links resolved, as Finder reports them.

import Foundation

/// What the agent knows about every archive it follows, for badges.
public struct BadgeState: Codable, Sendable, Equatable {
    /// One archive.
    public struct Archive: Codable, Sendable, Equatable {
        /// An item checked out, and where it's being worked on.
        public struct Checkout: Codable, Sendable, Equatable {
            public var item: String
            public var title: String
            /// The archive copy, read-only until it's returned.
            public var path: String?
            /// The working copy.
            public var working: String

            public init(item: String, title: String, path: String?, working: String) {
                self.item = item
                self.title = title
                self.path = path
                self.working = working
            }
        }

        /// A desk item, tracked where it is.
        public struct Desk: Codable, Sendable, Equatable {
            public var item: String
            public var title: String
            public var path: String

            public init(item: String, title: String, path: String) {
                self.item = item
                self.title = title
                self.path = path
            }
        }

        public var root: String
        public var name: String
        /// The sealed bundles, which Carrel never enters.
        public var sealed: [String]
        public var checkouts: [Checkout]
        public var desk: [Desk]

        public init(
            root: String, name: String, sealed: [String] = [], checkouts: [Checkout] = [],
            desk: [Desk] = []
        ) {
            self.root = root
            self.name = name
            self.sealed = sealed
            self.checkouts = checkouts
            self.desk = desk
        }
    }

    /// What a path is, for its badge.
    public enum Badge: String, Codable, Sendable {
        /// A sealed bundle.
        case sealed
        /// The archive copy of a checked-out item.
        case checkedOut
        /// A checkout's working copy.
        case workingCopy
        /// A desk item.
        case tracked
    }

    /// The archives, in the order the agent follows them.
    public var archives: [Archive]
    /// Whether the agent was paused when it wrote this; badges stay, actions are refused.
    public var paused: Bool

    public init(archives: [Archive] = [], paused: Bool = false) {
        self.archives = archives
        self.paused = paused
    }

    /// The file's name in the app group container.
    public static let fileName = "badges.json"

    /// The folders the Finder extension watches: every archive, working copy and desk item.
    public var folders: [String] {
        var seen = Set<String>()
        return archives.flatMap { [$0.root] + $0.checkouts.map(\.working) + $0.desk.map(\.path) }
            .filter { seen.insert($0).inserted }
    }

    /// The badge for `path`, and the archive and item it belongs to, if it has one.
    public func badge(for path: String) -> (badge: Badge, archive: String, item: String?)? {
        for archive in archives {
            if let checkout = archive.checkouts.first(where: { $0.working == path }) {
                return (.workingCopy, archive.root, checkout.item)
            }
            if let checkout = archive.checkouts.first(where: { $0.path == path }) {
                return (.checkedOut, archive.root, checkout.item)
            }
            if let desk = archive.desk.first(where: { $0.path == path }) {
                return (.tracked, archive.root, desk.item)
            }
            if archive.sealed.contains(path) {
                return (.sealed, archive.root, nil)
            }
        }
        return nil
    }

    /// The archive `path` is in, or works on through a checkout or the desk.
    public func archive(containing path: String) -> Archive? {
        func inside(_ folder: String) -> Bool { path == folder || path.hasPrefix(folder + "/") }
        return archives.first { archive in
            inside(archive.root) || archive.checkouts.contains { inside($0.working) }
                || archive.desk.contains { inside($0.path) }
        }
    }

    // MARK: - The file

    /// The state in the file at `url`, or `nil` if there's none or it can't be read.
    public static func read(from url: URL) -> BadgeState? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        return try? JSONDecoder().decode(BadgeState.self, from: data)
    }

    /// Writes the state to `url` atomically, so a reader never sees half of it.
    public func write(to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(self).write(to: url, options: .atomic)
    }
}
