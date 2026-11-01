// ArchiveModel and the core's operations, each group against its own temporary copy of the
// sample archive, with a configuration folder of its own and only the built-in providers, so
// nothing outside the copy is written.

import CarrelKit
import Foundation
import Testing

/// A temporary copy of the sample archive with its history started.
struct Sample {
    let folder: URL
    let model: ArchiveModel

    var root: URL { folder.appending(path: "archive") }

    @MainActor
    init() throws {
        folder = FileManager.default.temporaryDirectory
            .appending(path: "carrelkit-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: sampleArchive, to: folder.appending(path: "archive"))
        let handle = try ArchiveHandle.open(
            folder.appending(path: "archive"),
            options: .isolated(config: folder.appending(path: "config"))
        )
        try handle.startHistory()
        model = ArchiveModel(handle: handle)
    }

    func item(titled title: String) throws -> String {
        try #require(try model.handle.items().first { $0.title == title }).id
    }
}

/// A model written in Swift that answers every correction with a new title.
final class Retitling: ModelProvider {
    func complete(request: ModelRequest) throws -> ModelAnswer {
        #expect(request.schema != nil, "corrections ask for JSON")
        return ModelAnswer(text: #"{"title": "Lamp diffuser, frosted"}"#, model: "swift/test")
    }
}

/// Collects what a long operation reports.
final class Reports: ProgressListener, @unchecked Sendable {
    private let lock = NSLock()
    private var seen: [(UInt64, UInt64)] = []

    var count: Int { lock.withLock { seen.count } }

    func update(done: UInt64, total: UInt64, item: String?) {
        lock.withLock { seen.append((done, total)) }
    }
}

@MainActor
@Suite struct ReviewAndQuestions {
    @Test func theQueueIsListedByBatchAndDecided() async throws {
        let sample = try Sample()
        let model = sample.model
        try await model.refreshAll()
        #expect(model.reviewCount == 1)
        #expect(model.status?.review == 1)
        let proposal = try #require(model.review.batches.first?.proposals.first)
        #expect(proposal.destination == "Projects/Hardware")
        #expect(proposal.newTag == "lighting")

        let moved = try await model.move(proposal.id, to: "Holding")
        #expect(moved.to == "Holding")
        #expect(model.review.batches.first?.proposals.first?.destination == "Holding")

        try await model.reject([proposal.id])
        #expect(model.reviewCount == 0)
        await #expect(throws: CarrelError.self) { try await model.reject([proposal.id]) }
        #expect(model.lastError?.message.contains("already rejected") == true)
        #expect(model.questions.isEmpty)
    }

    @Test func movingToNoCollectionIsInvalid() async throws {
        let sample = try Sample()
        let id = try #require(try sample.model.handle.review().batches.first?.proposals.first).id
        do {
            try await sample.model.move(id, to: "Nowhere")
            Issue.record("moved")
        } catch let CarrelError.Invalid(message) {
            #expect(message.contains("Nowhere"))
        }
    }
}

@MainActor
@Suite struct CorrectionsAndEditing {
    @Test func aSwiftModelCorrectsAProposal() async throws {
        let sample = try Sample()
        let model = sample.model
        try await model.refreshReview()
        let id = try #require(model.review.batches.first?.proposals.first).id
        let correction = try await model.prepareCorrection(
            proposal: id, words: "call it a frosted lamp diffuser", model: Retitling())
        let changes = correction.changes()
        #expect(changes.after.title == "Lamp diffuser, frosted")
        #expect(changes.changed == ["title"])
        #expect(changes.model == "app/model")
        #expect(model.progress == nil)

        let saved = try await model.save(correction)
        #expect(!saved.filed)
        #expect(
            model.review.batches.first?.proposals.first?.fields.title == "Lamp diffuser, frosted")
        await #expect(throws: CarrelError.self) { try await model.save(correction) }
    }

