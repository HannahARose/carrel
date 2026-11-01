// Readings of what a proposal's item is in the inspector (#449), against a temporary copy of
// the sample archive whose proposal is given two readings, with a configuration folder of
// its own. Drafting them is tested in the core.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class ReadingsTests: XCTestCase {
    /// A window whose proposal has a second reading, which the file name doesn't fit.
    private func unsure() async throws -> SampleWindow {
        let sample = try await SampleWindow()
        let entry = sample.root.appending(path: "_System/queue/review/\(sampleProposal).json")
        var proposal = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: entry)) as? [String: Any])
        let card = try XCTUnwrap(proposal["card"] as? [String: Any])
        proposal["readings"] = [
            [
                "title": card["title"] ?? "", "summary": card["summary"] ?? "",
                "tags": card["tags"] ?? [], "reason": "The file name says so.",
            ],
            [
                "title": "Lampshade print file", "summary": "A print file for a lampshade.",
                "reason": "Its dome shape.", "against": ["the file name (“lamp-diffuser”)"],
            ],
        ]
        try JSONSerialization.data(withJSONObject: proposal).write(to: entry)
        try await sample.archive.refreshReview()
        return sample
    }

    func testTheInspectorOffersTheReadingsAndTheListNotesThem() async throws {
        let sample = try await unsure()
        defer { sample.remove() }
        let proposal = try XCTUnwrap(sample.review.proposals.first)
        XCTAssertTrue(proposal.readingsDoubted)
        XCTAssertEqual(proposal.readings.map(\.number), [1, 2])
        XCTAssertTrue(proposal.readings[0].onCard)
        XCTAssertFalse(proposal.readings[1].onCard)
        XCTAssertTrue(proposal.flags.contains("what is it?"))
        XCTAssertEqual(
            proposal.readings[1].against.map { ReadingRow.doesNotFit($0) },
            ["The file name (“lamp-diffuser”) doesn’t fit it"])
    }

    func testUsingAReadingOpensTheCorrectionAndSavingRewritesTheCard() async throws {
        let sample = try await unsure()
        defer { sample.remove() }
        let model = sample.model
        let proposal = try XCTUnwrap(sample.review.proposals.first)
        model.pickReading(proposal.readings[1], of: proposal)
        let correction = try XCTUnwrap(model.correction)
        XCTAssertEqual(correction.words, readingPickWords(number: 2))
        XCTAssertEqual(correction.target, .proposal(id: proposal.id, title: proposal.fields.title))

        await correction.prepare()
        XCTAssertNil(correction.problem)
        let changes = try XCTUnwrap(correction.changes)
        XCTAssertEqual(changes.after.describe("title"), "Lampshade print file")
        XCTAssertNil(changes.model, "picking needs no model")
        await correction.confirm()
        guard case .saved = correction.stage else { return XCTFail("not saved") }
        let after = try XCTUnwrap(sample.review.proposals.first)
        XCTAssertEqual(after.fields.title, "Lampshade print file")
        XCTAssertTrue(after.readings[1].onCard)
        XCTAssertFalse(after.readingsDoubted)
    }
}
