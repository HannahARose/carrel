// What the Review screen shows and does: the proposals waiting, grouped by destination as
// `carrel review` lists them, and the decisions on them, made through CarrelKit's ArchiveModel.

import CarrelKit
import Foundation
import Observation

/// The proposals going to one collection.
struct DestinationGroup: Identifiable, Equatable {
    let destination: String
    let proposals: [Proposal]
    var id: String { destination }
}

@Observable
final class ReviewModel {
    let archive: ArchiveModel

    /// The chosen proposals: the inspector shows one, the actions take them all.
    var selection: Set<Proposal.ID> = []
    /// The proposals the collection picker is open for.
    var moving: [Proposal.ID]?
    /// What the last decision did, when there's something to say about it.
    private(set) var report: String?
    /// Called after items are filed, so Browse shows them.
    @ObservationIgnored var filed: () async -> Void = {}

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    /// Every proposal waiting, oldest batch first, each batch's by destination.
    var proposals: [Proposal] {
        archive.review.batches.flatMap(\.proposals)
    }

    /// The proposals by destination, the collections in order, as `carrel review` lists them.
    var groups: [DestinationGroup] {
        Dictionary(grouping: proposals, by: \.destination)
            .sorted { $0.key < $1.key }
            .map { DestinationGroup(destination: $0.key, proposals: $0.value) }
    }

    /// The chosen proposals, in the list's order.
    var selected: [Proposal] {
        groups.flatMap(\.proposals).filter { selection.contains($0.id) }
    }

    /// The one proposal the inspector shows, when exactly one is chosen.
    var inspected: Proposal? {
        selection.count == 1 ? proposals.first { selection.contains($0.id) } : nil
    }

    /// The batch a proposal came from, whose proposals approve together.
    func batch(of id: Proposal.ID) -> ReviewBatch? {
        archive.review.batches.first { $0.proposals.contains { $0.id == id } }
    }

    // MARK: - Decisions

    /// Files the proposals `ids` where they're proposed, as one batch.
    func approve(_ ids: [Proposal.ID]) async {
        guard !ids.isEmpty else { return }
        await filing { try await $0.approve(ids) }
    }

    /// Files every proposal from the batch `id` came from, together.
    func approveBatch(of id: Proposal.ID) async {
        guard let batch = batch(of: id) else { return }
        await approve(batch.proposals.map(\.id))
    }

    /// Files every proposal approving with high confidence takes.
    func approveAllHigh() async {
        await filing { try await $0.approveAllHigh() }
    }

    /// Files the proposals `ids` in Holding.
    func hold(_ ids: [Proposal.ID]) async {
        guard !ids.isEmpty else { return }
        await filing { try await $0.hold(ids) }
    }

    /// Rejects the proposals `ids`: their items stay where they are.
    func reject(_ ids: [Proposal.ID]) async {
        guard !ids.isEmpty else { return }
        report = nil
        guard (try? await archive.reject(ids)) != nil else { return }
        selection.subtract(ids)
        report = ids.count == 1 ? "Rejected 1 proposal" : "Rejected \(ids.count) proposals"
    }

    /// Points the proposals `ids` at the collection `collection`, one at a time as the core
    /// moves them; stops at the first that can't be.
    func move(_ ids: [Proposal.ID], to collection: String) async {
        moving = nil
        report = nil
        var dropped: [String] = []
        for id in ids {
            guard let moved = try? await archive.move(id, to: collection) else { return }
            dropped += moved.dropped
        }
        report =
            dropped.isEmpty
            ? "Moved to \(collection)"
            : "Moved to \(collection), leaving off \(dropped.joined(separator: ", "))"
    }

    /// Runs a decision that files items, and says what it did.
    private func filing(_ decide: (ArchiveModel) async throws -> FilingOutcome) async {
        report = nil
        guard let outcome = try? await decide(archive) else { return }
        selection.subtract(outcome.filed.map(\.proposal))
        report = Self.describe(outcome)
        if !outcome.filed.isEmpty { await filed() }
    }

    /// What a filing did, in a sentence or two.
    static func describe(_ outcome: FilingOutcome) -> String {
        var lines: [String] = []
        let count = outcome.filed.count
        if count > 0 { lines.append(count == 1 ? "Filed 1 item" : "Filed \(count) items") }
        for failed in outcome.failed {
            lines.append("“\(failed.title)” wasn’t filed: \(failed.error)")
        }
        if let error = outcome.snapshotError { lines.append("No backup first: \(error)") }
        lines += outcome.listingProblems
        return lines.isEmpty ? "Nothing to file" : lines.joined(separator: "\n")
    }
}
