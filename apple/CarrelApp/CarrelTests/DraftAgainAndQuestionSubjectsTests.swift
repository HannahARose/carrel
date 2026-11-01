// Draft Again in Review (#445), and what a question is about in Questions (#446), each test
// against its own temporary copy of the sample archive with a configuration folder of its own.

import CarrelKit
import XCTest

@testable import Carrel

/// A model written in Swift that drafts every card the same, files it in the first collection
/// offered, and rates it personal.
final class Drafting: ModelProvider {
    func complete(request: ModelRequest) throws -> ModelAnswer {
        let schema = (request.schema ?? "").data(using: .utf8) ?? Data()
        let properties =
            ((try? JSONSerialization.jsonObject(with: schema)) as? [String: Any])?["properties"]
            as? [String: Any] ?? [:]
        let text: String
        if let choice = properties["choice"] as? [String: Any] {
            let first = (choice["enum"] as? [String])?.first ?? ""
            text = #"{"choice": "\#(first)", "fit": "well", "reason": "it fits"}"#
        } else if properties["level"] != nil {
            text = #"{"level": "personal", "reason": "nothing private"}"#
        } else {
            text =
                #"{"title": "Frosted lamp diffuser", "summary": "The diffuser's print file.", "created": "", "tags": []}"#
        }
        return ModelAnswer(text: text, model: "swift/test")
    }
}

@MainActor
final class DraftAgainTests: XCTestCase {
    /// A window whose proposal was drafted when no text model could be reached.
    private func failedDraft() async throws -> SampleWindow {
        let sample = try await SampleWindow(fileable: true)
        let entry = sample.root.appending(path: "_System/queue/review/\(sampleProposal).json")
        var proposal = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: entry)) as? [String: Any])
        var card = try XCTUnwrap(proposal["card"] as? [String: Any])
        card["title"] = "lamp-diffuser"
        card["summary"] = nil
        card["tags"] = []
        card["generated"] = nil
        proposal["card"] = card
        proposal["proposed_tags"] = []
        proposal["state"] = "needs-owner"
        proposal["problems"] = [
            ["stage": "card", "message": "no text model on this device; write the card by hand"]
        ]
        try JSONSerialization.data(withJSONObject: proposal).write(to: entry)
        try await sample.archive.refreshReview()
        return sample
    }

    func testAFailedDraftOffersDraftAgainFirst() async throws {
        let sample = try await failedDraft()
        defer { sample.remove() }
        let proposal = try XCTUnwrap(sample.review.proposals.first)
        XCTAssertTrue(proposal.draftAgain)
        XCTAssertTrue(proposal.flags.contains("draft again?"))
        let failed = try await sample.archive.failedDrafts()
        XCTAssertEqual(failed, [sampleProposal])
    }

    func testDraftAgainKeepsTheProposalAndDraftsItsCard() async throws {
        let sample = try await failedDraft()
        defer { sample.remove() }
        let review = sample.review
        review.redraftModel = Drafting()
        review.selection = [sampleProposal]
        await review.redraft([sampleProposal])
        XCTAssertEqual(review.report, "Drafted “Frosted lamp diffuser” again")
        let proposal = try XCTUnwrap(review.proposals.first)
        XCTAssertEqual(proposal.id, sampleProposal, "the same proposal, drafted again")
        XCTAssertEqual(proposal.fields.title, "Frosted lamp diffuser")
        XCTAssertFalse(proposal.draftAgain)
        XCTAssertEqual(review.proposals.count, 1)
    }

    func testDraftAgainWithoutAModelSaysWhatsMissing() async throws {
        let sample = try await failedDraft()
        defer { sample.remove() }
        let review = sample.review
        XCTAssertTrue(review.key("d") == false, "nothing chosen")
        await review.redraft([sampleProposal])
        let error = try XCTUnwrap(sample.archive.lastError)
        XCTAssertTrue(
            message(for: error).hasPrefix("no text model on this device; write the card by hand"),
            message(for: error))
        XCTAssertTrue(message(for: error).contains("Drafting cards needs a text model"))
        XCTAssertNil(review.report)
        XCTAssertEqual(review.proposals.first?.fields.title, "lamp-diffuser", "unchanged")
    }
}

@MainActor
final class QuestionSubjectsTests: XCTestCase {
    func testAQuestionShowsTheItemsItsAboutWithTheirFiles() async throws {
        let sample = try await SampleWindow(fileable: true)
        defer { sample.remove() }
        let item = try XCTUnwrap(
            sample.model.items.first { $0.card == "Projects/Hardware/parts-list.csv.md" })
        let id = "01a18701-0500-776a-9867-08f0000000d1"
        // A group question: the filed item, and the sample's proposal it holds up.
        var asked = question(id, kind: "group", subject: "lamp", choices: ["Projects/Hardware"])
        asked["about"] = [item.id]
        let queue = sample.root.appending(path: "_System/queue/questions")
        try FileManager.default.createDirectory(at: queue, withIntermediateDirectories: true)
        try JSONSerialization.data(withJSONObject: asked)
            .write(to: queue.appending(path: "\(id).json"))

        let subjects = await sample.questions.subjects(of: id)
        XCTAssertEqual(subjects.map(\.id), [item.id, sampleProposal])
        let filed = subjects[0]
        XCTAssertFalse(filed.proposal)
        XCTAssertEqual(filed.card, item.card)
        XCTAssertEqual(
            filed.url?.standardizedFileURL,
            sample.root.appending(path: "Projects/Hardware/parts-list.csv").standardizedFileURL)
        XCTAssertFalse(filed.restricted)
        // The filed item's preview is Browse's.
        let previews = try XCTUnwrap(sample.model.previews)
        await previews.reload()
        XCTAssertEqual(
            previews.content(for: item),
            .quickLook(URL(filePath: try XCTUnwrap(filed.file))))
        let waiting = subjects[1]
        XCTAssertTrue(waiting.proposal)
        XCTAssertTrue(waiting.title.hasPrefix("Desk lamp diffuser"))
        XCTAssertTrue(FileManager.default.fileExists(atPath: try XCTUnwrap(waiting.file)))
    }

    func testARestrictedItemIsMarkedSoItsPreviewWaits() async throws {
        let sample = try await SampleWindow(fileable: true)
        defer { sample.remove() }
        let entry = sample.root.appending(path: "_System/queue/review/\(sampleProposal).json")
        var proposal = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: entry)) as? [String: Any])
        var card = try XCTUnwrap(proposal["card"] as? [String: Any])
        card["sensitivity"] = "restricted"
        proposal["card"] = card
        try JSONSerialization.data(withJSONObject: proposal).write(to: entry)
        let id = "01a18701-0500-776a-9867-08f0000000d2"
        let queue = sample.root.appending(path: "_System/queue/questions")
        try FileManager.default.createDirectory(at: queue, withIntermediateDirectories: true)
        try JSONSerialization.data(
            withJSONObject: question(id, kind: "term", subject: "diffuser")
        ).write(to: queue.appending(path: "\(id).json"))

        let subjects = await sample.questions.subjects(of: id)
        XCTAssertEqual(subjects.count, 1)
        XCTAssertTrue(subjects[0].restricted)
        XCTAssertNotNil(subjects[0].url, "its file can still be opened when asked")
    }
}
