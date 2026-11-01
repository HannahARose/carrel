// The Questions screen: the questions by rank in the list, those skipped below when shown, and
// the chosen one in the inspector with what it's about, its likely answers and the owner's own,
// Skip and Dismiss.

import CarrelKit
import SwiftUI

struct QuestionsView: View {
    @Bindable var questions: QuestionsModel

    var body: some View {
        List(selection: $questions.selection) {
            ForEach(questions.questions) { QuestionRow(question: $0, questions: questions) }
            if questions.showsSkipped, !questions.skipped.isEmpty {
                Section("Skipped") {
                    ForEach(questions.skipped) { QuestionRow(question: $0, questions: questions) }
                }
            }
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "ds"), phases: .down) { press in
            guard press.modifiers.isEmpty, let id = questions.selection else { return .ignored }
            if press.characters == "s" {
                Task { await questions.skip(id) }
            } else {
                Task { await questions.dismiss(id) }
            }
            return .handled
        }
        .overlay {
            if questions.questions.isEmpty, !questions.showsSkipped || questions.skipped.isEmpty {
                ContentUnavailableView(
                    "No Questions", systemImage: "questionmark.bubble",
                    description: Text(
                        "Questions about terms, collections, senders and items appear here."))
            }
        }
        .toolbar {
            Toggle(isOn: $questions.showsSkipped) {
                Label("Show Skipped", systemImage: "clock.arrow.circlepath")
            }
            .help("Show the questions resting after a skip (\(questions.skipped.count))")
        }
        .safeAreaInset(edge: .bottom) {
            if let report = questions.report {
                ReportBar(text: report)
            }
        }
        .routingRuleOffer(questions)
    }
}

private struct QuestionRow: View {
    let question: Question
    let questions: QuestionsModel

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(question.question).lineLimit(2)
            Text(question.summary)
                .font(.caption).foregroundStyle(.secondary)
        }
        .tag(question.id)
        .contextMenu {
            Button("Skip") { Task { await questions.skip(question.id) } }
            Button("Dismiss") { Task { await questions.dismiss(question.id) } }
        }
    }
}

extension View {
    /// Asks before adding the routing rule offered after a sender's answer.
    func routingRuleOffer(_ questions: QuestionsModel) -> some View {
        confirmationDialog(
            "Add a routing rule?",
            isPresented: Binding(
                get: { questions.offeredRule != nil },
                set: { if !$0 { questions.offeredRule = nil } }),
            presenting: questions.offeredRule
        ) { _ in
            Button("Add Rule") { Task { await questions.addOfferedRule() } }
            Button("Not Now", role: .cancel) { questions.offeredRule = nil }
        } message: { rule in
            Text("File future items named for “\(rule.subject)” in \(rule.collection)?")
        }
    }
}

extension Question {
    /// What it's about and what it holds up, for its row.
    var summary: String {
        let count = proposals.count
        let holds =
            count == 0 ? nil : "holds up \(count == 1 ? "1 proposal" : "\(count) proposals")"
        let skippedNote = isSkipped ? "skipped" : nil
        return ([topicLine] + [holds, skippedNote].compactMap { $0 }).joined(separator: " · ")
    }
}

struct QuestionInspector: View {
    let question: Question
    @Bindable var questions: QuestionsModel
    /// Every collection, when the question offers no choices of its own.
    let collections: [String]
    /// Browse's previews, to show what the question is about.
    var previews: PreviewStore? = nil
    /// The filed item with an identifier.
    let item: (String) -> ItemSummary?

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label(question.kindName, systemImage: "questionmark.bubble")
                        .font(.caption).foregroundStyle(.secondary)
                    Text(question.question).font(.title3.bold())
                    if question.isSkipped {
                        Text(
                            question.skips == 1 ? "Skipped once" : "Skipped \(question.skips) times"
                        )
                        .font(.caption).foregroundStyle(.secondary)
                    }
                }
                .textSelection(.enabled)
            }
            QuestionSubjectsSection(
                question: question, questions: questions, previews: previews, item: item)
            Section("Answer") {
                QuestionAnswerer(
                    question: question, collections: collections, answer: answer
                ) { given in
                    let id = question.id
                    Task { await questions.answer(id, with: given) }
                }
                HStack {
                    Button("Skip") { Task { await questions.skip(question.id) } }
                        .help("Ask it again in a few days")
                    Button("Dismiss") { Task { await questions.dismiss(question.id) } }
                    Spacer()
                    Button(question.kind == .item ? "Answer…" : "Answer") {
                        let id = question.id
                        Task { await questions.answer(id) }
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(questions.answer(for: question.id).isEmpty)
                }
            }
        }
        .formStyle(.grouped)
    }

    private var answer: Binding<String> {
        let id = question.id
        return Binding {
            questions.answers[id] ?? ""
        } set: {
            questions.answers[id] = $0
        }
    }
}
