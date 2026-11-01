// The agent's side of the extensions' requests (docs/design/macos-integration.md): each one runs
// through the core, off the main thread, exactly as the matching CLI command does, against an
// archive the agent follows. The core takes the writer lease for each change; when another
// writer holds it, the request is refused with the core's reason, never queued silently. What's
// added is the exception: while another device is the writer, it goes into this Mac's inbox
// (FORMAT §7.8), which that device applies, and the answer says so.

import Foundation

/// Why the agent won't take a request.
public struct AgentRefusal: Error, Sendable, Equatable {
    public let reason: String

    public init(_ reason: String) {
        self.reason = reason
    }
}

/// Answers the extensions' requests for an agent.
@MainActor
public final class AgentRequestHandler {
    private let agent: Agent
    private let model: ModelProvider?
    private let memoryGB: UInt64?
    private let opener: @MainActor (AgentLink) -> Void
    private let sent: URL

    /// Answers for `agent`, asking the librarian with `model` (the device's own when `nil`),
    /// keeping shared text and data in `sent` (a folder in the app group's container unless
    /// given) and showing items in the app with `opener`.
    public init(
        agent: Agent, model: ModelProvider? = nil, memoryGB: UInt64? = nil, sent: URL? = nil,
        opener: @escaping @MainActor (AgentLink) -> Void
    ) {
        self.agent = agent
        self.sent = sent ?? AgentService.sentFolder
        self.model = model
        self.memoryGB = memoryGB ?? MacState.memoryGB
        self.opener = opener
    }

    /// Does what `request` asks, and says what happened, or why not.
    public func handle(_ request: AgentRequest) async -> AgentReply {
        let root: URL
        let handle: ArchiveHandle
        do {
            (root, handle) = try agent.archiveForRequest(request.archive, writes: request.writes)
        } catch {
            return .refused(error.reason, archive: request.archive)
        }
        let archive = root.path(percentEncoded: false)
        do {
            var reply = try await run(request, on: handle, root: root)
            reply.archive = archive
            if request.writes { await agent.requestChanged(root) }
            return reply
        } catch CarrelError.Busy(let message) {
            return .refused(message, archive: archive)
        } catch CarrelError.SentToInbox(let message) {
            // Taken, for the writer to apply: this device's inbox has it.
            if request.writes { await agent.requestChanged(root) }
            return AgentReply(.done, message, archive: archive)
        } catch {
            let message = (error as? CarrelError)?.message ?? error.localizedDescription
            return AgentReply(.failed, message, archive: archive)
        }
    }

    /// The item `item` names: an identifier as it is, or an absolute path resolved, as the
    /// CLI takes either.
    private func resolve(_ item: String, in root: URL) async throws -> String {
        guard item.hasPrefix("/") else { return item }
        guard let id = await agent.item(at: item, in: root) else {
            throw CarrelError.Invalid(
                message: "\(URL(filePath: item).lastPathComponent) isn’t an item in the archive.")
        }
        return id
    }

    private func run(_ request: AgentRequest, on handle: ArchiveHandle, root: URL) async throws
        -> AgentReply
    {
        switch request {
        case .addFiles(_, let paths, let each, let whole, let note):
            return try await addFiles(paths, each: each, whole: whole, note: note, on: handle)
        case .addText(_, let text, let title, let note):
            let name = Self.fileName(title ?? Self.firstWords(of: text), extension: "md")
            let file = try keep(Data(text.utf8), as: name)
            return try await addFiles([file], each: false, whole: true, note: note, on: handle)
        case .addData(_, let name, let data, let note):
            let file = try keep(data, as: Self.fileName(name, extension: nil))
            return try await addFiles([file], each: false, whole: true, note: note, on: handle)
        case .addURL(_, let url, let title, let note):
            let shared = try await Task.detached {
                try handle.shareUrl(url: url, title: title, note: note)
            }.value
            return Self.reply(to: shared, named: { _ in title ?? url })
        case .checkOut(_, let item, let folder, let due):
            let item = try await resolve(item, in: root)
            let out = try await Task.detached {
                try handle.checkOut(item: item, to: folder, due: due)
            }.value
            return AgentReply(
                .done, "Checked out “\(out.title)”." + (out.inbox.map { " " + $0 } ?? ""),
                items: [AgentFoundItem(id: out.item, title: out.title)], working: out.working)
        case .returnItem(_, let item, let from):
            let item = try await resolve(item, in: root)
            let returned = try await Task.detached {
                try handle.returnItem(item: item, from: from, trimSnapshots: false)
            }.value
            return AgentReply(.done, "Returned the working copy from \(returned.working).")
        case .cancelCheckout(_, let item):
            let item = try await resolve(item, in: root)
            _ = try await Task.detached { try handle.cancelCheckout(item: item) }.value
            return AgentReply(.done, "Cancelled the checkout; the archive copy is as it was.")
        case .open(_, let item, let action):
            var chosen: String?
            if let item { chosen = try await resolve(item, in: root) }
            opener(AgentLink(archive: root, item: chosen, action: chosen == nil ? nil : action))
            return AgentReply(.done, "Opened in Carrel.")
        case .search(_, let query, let limit):
            return try await search(query, limit: limit, on: handle)
        case .ask(_, let question):
            return try await ask(question, on: handle)
        case .snapshot:
            let taken = try await Task.detached { try handle.snapshotNow(password: nil) }.value
            return AgentReply(.done, "Took snapshot \(taken.snapshot).")
        case .items(_, let ids):
            let wanted = Set(ids)
            let items = try await Task.detached { try handle.items() }.value
                .filter { wanted.contains($0.id) }
                .map {
                    AgentFoundItem(
                        id: $0.id, title: $0.title,
                        path: $0.item.map { root.appending(path: $0).path(percentEncoded: false) })
                }
            return AgentReply(
                .done, items.isEmpty ? "No such items." : "Found \(items.count).", items: items)
        case .collections:
            let collections = try await Task.detached { try handle.collections() }.value
            return AgentReply(
                .done, "\(collections.count) collections.",
                items: collections.map {
                    AgentFoundItem(id: $0.path, title: $0.title, collection: $0.path)
                })
        case .active:
            let active = try await Task.detached { try handle.active() }.value
            let items = active.map { AgentFoundItem(id: $0.id, title: $0.title, path: $0.working) }
            let message =
                active.isEmpty
                ? "Nothing is checked out or on the desk."
                : active.map {
                    "\($0.title) (\($0.kind == .checkedOut ? "checked out" : "on the desk"))"
                }
                .joined(separator: ", ")
            return AgentReply(.done, message, items: items)
        }
    }

