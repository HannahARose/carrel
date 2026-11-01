// What the Restructure screen shows and does (docs/design/restructure.md, "In each
// interface"): the proposals waiting, and for the chosen one, its changes with their reasons
// and how invasive each is, which of them to approve, and what they'd leave beside what's there
// now. Approving all or part of a proposal is one change in History; rejecting changes nothing.

import CarrelKit
import Foundation
import Observation

@MainActor
@Observable
final class RestructureModel {
    let archive: ArchiveModel

    /// The proposal the inspector shows.
    var selection: RestructureProposal.ID? {
        didSet { if selection != oldValue { Task { await refreshPreview() } } }
    }
    /// The changes left out, by proposal; every other change is chosen.
    private(set) var leftOut: [RestructureProposal.ID: Set<String>] = [:]
    /// What the chosen proposal's chosen changes would leave.
    private(set) var preview: RestructurePreview?
    /// What the last action did.
    private(set) var report: String?
    /// Called after an approval changed the archive, so the window follows.
    @ObservationIgnored var didChange: () async -> Void = {}

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    var proposals: [RestructureProposal] { archive.restructureProposals }

    var selected: RestructureProposal? {
        selection.flatMap { id in proposals.first { $0.id == id } }
    }

    /// Whether `change` of `proposal` is chosen.
    func isChosen(_ change: RestructureChange, in proposal: RestructureProposal) -> Bool {
        !(leftOut[proposal.id] ?? []).contains(change.id)
    }

    /// Chooses `change` of `proposal`, or leaves it out.
    func choose(_ change: RestructureChange, in proposal: RestructureProposal, _ chosen: Bool) {
        var out = leftOut[proposal.id] ?? []
        if chosen { out.remove(change.id) } else { out.insert(change.id) }
        leftOut[proposal.id] = out
        Task { await refreshPreview() }
    }

    /// The changes of `proposal` that are chosen, in its order.
    func chosen(in proposal: RestructureProposal) -> [String] {
        proposal.changes.map(\.id).filter { !(leftOut[proposal.id] ?? []).contains($0) }
    }

    /// The chosen changes that need one left out, so approving drops them too.
    func blocked(in proposal: RestructureProposal) -> [RestructureChange] {
        let out = leftOut[proposal.id] ?? []
        return proposal.changes.filter { change in
            !out.contains(change.id) && change.dependsOn.contains { out.contains($0) }
        }
    }

    /// Reads what the chosen proposal's chosen changes would leave.
    func refreshPreview() async {
        guard let proposal = selected else {
            preview = nil
            return
        }
        let only = chosen(in: proposal)
        preview = try? await archive.preview(restructure: proposal.id, only: only)
    }

    /// Proposes tag restructures for the whole archive.
    func proposeTags() async {
        report = nil
        do {
            if let proposal = try await archive.proposeTagRestructure() {
                selection = proposal.id
            } else {
                report = "The tags look tidy: nothing to propose."
            }
        } catch {}
    }

    /// Approves the chosen proposal's chosen changes, or all of them.
    func approve(all: Bool = false) async {
        guard let proposal = selected else { return }
        let only = all ? nil : chosen(in: proposal)
        if only?.isEmpty == true { return }
        report = nil
        guard let approved = try? await archive.approve(restructure: proposal.id, only: only)
        else { return }
        leftOut[proposal.id] = nil
        selection = nil
        report = Self.describe(approved)
        await didChange()
    }

    /// Rejects the chosen proposal.
    func reject() async {
        guard let proposal = selected else { return }
        report = nil
        guard (try? await archive.reject(restructure: proposal.id)) != nil else { return }
        leftOut[proposal.id] = nil
        selection = nil
        report = "Rejected; nothing changed."
    }

    /// What approving did, in a sentence or two.
    static func describe(_ approved: RestructureApproved) -> String {
        var said = [
            approved.commit == nil ? "Nothing needed changing after all." : approved.summary
        ]
        if !approved.rejected.isEmpty {
            said.append(
                "Left out \(approved.rejected.count == 1 ? "1 change" : "\(approved.rejected.count) changes")."
            )
        }
        said += approved.dropped.map { "Dropped \($0)." }
        if !approved.skipped.isEmpty {
            said.append("Left alone: \(approved.skipped.joined(separator: "; ")).")
        }
        if approved.commit != nil { said.append("Undo it from History.") }
        return said.joined(separator: " ")
    }
}

extension RestructureProposal {
    /// What it covers and how many changes, for its row.
    var summary: String {
        let covers = section.isEmpty ? "The whole archive" : section
        let what = kind == "tags" ? "tags" : "collections"
        let count = changes.count == 1 ? "1 change" : "\(changes.count) changes"
        return "\(covers) · \(what) · \(count) · up to \(invasiveness.name.lowercased())"
    }
}

extension Invasiveness {
    /// Its name, for a badge.
    var name: String {
        switch self {
        case .rewriting: "Rewriting"
        case .subdividing: "Subdividing"
        case .renaming: "Renaming"
        case .betweenBranches: "Between branches"
        case .topLevel: "Top level"
        }
    }

    /// From 1, least invasive, to 5.
    var rank: Int {
        switch self {
        case .rewriting: 1
        case .subdividing: 2
        case .renaming: 3
        case .betweenBranches: 4
        case .topLevel: 5
        }
    }
}
