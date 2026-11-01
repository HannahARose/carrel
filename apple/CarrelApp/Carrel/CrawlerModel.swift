// The crawler in the window (docs/design/restructure.md, "Crawlers"): its findings in Review,
// under "From the crawler" (`f`, as in the interactive mode), approved or rejected; how far it
// has got and the changes it made on its own this week, on Status, undone together.

import CarrelKit
import Foundation
import Observation

@MainActor
@Observable
final class CrawlerModel {
    /// How far back its changes are listed: a week, as `carrel status` lists them.
    static let recentDays: Double = 7

    let archive: ArchiveModel

    /// Whether Review shows the crawler's findings rather than the proposals.
    var isShown = false
    /// The findings chosen in the list.
    var selection: Set<CrawlerFinding.ID> = []
    /// How far it has got, once read.
    private(set) var status: CrawlerStatus?
    /// The changes it made on its own this week, newest first.
    private(set) var changes: [Change] = []
    /// What the last action did.
    private(set) var report: String?
    /// The model to make corrections with; `nil` for this Mac's.
    @ObservationIgnored var provider: ModelProvider?
    /// Called after an approval or an undo changed the archive, so the window follows.
    @ObservationIgnored var didChange: () async -> Void = {}

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    var findings: [CrawlerFinding] { archive.crawlerFindings }

    /// The chosen findings, in the list's order.
    var selected: [CrawlerFinding] { findings.filter { selection.contains($0.id) } }

    /// The finding the inspector shows: the chosen one, when one is.
    var inspected: CrawlerFinding? { selected.count == 1 ? selected.first : nil }

    /// Switches Review between the proposals and the crawler's findings.
    func toggle() {
        isShown.toggle()
        selection = []
    }

    /// Reads how far it has got and its changes this week.
    func refresh() async {
        status = try? await archive.crawlerStatus()
        changes = (try? await archive.crawlerChanges(since: Self.since)) ?? []
    }

    /// Approves `ids`, each as the owner's change.
    func approve(_ ids: [CrawlerFinding.ID]) async {
        report = nil
        var done = 0
        var problems: [String] = []
        for id in ids {
            let title = findings.first { $0.id == id }?.description ?? id
            do {
                try await archive.approve(finding: id, model: provider)
                done += 1
            } catch {
                archive.lastError = nil
                problems.append("Couldn’t \(title): \(message(for: error))")
            }
        }
        selection.subtract(ids)
        var said = done == 0 ? [] : [done == 1 ? "Made 1 change" : "Made \(done) changes"]
        said += problems
        report = said.joined(separator: ". ")
        if done > 0 { await didChange() }
    }

    /// Rejects `ids`: nothing changes, and the crawler won't suggest them again.
    func reject(_ ids: [CrawlerFinding.ID]) async {
        report = nil
        for id in ids {
            guard (try? await archive.reject(finding: id)) != nil else { return }
        }
        selection.subtract(ids)
        report =
            ids.count == 1
            ? "Rejected; the crawler won’t suggest it again."
            : "Rejected \(ids.count); the crawler won’t suggest them again."
    }

    /// Lets it check a few items now, whatever its pace, as `carrel crawl run` does.
    func checkNow() async {
        report = nil
        do {
            let passed = try await archive.crawlNow(
                model: provider, memoryGB: LibrarianModel.thisMac)
            report = passed.summary.map { "The crawler \($0)." } ?? "Nothing was due to check."
            await refresh()
            if !passed.made.isEmpty { await didChange() }
        } catch {
            archive.lastError = nil
            report = message(for: error)
        }
    }

    /// Undoes every change it made on its own this week, newest first.
    func undoAll() async {
        do {
            let undone = try await archive.undoCrawlerChanges(since: Self.since)
            report =
                undone.failed.isEmpty
                ? nil : "Some changes couldn’t be undone: " + undone.failed.joined(separator: "; ")
            await refresh()
            await didChange()
        } catch {}
    }

    /// Acts on a single letter typed in the list; returns whether it meant something.
    func key(_ characters: String) -> Bool {
        let ids = selected.map(\.id)
        switch characters {
        case "f": toggle()
        case "a" where !ids.isEmpty: Task { await approve(ids) }
        case "r" where !ids.isEmpty: Task { await reject(ids) }
        default: return false
        }
        return true
    }

    private static var since: Date { .now.addingTimeInterval(-recentDays * 86_400) }
}

extension CrawlerStatus {
    /// Where it has got, in a sentence, as `carrel crawl status` says it.
    var line: String {
        let makes =
            changes
            ? "makes the changes it’s sure of itself" : "only suggests changes"
        return "Pace \(pace); it \(makes). Checked \(judged) of \(items) items."
    }
}

extension CrawlerFinding {
    /// What it changes, short, for a row.
    var change: String {
        if let to { return "Move to \(to)" }
        if let field, let value { return "\(field.capitalized): \(value)" }
        return description
    }
}
