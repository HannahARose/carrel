// Status's few new questions a day (docs/design/librarian.md, "Questions"): the same all day,
// chosen by the core and kept on this Mac, each answered or skipped where it's shown.

import CarrelKit
import SwiftUI

struct StatusQuestionsSection: View {
    let window: ArchiveWindowModel

    @State private var questions: [Question] = []

    var body: some View {
        Group {
            if !questions.isEmpty {
                Section("Today’s Questions") {
                    ForEach(questions) { question in
                        AskedQuestionView(question: question, window: window)
                    }
                }
            }
        }
        .task(id: window.handle != nil) {
            questions = (try? await window.archive?.questionsToday()) ?? []
        }
    }
}
