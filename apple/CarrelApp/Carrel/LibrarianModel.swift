// The Librarian panel's conversation (docs/design/librarian.md): the owner asks about the
// archive, the core's librarian answers on a local model with the items it found, the
// passages it read and what it suggests, as `carrel ask` does. The conversation is kept only
// in memory until it's saved to the Journal. With `librarian.changes` on, the changes it made
// itself are listed here and undone one at a time or all together, as
// `carrel undo --librarian` does.

import CarrelKit
import Foundation
import Observation

/// How far the librarian has got with the question it's answering.
struct LibrarianProgress: Equatable {
    /// The step it's on, from 1.
    var step: UInt64
    /// The most steps it may take.
    var steps: UInt64
    /// What its last step did, if it's taken one.
    var did: String?
}

@MainActor
@Observable
final class LibrarianModel {
    /// How far back the librarian's changes are listed: a week, as `carrel status` lists them.
    static let recentDays: Int64 = 7

    let archive: ArchiveModel
    /// The model to answer with; `nil` for this Mac's (`librarian.model`, else the strongest).
    @ObservationIgnored let provider: ModelProvider?
    /// This Mac's memory in gigabytes, which the strongest model has to fit in.
    @ObservationIgnored let memoryGB: UInt64?

    /// Whether the panel is showing.
    var isShown = false
    /// The question being written.
    var draft = ""
    /// Asked each time the panel is to put the cursor in the question.
    private(set) var focusRequests = 0
    /// The answers so far, oldest first.
    private(set) var answers: [LibrarianAnswer] = []
    /// The question being answered now, while the librarian works.
    private(set) var asking: String?
    /// How far it has got with it.
    private(set) var progress: LibrarianProgress?
    /// The model it answers with, for people, once known.
    private(set) var modelLabel: String?
    /// Which model it answers with, or why there's none and what would let it.
    private(set) var modelChoice: LibrarianModelChoice?
    /// The model the owner asked to download for it, waiting for them to confirm, having
    /// seen its size.
    var confirmingDownload: CatalogModel?
    /// Whether it makes the changes it's sure of itself (`librarian.changes`).
    private(set) var makesChanges = false
    /// The changes it made itself this week, newest first.
    private(set) var changes: [Change] = []
    /// Where the conversation was last saved, for the panel to say.
    private(set) var savedTo: String?
    /// What went wrong, for the panel to say.
    var problem: String?
    /// Called after the librarian or an undo changed the archive, so the window follows.
    @ObservationIgnored var didChange: () async -> Void = {}

    @ObservationIgnored private var conversation = LibrarianConversation()
    /// Which question's progress is current, so a late report from an earlier one is ignored.
    @ObservationIgnored private var askToken = 0

    init(archive: ArchiveModel, provider: ModelProvider? = nil, memoryGB: UInt64? = thisMac) {
        self.archive = archive
        self.provider = provider
        self.memoryGB = memoryGB
    }

    /// This Mac's memory, rounded to whole gigabytes.
    nonisolated static var thisMac: UInt64? {
        let bytes = ProcessInfo.processInfo.physicalMemory
        return bytes > 0 ? (bytes + (1 << 29)) >> 30 : nil
    }

    var isAsking: Bool { asking != nil }

