// The archive window's sidebar (#334): every screen in it can be chosen, by selecting its row
// as a click does, and the window then has that screen's content to show. Each test runs against its
// own temporary copy of the sample archive, once with proposals and questions waiting and once
// with both queues empty.

import AppKit
import CarrelKit
import SwiftUI
import XCTest

@testable import Carrel

@MainActor
final class SidebarTests: XCTestCase {
    func testEveryScreenCanBeChosenWithItemsWaiting() async throws {
        let sample = try await SampleWindow(questions: [
            QuestionsModelTests.term: question(
                QuestionsModelTests.term, kind: "term", subject: "PLA")
        ])
        defer { sample.remove() }
        XCTAssertEqual(sample.archive.reviewCount, 1)
        XCTAssertEqual(sample.archive.questions.count, 1)
        try await checkEveryScreen(sample.model)
    }

    func testEveryScreenCanBeChosenWithEmptyQueues() async throws {
        let sample = try await SampleWindow()
        defer { sample.remove() }
        let review = sample.root.appending(path: "_System/queue/review")
        for entry in try FileManager.default.contentsOfDirectory(atPath: review.path()) {
            try FileManager.default.removeItem(at: review.appending(path: entry))
        }
        try await sample.archive.refreshAll()
        XCTAssertEqual(sample.archive.reviewCount, 0)
        XCTAssertEqual(sample.archive.questions.count, 0)
        try await checkEveryScreen(sample.model)
    }

    /// Selects each of the sidebar's rows in turn and checks the window follows to its screen,
    /// and that the content column shows something for it.
    private func checkEveryScreen(_ model: ArchiveWindowModel) async throws {
        let sidebar = Hosted(ArchiveSidebar(model: model))
        let outline = try XCTUnwrap(sidebar.view.firstDescendant(NSOutlineView.self))
        var chosen: [Screen] = []
        for row in 0..<outline.numberOfRows {
            model.screen = nil
            outline.selectRowIndexes([row], byExtendingSelection: false)
            await settle()
            guard let screen = model.screen else { continue }
            chosen.append(screen)
            XCTAssertEqual(model.screenTitle, title(of: screen, in: model))
            XCTAssert(hasContent(screen, model), "\(screen) has nothing to show")
            // The content column draws it.
            _ = Hosted(ArchiveScreenContent(model: model))
            await settle()
        }
        let expected: [Screen] =
            [.status, .review, .questions, .restructure, .active]
            + model.tree.map { .collection($0.collection.path) }
            + DiscoverPage.allCases.map { .discover($0) } + [.history, .backups]
        XCTAssertEqual(chosen, expected)
    }

    /// Whether the window has what the content column draws for `screen`: the screen's own
    /// model, or for Status and Browse the open archive. Without it the column is empty.
    private func hasContent(_ screen: Screen, _ model: ArchiveWindowModel) -> Bool {
        switch screen {
        case .status, .collection: model.handle != nil
        case .review: model.review != nil
        case .questions: model.questions != nil
        case .restructure: model.restructure != nil
        case .active: model.activeScreen != nil
        case .history: model.historyScreen != nil
        case .backups: model.backups != nil
        case .discover: model.discover != nil
        }
    }

    private func title(of screen: Screen, in model: ArchiveWindowModel) -> String {
        let before = model.screen
        defer { model.screen = before }
        model.screen = screen
        return model.screenTitle
    }

    /// Lets SwiftUI and AppKit catch up with a change.
    private func settle() async {
        for _ in 0..<3 { try? await Task.sleep(for: .milliseconds(50)) }
    }
}

/// A SwiftUI view in a window of its own, laid out, as the app shows it.
@MainActor
private struct Hosted<Content: View> {
    let window: NSWindow
    let view: NSHostingView<Content>

    init(_ content: Content) {
        view = NSHostingView(rootView: content)
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 1200), styleMask: [.titled],
            backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        view.layoutSubtreeIfNeeded()
    }

}

extension NSView {
    /// The first view of type `type` in this one, searching depth first.
    fileprivate func firstDescendant<V: NSView>(_ type: V.Type) -> V? {
        for subview in subviews {
            if let match = subview as? V ?? subview.firstDescendant(type) { return match }
        }
        return nil
    }

}
