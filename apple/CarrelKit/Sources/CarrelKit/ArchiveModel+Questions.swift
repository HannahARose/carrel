// The questions beyond answering and dismissing (docs/design/librarian.md, "Questions"):
// skipping, a routing rule after a sender's answer, an item question answered through a
// correction, and questions at natural moments: about the item shown, and the day's few.

import Foundation

extension ArchiveModel {
    /// Skips the question `id`: it rests for the archive's `questions.skip_days`.
    @discardableResult
    public func skip(_ id: String) async throws -> Question {
        let skipped = try await call { try $0.skipQuestion(id: id) }
        try await refreshQuestions()
        return skipped
    }

    /// Adds a routing rule filing future items named for `subject` into `collection`, as the
    /// owner agreed after answering a sender's question.
    public func addRule(for subject: String, to collection: String) async throws {
        _ = try await call { try $0.addQuestionRule(subject: subject, collection: collection) }
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
    }

    /// Works out the correction `answer` to the item question `id` makes to its item's card.
    /// Nothing is saved until `save(_:)`, which also takes the question off the queue.
    public func prepareCorrection(
        question id: String, answer: String, model: ModelProvider? = nil
    ) async throws -> Correction {
        try await call(reporting: "Working out what the answer changes") { handle, _ in
            try handle.prepareQuestionCorrection(id: id, answer: answer, model: model)
        }
    }

    /// Takes the item question `id` off the queue, its answer changing nothing on the card.
    public func settle(_ id: String) async throws {
        try await call { try $0.settleQuestion(id: id) }
        try await refreshQuestions()
    }

    /// The open questions about the filed item `id`; none is in the queue until it's asked.
    public func questions(about id: String) async throws -> [Question] {
        try await call { try $0.questionsAbout(item: id) }
    }

    /// Puts the question `question` about the filed item `id` in the queue, to answer it.
    public func ask(_ question: String, about id: String) async throws -> Question {
        let asked = try await call { try $0.askAboutItem(item: id, question: question) }
        try await refreshQuestions()
        return asked
    }

    /// The day's few new questions, the same all day. Choosing them puts them in the queue.
    public func questionsToday() async throws -> [Question] {
        let today = try await call { try $0.questionsToday() }
        try await refreshQuestions()
        return today
    }
}
