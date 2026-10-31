// Settings' agent switch, with a login service of the tests' own so nothing is registered on
// this Mac, and links from the agent's menu.

import CarrelKit
import ServiceManagement
import XCTest

@testable import Carrel

/// A login service that remembers what it was asked to do.
final class FakeLoginService: LoginService {
    var status: SMAppService.Status = .notRegistered
    var approvalNeeded = false
    var failure: (any Error)?

    func register() throws {
        if let failure { throw failure }
        status = approvalNeeded ? .requiresApproval : .enabled
    }

    func unregister() throws {
        if let failure { throw failure }
        status = .notRegistered
    }
}

@MainActor
final class AgentLoginItemTests: XCTestCase {
    func testTheSwitchRegistersAndUnregistersTheAgent() {
        let service = FakeLoginService()
        let item = AgentLoginItem(service: service)
        XCTAssertFalse(item.isOn)
        item.set(true)
        XCTAssertTrue(item.isOn)
        XCTAssertEqual(service.status, .enabled)
        item.set(false)
        XCTAssertFalse(item.isOn)
        XCTAssertNil(item.problem)
    }

    func testAnAgentWaitingForApprovalCountsAsOnAndSaysSo() {
        let service = FakeLoginService()
        service.approvalNeeded = true
        let item = AgentLoginItem(service: service)
        item.set(true)
        XCTAssertTrue(item.isOn)
        XCTAssertTrue(item.needsApproval)
        service.status = .enabled
        item.refresh()
        XCTAssertFalse(item.needsApproval)
    }

    func testAFailedChangeSaysWhy() {
        let service = FakeLoginService()
        service.failure = CocoaError(.featureUnsupported)
        let item = AgentLoginItem(service: service)
        item.set(true)
        XCTAssertFalse(item.isOn)
        XCTAssertNotNil(item.problem)
    }
}

@MainActor
final class AgentLinkTests: XCTestCase {
    func testALinkOpensItsArchiveAndLeavesTheItemForItsWindow() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "carrel-tests-\(UUID().uuidString)"))
        let app = AppModel(recents: RecentArchives(defaults: defaults))
        let link = AgentLink(archive: sampleArchive, item: "0193")
        var shown: [URL] = []
        app.open(link: link.url) { shown.append($0) }
        XCTAssertEqual(shown.map(\.standardizedFileURL.path), [sampleArchive.path])
        XCTAssertNil(app.takePendingItem(for: URL(filePath: "/elsewhere")))
        XCTAssertEqual(app.takePendingItem(for: shown[0]), "0193")
        XCTAssertNil(app.takePendingItem(for: shown[0]), "chosen once")

        app.open(link: URL(string: "https://example.com")!) { shown.append($0) }
        XCTAssertEqual(shown.count, 1, "not a link from the agent")
    }
}
