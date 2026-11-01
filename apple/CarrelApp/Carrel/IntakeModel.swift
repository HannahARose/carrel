// Adding items: files and folders dropped on the window or chosen with File → Add to
// Archive…, with the owner's note, a web address (File → Add Web Address…) and a note written
// in the window (File → Add Note…), or any of these pasted onto it, join the intake queue as
// `carrel ingest` adds them, and a worker drafts them. While another device is the writer they
// go into this Mac's inbox instead. The agent runs the worker; Status shows its progress, read
// from the worker's status file.

import AppKit
import CarrelKit
import Foundation
import Observation

@Observable
final class IntakeModel {
    let archive: ArchiveModel

    /// What the worker is doing, while one is running for this archive on this device.
    private(set) var worker: WorkerStatus?
    /// What the last add did, in words, until it's dismissed.
    var report: AddReport?

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    /// The intake queue on this device.
    var queue: IntakeQueue { archive.intake }

    /// Whether this window is drafting what's waiting, with no agent to.
    private(set) var isDrafting = false

    /// Whether drafting can start here: items wait and no worker is drafting them.
    var canDraft: Bool { worker == nil && !isDrafting && !queue.waiting.isEmpty }

    /// The worker's activity, in the interactive mode's words, while it's running.
    var workerLine: String? { worker.map(Self.line(for:)) }

    static func line(for worker: WorkerStatus) -> String {
        let stage = worker.stage.map { " (\($0))" } ?? ""
        return "Drafting \(worker.item ?? "…")\(stage), \(worker.waiting) waiting"
    }

    /// What's being added, while the owner writes a note for it and, for folders, chooses how
    /// they go in.
    var adding: PendingAdd?

    /// Starts adding the files and folders at `urls`: the add sheet asks for a note, and how a
    /// folder among them goes in.
    func add(_ urls: [URL]) async {
        guard !urls.isEmpty else { return }
        adding = PendingAdd(urls: urls, hasFolders: urls.contains(where: Self.isFolder))
    }

    /// Adds the files and folders at `urls` to the intake queue, folders as `how` says, as
    /// `carrel ingest --each` and `--whole` do, with the owner's `note` for their cards. While
    /// another device is the writer, they go into this Mac's inbox.
    func add(_ urls: [URL], as how: FolderIntake, note: String? = nil) async {
        adding = nil
        guard !urls.isEmpty else { return }
        await report {
            try await $0.share(urls, each: how == .each, whole: how == .whole, note: note)
        } named: {
            URL(filePath: $0).lastPathComponent
        }
    }

    /// Adds the web address `address` as a `url` item, with its `title` and the owner's
    /// `note`, as `carrel ingest --url` does.
    func addWebAddress(_ address: String, title: String, note: String) async {
        writing = nil
        let title = title.cleanedForIntake
        await report {
            try await $0.shareWebAddress(
                address.trimmingCharacters(in: .whitespacesAndNewlines), title: title,
                note: note.cleanedForIntake)
        } named: {
            title ?? $0
        }
    }

    /// Adds `text` as a note item of its own, with its `title` and the owner's `note`, as
    /// `carrel ingest --text` does.
    func addNote(_ text: String, title: String, note: String) async {
        writing = nil
        await report {
            try await $0.shareNote(text, title: title.cleanedForIntake, note: note.cleanedForIntake)
        } named: {
            URL(filePath: $0).lastPathComponent
        }
    }

    /// Runs `share` and keeps what it did as `report`, naming each entry by its path with
    /// `named`.
    private func report(
        _ share: (ArchiveModel) async throws -> Shared, named: (String) -> String
    ) async {
        do {
            report = AddReport(try await share(archive), named: named)
        } catch {
            report = AddReport(problem: message(for: error))
        }
        await refresh()
    }

    // MARK: - Writing a web address or a note

    /// The web address or note sheet, while it's open.
    var writing: Writing?

    /// Adds what's on `pasteboard`, as pasting onto the window does: files and folders go to
    /// the add sheet, a web address to Add Web Address and text to Add Note, filled in for the
    /// owner to add. Returns whether there was anything to add.
    @discardableResult
    func paste(from pasteboard: NSPasteboard) async -> Bool {
        let files =
            pasteboard.readObjects(
                forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
            ?? []
        if !files.isEmpty {
            await add(files)
            return true
        }
        let text =
            pasteboard.string(forType: .URL) ?? pasteboard.string(forType: .string) ?? ""
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return false }
        writing =
            Self.isWebAddress(trimmed)
            ? Writing(kind: .webAddress, text: trimmed) : Writing(kind: .note, text: text)
        return true
    }

    /// Whether `text` is one `http` or `https` address, as the interactive mode's Ingest line
    /// tells them from paths.
    static func isWebAddress(_ text: String) -> Bool {
        let lower = text.lowercased()
        return !text.contains(where: \.isWhitespace)
            && (lower.hasPrefix("https://") || lower.hasPrefix("http://")) && lower.count > 8
    }