    @Test func aFiledItemsCorrectionCanBeUndone() async throws {
        let sample = try Sample()
        let model = sample.model
        let item = try sample.item(titled: "Desk lamp parts list")
        let correction = try await model.prepareCorrection(
            item: item, words: "title: Lamp parts list")
        let saved = try await model.save(correction)
        #expect(saved.filed)
        let commit = try #require(saved.commit)
        #expect(model.history.first?.id == commit)
        #expect(try model.handle.itemDetail(id: item).fields.title == "Lamp parts list")

        try await model.undo(commit)
        #expect(try model.handle.itemDetail(id: item).fields.title == "Desk lamp parts list")
    }

    @Test func editsAreCheckedAsCarrelEditChecksThem() async throws {
        let sample = try Sample()
        let model = sample.model
        let item = try sample.item(titled: "Desk lamp parts list")
        let editable = try model.handle.editableCard(item: item)
        #expect(editable.tags.contains("3d-printing") || !editable.tags.isEmpty)

        var fields = editable.fields
        fields.title = " "
        guard case .problems(let problems) = try await model.edit(item: item, fields: fields) else {
            Issue.record("an empty title was saved")
            return
        }
        #expect(problems.contains { $0.contains("title") })

        fields = editable.fields
        fields.summary = "Every part, with where it came from."
        guard
            case .saved(let changed, let commit) = try await model.edit(item: item, fields: fields)
        else {
            Issue.record("not saved")
            return
        }
        #expect(changed == ["summary"])
        #expect(commit != nil)
        #expect(model.history.first?.summary.contains("Desk lamp parts list") == true)
    }

    @Test func aProposalTakesOneNewTag() async throws {
        let sample = try Sample()
        let id = try #require(try sample.model.handle.review().batches.first?.proposals.first).id
        var fields = try sample.model.handle.editableProposal(id: id).fields
        fields.tags += ["one-new", "two-new"]
        guard case .problems = try await sample.model.edit(proposal: id, fields: fields) else {
            Issue.record("two new tags were saved")
            return
        }
    }
}

@MainActor
@Suite struct Active {
    @Test func checkingOutAndReturning() async throws {
        let sample = try Sample()
        let model = sample.model
        let item = try sample.item(titled: "Desk lamp parts list")
        let desktop = sample.folder.appending(path: "Desktop")
        try FileManager.default.createDirectory(at: desktop, withIntermediateDirectories: true)

        let out = try await model.checkOut(item, to: desktop, due: "2026-12-01")
        let listed = try #require(model.active.first { $0.id == item })
        #expect(listed.kind == .checkedOut)
        #expect(listed.due == "2026-12-01")
        #expect(listed.state == "unchanged")

        try "part,source\nLED strip,shop\n".write(
            toFile: out.working, atomically: true, encoding: .utf8)
        let preview = try model.handle.returnPreview(item: item, from: nil)
        #expect(preview.changes.changed.count == 1)
        let returned = try await model.returnItem(item)
        #expect(returned.commit != nil)
        #expect(!model.active.contains { $0.id == item })
        #expect(model.history.first?.operation == "return")
    }

    @Test func theDeskTakesItemsAndLetsThemGo() async throws {
        let sample = try Sample()
        let model = sample.model
        let plans = sample.folder.appending(path: "plans.txt")
        try "plans".write(to: plans, atomically: true, encoding: .utf8)

        let desk = try await model.addToDesk(plans, title: "Plans", summary: "What's next.")
        #expect(model.active.contains { $0.id == desk.id && $0.kind == .desk })
        do {
            try await model.decide(entries: [], of: desk.id, .approve)
            Issue.record("decided nothing")
        } catch let CarrelError.Invalid(message) {
            #expect(message.contains("no pending entries"))
        }
        try await model.removeFromDesk(plans, deletingCard: true)
        #expect(!model.active.contains { $0.id == desk.id })
    }
}

@MainActor
@Suite struct SearchAndHistory {
    @Test func searchFindsByWordsWithReasonsAndFilters() async throws {
        let sample = try Sample()
        let model = sample.model
        let results = try await model.search("lamp")
        let hit = try #require(results.hits.first { $0.title == "Desk lamp parts list" })
        #expect(!hit.why.isEmpty)
        #expect(hit.found == .item)
        #expect(model.searchResults == results)

        let physical = try await model.search("kind:physical")
        #expect(!physical.hits.isEmpty)
        #expect(physical.hits.allSatisfy { $0.kind == .physical })
        await #expect(throws: CarrelError.self) { try await model.search("  ") }
    }

