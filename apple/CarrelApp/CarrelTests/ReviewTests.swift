// Review, Questions, corrections and card editing, each test against its own temporary copy of
// the sample archive with a configuration folder of its own, so nothing outside it is written.

import CarrelKit
import CryptoKit
import XCTest

@testable import Carrel

/// A window on a temporary copy of the sample archive, with its history started.
@MainActor
struct SampleWindow {
    let folder: URL
    let model: ArchiveWindowModel

    var root: URL { folder.appending(path: "archive") }
    var archive: ArchiveModel { model.archive! }
    var review: ReviewModel { model.review! }
    var questions: QuestionsModel { model.questions! }

    /// Opens the copy, first adding the questions `questions` to its queue. When `fileable`,
    /// the sample's proposal is for a real file on this device, so it can be filed.
    /// When `duplicating`, the proposal duplicates the archived item with that identifier.
    init(
        questions: [String: Any] = [:], fileable: Bool = false, duplicating: String? = nil
    ) async throws {
        folder = FileManager.default.temporaryDirectory
            .appending(path: "carrel-app-\(UUID().uuidString)")
        let root = folder.appending(path: "archive")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: sampleArchive, to: root)
        let queue = root.appending(path: "_System/queue/questions")
        try FileManager.default.createDirectory(at: queue, withIntermediateDirectories: true)
        for (id, question) in questions {
            let data = try JSONSerialization.data(withJSONObject: question)
            try data.write(to: queue.appending(path: "\(id).json"))
        }
        if fileable { try Self.makeFileable(root: root, folder: folder) }
        if let duplicating {
            let entry = root.appending(path: "_System/queue/review/\(sampleProposal).json")
            var proposal = try XCTUnwrap(
                JSONSerialization.jsonObject(with: Data(contentsOf: entry)) as? [String: Any])
            proposal["duplicate_of"] = duplicating
            try JSONSerialization.data(withJSONObject: proposal).write(to: entry)
        }
        model = ArchiveWindowModel(
            url: root, options: .isolated(config: folder.appending(path: "config")))
        await model.load()
        try XCTUnwrap(model.handle).startHistory()
        try await model.archive?.refreshAll()
    }

    /// Points the sample's proposal at a file in `folder`, on the device the configuration
    /// folder says this is.
    private static func makeFileable(root: URL, folder: URL) throws {
        let file = folder.appending(path: "lamp-diffuser.stl")
        let contents = Data("solid diffuser\nendsolid diffuser\n".utf8)
        try contents.write(to: file)
        let entry = root.appending(path: "_System/queue/review/\(sampleProposal).json")
        var proposal = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: entry)) as? [String: Any])
        var source = try XCTUnwrap(proposal["source"] as? [String: Any])
        let checksum = SHA256.hash(data: contents).map { String(format: "%02x", $0) }.joined()
        source["path"] = file.path(percentEncoded: false)
        source["checksum"] = "sha256:\(checksum)"
        source["size"] = contents.count
        proposal["source"] = source
        try JSONSerialization.data(withJSONObject: proposal).write(to: entry)
        let config = folder.appending(path: "config")
        try FileManager.default.createDirectory(at: config, withIntermediateDirectories: true)
        let identity = ["person": UUID().uuidString.lowercased(), "device": source["device"]!]
        try JSONSerialization.data(withJSONObject: identity)
            .write(to: config.appending(path: "identity.json"))
    }

    func remove() {
        try? FileManager.default.removeItem(at: folder)
    }
}

/// The sample's one proposal.
let sampleProposal = "01a18701-0500-776a-9867-08f000000001"

/// A question about the sample's proposal, as ingest writes one.
func question(_ id: String, kind: String, subject: String, choices: [String] = []) -> [String: Any]
{
    [
        "carrel": "question", "format": "1.0", "id": id,
        "batch": "01a18701-0500-776a-9867-08f0000000b1",
        "created": "2026-10-29T16:21:00-07:00", "kind": kind, "subject": subject,
        "question": "What about \(subject)?", "items": [sampleProposal], "choices": choices,
    ]
}

/// A model written in Swift that answers every correction with a new title.
final class Retitling: ModelProvider {
    func complete(request: ModelRequest) throws -> ModelAnswer {
        ModelAnswer(text: #"{"title": "Lamp diffuser, frosted"}"#, model: "swift/test")
    }
}

@MainActor
final class ReviewModelTests: XCTestCase {
    func testGroupsProposalsByDestinationWithABadge() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let review = sample.review
        XCTAssertEqual(review.groups.map(\.destination), ["Projects/Hardware"])
        XCTAssertEqual(review.groups.first?.proposals.map(\.id), [sampleProposal])
        XCTAssertEqual(sample.archive.reviewCount, 1, "the sidebar's badge")
        XCTAssertEqual(review.proposals.first?.flags.first, review.proposals.first?.confidence.name)
        XCTAssertNil(review.inspected)
        review.selection = [sampleProposal]
        XCTAssertEqual(review.inspected?.id, sampleProposal)
        XCTAssertEqual(review.batch(of: sampleProposal)?.proposals.count, 1)
    }

