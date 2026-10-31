// The Questions screen: the questions by rank in the list, and the chosen one in the inspector
// with the items it holds up, answered in a text field or by choosing a collection.

import CarrelKit
import SwiftUI

struct QuestionsView: View {
    @Bindable var questions: QuestionsModel

    var body: some View {
        List(selection: $questions.selection) {
            ForEach(questions.questions) { question in
                VStack(alignment: .leading, spacing: 2) {
                    Text(question.question).lineLimit(2)
                    Text(question.summary)
                        .font(.caption).foregroundStyle(.secondary)
                }
                .tag(question.id)
                .contextMenu {
                    Button("Dismiss") { Task { await questions.dismiss(question.id) } }
                }
            }
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "d"), phases: .down) { press in
            guard press.modifiers.isEmpty, let id = questions.selection else { return .ignored }
            Task { await questions.dismiss(id) }
            return .handled
        }
        .overlay {
            if questions.questions.isEmpty {
                ContentUnavailableView(
                    "No Questions", systemImage: "questionmark.bubble",
                    description: Text("Questions about terms and collections appear here."))
            }
        }
        .safeAreaInset(edge: .bottom) {
            if let report = questions.report {
                ReportBar(text: report)
            }
        }
    }
}

extension Question {
    /// What it's about and what it holds up, for its row.
    var summary: String {
        let about = topic == .term ? "Term “\(subject)”" : "Where “\(subject)” goes"
        let count = proposals.count
        return "\(about) · holds up \(count == 1 ? "1 proposal" : "\(count) proposals")"
    }
}

struct QuestionInspector: View {
    let question: Question
    @Bindable var questions: QuestionsModel
    /// Every collection, when the question offers no choices of its own.
    let collections: [String]

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label(
                        question.topic == .term ? "Term" : "Collection",
                        systemImage: "questionmark.bubble"
                    )
                    .font(.caption).foregroundStyle(.secondary)
                    Text(question.question).font(.title3.bold())
                }
                .textSelection(.enabled)
            }
            Section("Answer") {
                switch question.topic {
                case .term:
                    TextField(
                        "Meaning", text: answer, prompt: Text("What “\(question.subject)” means"),
                        axis: .vertical
                    )
                    .lineLimit(1...4)
                    .onSubmit(submit)
                case .collection:
                    Picker("Collection", selection: answer) {
                        Text("Choose…").tag("")
                        ForEach(
                            question.choices.isEmpty ? collections : question.choices, id: \.self
                        ) {
                            Text($0).tag($0)
                        }
                    }
                }
                HStack {
                    Button("Dismiss") { Task { await questions.dismiss(question.id) } }
                    Spacer()
                    Button("Answer", action: submit)
                        .keyboardShortcut(.defaultAction)
                        .disabled(questions.answer(for: question.id).isEmpty)
                }
            }
            Section("Holds Up") {
                ForEach(Array(questions.affected(by: question).enumerated()), id: \.offset) {
                    Text($0.element)
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

    private func submit() {
        let id = question.id
        Task { await questions.answer(id) }
    }
}
