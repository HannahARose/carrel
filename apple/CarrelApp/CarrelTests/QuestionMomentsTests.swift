// Every kind of question on the Questions screen (#319), and questions at natural moments
// (#316): under an item's card, on Status, and answered through the correction sheet. Each
// test runs against its own temporary copy of the sample archive.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class QuestionMomentsTests: XCTestCase {
    static let sender = "01a18701-0500-776a-9867-08f0000000d1"
    /// The sample item Carrel has a question about: a project with no description.
    static let askedAbout = "Projects/Hardware/notes.card.md"

    private func senderQuestion() -> [String: Any] {
        var asked = question(Self.sender, kind: "sender", subject: "Northbridge Bank")
        asked["choices"] = ["Projects/Northbridge"]
        asked["suggestions"] = [
            ["answer": "Projects/Northbridge", "why": "12 statements", "new_collection": true],
            ["answer": "no", "no_change": true],
        ]
        return asked
    }

    func testASendersQuestionShowsItsSuggestionsAndOffersARule() async throws {
        let sample = try await SampleWindow(questions: [Self.sender: senderQuestion()])
        defer { sample.remove() }
        let questions = sample.questions
        let asked = try XCTUnwrap(questions.questions.first { $0.id == Self.sender })
        XCTAssertEqual(asked.kind, .sender)
        XCTAssertEqual(
            asked.suggestions.map(\.label).first, "Projects/Northbridge (a new collection)")
        await questions.answer(Self.sender, with: "yes")
        XCTAssertEqual(
            questions.report,
            "Created Projects/Northbridge; Moved 1 proposal to Projects/Northbridge")
        XCTAssertEqual(
            questions.offeredRule,
            OfferedRule(subject: "Northbridge Bank", collection: "Projects/Northbridge"))
        await questions.addOfferedRule()
        XCTAssertNil(questions.offeredRule)
        XCTAssertTrue(questions.report?.hasPrefix("Added a routing rule") == true)
    }

    func testASuggestionThatChangesNothingSettlesIt() async throws {
        let sample = try await SampleWindow(questions: [Self.sender: senderQuestion()])
        defer { sample.remove() }
        await sample.questions.answer(Self.sender, with: "2")
        XCTAssertEqual(
            sample.questions.report, "Settled: nothing changes, and Carrel won’t ask it again.")
        XCTAssertFalse(sample.questions.questions.contains { $0.id == Self.sender })
    }

    func testASkippedQuestionRestsWithTheSkipped() async throws {
        let sample = try await SampleWindow(questions: [Self.sender: senderQuestion()])
        defer { sample.remove() }
        let questions = sample.questions
        await questions.skip(Self.sender)
        XCTAssertFalse(questions.questions.contains { $0.id == Self.sender })
        XCTAssertEqual(questions.skipped.map(\.id), [Self.sender])
        XCTAssertTrue(questions.skipped[0].isSkipped)
        XCTAssertTrue(questions.report?.hasPrefix("Skipped") == true)
    }

    private func item(_ sample: SampleWindow) throws -> ItemSummary {
        try XCTUnwrap(sample.model.items.first { $0.card == Self.askedAbout })
    }

    func testAnItemQuestionIsAnsweredThroughTheCorrectionSheet() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let item = try item(sample)
        let open = try await sample.archive.questions(about: item.id)
        let first = try XCTUnwrap(open.first)
        XCTAssertEqual(first.kind, .item)
        XCTAssertFalse(
            sample.questions.questions.contains { $0.question == first.question },
            "looking doesn't fill the queue")
        let asked = try await sample.archive.ask(first.question, about: item.id)
        XCTAssertTrue(sample.questions.questions.contains { $0.id == asked.id })

        // Answering on Questions opens the correction sheet with the answer.
        sample.questions.answers[asked.id] = "notes from the lamp build"
        await sample.questions.answer(asked.id)
        let correction = try XCTUnwrap(sample.model.correction)
        XCTAssertEqual(correction.target, .question(id: asked.id, question: asked.question))
        XCTAssertEqual(correction.words, "notes from the lamp build")

        // Worked out with a Swift model and confirmed, the question leaves the queue.
        let answering = CorrectionModel(
            archive: sample.archive, target: correction.target, provider: Retitling())
        answering.words = correction.words
        await answering.prepare()
        XCTAssertNil(answering.problem)
        await answering.confirm()
        guard case .saved = answering.stage else {
            return XCTFail("not saved: \(answering.problem ?? "")")
        }
        XCTAssertFalse(sample.questions.questions.contains { $0.id == asked.id })
    }

    func testAnAnswerThatChangesNothingStillAnswers() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let item = try item(sample)
        let open = try await sample.archive.questions(about: item.id)
        let first = try XCTUnwrap(open.first)
        let asked = try await sample.archive.ask(first.question, about: item.id)
        let answering = CorrectionModel(
            archive: sample.archive, target: .question(id: asked.id, question: asked.question))
        await answering.settle()
        guard case .settled = answering.stage else { return XCTFail("not settled") }
        XCTAssertFalse(sample.questions.questions.contains { $0.id == asked.id })
    }

    func testStatusNamesTheDaysQuestionsTheSameAllDay() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let today = try await sample.archive.questionsToday()
        XCTAssertFalse(today.isEmpty, "the sample has an item to ask about")
        let again = try await sample.archive.questionsToday()
        XCTAssertEqual(today.map(\.id), again.map(\.id))
        XCTAssertTrue(sample.questions.questions.contains { $0.id == today[0].id })
    }

    func testAWanderAsksNowAndThen() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let discover = try XCTUnwrap(sample.model.discover)
        await discover.startWalk(seed: 7)
        for _ in 0..<8 { await discover.step() }
        let asked = discover.steps.compactMap(\.question)
        XCTAssertNil(discover.steps.first?.question, "the first step is for looking around")
        for (a, b) in zip(asked, asked.dropFirst()) { XCTAssertNotEqual(a.id, b.id) }
    }
}
