// Spotlight: what the agent indexes for an archive, by `spotlight.index`, kept in an index of
// the test's own rather than the Mac's.

import CarrelKit
import CoreSpotlight
import Foundation
import Testing

/// A Spotlight index that remembers what it holds.
actor FakeSpotlightIndex: SpotlightIndexing {
    private(set) var entries: [String: SpotlightIndexed] = [:]
    private(set) var indexedCount = 0

    func index(_ entries: [SpotlightIndexed]) async throws {
        indexedCount += entries.count
        for entry in entries { self.entries[entry.identifier] = entry }
    }

    func delete(identifiers: [String]) async throws {
        for identifier in identifiers { entries[identifier] = nil }
    }

    func deleteDomain(_ domain: String) async throws {
        entries = entries.filter { $0.value.domain != domain }
    }

    func held() -> [SpotlightEntry] { entries.values.map(\.entry) }
}

@MainActor
struct SpotlightSample {
    let sample: Sample
    let index = FakeSpotlightIndex()
    let agent: Agent

    init() throws {
        sample = try Sample()
        agent = Agent(
            options: .isolated(config: sample.folder.appending(path: "config")),
            state: try #require(UserDefaults(suiteName: "carrelkit-spot-\(UUID().uuidString)")),
            watching: false, spotlight: index)
    }

    var handle: ArchiveHandle { sample.model.handle }
    var root: URL { URL(filePath: realPath(sample.root.path(percentEncoded: false))) }

    /// Makes the card at `path` (relative to the archive) `sensitivity`.
    func mark(_ path: String, _ sensitivity: String) throws {
        let card = sample.root.appending(path: path)
        let text = try String(contentsOf: card, encoding: .utf8)
        let marked = text.replacing(
            /sensitivity: [a-z]+/, maxReplacements: 1, with: { _ in "sensitivity: \(sensitivity)" })
        try marked.write(to: card, atomically: true, encoding: .utf8)
    }
}

@MainActor
@Suite struct SpotlightResults {
    @Test func restrictedCardsNeverReachTheIndexUnderAnySetting() async throws {
        let test = try SpotlightSample()
        try test.mark("Reading/Coupled oscillators note.pdf.md", "restricted")
        let restricted = try #require(
            try test.handle.items().first { $0.title == "A note on coupled oscillators" }
        ).id
        await test.agent.follow([test.sample.root])
        for choice in ["standard", "all-but-restricted", "off", "standard"] {
            _ = try test.handle.setSetting(key: "spotlight.index", value: choice)
            test.agent.noticed(
                [test.root.appending(path: "_System/settings.json").path(percentEncoded: false)],
                in: test.root)
            await test.agent.spotlightIndexed()
            let held = await test.index.held()
            #expect(!held.contains { $0.item == restricted }, "\(choice)")
            #expect(held.isEmpty == (choice == "off"), "\(choice)")
        }
    }

    @Test func sensitiveItemsAreFoundByTitleOnly() async throws {
        let test = try SpotlightSample()
        try test.mark("Projects/oscillator-sim.md", "sensitive")
        await test.agent.follow([test.sample.root])
        await test.agent.spotlightIndexed()
        let held = await test.index.held()
        let sim = try #require(held.first { $0.title == "oscillator-sim" && $0.kind == .item })
        #expect(sim.titleOnly && sim.description == nil && sim.keywords.isEmpty)
        #expect(held.contains { $0.kind == .collection && $0.collectionPath == "Projects" })
        #expect(held.contains { $0.kind == .fact })
    }

    @Test func onlyWhatChangedIsIndexedAgainAndAClosedArchiveIsRemoved() async throws {
        let test = try SpotlightSample()
        await test.agent.follow([test.sample.root])
        await test.agent.spotlightIndexed()
        let first = await test.index.indexedCount
        #expect(first > 0)

        // Public instead of personal shows the same, so only an edit to the summary counts.
        try test.mark("Reading/RFC 9562 UUIDs.md", "personal")
        let card = test.sample.root.appending(path: "Projects/oscillator-sim.md")
        let text = try String(contentsOf: card, encoding: .utf8)
        try text.replacingOccurrences(of: "A tiny Rust program", with: "A small Rust program")
            .write(to: card, atomically: true, encoding: .utf8)
        test.agent.noticed(
            [test.root.appending(path: "Projects/oscillator-sim.md").path(percentEncoded: false)],
            in: test.root)
        await test.agent.spotlightIndexed()
        #expect(await test.index.indexedCount == first + 1, "only the card that changed")

        await test.agent.follow([])
        await test.agent.spotlightIndexed()
        #expect(await test.index.held().isEmpty)
    }

    @Test func aResultOpensTheItemInCarrel() async throws {
        let test = try SpotlightSample()
        await test.agent.follow([test.sample.root])
        await test.agent.spotlightIndexed()
        let identifiers = await test.index.entries.keys
        let sim = try #require(
            try test.handle.items().first { $0.title == "oscillator-sim" }
        ).id
        let link = try #require(
            identifiers.compactMap { URL(string: $0).flatMap(AgentLink.init) }
                .first { $0.item == sim })
        #expect(link.archive.path(percentEncoded: false) == test.root.path(percentEncoded: false))
        #expect(Set(identifiers).count == identifiers.count)

        let activity = NSUserActivity(activityType: CSSearchableItemActionType)
        activity.userInfo = [CSSearchableItemActivityIdentifier: link.url.absoluteString]
        #expect(AgentLink(spotlightResult: activity) == link)
        #expect(AgentLink(spotlightResult: NSUserActivity(activityType: "other")) == nil)
    }
}
