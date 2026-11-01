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

/// Debug builds and test runs keep clear of the installed app (#443).
@MainActor
final class DebugIdentityTests: XCTestCase {
    func testTheTestHostHasItsOwnIdentityGroupAndAgent() {
        XCTAssertEqual(Bundle.main.bundleIdentifier, "io.github.hannaharose.Carrel.dev")
        XCTAssertEqual(AgentService.appGroup, "io.github.hannaharose.Carrel.dev")
        XCTAssertEqual(AgentLoginItem.identifier, "io.github.hannaharose.Carrel.dev.Agent")
    }

    func testTheTestHostIsBareAndKeepsItsOwnDefaults() {
        XCTAssertTrue(CarrelMain.isHostingTests)
        XCTAssertTrue(NSApp.windows.filter(\.isVisible).isEmpty, "no windows on the screen")
        XCTAssertFalse(UserDefaults.app === UserDefaults.standard)
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
        XCTAssertNil(app.takePendingLink(for: URL(filePath: "/elsewhere")))
        XCTAssertEqual(app.takePendingLink(for: shown[0])?.item, "0193")
        XCTAssertNil(app.takePendingLink(for: shown[0]), "chosen once")

        let finder = AgentLink(archive: sampleArchive, item: "0193", action: .returnItem)
        app.open(link: finder.url) { shown.append($0) }
        XCTAssertEqual(app.takePendingLink(for: shown[1])?.action, .returnItem)

        app.open(link: URL(string: "https://example.com")!) { shown.append($0) }
        XCTAssertEqual(shown.count, 2, "not a link from the agent")
    }

    func testALinkFromFinderOpensTheSheetItsActionNeeds() async throws {
        let sample = try await SampleWindow()
        let model = sample.model
        let checkedOut = try XCTUnwrap(sample.archive.active.first { $0.kind == .checkedOut })
        await model.start(.cancelCheckout, on: checkedOut.id)
        XCTAssertEqual(model.screen, .active)
        XCTAssertEqual(model.activeScreen?.cancelling?.id, checkedOut.id)

        let item = try XCTUnwrap(model.items.first { $0.id != checkedOut.id })
        await model.start(.checkOut, on: item.id)
        XCTAssertEqual(model.activeScreen?.checkingOut?.item, item.id)
    }

    func testALinkFromTheCLILeavesItsScreenForTheWindow() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "carrel-tests-\(UUID().uuidString)"))
        let app = AppModel(recents: RecentArchives(defaults: defaults))
        let link = AgentLink(archive: sampleArchive, screen: .discover(.wander))
        var shown: [URL] = []
        app.open(link: link.url) { shown.append($0) }
        let pending = try XCTUnwrap(app.takePendingLink(for: shown[0]))
        XCTAssertEqual(pending.screen, .discover(.wander))
        XCTAssertNil(pending.item)

        app.open(link: AgentLink(archive: sampleArchive).url) { shown.append($0) }
        XCTAssertNil(app.takePendingLink(for: shown[1]), "nothing for the window to do")
    }

    func testTheWindowShowsTheScreenALinkNames() async throws {
        let model = try await SampleWindow().model
        let screens: [(AgentLink.Screen, Screen?)] = [
            (.review, .review), (.questions, .questions), (.active, .active),
            (.history, .history), (.backups, .backups), (.discover(.map), .discover(.map)),
            (.discover(nil), .discover(.today)), (.ingest, .status), (.status, .status),
        ]
        for (linked, screen) in screens {
            model.show(linked: linked)
            XCTAssertEqual(model.screen, screen, "\(linked)")
        }
        model.show(linked: .browse)
        guard case .collection = model.screen else {
            return XCTFail("Browse shows a collection, not \(String(describing: model.screen))")
        }
        model.show(linked: .librarian)
        XCTAssertEqual(model.librarian?.isShown, true)
    }
}