    var canAsk: Bool {
        !isAsking && !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Opens the panel with the cursor in the question.
    func show() {
        isShown = true
        focusRequests += 1
    }

    /// Opens the panel with `item` in the question, for the owner to finish and ask.
    func ask(about item: ItemSummary) {
        draft = "Tell me about “\(item.title)” (\(item.card)). "
        show()
    }

    /// Reads which model answers, whether the librarian makes changes, and what it changed
    /// this week.
    func refresh() async {
        await refreshModel()
        await refreshChanges()
    }

    /// Reads which model answers, or why there's none.
    func refreshModel() async {
        let provider = provider
        let memory = memoryGB
        modelChoice = try? await archive.handle.librarianModelChoiceOffMain(provider, memory)
        modelLabel = modelChoice?.label
    }

    /// Downloads the model the owner confirmed, then answers with it.
    func downloadConfirmed() async {
        guard let model = confirmingDownload else { return }
        confirmingDownload = nil
        do {
            _ = try await archive.download(model: model.name)
            problem = nil
        } catch {
            take(error)
        }
        await refreshModel()
    }

    /// Clears a `librarian.model` naming a model that's gone, so the librarian answers with
    /// the strongest one this Mac has.
    func useStrongest() async {
        do {
            _ = try await archive.setDevice("librarian.model", to: nil)
            problem = nil
        } catch {
            take(error)
        }
        await refreshModel()
    }

    /// Reads the librarian's changes this week again.
    func refreshChanges() async {
        let since = Self.since
        do {
            makesChanges = try await archive.call { try $0.librarianMakesChanges() }
            changes = try await archive.call { try $0.librarianChanges(since: since) }
        } catch {
            take(error)
        }
    }

    /// Asks the question being written, off the main thread, following its progress.
    func ask() async {
        guard canAsk else { return }
        let question = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        draft = ""
        asking = question
        problem = nil
        savedTo = nil
        askToken += 1
        let token = askToken
        progress = LibrarianProgress(step: 0, steps: 0, did: nil)
        defer {
            if askToken == token {
                asking = nil
                progress = nil
            }
        }
        let relay = ProgressRelay { [weak self] step, steps, did in
            Task { @MainActor in
                guard let self, self.askToken == token, self.asking != nil else { return }
                self.progress = LibrarianProgress(step: step, steps: steps, did: did)
            }
        }
        let conversation = conversation
        let provider = provider
        let memory = memoryGB
        do {
            let answer = try await archive.call {
                try $0.askLibrarian(
                    conversation: conversation, question: question, model: provider,
                    memoryGb: memory, progress: relay)
            }
            guard askToken == token else { return }
            answers.append(answer)
            modelLabel = answer.model
            // Its question about an item is in the queue now, for Questions and the badge.
            if answer.asks != nil { try? await archive.refreshQuestions() }
            if answer.suggestions.contains(where: \.isMade) {
                await refreshChanges()
                await didChange()
            }
        } catch {
            guard askToken == token else { return }
            draft = question
            take(error)
            await refreshModel()
        }
    }

    /// Starts a new conversation; the one before is gone unless it was saved.
    func clear() {
        askToken += 1
        conversation = LibrarianConversation()
        answers = []
        asking = nil
        progress = nil
        savedTo = nil
        problem = nil
    }

    /// Saves the conversation to this month's Journal, as one change.
    func save() async {
        guard !answers.isEmpty, !isAsking else { return }
        let conversation = conversation
        do {
            let saved = try await archive.call {
                try $0.saveConversation(conversation: conversation)
            }
            savedTo = saved.path
            await didChange()
        } catch {
            take(error)
        }
    }

    /// Undoes one of the librarian's changes.
    func undo(_ change: Change) async {
        do {
            try await archive.undo(change.id)
            await refreshChanges()
            await didChange()
        } catch {
            take(error)
        }
    }

    /// Undoes every change the librarian made itself this week, newest first.
    func undoAll() async {
        let since = Self.since
        do {
            let undone = try await archive.call { try $0.undoLibrarianChanges(since: since) }
            if !undone.failed.isEmpty {
                problem =
                    "Some changes couldn’t be undone: "
                    + undone.failed.joined(separator: "; ")
            }
            try? await archive.refreshHistory()
            await refreshChanges()
            await didChange()
        } catch {
            take(error)
        }
    }

    /// The start of the week the librarian's changes are listed for.
    private static var since: Int64 {
        Int64(Date.now.timeIntervalSince1970) - recentDays * 86_400
    }

    private func take(_ error: Error) {
        archive.lastError = nil
        problem = message(for: error)
    }
}

extension ArchiveHandle {
    /// Which model the librarian answers with, worked out off the main thread: it looks at
    /// the providers.
    fileprivate func librarianModelChoiceOffMain(
        _ provider: ModelProvider?, _ memory: UInt64?
    ) async throws -> LibrarianModelChoice {
        try await Task.detached(priority: .utility) {
            try self.librarianModelChoice(model: provider, memoryGb: memory)
        }.value
    }
}

extension LibrarianSuggestion {
    /// Whether the librarian made it itself.
    var isMade: Bool {
        if case .made = outcome { true } else { false }
    }

    /// Whether it's for the owner to confirm.
    var isProposed: Bool {
        if case .proposed = outcome { true } else { false }
    }
}
