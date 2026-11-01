// What the Questions screen shows and does: the questions waiting, the most worth answering
// first as the core ranks them, and those resting after a skip; each answered with a
// suggestion or the owner's own words, skipped or dismissed, through ArchiveModel. An item
// question's answer goes through the correction sheet, and a sender's can end in a routing
// rule, offered first (docs/design/librarian.md, "Questions").

import CarrelKit
import Foundation
import Observation

/// A routing rule offered after a sender's answer, waiting for the owner's yes.
struct OfferedRule: Identifiable, Equatable {
    /// Files future items named for this.
    let subject: String
    /// Into this collection.
    let collection: String
    var id: String { subject + "\u{0}" + collection }
}

@Observable
final class QuestionsModel {
    let archive: ArchiveModel

    /// The question the inspector shows.
    var selection: Question.ID?
    /// The answer being typed or chosen, by question.
    var answers: [Question.ID: String] = [:]
    /// Whether the list shows the questions resting after a skip, too.
    var showsSkipped = false
    /// A routing rule offered after a sender's answer.
    var offeredRule: OfferedRule?
    /// What the last answer did.
    private(set) var report: String?
    /// Answers an item question through the correction sheet, with the owner's words.
    @ObservationIgnored var answerItem: (Question, String) -> Void = { _, _ in }

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    /// The questions, in the core's order.
    var questions: [Question] { archive.questions }

    /// Those resting after a skip, the soonest back first.
    var skipped: [Question] { archive.skippedQuestions }

    var selected: Question? {
        selection.flatMap { id in (questions + skipped).first { $0.id == id } }
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

    /// Answers the question `id` with `given` (a suggestion's number, yes or no), or with
    /// what's typed or chosen for it.
    func answer(_ id: Question.ID, with given: String? = nil) async {
        guard let question = (questions + skipped).first(where: { $0.id == id }) else { return }
        let answer = given ?? answer(for: id)
        guard !answer.isEmpty else { return }
        report = nil
        if question.kind == .item {
            let picked = question.picked(answer)
            if picked?.noChange != true {
                answers[id] = nil
                answerItem(question, picked?.answer ?? answer)
                return
            }
        }
        guard let answered = try? await archive.answer(id, with: answer) else { return }
        answers[id] = nil
        if selection == id { selection = nil }
        report = Self.describe(answered)
        if let subject = answered.ruleFor, let collection = answered.collection {
            offeredRule = OfferedRule(subject: subject, collection: collection)
        }
    }

    /// Adds the routing rule offered after a sender's answer, as the owner agreed.
    func addOfferedRule() async {
        guard let rule = offeredRule else { return }
        offeredRule = nil
        guard (try? await archive.addRule(for: rule.subject, to: rule.collection)) != nil else {
            return
        }
        report = "Added a routing rule: items named for “\(rule.subject)” go to \(rule.collection)."
    }

    /// Skips the question `id`: it rests for the archive's `questions.skip_days`.
    func skip(_ id: Question.ID) async {
        report = nil
        guard (try? await archive.skip(id)) != nil else { return }
        answers[id] = nil
        if selection == id, !showsSkipped { selection = nil }
        let days = archive.settings.first { $0.key == "questions.skip_days" }?.value ?? "a few"
        report = "Skipped; it’s asked again after \(days) days."
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
        if answered.declined {
            return "Settled: nothing changes, and Carrel won’t ask it again."
        }
        if let collection = answered.collection {
            var said: [String] = answered.created ? ["Created \(collection)"] : []
            let moved = answered.moved.count
            if moved > 0 {
                said.append(
                    moved == 1
                        ? "Moved 1 proposal to \(collection)"
                        : "Moved \(moved) proposals to \(collection)")
            }
            let refiled = answered.refiled.count
            if refiled > 0 {
                said.append(
                    refiled == 1
                        ? "Refiled 1 item to \(collection)"
                        : "Refiled \(refiled) items to \(collection)")
            }
            said += answered.notRefiled.map { "Not refiled: \($0)" }
            return said.isEmpty ? "Its items go to \(collection)" : said.joined(separator: "; ")
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

extension Question {
    /// The suggestion `answer` picks: its number, counting from 1, or its words.
    func picked(_ answer: String) -> QuestionSuggestion? {
        let answer = answer.trimmingCharacters(in: .whitespacesAndNewlines)
        if let n = Int(answer) {
            return suggestions.indices.contains(n - 1) ? suggestions[n - 1] : nil
        }
        return suggestions.first { $0.answer.caseInsensitiveCompare(answer) == .orderedSame }
    }
}
