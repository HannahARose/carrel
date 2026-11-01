// Discover's Connections page and the Restructure screen (#327): the tag and link graph read
// from the core and laid out for drawing, and a proposal previewed, approved in part or
// rejected, each against a temporary copy of the sample archive.

import CarrelKit
import XCTest

@testable import Carrel

@MainActor
final class GraphAndRestructureTests: XCTestCase {
    func testTheGraphShowsWhatTiesCollectionsAndLaysThemOut() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let discover = try XCTUnwrap(sample.model.discover)
        await discover.refresh(.graph)
        XCTAssertNil(discover.problem)
        let graph = try XCTUnwrap(discover.graph)
        let connection = try XCTUnwrap(
            graph.connections.first { $0.a == "Projects" && $0.b == "Reading" })
        XCTAssertTrue(connection.ties.contains("1 link"), connection.ties)
        XCTAssertTrue(connection.ties.contains("oscillators"), connection.ties)
        XCTAssertTrue(connection.touches("Reading"))

        let layout = GraphLayout(graph: graph, in: CGSize(width: 400, height: 400))
        let projects = try XCTUnwrap(layout.positions["Projects"])
        let reading = try XCTUnwrap(layout.positions["Reading"])
        XCTAssertNotEqual(projects, reading)
        for point in layout.positions.values {
            XCTAssertTrue((0...400).contains(point.x) && (0...400).contains(point.y))
        }
        XCTAssertNil(layout.positions["Holding"], "a collection with no connection isn't drawn")
    }

    func testConnectionsIsADiscoverPage() {
        XCTAssertTrue(DiscoverPage.allCases.contains(.graph))
        XCTAssertEqual(DiscoverPage.graph.title, "Connections")
    }

    static let proposal = "01a18701-0500-776a-9867-08f0000000e1"

    /// Puts a tag proposal in the sample's queue: retire "oscillators" on the sample note, and
    /// merge "notes" into "note" on no item.
    func queueProposal(_ sample: SampleWindow) throws {
        let queue = sample.root.appending(path: "_System/queue/proposals")
        try FileManager.default.createDirectory(at: queue, withIntermediateDirectories: true)
        let change: (String, [String: Any], [[String: Any]]) -> [String: Any] = {
            [
                "id": $0, "change": $1, "reason": "rarely used", "invasiveness": "rewriting",
                "items": $2,
            ]
        }
        let proposal: [String: Any] = [
            "carrel": "proposal", "format": "1.0", "id": Self.proposal, "kind": "tags",
            "section": "", "created": "2026-11-01T10:00:00-05:00", "by": "carrel test",
            "invasiveness": "rewriting", "rationale": "Tidier tags.",
            "changes": [
                change(
                    "1", ["kind": "retire-tag", "tag": "oscillators"],
                    [["id": sampleNote, "card": sampleNoteCard, "title": "Coupled oscillators"]]),
                change("2", ["kind": "merge-tags", "from": ["notes"], "into": "note"], []),
            ],
        ]
        try JSONSerialization.data(withJSONObject: proposal)
            .write(to: queue.appending(path: "\(Self.proposal).json"))
    }

    func testAProposalIsPreviewedAndApprovedInPart() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        try queueProposal(sample)
        try await sample.archive.refreshRestructure()
        let restructure = try XCTUnwrap(sample.model.restructure)
        XCTAssertEqual(restructure.proposals.map(\.id), [Self.proposal])
        restructure.selection = Self.proposal
        let proposal = try XCTUnwrap(restructure.selected)
        XCTAssertEqual(proposal.changes.first?.description, "retire “oscillators”")
        XCTAssertEqual(proposal.changes.first?.invasiveness, .rewriting)

        await restructure.refreshPreview()
        let both = try XCTUnwrap(restructure.preview)
        let count = { (tags: [PreviewedTag]) in tags.first { $0.tag == "oscillators" }?.items ?? 0 }
        XCTAssertEqual(count(both.tagsAfter), count(both.tagsBefore) - 1, "the note loses it")
        XCTAssertEqual(both.before, both.after, "tags move no files")
        restructure.choose(proposal.changes[0], in: proposal, false)
        XCTAssertEqual(restructure.chosen(in: proposal), ["2"])
        await restructure.refreshPreview()
        let without = try XCTUnwrap(restructure.preview)
        XCTAssertEqual(count(without.tagsAfter), count(without.tagsBefore), "left out, it stays")
        restructure.choose(proposal.changes[0], in: proposal, true)

        restructure.choose(proposal.changes[1], in: proposal, false)
        await restructure.approve()
        XCTAssertTrue(
            restructure.report?.contains("Left out 1 change.") == true, restructure.report ?? "")
        XCTAssertTrue(restructure.proposals.isEmpty)
        XCTAssertNil(restructure.selection)
    }

    func testARejectedProposalChangesNothing() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        try queueProposal(sample)
        try await sample.archive.refreshRestructure()
        let restructure = try XCTUnwrap(sample.model.restructure)
        restructure.selection = Self.proposal
        await restructure.reject()
        XCTAssertEqual(restructure.report, "Rejected; nothing changed.")
        XCTAssertTrue(restructure.proposals.isEmpty)
    }

    func testDescribesWhatApprovingDid() {
        let approved = RestructureApproved(
            summary: "Restructured the tags", commit: "abc", approved: ["1"], rejected: ["2"],
            dropped: ["3, which needs 2"], skipped: ["“Lamp”: checked out"])
        XCTAssertEqual(
            RestructureModel.describe(approved),
            "Restructured the tags Left out 1 change. Dropped 3, which needs 2. Left alone: “Lamp”: checked out. Undo it from History."
        )
    }

    func testACollectionsEagernessIsItsOwnOrInherited() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let archive = sample.archive
        let before = try await archive.eagerness(at: "Projects")
        XCTAssertEqual(before.level, "balanced")
        XCTAssertNil(before.fromCollection)
        try await archive.setEagerness(of: "Projects", to: "eager")
        let inside = try await archive.eagerness(at: "Projects/Hardware")
        XCTAssertEqual(inside.level, "eager")
        XCTAssertEqual(inside.fromCollection, "Projects", "inherited")
        XCTAssertTrue(
            archive.history.first?.summary.isEmpty == false, "a change History can undo")
        try await archive.setEagerness(of: "Projects", to: nil)
        let after = try await archive.eagerness(at: "Projects")
        XCTAssertNil(after.fromCollection)
    }

    func testTheArchivesEagernessIsInSettings() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let eagerness = try XCTUnwrap(
            sample.archive.settings.first { $0.key == "restructure.eagerness" })
        XCTAssertEqual(eagerness.value, "balanced")
        XCTAssertEqual(eagerness.choices, ["settled", "balanced", "eager"])
    }

    func testASectionIsProposedFromTheApp() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let restructure = try XCTUnwrap(sample.model.restructure)
        await restructure.proposeSection("Projects")
        if let proposal = restructure.selected {
            XCTAssertEqual(proposal.kind, "section")
            XCTAssertEqual(proposal.assessment?.eagerness, "balanced")
        } else {
            let report = try XCTUnwrap(restructure.report, "says why nothing was proposed")
            XCTAssertFalse(report.isEmpty)
        }
    }

    func testASectionProposalShowsTheNewCollectionAfter() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let item = try XCTUnwrap(
            sample.model.items.first { $0.card.hasPrefix("Projects/Hardware/") })
        let queue = sample.root.appending(path: "_System/queue/proposals")
        try FileManager.default.createDirectory(at: queue, withIntermediateDirectories: true)
        let id = "01a18701-0500-776a-9867-08f0000000e2"
        let proposal: [String: Any] = [
            "carrel": "proposal", "format": "1.0", "id": id, "kind": "section",
            "section": "Projects", "created": "2026-11-01T10:00:00-05:00", "by": "carrel test",
            "invasiveness": "subdividing", "rationale": "The lamp build has grown.",
            "changes": [
                [
                    "id": "1",
                    "change": [
                        "kind": "create-collection", "path": "Projects/Lamp",
                        "description": "The lamp build",
                    ],
                    "reason": "Its items belong together", "invasiveness": "subdividing",
                ],
                [
                    "id": "2", "change": ["kind": "move-items", "to": "Projects/Lamp"],
                    "reason": "It's part of the build", "invasiveness": "subdividing",
                    "items": [["id": item.id, "card": item.card, "title": item.title]],
                    "depends_on": ["1"],
                ],
            ],
        ]
        try JSONSerialization.data(withJSONObject: proposal)
            .write(to: queue.appending(path: "\(id).json"))
        try await sample.archive.refreshRestructure()
        let restructure = try XCTUnwrap(sample.model.restructure)
        restructure.selection = id
        await restructure.refreshPreview()
        let preview = try XCTUnwrap(restructure.preview)
        XCTAssertNil(preview.before.first { $0.path == "Projects/Lamp" })
        let lamp = try XCTUnwrap(preview.after.first { $0.path == "Projects/Lamp" })
        XCTAssertTrue(lamp.changed)
        XCTAssertEqual(lamp.items, 1)

        // Leaving out the new collection drops the move that needs it.
        let shown = try XCTUnwrap(restructure.selected)
        restructure.choose(shown.changes[0], in: shown, false)
        XCTAssertEqual(restructure.blocked(in: shown).map(\.id), ["2"])
    }
}
