// The observable archive the Mac app's screens show (docs/design/mac-app.md, "How the app
// reaches the core"). It wraps an ArchiveHandle, runs each core call off the main thread, and
// publishes what each screen shows, refreshing it after the changes that affect it.

import Foundation
import Observation

/// How far a long operation has got, as the app's progress bars show it.
public struct OperationProgress: Sendable, Equatable {
    /// What's happening, such as "Reading text".
    public var title: String
    /// How many are done.
    public var done: UInt64
    /// Of how many; 0 while that isn't known.
    public var total: UInt64
    /// The one being worked on, when there is one.
    public var item: String?

    /// How much is done, from 0 to 1, when the total is known.
    public var fraction: Double? {
        total > 0 ? Double(done) / Double(total) : nil
    }
}

/// A Swift closure as the core's progress callback. The core calls it from the thread doing
/// the work.
public final class ProgressRelay: ProgressListener {
    private let report: @Sendable (UInt64, UInt64, String?) -> Void

    /// Calls `report` with how many are done, of how many, and which, as the work goes.
    public init(_ report: @escaping @Sendable (UInt64, UInt64, String?) -> Void) {
        self.report = report
    }

    public func update(done: UInt64, total: UInt64, item: String?) {
        report(done, total, item)
    }
}

/// An open archive, observable from SwiftUI. Every change goes through the core, which takes
/// the archive's writer lease as the CLI does, so the app, the CLI and the agent can all be
/// open on one archive; a change that can't get it throws `CarrelError.Busy`, saying who
/// holds it.
@MainActor
@Observable
public final class ArchiveModel {
    /// The archive, for calls this model doesn't wrap.
    public nonisolated let handle: ArchiveHandle

    /// The archive's name.
    public let name: String

    /// What the Status screen shows.
    public private(set) var status: StatusSummary?
    /// The proposals waiting for review, by batch.
    public private(set) var review = ReviewList(batches: [], unreadable: [])
    /// The questions waiting, those holding up the most proposals first.
    public private(set) var questions: [Question] = []
    /// The restructure proposals waiting, oldest first.
    public private(set) var restructureProposals: [RestructureProposal] = []
    /// What's checked out and on this device's desk.
    public private(set) var active: [ActiveItem] = []
    /// The history, newest first.
    public private(set) var history: [Change] = []
    /// The intake queue on this device.
    public private(set) var intake = IntakeQueue(waiting: [], failed: [])
    /// The archive's shared settings.
    public private(set) var settings: [Setting] = []
    /// The last search's results.
    public private(set) var searchResults: SearchResults?
    /// The embedding index, once read with `refreshIndex()`; it reads every card, so
    /// `refreshAll()` leaves it.
    public private(set) var index: IndexStatus?

    /// The long operation under way, if any.
    public private(set) var progress: OperationProgress?
    /// How many core calls are running.
    public private(set) var running = 0
    /// The last call's error, with the core's message, for an alert.
    public var lastError: CarrelError?

    /// Whether any core call is running.
    public var isWorking: Bool { running > 0 }

    /// How many proposals wait in Review, for the sidebar's badge.
    public var reviewCount: Int {
        review.batches.reduce(0) { $0 + $1.proposals.count }
    }

    /// The progress of the operation that reports this token, so a late report from one
    /// that has finished is ignored.
    private var progressToken = 0

    /// Wraps an open archive.
    public init(handle: ArchiveHandle) {
        self.handle = handle
        self.name = handle.name()
    }

    /// Opens the archive containing `url`.
    public convenience init(opening url: URL, options: OpenOptions? = nil) throws {
        let handle =
            if let options {
                try ArchiveHandle.openWith(path: url.path(percentEncoded: false), options: options)
            } else {
                try ArchiveHandle.open(url)
            }
        self.init(handle: handle)
    }

    // MARK: - Calling the core

    /// Runs `work` with the archive off the main thread, keeping its error as `lastError`.
    public func call<T: Sendable>(
        _ work: @escaping @Sendable (ArchiveHandle) throws -> T
    ) async throws -> T {
        running += 1
        defer { running -= 1 }
        let handle = handle
        do {
            return try await Task.detached(priority: .userInitiated) { try work(handle) }.value
        } catch let error as CarrelError {
            lastError = error
            throw error
        }
    }