    func testLettersOnlyActOnASelection() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let review = sample.review
        XCTAssertFalse(review.key("m"), "nothing chosen")
        review.selection = [sampleProposal]
        XCTAssertFalse(review.key("x"))
        XCTAssertTrue(review.key("m"))
        XCTAssertEqual(review.moving, [sampleProposal])
    }

    func testMovesAndRejects() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let review = sample.review
        review.moving = [sampleProposal]
        await review.move([sampleProposal], to: "Holding")
        XCTAssertNil(review.moving)
        XCTAssertEqual(review.groups.map(\.destination), ["Holding"])
        XCTAssertEqual(review.report?.hasPrefix("Moved to Holding"), true)

        await review.move([sampleProposal], to: "Nowhere")
        XCTAssertEqual(sample.archive.lastError?.message.contains("Nowhere"), true)
        sample.archive.lastError = nil

        review.selection = [sampleProposal]
        await review.reject([sampleProposal])
        XCTAssertTrue(review.groups.isEmpty)
        XCTAssertTrue(review.selection.isEmpty)
        XCTAssertEqual(sample.archive.reviewCount, 0)
    }

    func testApprovingFilesTheItemAndBrowseShowsIt() async throws {
        let sample = try await SampleWindow(fileable: true)
        defer { sample.remove() }
        let before = sample.model.items.count
        let title = try XCTUnwrap(sample.review.proposals.first?.fields.title)
        sample.review.selection = [sampleProposal]
        await sample.review.approveBatch(of: sampleProposal)
        XCTAssertNil(sample.archive.lastError)
        XCTAssertEqual(sample.review.report, "Filed 1 item")
        XCTAssertTrue(sample.review.selection.isEmpty)
        XCTAssertTrue(sample.review.groups.isEmpty)
        XCTAssertEqual(sample.model.items.count, before + 1)
        XCTAssertTrue(sample.model.items(in: "Projects/Hardware").contains { $0.title == title })
        XCTAssertEqual(sample.archive.history.first?.operation, "ingest")
    }

    func testHoldingFilesInHolding() async throws {
        let sample = try await SampleWindow(fileable: true)
        defer { sample.remove() }
        let holding = sample.model.items(in: "Holding").count
        await sample.review.hold([sampleProposal])
        XCTAssertEqual(sample.model.items(in: "Holding").count, holding + 1)
    }

    func testADuplicateIsComparedAndKeptBesideTheOriginal() async throws {
        let original = "01a18701-0500-776a-9867-080d24773181"
        let sample = try await SampleWindow(fileable: true, duplicating: original)
        defer { sample.remove() }
        let review = sample.review
        XCTAssertEqual(review.proposals.first?.flags.last, "duplicate")
        let comparisons = await review.comparisons(of: sampleProposal)
        XCTAssertEqual(comparisons.first?.duplicate.id, original)
        XCTAssertTrue(
            comparisons.first?.rows.contains { $0.label == "title" && $0.differs } ?? false)

        review.selection = [sampleProposal]
        await review.keepDuplicate(sampleProposal, asVersion: false, of: original)
        XCTAssertNil(sample.archive.lastError)
        XCTAssertEqual(review.report, "Filed 1 item")
        XCTAssertTrue(review.groups.isEmpty)
    }

    func testCompressedChoicesNeedACompressedFile() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let review = sample.review
        review.selection = [sampleProposal]
        XCTAssertFalse(review.key("z"), "not a compressed file")
        XCTAssertFalse(review.key("v"), "not a duplicate")
        await review.chooseCompressed([sampleProposal], expand: true)
        XCTAssertNotNil(sample.archive.lastError, "the core says why")
        XCTAssertNil(review.report)
    }

    func testDescribesWhatFilingDid() {
        let outcome = FilingOutcome(
            filed: [], failed: [NotFiled(proposal: "p", title: "Lamp", error: "it moved")],
            commit: nil, snapshot: nil, snapshotError: nil, listingProblems: [])
        XCTAssertEqual(ReviewModel.describe(outcome), "“Lamp” wasn’t filed: it moved")
    }
}

@MainActor
final class QuestionsModelTests: XCTestCase {
    static let collection = "01a18701-0500-776a-9867-08f0000000c1"
    static let term = "01a18701-0500-776a-9867-08f0000000c2"

    private func sample() async throws -> SampleWindow {
        try await SampleWindow(questions: [
            Self.collection: question(
                Self.collection, kind: "collection", subject: "diffuser",
                choices: ["Projects/Hardware", "Holding"]),
            Self.term: question(Self.term, kind: "term", subject: "PLA"),
        ])
    }

