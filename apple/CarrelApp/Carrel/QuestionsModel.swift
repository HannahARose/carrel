// What the Questions screen shows and does: the questions waiting, those holding up the most
// proposals first as the core ranks them, answered or dismissed through ArchiveModel.

import CarrelKit
import Foundation
import Observation

@Observable
final class QuestionsModel {
    let archive: ArchiveModel

    /// The question the inspector shows.
    var selection: Question.ID?
    /// The answer being typed or chosen, by question.
    var answers: [Question.ID: String] = [:]
    /// What the last answer did.
    private(set) var report: String?

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    /// The questions, in the core's order.
    var questions: [Question] { archive.questions }

    var selected: Question? {
        selection.flatMap { id in questions.first { $0.id == id } }
    }

    /// The titles of the proposals a question holds up; an identifier when it's no longer in
    /// Review.
    func affected(by question: Question) -> [String] {
        let proposals = archive.review.batches.flatMap(\.proposals)
        return question.proposals.map { id in
            proposals.first { $0.id == id }?.fields.title ?? id
        }
    }

    /// The answer typed or chosen for `id`, trimmed.
    func answer(for id: Question.ID) -> String {
        (answers[id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Answers the question `id` with what's typed or chosen for it.
    func answer(_ id: Question.ID) async {
        let answer = answer(for: id)
        guard !answer.isEmpty else { return }
        report = nil
        guard let answered = try? await archive.answer(id, with: answer) else { return }
        answers[id] = nil
        if selection == id { selection = nil }
        report = Self.describe(answered)
    }

    /// Dismisses the question `id`: its proposals stop waiting on it.
    func dismiss(_ id: Question.ID) async {
        report = nil
        guard (try? await archive.dismiss(id)) != nil else { return }
        answers[id] = nil
        if selection == id { selection = nil }
    }

    /// What answering did, in a sentence.
    static func describe(_ answered: Answered) -> String {
        if !answered.moved.isEmpty {
            let count = answered.moved.count
            let to = Set(answered.moved.map(\.to)).sorted().joined(separator: ", ")
            return count == 1 ? "Moved 1 proposal to \(to)" : "Moved \(count) proposals to \(to)"
        }
        if !answered.requeued.isEmpty {
            let count = answered.requeued.count
            return count == 1
                ? "Added to the glossary; 1 item will be drafted again"
                : "Added to the glossary; \(count) items will be drafted again"
        }
        return "Added to the glossary"
    }
}
