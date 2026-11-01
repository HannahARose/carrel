// What the macOS extensions ask the agent for, and its answers (docs/design/macos-integration.md,
// "One rule for every extension"). Extensions are sandboxed and never write the archive: they
// send one of these over XPC, and the agent does the work through the core and the writer
// lease, as the matching CLI command does. Both travel as JSON, so either side can be built
// alone.

import Foundation

/// A request to the agent. `archive` is the archive's root folder; when it's `nil` the request
/// goes to the first archive the agent follows, the one the share sheet names. An `item` is its
/// identifier, or an absolute path to its contents, its card or its working copy.
public enum AgentRequest: Codable, Sendable, Equatable {
    /// Adds files and folders to the intake queue, as `carrel ingest` does. A folder is one
    /// item when `whole`, entry by entry when `each`, and as detection decides otherwise.
    case addFiles(archive: String?, paths: [String], each: Bool, whole: Bool)
    /// Adds shared text as a Markdown note item, titled `title` if given, through the intake
    /// queue as a file would go.
    case addText(archive: String?, text: String, title: String?)
    /// Adds a shared file's contents, such as an image another app shares, as the file `name`,
    /// through the intake queue. The extension sends the bytes, since it can't share files.
    case addData(archive: String?, name: String, data: Data)
    /// Checks an item out, as `carrel checkout` does: to `folder` (the checkout folder
    /// setting when `nil`), due back on `due` (`YYYY-MM-DD`) if given.
    case checkOut(archive: String?, item: String, folder: String?, due: String?)
    /// Returns a checked-out item, as `carrel return` does, from `from` if the working copy
    /// has moved.
    case returnItem(archive: String?, item: String, from: String?)
    /// Cancels a checkout, as `carrel return --discard` does.
    case cancelCheckout(archive: String?, item: String)
    /// Shows the archive, with `item` chosen if given, in Carrel's window, starting `action`
    /// on it in the app's sheet for it if given.
    case open(archive: String?, item: String?, action: AgentLink.Action? = nil)
    /// Searches, as `carrel search` does.
    case search(archive: String?, query: String, limit: UInt32)
    /// Asks the librarian, as `carrel ask` does.
    case ask(archive: String?, question: String)
    /// Takes a backup snapshot now, as `carrel backup now` does.
    case snapshot(archive: String?)

    /// The archive the request names, if it names one.
    public var archive: String? {
        switch self {
        case .addFiles(let archive, _, _, _), .addText(let archive, _, _),
            .addData(let archive, _, _), .checkOut(let archive, _, _, _),
            .returnItem(let archive, _, _), .cancelCheckout(let archive, _),
            .open(let archive, _, _), .search(let archive, _, _), .ask(let archive, _),
            .snapshot(let archive):
            archive
        }
    }

    /// Whether it changes the archive, and so needs the agent's claim on it.
    public var writes: Bool {
        switch self {
        case .open, .search, .ask: false
        case .addFiles, .addText, .addData, .checkOut, .returnItem, .cancelCheckout, .snapshot: true
        }
    }
}

/// An item an answer is about.
public struct AgentFoundItem: Codable, Sendable, Equatable {
    public var id: String
    public var title: String
    /// The collection it's in, when known.
    public var collection: String?
    /// The item's own file or folder, when it has one.
    public var path: String?

    public init(id: String, title: String, collection: String? = nil, path: String? = nil) {
        self.id = id
        self.title = title
        self.collection = collection
        self.path = path
    }
}

/// The agent's answer to a request.
public struct AgentReply: Codable, Sendable, Equatable {
    /// How it went.
    public enum Outcome: String, Codable, Sendable {
        /// Done, as `message` says.
        case done
        /// Not taken, for the reason in `message`: the agent is paused, not running, doesn't
        /// follow that archive, or another writer has it. Nothing changed.
        case refused
        /// Taken, but the core couldn't do it, for the reason in `message`.
        case failed
    }

    public var outcome: Outcome
    /// What happened, or why not, for people.
    public var message: String
    /// The archive it went to.
    public var archive: String?
    /// The items it's about: those added, checked out or found.
    public var items: [AgentFoundItem]
    /// A working copy's folder, for a checkout.
    public var working: String?
    /// The librarian's answer, for a question.
    public var answer: String?

    public init(
        _ outcome: Outcome, _ message: String, archive: String? = nil,
        items: [AgentFoundItem] = [], working: String? = nil, answer: String? = nil
    ) {
        self.outcome = outcome
        self.message = message
        self.archive = archive
        self.items = items
        self.working = working
        self.answer = answer
    }

    /// A refusal, saying why.
    public static func refused(_ why: String, archive: String? = nil) -> AgentReply {
        AgentReply(.refused, why, archive: archive)
    }

    /// The reply when the agent can't be reached at all.
    public static let notRunning = AgentReply.refused(
        "Carrel Agent isn’t running. Turn it on in Carrel’s Settings → Agent.")
}