    func testListsQuestionsWithWhatTheyHoldUp() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let questions = sample.questions
        XCTAssertEqual(Set(questions.questions.map(\.id)), [Self.collection, Self.term])
        XCTAssertEqual(sample.archive.questions.count, 2, "the sidebar's badge")
        let asked = try XCTUnwrap(questions.questions.first { $0.id == Self.collection })
        let title = try XCTUnwrap(sample.review.proposals.first?.fields.title)
        XCTAssertEqual(questions.affected(by: asked), [title])
        XCTAssertTrue(asked.summary.contains("holds up 1 proposal"))
    }

    func testAnsweringACollectionMovesItsProposals() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let questions = sample.questions
        questions.selection = Self.collection
        await questions.answer(Self.collection)
        XCTAssertEqual(questions.questions.count, 2, "nothing chosen, nothing answered")

        questions.answers[Self.collection] = "Holding"
        await questions.answer(Self.collection)
        XCTAssertNil(sample.archive.lastError)
        XCTAssertNil(questions.selection)
        XCTAssertFalse(questions.questions.contains { $0.id == Self.collection })
        XCTAssertEqual(questions.report, "Moved 1 proposal to Holding")
        XCTAssertEqual(sample.review.groups.map(\.destination), ["Holding"])
    }

    func testDismissing() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        await sample.questions.dismiss(Self.term)
        XCTAssertNil(sample.archive.lastError)
        XCTAssertEqual(sample.questions.questions.map(\.id), [Self.collection])
    }
}

@MainActor
final class CardEditingTests: XCTestCase {
    func testAProposalsCardIsCheckedAndSaved() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let edit = CardEditModel(archive: sample.archive, target: .proposal(sampleProposal))
        await edit.load()
        XCTAssertNotNil(edit.fields)
        XCTAssertFalse(edit.hasChanges)
        XCTAssertEqual(edit.sensitivities.first, edit.floor)

        edit.fields?.title = " "
        let saved = await edit.save()
        XCTAssertFalse(saved)
        XCTAssertFalse(edit.problems(about: "title").isEmpty)

        edit.revert()
        XCTAssertTrue(edit.problems.isEmpty)
        edit.fields?.summary = "A frosted diffuser for the desk lamp."
        let savedSummary = await edit.save()
        XCTAssertTrue(savedSummary)
        XCTAssertFalse(edit.hasChanges)
        XCTAssertEqual(
            sample.review.proposals.first?.fields.summary, "A frosted diffuser for the desk lamp.")
    }

    func testAFiledCardIsEditedFromTheInspector() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let model = sample.model
        XCTAssertFalse(model.canEditSelectedItem)
        model.screen = .collection("Projects/Hardware")
        let item = try XCTUnwrap(model.items.first { $0.title == "Desk lamp parts list" })
        model.selectedItemID = item.id
        XCTAssertTrue(model.canEditSelectedItem)
        model.editSelectedItem()
        let edit = try XCTUnwrap(model.editing)
        XCTAssertEqual(edit.target, .item(item.id))
        await edit.load()
        edit.fields?.title = "Lamp parts list"
        let saved = await edit.save()
        XCTAssertTrue(saved)
        XCTAssertEqual(model.selectedItem?.title, "Lamp parts list")
        XCTAssertEqual(sample.archive.history.first?.summary.contains("parts list"), true)

        model.selectedItemID = nil
        XCTAssertNil(model.editing, "choosing something else stops editing")
    }
}

@MainActor
final class CorrectionModelTests: XCTestCase {
    func testCorrectsTheProposalInTheInspector() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let model = sample.model
        model.screen = .review
        XCTAssertNil(model.correctionTarget)
        model.review?.selection = [sampleProposal]
        let target = try XCTUnwrap(model.correctionTarget)
        guard case .proposal(let id, _) = target else { return XCTFail("not the proposal") }
        XCTAssertEqual(id, sampleProposal)

        let correction = CorrectionModel(
            archive: sample.archive, target: target, provider: Retitling())
        XCTAssertFalse(correction.canPrepare)
        correction.words = "call it a frosted lamp diffuser"
        await correction.prepare()
        XCTAssertNil(correction.problem)
        let changes = try XCTUnwrap(correction.changes)
        XCTAssertEqual(changes.changed, ["title"])
        XCTAssertEqual(changes.after.describe("title"), "Lamp diffuser, frosted")

        await correction.confirm()
        guard case .saved(let saved) = correction.stage else { return XCTFail("not saved") }
        XCTAssertFalse(saved.filed)
        XCTAssertEqual(sample.review.proposals.first?.fields.title, "Lamp diffuser, frosted")
    }

    func testRewritingDropsThePreparedCorrection() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let correction = CorrectionModel(
            archive: sample.archive, target: .proposal(id: sampleProposal, title: "Lamp"),
            provider: Retitling())
        correction.words = "frosted"
        await correction.prepare()
        XCTAssertNotNil(correction.changes)
        correction.rewrite()
        XCTAssertNil(correction.changes)
        XCTAssertTrue(correction.canPrepare)
    }

    func testOpensTheSheetForTheChosenItem() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let model = sample.model
        model.screen = .collection("Projects/Hardware")
        let item = try XCTUnwrap(model.items.first { $0.title == "Desk lamp parts list" })
        model.selectedItemID = item.id
        model.correct()
        XCTAssertEqual(model.correction?.target, .item(id: item.id, title: item.title))
    }
}
