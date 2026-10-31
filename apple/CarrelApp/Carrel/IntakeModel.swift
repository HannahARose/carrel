// Adding items: files and folders dropped on the window or chosen with File → Add to
// Archive… join the intake queue, as `carrel ingest` adds them, and a worker drafts them. The
// agent runs the worker; Status shows its progress, read from the worker's status file.

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
    var workerLine: String? {
        guard let worker else { return nil }
        let stage = worker.stage.map { " (\($0))" } ?? ""
        return "Drafting \(worker.item ?? "…")\(stage), \(worker.waiting) waiting"
    }

    /// Adds the files and folders at `urls` to the intake queue.
    func add(_ urls: [URL]) async {
        guard !urls.isEmpty else { return }
        do {
            report = AddReport(try await archive.add(urls))
        } catch {
            report = AddReport(problem: message(for: error))
        }
        await refresh()
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

/// What adding to the intake queue did, as `carrel ingest` reports it.
struct AddReport: Equatable {
    /// The names of the items added.
    var added: [String] = []
    /// The paths left out, with why.
    var leftOut: [(name: String, reason: String)] = []
    /// Why nothing could be added.
    var problem: String?

    init(_ added: Added) {
        self.added = added.added.map { URL(filePath: $0.path).lastPathComponent }
        leftOut = added.leftOut.map { (URL(filePath: $0.path).lastPathComponent, $0.reason) }
    }

    init(problem: String) {
        self.problem = problem
    }

    var title: String {
        if problem != nil { return "Couldn’t Add to the Archive" }
        return switch added.count {
        case 0: "Nothing to Add"
        case 1: "Added “\(added[0])” to the Intake Queue"
        default: "Added \(added.count) Items to the Intake Queue"
        }
    }

    /// What was left out and why, one to a line, or what went wrong.
    var details: String? {
        if let problem { return problem }
        guard !leftOut.isEmpty else { return nil }
        return leftOut.map { "\($0.name): \($0.reason)" }.joined(separator: "\n")
    }

    static func == (a: Self, b: Self) -> Bool {
        a.added == b.added && a.problem == b.problem
            && a.leftOut.map(\.name) == b.leftOut.map(\.name)
            && a.leftOut.map(\.reason) == b.leftOut.map(\.reason)
    }
}
