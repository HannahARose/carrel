// The crawler in the Mac app (#327): its findings under "From the crawler" in Review, approved
// as the owner's change or rejected for good, and how far it has got on Status. Each test runs
// against its own temporary copy of the sample archive.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class CrawlerTests: XCTestCase {
    static let finding = "01a18701-0500-776a-9867-08f0000000f1"

    /// A window on a sample copy with the crawler's finding that the sample note belongs in
    /// Projects.
    private func sample() async throws -> SampleWindow {
        let sample = try await SampleWindow()
        let queue = sample.root.appending(path: "_System/queue/crawler")
        try FileManager.default.createDirectory(at: queue, withIntermediateDirectories: true)
        let finding: [String: Any] = [
            "carrel": "crawl", "format": "1.0", "id": Self.finding,
            "created": "2026-11-01T10:00:00Z", "item": sampleNote, "card": sampleNoteCard,
            "title": "Coupled oscillators", "change": "move", "from": "Reading",
            "to": "Projects", "reason": "It's about the lamp build.", "confidence": 0.6,
            "model": "test/model",
        ]
        try JSONSerialization.data(withJSONObject: finding)
            .write(to: queue.appending(path: "\(Self.finding).json"))
        try await sample.archive.refreshCrawler()
        return sample
    }

    func testReviewSwitchesToTheCrawlersFindingsWithF() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let crawler = try XCTUnwrap(sample.model.crawler)
        XCTAssertFalse(crawler.isShown)
        XCTAssertTrue(sample.review.key("f"))
        XCTAssertTrue(crawler.isShown)
        let finding = try XCTUnwrap(crawler.findings.first)
        XCTAssertEqual(finding.change, "Move to Projects")
        XCTAssertEqual(finding.description, "move “Coupled oscillators” to Projects")
        crawler.selection = [finding.id]
        XCTAssertEqual(crawler.inspected?.id, Self.finding)
        XCTAssertTrue(crawler.key("f"))
        XCTAssertFalse(crawler.isShown, "f switches back")
    }

    func testAnApprovedFindingIsTheOwnersChange() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let crawler = try XCTUnwrap(sample.model.crawler)
        await crawler.approve([Self.finding])
        XCTAssertEqual(crawler.report, "Made 1 change")
        XCTAssertTrue(crawler.findings.isEmpty)
        let note = try XCTUnwrap(sample.model.items.first { $0.id == sampleNote })
        XCTAssertTrue(note.card.hasPrefix("Projects/"), note.card)
        await crawler.refresh()
        XCTAssertEqual(crawler.changes.count, 0, "the owner's, not the crawler's own")
    }

    func testARejectedFindingIsGoneForGood() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let crawler = try XCTUnwrap(sample.model.crawler)
        await crawler.reject([Self.finding])
        XCTAssertEqual(crawler.report, "Rejected; the crawler won’t suggest it again.")
        XCTAssertTrue(crawler.findings.isEmpty)
        let note = try XCTUnwrap(sample.model.items.first { $0.id == sampleNote })
        XCTAssertEqual(note.card, sampleNoteCard, "nothing moved")
    }

    func testStatusSaysHowFarItHasGot() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let crawler = try XCTUnwrap(sample.model.crawler)
        await crawler.refresh()
        let status = try XCTUnwrap(crawler.status)
        XCTAssertEqual(status.findings, 1)
        XCTAssertTrue(status.line.hasPrefix("Pace steady; it only suggests changes."), status.line)
    }

    func testItsSettingsAreInSettings() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let settings = sample.archive.settings
        let changes = try XCTUnwrap(settings.first { $0.key == "crawler.changes" })
        XCTAssertEqual(changes.value, "off")
        let pace = try XCTUnwrap(settings.first { $0.key == "crawler.pace" })
        XCTAssertEqual(pace.choices, ["off", "gentle", "steady", "brisk"])
    }

    func testCheckingNowSaysWhyWithNoModel() async throws {
        let sample = try await sample()
        defer { sample.remove() }
        let crawler = try XCTUnwrap(sample.model.crawler)
        // The sample's window starts no provider programs, so this Mac has no text model.
        await crawler.checkNow()
        XCTAssertEqual(crawler.report, "the crawler needs a text model, and this Mac has none")
        XCTAssertNil(sample.archive.lastError, "said on Status, not in an alert")
    }
}
