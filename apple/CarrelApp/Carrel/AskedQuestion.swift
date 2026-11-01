// Carrel's questions at natural moments in the app (docs/design/librarian.md, "Questions"),
// never as pop-ups: under an item's card in the inspector, now and then on a wander step, after
// the librarian's answer, and a few a day on Status. Each is answered where it's asked, with a
// suggestion or the owner's own words; an item question's answer goes through the correction
// sheet, shown before and after and saved only on yes.

import CarrelKit
import SwiftUI

extension ArchiveWindowModel {
    /// Answers `question`, already in the queue, with `given`: an item question through the
    /// correction sheet, unless the answer is a suggestion that changes nothing; any other
    /// kind straight away. Returns what it did, in a sentence, once it's done.
    func answerAsked(_ question: Question, with given: String) async -> String? {
        guard let archive else { return nil }
        if question.kind == .item, question.picked(given)?.noChange != true {
            answerItemQuestion(question, with: question.picked(given)?.answer ?? given)
            return nil
        }
        guard let answered = try? await archive.answer(question.id, with: given) else {
            return nil
        }
        return QuestionsModel.describe(answered)
    }
}

/// A question asked at a moment: its words, its likely answers and the owner's own, and Skip.
/// `ask` puts it in the queue first when it isn't there yet, as for the inspector's.
struct AskedQuestionView: View {
    let question: Question
    let window: ArchiveWindowModel
    var ask: (() async -> Question?)?
    /// Called once it's answered or skipped, so the place it was asked can move on.
    var done: () -> Void = {}

    @State private var answer = ""
    @State private var said: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(question.question).bold().textSelection(.enabled)
            if let said {
                Label(said, systemImage: "checkmark.circle").font(.callout)
            } else {
                QuestionAnswerer(
                    question: question, collections: window.collections.map(\.path),
                    answer: $answer
                ) { given in Task { await respond(given) } }
                HStack {
                    Button("Skip") { Task { await skip() } }
                        .help("Ask it again in a few days")
                    Spacer()
                    Button(question.kind == .item ? "Answer…" : "Answer") {
                        Task { await respond(answer) }
                    }
                    .disabled(answer.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
    }

    /// The question as the queue has it, asking it first when it isn't there yet.
    private func queued() async -> Question? {
        guard let ask else { return question }
        return await ask()
    }

    private func respond(_ given: String) async {
        let given = given.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !given.isEmpty, let asked = await queued() else { return }
        if let report = await window.answerAsked(asked, with: given) {
            said = report
            done()
        }
        answer = ""
    }

    private func skip() async {
        guard let asked = await queued(), let archive = window.archive else { return }
        guard (try? await archive.skip(asked.id)) != nil else { return }
        said = "Skipped; it’s asked again in a few days."
        done()
    }
}

/// The open questions about the item in the inspector, under its card. None is put in the
/// queue until the owner answers or skips it.
struct ItemQuestionsSection: View {
    let item: ItemSummary
    let window: ArchiveWindowModel

    @State private var questions: [Question] = []

    var body: some View {
        Group {
            if !questions.isEmpty {
                Section("Carrel Asks") {
                    ForEach(questions, id: \.question) { question in
                        AskedQuestionView(question: question, window: window) {
                            try? await window.archive?.ask(question.question, about: item.id)
                        }
                    }
                }
            }
        }
        .task(id: item.id) {
            questions = (try? await window.archive?.questions(about: item.id)) ?? []
        }
    }
}
