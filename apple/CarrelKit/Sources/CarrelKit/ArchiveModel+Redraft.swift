// Drafting waiting proposals again with the models this device has now (#445).

import Foundation

extension ArchiveModel {
    /// Drafts the waiting proposals `ids` again with the device's models, and `model` when
    /// one is given, keeping each one's identifier and what the owner set. `memoryGB` is this
    /// device's memory, to say which model to download when there's none.
    @discardableResult
    public func redraft(
        _ ids: [String], model: ModelProvider? = nil, memoryGB: UInt64? = nil
    ) async throws -> RedraftOutcome {
        let outcome = try await call(reporting: "Drafting again") { handle, progress in
            try handle.redraftProposals(
                ids: ids, model: model, memoryGb: memoryGB, progress: progress)
        }
        try await refreshReview()
        try await refreshQuestions()
        try await refreshStatus()
        return outcome
    }

    /// The waiting proposals drafted here whose card or collection couldn't be settled.
    public func failedDrafts() async throws -> [String] {
        try await call { try $0.failedDrafts() }
    }
}