    /// Runs `work` as `call(_:)` does, publishing its progress as `progress` under `title`.
    public func call<T: Sendable>(
        reporting title: String,
        _ work: @escaping @Sendable (ArchiveHandle, ProgressListener) throws -> T
    ) async throws -> T {
        progressToken += 1
        let token = progressToken
        progress = OperationProgress(title: title, done: 0, total: 0, item: nil)
        defer {
            if progressToken == token { progress = nil }
        }
        let relay = ProgressRelay { [weak self] done, total, item in
            Task { @MainActor in
                guard let self, self.progressToken == token, self.progress != nil else { return }
                self.progress = OperationProgress(
                    title: title, done: done, total: total, item: item)
            }
        }
        return try await call { try work($0, relay) }
    }

    // MARK: - Refreshing

    /// Reads Status again.
    public func refreshStatus() async throws {
        status = try await call { try $0.status() }
    }

    /// Reads the review queue again.
    public func refreshReview() async throws {
        review = try await call { try $0.review() }
    }

    /// Reads the questions again.
    public func refreshQuestions() async throws {
        questions = try await call { try $0.questions() }
    }

    /// Reads the restructure proposals waiting again.
    public func refreshRestructure() async throws {
        restructureProposals = try await call { try $0.restructureProposals() }
    }

    /// Reads what's checked out and on the desk again.
    public func refreshActive() async throws {
        active = try await call { try $0.active() }
    }

    /// How many of the newest changes `history` holds; `nil` for all of them.
    public var historyLimit: UInt32? = 200

    /// Reads the history again, the newest `historyLimit` changes.
    public func refreshHistory() async throws {
        let limit = historyLimit
        history = try await call { try $0.history(limit: limit) }
    }

    /// Reads the intake queue again.
    public func refreshIntake() async throws {
        intake = try await call { try $0.intake() }
    }

    /// Reads the shared settings again.
    public func refreshSettings() async throws {
        settings = try await call { try $0.settings() }
    }

    /// Reads everything the screens show again. An archive with no history yet shows none.
    public func refreshAll() async throws {
        try await refreshStatus()
        try await refreshReview()
        try await refreshQuestions()
        try await refreshRestructure()
        try await refreshActive()
        try await refreshIntake()
        try await refreshSettings()
        if status?.hasHistory == true {
            try await refreshHistory()
        } else {
            history = []
        }
    }

    /// After a change: the screens it shows on, and Status and History, which every change
    /// shows on.
    private func changed(_ refresh: (ArchiveModel) async throws -> Void) async throws {
        try await refresh(self)
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
    }

    // MARK: - Review and Questions

    /// Files the proposals `ids` where they're proposed, as one batch.
    @discardableResult
    public func approve(_ ids: [String]) async throws -> FilingOutcome {
        let outcome = try await call(reporting: "Filing") { handle, _ in
            try handle.approve(ids: ids)
        }
        try await changed { try await $0.refreshReview() }
        return outcome
    }

    /// Files every proposal approving with high confidence takes.
    @discardableResult
    public func approveAllHigh() async throws -> FilingOutcome {
        let outcome = try await call(reporting: "Filing") { handle, _ in try handle.approveAllHigh()
        }
        try await changed { try await $0.refreshReview() }
        return outcome
    }

    /// Files a duplicate linked to the archived item it duplicates: as a new version of it,
    /// or beside it.
    @discardableResult
    public func approveDuplicate(_ id: String, asVersion: Bool, of target: String? = nil)
        async throws -> FilingOutcome
    {
        let outcome = try await call {
            try $0.approveDuplicate(id: id, asVersion: asVersion, target: target)
        }
        try await changed { try await $0.refreshReview() }
        return outcome
    }

    /// Files the proposals `ids` in Holding.
    @discardableResult
    public func hold(_ ids: [String]) async throws -> FilingOutcome {
        let outcome = try await call(reporting: "Holding") { handle, _ in try handle.hold(ids: ids)
        }
        try await changed { try await $0.refreshReview() }
        return outcome
    }

    /// Rejects the proposals `ids`: their items stay where they are.
    public func reject(_ ids: [String]) async throws {
        _ = try await call { try $0.reject(ids: ids) }
        try await refreshReview()
        try await refreshStatus()
    }

