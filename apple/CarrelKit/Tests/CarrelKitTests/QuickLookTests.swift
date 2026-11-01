// Quick Look: cards as the extension shows them, and what the agent keeps for it, in a store
// folder of the test's own rather than the real one in Application Support.

import CarrelKit
import Foundation
import Testing

@MainActor
@Suite struct QuickLookCards {
    @Test func aCardIsLaidOutAsTheInspectorShowsIt() throws {
        let sample = try Sample()
        let card = sample.root.appending(path: "Projects/oscillator-sim.md")
        let text = try String(contentsOf: card, encoding: .utf8)
        let preview = try quickLookCard(
            text: text, path: card.path(percentEncoded: false),
            roots: [sample.root.path(percentEncoded: false)])
        let html = QuickLookCardPage.html(preview)
        #expect(html.contains("<h1>oscillator-sim</h1>"))
        #expect(html.contains("<p class=\"collection\">Projects</p>"))
        #expect(html.contains("<span class=\"tag\">simulation</span>"))
        #expect(html.contains("<h2>History</h2>"))
        #expect(html.contains("class=\"entry pending\""))
        #expect(html.contains("Sealed because it is a Rust project"))
    }

    @Test func aRestrictedCardShowsItsTitleOnly() throws {
        let sample = try Sample()
        let card = sample.root.appending(path: "Projects/oscillator-sim.md")
        let text = try String(contentsOf: card, encoding: .utf8)
            .replacingOccurrences(of: "sensitivity: personal", with: "sensitivity: restricted")
        let preview = try quickLookCard(
            text: text, path: card.path(percentEncoded: false), roots: [])
        let html = QuickLookCardPage.html(preview)
        #expect(html.contains("<h1>oscillator-sim</h1>"))
        #expect(html.contains("Restricted: open in Carrel to see it"))
        for hidden in ["simulation", "History", "Sealed because", "Fields"] {
            #expect(!html.contains(hidden), "\(hidden)")
        }
    }

    @Test func markdownThatIsntACardIsShownAsText() {
        #expect(throws: CarrelError.self) {
            try quickLookCard(text: "# Notes\n", path: "/tmp/notes.md", roots: [])
        }
        let html = QuickLookCardPage.plain("<b>& notes</b>", title: "notes.md")
        #expect(html.contains("&lt;b&gt;&amp; notes&lt;/b&gt;"))
    }
}

@MainActor
@Suite struct QuickLookKeeping {
    @Test func theAgentKeepsTheArchivesFollowedAndPrunesStaleDrawings() async throws {
        let sample = try Sample()
        let store = QuickLookStore(
            folder: sample.folder.appending(path: "QuickLook"))
        try FileManager.default.createDirectory(
            at: store.drawings, withIntermediateDirectories: true)
        let stale = store.drawings.appending(path: "stale.png")
        try Data("png".utf8).write(to: stale)

        let agent = Agent(
            options: .isolated(config: sample.folder.appending(path: "config")),
            state: try #require(UserDefaults(suiteName: "carrelkit-ql-\(UUID().uuidString)")),
            watching: false, quickLook: store)
        await agent.follow([sample.root])
        await agent.quickLookKept()
        #expect(store.roots() == [realPath(sample.root.path(percentEncoded: false))])
        #expect(!FileManager.default.fileExists(atPath: stale.path(percentEncoded: false)))

        await agent.follow([])
        #expect(store.roots().isEmpty)
    }

    @Test func drawnFilesAreKnownByTheirExtension() {
        #expect(QuickLookStore.isDrawn("/Parts/Bracket.STEP"))
        #expect(QuickLookStore.isDrawn("/Parts/plate.3mf"))
        #expect(!QuickLookStore.isDrawn("/Parts/notes.md"))
        #expect(quickLookDrawing(store: "/store", path: "/missing.step") == nil)
    }
}
