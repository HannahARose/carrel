// Answering one of Carrel's questions, wherever it's asked: on Questions, under an item's card
// in the inspector, on a wander step, after the librarian's answer and on Status. The likely
// answers to pick come first, then an answer of the owner's own, in the form its kind takes: a
// term's meaning, a collection's path, a sender's yes or no, or words about an item, which
// become a correction shown before it's saved.

import CarrelKit
import SwiftUI

/// A question's suggestions to pick, and the field for the owner's own answer.
struct QuestionAnswerer: View {
    let question: Question
    /// Every collection, when the question offers no choices of its own.
    let collections: [String]
    @Binding var answer: String
    /// Answers with what's given: a suggestion's number, yes or no, or the typed answer.
    let submit: (String) -> Void

    var body: some View {
        if !question.suggestions.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(question.suggestions.enumerated()), id: \.offset) { index, offer in
                    Button {
                        submit("\(index + 1)")
                    } label: {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(offer.label)
                            if let why = offer.why {
                                Text(why).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .buttonStyle(.bordered)
                    .help("Answer with this")
                }
            }
        }
        switch question.kind {
        case .term:
            TextField(
                "Meaning", text: $answer, prompt: Text("What “\(question.subject)” means"),
                axis: .vertical
            )
            .lineLimit(1...4)
            .onSubmit { submit(answer) }
        case .collection, .group:
            collectionPicker
        case .sender:
            HStack {
                Button("Yes") { submit("yes") }
                    .help("Gather its items in a collection of their own")
                Button("No") { submit("no") }
                    .help("Leave them where they are; Carrel won’t ask again")
            }
            collectionPicker
        case .item:
            TextField("Answer", text: $answer, prompt: Text("Your answer"), axis: .vertical)
                .lineLimit(1...4)
                .onSubmit { submit(answer) }
        }
    }

    private var collectionPicker: some View {
        Picker("Collection", selection: $answer) {
            Text("Choose…").tag("")
            ForEach(question.choices.isEmpty ? collections : question.choices, id: \.self) {
                Text($0).tag($0)
            }
        }
    }
}

extension QuestionSuggestion {
    /// The suggestion as a button says it.
    var label: String {
        newCollection ? "\(answer) (a new collection)" : answer
    }
}

extension Question {
    /// What it's about, for its row.
    var topicLine: String {
        switch kind {
        case .term: "Term “\(subject)”"
        case .collection: "Where “\(subject)” goes"
        case .group: "Where “\(subject)” go together"
        case .sender: "A collection for “\(subject)”?"
        case .item: "About “\(subject)”"
        }
    }

    /// The kind, for a label.
    var kindName: String {
        switch kind {
        case .term: "Term"
        case .collection: "Collection"
        case .group: "Group"
        case .sender: "Sender or project"
        case .item: "Item"
        }
    }

    /// Whether it rests after a skip.
    var isSkipped: Bool { skipped != nil && skips > 0 }
}

extension ArchiveWindowModel {
    /// Answers the item `question` with `answer` through the correction sheet: the model
    /// works out what it changes on the card, shown before and after, and it's saved, and the
    /// question taken off the queue, only on the owner's yes.
    func answerItemQuestion(_ question: Question, with answer: String) {
        guard let archive else { return }
        let correction = CorrectionModel(
            archive: archive, target: .question(id: question.id, question: question.question))
        correction.words = answer
        correction.saved = { [weak self] in await self?.refreshItems() }
        self.correction = correction
        Task { await correction.prepare() }
    }
}