    private func addFiles(
        _ paths: [String], each: Bool, whole: Bool, note: String?, on handle: ArchiveHandle
    ) async throws -> AgentReply {
        let shared = try await Task.detached {
            try handle.sharePaths(paths: paths, each: each, whole: whole, note: note)
        }.value
        return Self.reply(to: shared, named: { URL(filePath: $0).lastPathComponent })
    }

    /// The answer to sharing what became `shared`: what joined the queue, what was left out,
    /// or that it went into this Mac's inbox while another device is the writer. `named`
    /// names an entry by its path.
    static func reply(to shared: Shared, named: (String) -> String) -> AgentReply {
        if let sent = shared.sent {
            return AgentReply(.done, sent)
        }
        if let writer = shared.writer {
            return AgentReply(
                .done,
                "Another device (\(writer.prefix(8))) is the writer, so this went into this Mac’s "
                    + "inbox; that device adds it to its queue.")
        }
        let added = shared.added
        // Until they're drafted, what's added is known by its intake entry.
        let items = added.added.map {
            AgentFoundItem(id: $0.id, title: named($0.path), path: $0.path)
        }
        let leftOut = added.leftOut.map {
            "\(URL(filePath: $0.path).lastPathComponent): \($0.reason)"
        }
        guard !items.isEmpty else {
            return AgentReply(
                .failed, "Nothing was added. " + leftOut.joined(separator: "; "), items: [])
        }
        let what = items.count == 1 ? "1 item" : "\(items.count) items"
        let message =
            "Added \(what) to the intake queue."
            + (leftOut.isEmpty ? "" : " Left out " + leftOut.joined(separator: "; ") + ".")
        return AgentReply(.done, message, items: items)
    }

    /// Saves what was shared as `name` in a folder of its own in `sent`, for the intake queue to
    /// take from there as it takes any file.
    private func keep(_ data: Data, as name: String) throws -> String {
        let folder = sent.appending(path: UUID().uuidString.lowercased())
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let file = folder.appending(path: name)
        try data.write(to: file, options: .atomic)
        return file.path(percentEncoded: false)
    }

    /// The first few words of `text`, to name a note by.
    static func firstWords(of text: String) -> String {
        let words = text.split(whereSeparator: \.isWhitespace).prefix(8).joined(separator: " ")
        return words.isEmpty ? "Note" : words
    }

    /// `name` made safe as a file name, with `extension` added unless it has one.
    static func fileName(_ name: String, extension wanted: String?) -> String {
        var safe = name.replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-")
            .trimmingCharacters(in: .whitespacesAndNewlines.union(CharacterSet(charactersIn: ".")))
        if safe.isEmpty { safe = "Shared" }
        if safe.count > 80 { safe = String(safe.prefix(80)) }
        if let wanted, URL(filePath: safe).pathExtension.isEmpty { safe += "." + wanted }
        return safe
    }

    private func search(_ query: String, limit: UInt32, on handle: ArchiveHandle) async throws
        -> AgentReply
    {
        let results = try await Task.detached {
            try handle.search(query: query, limit: limit, progress: nil)
        }.value
        let items = results.hits.compactMap { hit in
            hit.itemId.map {
                AgentFoundItem(id: $0, title: hit.title, collection: hit.collection, path: hit.item)
            }
        }
        let message =
            items.isEmpty
            ? "Nothing matched." : "Found \(results.total) for “\(query)”."
        return AgentReply(.done, message, items: items)
    }

    private func ask(_ question: String, on handle: ArchiveHandle) async throws -> AgentReply {
        let model = model
        let memory = memoryGB
        let answer = try await Task.detached {
            try handle.askLibrarian(
                conversation: LibrarianConversation(), question: question, model: model,
                memoryGb: memory, progress: nil)
        }.value
        let items = answer.items.map {
            AgentFoundItem(id: $0.id, title: $0.title, collection: $0.collection)
        }
        return AgentReply(.done, answer.text, items: items, answer: answer.text)
    }
}