    private static func isFolder(_ url: URL) -> Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(
            atPath: url.path(percentEncoded: false), isDirectory: &isDirectory)
            && isDirectory.boolValue
    }

    // MARK: - The queue

    /// Takes `entry` out of the queue.
    func remove(_ entry: Queued) async {
        await change { try await $0.unqueue(entry.id) }
    }

    /// Moves `entry` to the front of the queue, to be drafted next.
    func moveToFront(_ entry: Queued) async {
        await change { try await $0.queueFirst(entry.id) }
    }

    /// Lets the failed `entry` be tried again.
    func tryAgain(_ entry: Queued) async {
        await change { try await $0.retryQueued(entry.id) }
    }

    /// Runs `work`; a failure shows in the window's alert, with the core's message.
    private func change(_ work: (ArchiveModel) async throws -> Void) async {
        try? await work(archive)
    }

    /// Reads the queue and the worker's status again.
    func refresh() async {
        try? await archive.refreshIntake()
        worker = await readWorker()
    }

    /// Reads the worker's status every `interval` until cancelled, and the queue again when
    /// the worker starts, stops or finishes an item, as the interactive mode polls it.
    func watch(every interval: Duration = .seconds(1)) async {
        await refresh()
        while !Task.isCancelled {
            try? await Task.sleep(for: interval)
            let status = await readWorker()
            let changed = (status == nil) != (worker == nil) || status?.done != worker?.done
            worker = status
            if changed { try? await archive.refreshIntake() }
        }
    }

    /// Drafts what's waiting here, when no worker, such as the agent's, is running.
    func draft() async {
        isDrafting = true
        defer { isDrafting = false }
        _ = try? await archive.draftQueued()
        await refresh()
    }

    private func readWorker() async -> WorkerStatus? {
        let handle = archive.handle
        return await Task.detached { handle.workerStatus() }.value
    }
}

/// How a folder joins the intake queue.
enum FolderIntake {
    /// As Carrel decides: whole, unless its name suggests a pile, such as Downloads.
    case decide
    /// Each entry inside it as its own item (`carrel ingest --each`).
    case each
    /// As one item, whatever its name (`carrel ingest --whole`).
    case whole
}

/// What adding to the intake queue did, as `carrel ingest` reports it.
struct AddReport: Equatable {
    /// The names of the items added.
    var added: [String] = []
    /// The paths left out, with why.
    var leftOut: [(name: String, reason: String)] = []
    /// Why nothing could be added.
    var problem: String?
    /// The writing device, when it went into this Mac's inbox instead.
    var writer: String?

    init(_ added: Added) {
        self.init(Shared(added: added, inbox: nil, writer: nil)) {
            URL(filePath: $0).lastPathComponent
        }
    }

    /// What `shared` did, naming each entry by its path with `named`.
    init(_ shared: Shared, named: (String) -> String) {
        added = shared.added.added.map { named($0.path) }
        leftOut = shared.added.leftOut.map { (URL(filePath: $0.path).lastPathComponent, $0.reason) }
        writer = shared.writer
    }

    init(problem: String) {
        self.problem = problem
    }

    var title: String {
        if problem != nil { return "Couldn’t Add to the Archive" }
        if writer != nil { return "Sent to This Mac’s Inbox" }
        return switch added.count {
        case 0: "Nothing to Add"
        case 1: "Added “\(added[0])” to the Intake Queue"
        default: "Added \(added.count) Items to the Intake Queue"
        }
    }

    /// What was left out and why, one to a line, or what went wrong.
    var details: String? {
        if let problem { return problem }
        if let writer {
            return "Another device (\(writer.prefix(8))) is the writer now, so this went into "
                + "this Mac’s inbox; that device adds it to its queue and drafts it."
        }
        guard !leftOut.isEmpty else { return nil }
        return leftOut.map { "\($0.name): \($0.reason)" }.joined(separator: "\n")
    }

    static func == (a: Self, b: Self) -> Bool {
        a.added == b.added && a.problem == b.problem && a.writer == b.writer
            && a.leftOut.map(\.name) == b.leftOut.map(\.name)
            && a.leftOut.map(\.reason) == b.leftOut.map(\.reason)
    }
}

/// Files and folders being added, while the owner writes a note for them and, for folders,
/// chooses how they go in.
struct PendingAdd: Identifiable, Equatable {
    let id = UUID()
    /// What's being added.
    var urls: [URL]
    /// Whether a folder is among them, so the sheet asks how folders go in.
    var hasFolders: Bool
}

/// A web address or a note being written in the window, before it's added.
struct Writing: Identifiable, Equatable {
    enum Kind {
        /// File → Add Web Address…
        case webAddress
        /// File → Add Note…
        case note
    }

    let id = UUID()
    var kind: Kind
    /// What's filled in: the address, or the note's text.
    var text = ""
}

extension String {
    /// Trimmed, or `nil` when there's nothing in it, as the core takes an optional title or
    /// note.
    fileprivate var cleanedForIntake: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