    @Test func searchReportsReadingText() throws {
        let sample = try Sample()
        let reports = Reports()
        _ = try sample.model.handle.search(query: "oscillators", limit: 5, progress: reports)
        #expect(reports.count > 0, "the sample's items' text is read the first time")
    }

    @Test func anItemShowsItsCardHistoryAndRelated() async throws {
        let sample = try Sample()
        let model = sample.model
        let item = try sample.item(titled: "Desk lamp parts list")
        let detail = try model.handle.itemDetail(id: item)
        #expect(detail.collection == "Projects/Hardware")
        #expect(try model.handle.itemHistory(id: item).isEmpty)
        let related = try model.handle.related(id: item)
        #expect(related.items.isEmpty)
        #expect(related.whyNone != nil)
    }

    @Test func statusAndRecentFollowChanges() async throws {
        let sample = try Sample()
        let model = sample.model
        try await model.refreshAll()
        let status = try #require(model.status)
        #expect(status.items == 12)
        #expect(status.hasHistory)
        #expect(status.writer == nil)
        try await model.set("ingest.originals", to: "keep")
        #expect(model.status?.recent.first?.title.contains("ingest.originals") == true)
        #expect(model.settings.first { $0.key == "ingest.originals" }?.value == "keep")
    }
}

@MainActor
@Suite struct SettingsAndModels {
    @Test func settingsChangeAsOneChange() async throws {
        let sample = try Sample()
        let model = sample.model
        try await model.refreshSettings()
        let originals = try #require(model.settings.first { $0.key == "ingest.originals" })
        #expect(originals.choices == ["move", "keep"])
        let changed = try await model.set("ingest.originals", to: "keep")
        #expect(changed.from == "move" && changed.to == "keep")
        await #expect(throws: CarrelError.self) {
            try await model.set("ingest.originals", to: "shred")
        }
    }

    @Test func modelsAreChosenFromThoseAvailable() async throws {
        let sample = try Sample()
        let model = sample.model
        await #expect(throws: CarrelError.self) {
            try await model.chooseModels(drafting: "nope/nothing", secondOpinion: nil)
        }
        let off = try await model.chooseModels(drafting: nil, secondOpinion: "off")
        #expect(off.secondModel == "off")
        #expect(!model.handle.modelCatalog().isEmpty)
        #expect(model.handle.providers().contains { $0.name == "built-in" })
    }
}

@MainActor
@Suite struct Intake {
    @Test func queuedFilesAreDraftedThenFiled() async throws {
        let sample = try Sample()
        let model = sample.model
        let inbox = sample.folder.appending(path: "Inbox")
        try FileManager.default.createDirectory(at: inbox, withIntermediateDirectories: true)
        let list = inbox.appending(path: "shopping list.txt")
        try "LED strip, diffuser, screws".write(to: list, atomically: true, encoding: .utf8)

        let added = try await model.add([list])
        #expect(added.added.count == 1)
        #expect(model.intake.waiting.count == 1)

        let drafted = try await model.draftQueued()
        #expect(drafted.drafted == 1)
        #expect(model.intake.waiting.isEmpty)
        let proposal = try #require(
            model.review.batches.flatMap(\.proposals).first {
                $0.source.hasSuffix("shopping list.txt")
            })

        let outcome = try await model.hold([proposal.id])
        #expect(outcome.filed.count == 1)
        #expect(outcome.filed.first?.path.hasPrefix("Holding/") == true)
        #expect(model.history.first?.id == outcome.commit)
    }
}
