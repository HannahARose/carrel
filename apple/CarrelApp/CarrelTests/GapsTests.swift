// The gap log on Status (#455): a missing feature reported on an item keeps its type and the
// owner's words, never its name; Report… builds a draft for the browser that holds nothing
// else from the archive; and the issue's address marks the gap as reported.

import CarrelKit
import XCTest

@testable import Carrel

final class GapsTests: SampleCopyTestCase {
    func testAMissingFeatureIsDraftedForTheBrowserAndMarkedReported() async throws {
        let pdf = try XCTUnwrap(
            try archive.handle.items().first { $0.item?.hasSuffix(".pdf") == true })
        let gap = try await archive.reportMissingFeature(about: pdf.id, words: "Read its equations")
        XCTAssertEqual(gap.kind, "owner-reported")
        XCTAssertEqual(gap.extension, "pdf")

        let gaps = try await archive.gaps()
        XCTAssertEqual(gaps.map(\.number), [gap.number])

        let draft = try await archive.gapIssueDraft(gap.number, added: "Like LaTeX")
        XCTAssertTrue(draft.url.hasPrefix("https://github.com/Horizon-Labs/carrel/issues/new?"))
        XCTAssertTrue(draft.body.contains("Read its equations"))
        XCTAssertTrue(draft.body.contains("Like LaTeX"))
        XCTAssertFalse(draft.body.contains(pdf.title), "no titles in the draft")
        let saved = try String(
            contentsOf: folder.appending(path: "config/gaps.json"), encoding: .utf8)
        XCTAssertFalse(saved.contains(pdf.title))

        let issue = "https://github.com/Horizon-Labs/carrel/issues/9"
        let reported = try await archive.markGapReported(gap.number, at: issue)
        XCTAssertEqual(reported.reported, issue)
        XCTAssertTrue(StatusGapsSection.detail(of: reported).contains("Reported: \(issue)"))
        do {
            try await archive.markGapReported(gap.number, at: "soon")
            XCTFail("not an address")
        } catch {}
    }
}
