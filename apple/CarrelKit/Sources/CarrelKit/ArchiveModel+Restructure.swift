// Restructure proposals (docs/design/restructure.md): proposing tag restructures, what a
// proposal's chosen changes would leave, and approving all or part of one, or rejecting it.

import Foundation

extension ArchiveModel {
    /// Proposes tag restructures for the whole archive, in place of any tag proposal waiting.
    /// `nil` when there's nothing worth proposing.
    @discardableResult
    public func proposeTagRestructure() async throws -> RestructureProposal? {
        let proposal = try await call(reporting: "Looking over the tags") { handle, _ in
            try handle.proposeTagRestructure()
        }
        try await refreshRestructure()
        return proposal
    }

    /// What the proposal `id`'s changes `only` would leave, beside the archive as it is.
    public func preview(restructure id: String, only: [String]) async throws -> RestructurePreview {
        try await call { try $0.restructurePreview(id: id, only: only) }
    }

    /// Approves the proposal `id`: the changes `only` names, or all of them when it's `nil`,
    /// as one change in History.
    @discardableResult
    public func approve(restructure id: String, only: [String]?) async throws
        -> RestructureApproved
    {
        let approved = try await call(reporting: "Restructuring") { handle, _ in
            try handle.approveRestructure(id: id, only: only)
        }
        try await refreshRestructure()
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
        return approved
    }

    /// Rejects the proposal `id`: it leaves the queue, and nothing else changes.
    public func reject(restructure id: String) async throws {
        try await call { try $0.rejectRestructure(id: id) }
        try await refreshRestructure()
    }
}