    /// Points the proposal `id` at the collection at `collection`.
    @discardableResult
    public func move(_ id: String, to collection: String) async throws -> MovedProposal {
        let moved = try await call { try $0.moveProposal(id: id, collection: collection) }
        try await refreshReview()
        return moved
    }

    /// Says whether a compressed file is expanded on approval or kept compressed.
    public func chooseCompressed(_ id: String, expand: Bool) async throws {
        _ = try await call { try $0.chooseCompressed(id: id, expand: expand) }
        try await refreshReview()
    }

    /// Answers the question `id`: a term's meaning, or a collection's path.
    @discardableResult
    public func answer(_ id: String, with answer: String) async throws -> Answered {
        let answered = try await call { try $0.answerQuestion(id: id, answer: answer) }
        try await refreshQuestions()
        try await changed { model in
            try await model.refreshReview()
            try await model.refreshIntake()
        }
        return answered
    }

    /// Dismisses the question `id`: its proposals stop waiting on it.
    public func dismiss(_ id: String) async throws {
        try await call { try $0.dismissQuestion(id: id) }
        try await refreshQuestions()
        try await refreshReview()
        try await refreshStatus()
    }

    // MARK: - Corrections and editing

    /// Works out what `words` change in the proposal `id`, with `model` or the device's.
    /// Nothing is saved until `save(_:)`.
    public func prepareCorrection(
        proposal id: String, words: String, model: ModelProvider? = nil
    ) async throws -> Correction {
        try await call(reporting: "Working out the correction") { handle, _ in
            try handle.prepareProposalCorrection(id: id, words: words, model: model)
        }
    }

    /// Works out what `words` change in the filed item `id`.
    public func prepareCorrection(
        item id: String, words: String, model: ModelProvider? = nil
    ) async throws -> Correction {
        try await call(reporting: "Working out the correction") { handle, _ in
            try handle.prepareItemCorrection(item: id, words: words, model: model)
        }
    }

    /// Saves a correction the owner agreed to.
    @discardableResult
    public func save(_ correction: Correction) async throws -> SavedCorrection {
        let saved = try await call { try $0.saveCorrection(correction: correction) }
        try await changed { try await $0.refreshReview() }
        return saved
    }

    /// Adds a fact a correction offered to the glossary, as the owner agreed.
    public func learn(_ fact: GlossaryFact) async throws {
        _ = try await call { try $0.learn(fact: fact) }
        try await changed { _ in }
    }

    /// Saves edited fields to a filed card, with `carrel edit`'s checks.
    @discardableResult
    public func edit(item id: String, fields: CardFields) async throws -> EditOutcome {
        let outcome = try await call { try $0.editCard(item: id, fields: fields) }
        if case .saved = outcome { try await changed { _ in } }
        return outcome
    }

    /// Saves an edited body, Notes and all, to a filed card; Carrel's sections must be as
    /// they were.
    @discardableResult
    public func edit(item id: String, body: String) async throws -> EditOutcome {
        let outcome = try await call { try $0.editCardBody(item: id, body: body) }
        if case .saved = outcome { try await changed { _ in } }
        return outcome
    }

    /// Saves edited fields to a proposal's card.
    @discardableResult
    public func edit(proposal id: String, fields: CardFields) async throws -> EditOutcome {
        let outcome = try await call { try $0.editProposal(id: id, fields: fields) }
        if case .saved = outcome { try await refreshReview() }
        return outcome
    }

    // MARK: - Active

    /// Checks out the item `id` to `folder` (the Desktop when `nil`), due back on `due`.
    @discardableResult
    public func checkOut(_ id: String, to folder: URL? = nil, due: String? = nil) async throws
        -> CheckedOut
    {
        let to = folder?.path(percentEncoded: false)
        let out = try await call { try $0.checkOut(item: id, to: to, due: due) }
        try await changed { try await $0.refreshActive() }
        return out
    }

    /// Returns the checked-out item `id`, from where it went or from `folder` when the
    /// working copy has moved; `trimmingSnapshots` forgets the checkout's session snapshots
    /// afterwards, as the owner chose. Show `handle.returnPreview(item:from:)` first.
    @discardableResult
    public func returnItem(_ id: String, from folder: URL? = nil, trimmingSnapshots: Bool = false)
        async throws -> Returned
    {
        let from = folder?.path(percentEncoded: false)
        let returned = try await call(reporting: "Returning") { handle, _ in
            try handle.returnItem(item: id, from: from, trimSnapshots: trimmingSnapshots)
        }
        try await changed { try await $0.refreshActive() }
        return returned
    }

