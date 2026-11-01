// Discover's Connections page (#327): the tag and link graph read from the core and laid out
// for drawing, against a temporary copy of the sample archive.

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
}
