// The correction sheet (⌘K): the owner types what's wrong with a proposal or a filed item,
// sees the changes the model works out, and confirms them, as `c` does in the interactive
// mode. The core prepares and saves the correction; nothing changes until it's confirmed.

import CarrelKit
import Foundation
import Observation

@Observable
final class CorrectionModel {
    /// What's being corrected.
    enum Target: Hashable {
        case proposal(id: String, title: String)
        case item(id: String, title: String)
        /// The item question `id`, asking `question`, whose answer the words are.
        case question(id: String, question: String)

        var title: String {
            switch self {
            case .proposal(_, let title), .item(_, let title): title
            case .question(_, let question): question
            }
        }

        /// Whether the words answer an item question.
        var isAnswer: Bool {
            if case .question = self { true } else { false }
        }
    }

    /// Where the sheet is.
    enum Stage {
        /// Typing the correction.
        case writing
        /// The model is working it out.
        case preparing
        /// Showing the changes, to confirm.
        case prepared(Correction, CorrectionChanges)
        /// Saved.
        case saved(SavedCorrection)
        /// An item question answered with no change to the card.
        case settled
    }

    let archive: ArchiveModel
    let target: Target
    /// The model to work it out with; `nil` for the device's.
    @ObservationIgnored let provider: ModelProvider?

    /// The correction, in the owner's words.
    var words = ""
    private(set) var stage = Stage.writing
    /// Why it couldn't be worked out or saved.
    private(set) var problem: String?
    /// Whether to add the fact it offers to the glossary when it's saved.
    var learnFact = true
    /// Called after it's saved, so the lists show the change.
    @ObservationIgnored var saved: () async -> Void = {}

    init(archive: ArchiveModel, target: Target, provider: ModelProvider? = nil) {
        self.archive = archive
        self.target = target
        self.provider = provider
    }

    var changes: CorrectionChanges? {
        if case .prepared(_, let changes) = stage { changes } else { nil }
    }

    var canPrepare: Bool {
        if case .writing = stage {
            !words.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        } else {
            false
        }
    }

    /// Works out what the words change. Nothing is saved.
    func prepare() async {
        guard canPrepare else { return }
        stage = .preparing
        problem = nil
        let words = words
        do {
            let correction =
                switch target {
                case .proposal(let id, _):
                    try await archive.prepareCorrection(
                        proposal: id, words: words, model: provider)
                case .item(let id, _):
                    try await archive.prepareCorrection(item: id, words: words, model: provider)
                case .question(let id, _):
                    try await archive.prepareCorrection(
                        question: id, answer: words, model: provider)
                }
            stage = .prepared(correction, correction.changes())
        } catch {
            archive.lastError = nil
            problem = message(for: error)
            stage = .writing
        }
    }

    /// Back to the words, to change them; the prepared correction is dropped.
    func rewrite() {
        stage = .writing
        problem = nil
    }

    /// Takes the item question being answered off the queue when its answer changes nothing
    /// on the card.
    func settle() async {
        guard case .question(let id, _) = target else { return }
        problem = nil
        do {
            try await archive.settle(id)
        } catch {
            archive.lastError = nil
            problem = message(for: error)
            return
        }
        stage = .settled
        await saved()
    }

    /// Saves the correction the owner agreed to, and the fact it offers when they agreed to
    /// that too.
    func confirm() async {
        guard case .prepared(let correction, let changes) = stage, !changes.isEmpty else { return }
        problem = nil
        do {
            stage = .saved(try await archive.save(correction))
        } catch {
            archive.lastError = nil
            problem = message(for: error)
            return
        }
        if learnFact, let fact = changes.fact {
            do {
                try await archive.learn(fact)
            } catch {
                archive.lastError = nil
                problem = "Saved, but the fact wasn’t added to the glossary: \(message(for: error))"
            }
        }
        await saved()
    }
}