    /// Puts the returned working copy at `path` in the system Trash, where it can be
    /// recovered, once the owner has said to (`Returned.workingCopy` is `.ask`).
    public func trashWorkingCopy(_ path: String) async throws {
        try await call { try $0.trashWorkingCopy(path: path) }
    }

    /// Cancels the checkout of `id`; the working copy's changes aren't brought back.
    public func cancelCheckout(_ id: String) async throws {
        _ = try await call { try $0.cancelCheckout(item: id) }
        try await changed { try await $0.refreshActive() }
    }

    /// Puts the file or folder at `url` on this device's desk.
    @discardableResult
    public func addToDesk(_ url: URL, title: String? = nil, summary: String? = nil) async throws
        -> DeskItem
    {
        let path = url.path(percentEncoded: false)
        let item = try await call(reporting: "Drafting its card") { handle, _ in
            try handle.addToDesk(path: path, title: title, summary: summary)
        }
        try await changed { try await $0.refreshActive() }
        return item
    }

    /// Takes the desk item at `url` off the desk.
    public func removeFromDesk(_ url: URL, deletingCard: Bool = false) async throws {
        let path = url.path(percentEncoded: false)
        _ = try await call { try $0.removeFromDesk(path: path, deleteCard: deletingCard) }
        try await changed { try await $0.refreshActive() }
    }

    /// Queues the desk item at `url` for the archive and takes it off the desk.
    public func archiveDeskItem(_ url: URL) async throws {
        let path = url.path(percentEncoded: false)
        _ = try await call { try $0.archiveDeskItem(path: path) }
        try await changed { model in
            try await model.refreshActive()
            try await model.refreshIntake()
        }
    }

    /// Approves, drops or combines the active item `id`'s pending entries `numbers` (from 1;
    /// empty means all of them).
    @discardableResult
    public func decide(entries numbers: [UInt32], of id: String, _ decision: EntryDecision)
        async throws -> EntriesDecided
    {
        let decided = try await call {
            try $0.decideEntries(item: id, numbers: numbers, decision: decision)
        }
        try await changed { try await $0.refreshActive() }
        return decided
    }

    /// Replaces the text of the active item `id`'s pending entry `number`.
    public func edit(entry number: UInt32, of id: String, text: String) async throws {
        _ = try await call { try $0.editEntry(item: id, number: number, text: text) }
        try await changed { try await $0.refreshActive() }
    }

    // MARK: - Browse

    /// Creates the collection `path`, saying what belongs there and with its tags, as one
    /// change.
    @discardableResult
    public func createCollection(
        _ path: String, description: String? = nil, tags: [CollectionTag] = []
    ) async throws -> CreatedCollection {
        let created = try await call {
            try $0.createCollection(path: path, description: description, tags: tags)
        }
        try await changed { _ in }
        return created
    }

    /// Renames the item `id` and its card together to `name`, as one change.
    @discardableResult
    public func rename(_ id: String, to name: String) async throws -> RenamedItem {
        let renamed = try await call { try $0.renameItem(item: id, newName: name) }
        try await changed { _ in }
        return renamed
    }

    /// Expands the filed compressed file `id` in place, as one change; `anyway` when it's
    /// kept compressed for a reason the owner overrides. Ask with `handle.expandable(item:)`
    /// first.
    @discardableResult
    public func expand(_ id: String, anyway: Bool = false) async throws -> ExpandedItem {
        let expanded = try await call(reporting: "Expanding") { handle, _ in
            try handle.expandItem(item: id, anyway: anyway)
        }
        try await changed { _ in }
        return expanded
    }

    // MARK: - Search and History

    /// Searches for `query`, words and filters, publishing the results.
    @discardableResult
    public func search(_ query: String, limit: UInt32 = 50) async throws -> SearchResults {
        let results = try await call(reporting: "Reading text") { handle, relay in
            try handle.search(query: query, limit: limit, progress: relay)
        }
        searchResults = results
        return results
    }

    /// Clears the search.
    public func clearSearch() {
        searchResults = nil
    }

    /// Starts the history of an archive that has none yet, as `carrel init` does in an
    /// existing archive, and reads everything again.
    public func startHistory() async throws {
        try await call { try $0.startHistory() }
        try await refreshAll()
    }

    /// Undoes the change `id`, as a new change.
    public func undo(_ id: String) async throws {
        _ = try await call { try $0.undo(id: id) }
        try await refreshAll()
    }

    // MARK: - Settings and models

    /// Sets the shared setting `key` to `value`, as one change.
    @discardableResult
    public func set(_ key: String, to value: String) async throws -> SettingChanged {
        let changed = try await call { try $0.setSetting(key: key, value: value) }
        try await self.changed { try await $0.refreshSettings() }
        return changed
    }

    /// Chooses this device's text models.
    @discardableResult
    public func chooseModels(drafting: String?, secondOpinion: String?) async throws
        -> ModelChoices
    {
        try await call { try $0.chooseModels(model: drafting, secondModel: secondOpinion) }
    }

    /// Sets this device's `picker` or `editor`; `nil` lets Carrel choose.
    @discardableResult
    public func setDevice(_ key: String, to value: String?) async throws -> DeviceSettings {
        try await call { try $0.setDeviceSetting(key: key, value: value) }
    }

    /// Downloads the catalog model `name`. Only once the owner has confirmed, having seen
    /// its size.
    @discardableResult
    public func download(model name: String) async throws -> String {
        try await call(reporting: "Downloading \(name)") { handle, relay in
            try handle.downloadModel(name: name, progress: relay)
        }
    }

    /// Deletes the downloaded model `name`. Only once the owner has confirmed.
    @discardableResult
    public func remove(model name: String) async throws -> [String] {
        try await call { try $0.removeModel(name: name) }
    }

    // MARK: - The embedding index

    /// Reads the embedding index's status again.
    public func refreshIndex() async throws {
        index = try await call { try $0.indexStatus() }
    }

    /// Embeds every item again with the archive's embedding model.
    @discardableResult
    public func rebuildIndex() async throws -> IndexBuilt {
        let model = index?.model ?? "the archive’s model"
        let built = try await call(reporting: "Embedding with \(model)") { handle, relay in
            try handle.rebuildIndex(progress: relay)
        }
        try await refreshIndex()
        return built
    }

    /// Switches the archive to the embedding model `model`, building its index while search
    /// keeps using the current one; the archive's own model stops a switch under way.
    @discardableResult
    public func switchIndex(to model: String) async throws -> IndexSwitched {
        let switched: IndexSwitched
        do {
            switched = try await call(reporting: "Embedding with \(model)") { handle, relay in
                try handle.switchIndex(model: model, progress: relay)
            }
        } catch {
            // A switch that stopped part way is recorded, so show where it got to.
            try? await refreshIndex()
            throw error
        }
        try await refreshIndex()
        try await changed { try await $0.refreshSettings() }
        return switched
    }

    // MARK: - The intake queue

    /// Adds the files and folders at `urls` to the intake queue.
    @discardableResult
    public func add(_ urls: [URL], each: Bool = false, whole: Bool = false) async throws -> Added {
        let paths = urls.map { $0.path(percentEncoded: false) }
        let added = try await call { try $0.queue(paths: paths, each: each, whole: whole) }
        try await refreshIntake()
        return added
    }

    /// Takes the entry `id` out of the intake queue.
    public func unqueue(_ id: String) async throws {
        try await call { try $0.unqueue(id: id) }
        try await refreshIntake()
    }

    /// Moves the entry `id` to the front of the intake queue.
    public func queueFirst(_ id: String) async throws {
        try await call { try $0.queueFirst(id: id) }
        try await refreshIntake()
    }

    /// Lets the failed entry `id` be tried again.
    public func retryQueued(_ id: String) async throws {
        try await call { try $0.retryQueued(id: id) }
        try await refreshIntake()
    }

    /// Drafts what's queued here, when the agent isn't running to do it: with it running,
    /// this does nothing and says so, and the agent drafts.
    @discardableResult
    public func draftQueued() async throws -> Drafted {
        if handle.agentRunning() {
            return Drafted(drafted: 0, embedded: 0, failures: [], alreadyRunning: true)
        }
        let drafted = try await call(reporting: "Drafting") { handle, relay in
            try handle.draftQueued(progress: relay)
        }
        try await refreshIntake()
        try await changed { try await $0.refreshReview() }
        return drafted
    }
}
