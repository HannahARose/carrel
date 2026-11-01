// The extensions' requests to the agent, and the badge state file it keeps, against temporary
// copies of the sample archive with configuration folders of their own.

import CarrelKit
import Foundation
import Testing

/// An agent following a sample archive, answering requests and keeping its badge state in
/// the sample's folder.
@MainActor
struct RequestSample {
    let sample: Sample
    let agent: Agent
    let handler: AgentRequestHandler
    let badges: URL
    let opened: Opened

    /// The links the agent was asked to show.
    final class Opened {
        var links: [AgentLink] = []
    }

    init() async throws {
        sample = try Sample()
        badges = sample.folder.appending(path: "group/\(BadgeState.fileName)")
        let state = try #require(UserDefaults(suiteName: "carrelkit-requests-\(UUID().uuidString)"))
        agent = Agent(
            options: .isolated(config: sample.folder.appending(path: "config")), state: state,
            watching: false, badges: badges)
        let opened = Opened()
        self.opened = opened
        handler = AgentRequestHandler(agent: agent, sent: sample.folder.appending(path: "Sent")) {
            opened.links.append($0)
        }
        await agent.follow([sample.root])
    }

    var handle: ArchiveHandle { sample.model.handle }
    var root: String { realPath(sample.root.path(percentEncoded: false)) }

    /// The badge state file as the agent last wrote it.
    var written: BadgeState.Archive? { BadgeState.read(from: badges)?.archives.first }

    func folder(_ name: String) throws -> URL {
        let folder = sample.folder.appending(path: name)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// Holds the writer lease as another device would.
    func holdLeaseElsewhere() throws {
        let now = Date()
        let format = ISO8601DateFormatter()
        let lock: [String: String] = [
            "token": UUID().uuidString.lowercased(), "person": UUID().uuidString.lowercased(),
            "device": UUID().uuidString.lowercased(), "purpose": "ingest",
            "program": "carrel 0.6.0", "acquired": format.string(from: now),
            "expires": format.string(from: now.addingTimeInterval(3600)),
        ]
        try JSONSerialization.data(withJSONObject: lock).write(
            to: sample.root.appending(path: "_System/writer.lock"))
    }
}

@MainActor
@Suite struct AgentRequests {
    @Test func addingFilesQueuesThemAsIngestDoes() async throws {
        let test = try await RequestSample()
        let note = test.sample.folder.appending(path: "a note.txt")
        try "A note about the lamp.\n".write(to: note, atomically: true, encoding: .utf8)
        let reply = await test.handler.handle(
            .addFiles(
                archive: nil, paths: [note.path(percentEncoded: false)], each: false, whole: false))
        #expect(reply.outcome == .done, "\(reply.message)")
        #expect(reply.archive == test.root)
        #expect(reply.items.map(\.title) == ["a note.txt"])
        #expect(try test.handle.intake().waiting.count == 1)
        #expect(test.agent.archives.first?.queued == 1)

        let again = await test.handler.handle(
            .addFiles(
                archive: test.root, paths: [note.path(percentEncoded: false)], each: false,
                whole: false))
        #expect(again.outcome == .failed, "already queued")
        #expect(again.message.contains("a note.txt"))
    }

    @Test func sharedTextAndDataAreKeptAndQueuedAsFiles() async throws {
        let test = try await RequestSample()
        let text = await test.handler.handle(
            .addText(archive: nil, text: "Ideas for the lamp: a frosted diffuser.", title: nil))
        #expect(text.outcome == .done, "\(text.message)")
        let note = try #require(text.items.first?.path)
        #expect(note.hasPrefix(realPath(test.sample.folder.path(percentEncoded: false)) + "/Sent/"))
        #expect(
            URL(filePath: note).lastPathComponent == "Ideas for the lamp- a frosted diffuser.md")
        #expect(try String(contentsOfFile: note, encoding: .utf8).contains("frosted"))

        let image = await test.handler.handle(
            .addData(archive: nil, name: "sketch.png", data: Data([0x89, 0x50, 0x4e, 0x47])))
        #expect(image.outcome == .done, "\(image.message)")
        #expect(image.items.first?.title == "sketch.png")
        #expect(try test.handle.intake().waiting.count == 2)

        let titled = await test.handler.handle(
            .addText(archive: nil, text: "x", title: "Lamp/ideas"))
        #expect(titled.items.first?.title == "Lamp-ideas.md")
    }

    @Test func checkingOutAndReturningFollowTheBadgeState() async throws {
        let test = try await RequestSample()
        let item = try test.sample.item(titled: "Desk lamp parts list")
        let desktop = try test.folder("Desktop")
        let out = await test.handler.handle(
            .checkOut(
                archive: nil, item: item, folder: desktop.path(percentEncoded: false),
                due: "2026-12-01"))
        #expect(out.outcome == .done, "\(out.message)")
        let working = try #require(out.working)
        #expect(FileManager.default.fileExists(atPath: working))
        #expect(try test.handle.active().contains { $0.id == item && $0.due == "2026-12-01" })

        let checkout = try #require(test.written?.checkouts.first { $0.item == item })
        #expect(checkout.working == realPath(working))
        let archived = try #require(checkout.path)
        #expect(archived.hasPrefix(test.root + "/"))
        #expect(FileManager.default.fileExists(atPath: archived))
        let state = try #require(BadgeState.read(from: test.badges))
        #expect(state.badge(for: realPath(working))?.badge == .workingCopy)
        #expect(state.badge(for: archived)?.badge == .checkedOut)
        #expect(state.folders.contains(realPath(working)))

        try "part,source\nLED strip,shop\n".write(
            toFile: working, atomically: true, encoding: .utf8)
        let returned = await test.handler.handle(.returnItem(archive: nil, item: item, from: nil))
        #expect(returned.outcome == .done, "\(returned.message)")
        #expect(test.written?.checkouts.contains { $0.item == item } == false)
        #expect(try test.handle.history(limit: 1).first?.operation == "return")
    }

    @Test func anItemCanBeNamedByItsPathAsFinderKnowsIt() async throws {
        let test = try await RequestSample()
        let item = try test.sample.item(titled: "Desk lamp parts list")
        let summary = try #require(try test.handle.items().first { $0.id == item })
        let contents = test.root + "/" + (try #require(summary.item))
        let desktop = try test.folder("Desktop")
        let out = await test.handler.handle(
            .checkOut(
                archive: test.root, item: contents, folder: desktop.path(percentEncoded: false),
                due: nil))
        #expect(out.outcome == .done, "\(out.message)")
        let working = try #require(out.working)

        let shown = await test.handler.handle(.open(archive: test.root, item: working))
        #expect(shown.outcome == .done)
        #expect(test.opened.links.last?.item == item, "a working copy names its item")

        let cancelled = await test.handler.handle(
            .cancelCheckout(archive: test.root, item: test.root + "/" + summary.card))
        #expect(cancelled.outcome == .done, "\(cancelled.message)")

        let stray = await test.handler.handle(
            .checkOut(
                archive: test.root, item: test.root + "/Nowhere/file.txt", folder: nil, due: nil))
        #expect(stray.outcome == .failed)
        #expect(stray.message.contains("isn’t an item"))
    }

    @Test func cancellingACheckoutLeavesTheArchiveCopyAsItWas() async throws {
        let test = try await RequestSample()
        let item = try test.sample.item(titled: "Desk lamp parts list")
        let desktop = try test.folder("Desktop")
        _ = await test.handler.handle(
            .checkOut(
                archive: nil, item: item, folder: desktop.path(percentEncoded: false), due: nil))
        #expect(test.written?.checkouts.contains { $0.item == item } == true)
        let cancelled = await test.handler.handle(.cancelCheckout(archive: nil, item: item))
        #expect(cancelled.outcome == .done, "\(cancelled.message)")
        #expect(try !test.handle.active().contains { $0.id == item })
        #expect(test.written?.checkouts.contains { $0.item == item } == false)

        let again = await test.handler.handle(.cancelCheckout(archive: nil, item: item))
        #expect(again.outcome == .failed, "it isn't checked out any more")
        #expect(!again.message.isEmpty)
    }

    @Test func theDeskAndSealedBundlesAreInTheBadgeState() async throws {
        let test = try await RequestSample()
        // The sample's sealed bundle is checked out, which its badge shows first.
        let bundle = test.root + "/Projects/oscillator-sim"
        #expect(test.written?.sealed.contains(bundle) == true)
        #expect(BadgeState.read(from: test.badges)?.badge(for: bundle)?.badge == .checkedOut)

        let desk = try test.folder("Desk/Lamp notes")
        _ = try test.handle.addToDesk(
            path: desk.path(percentEncoded: false), title: "Lamp notes", summary: nil)
        await test.agent.refresh()
        let tracked = try #require(test.written?.desk.first)
        #expect(tracked.title == "Lamp notes")
        #expect(tracked.path == realPath(desk.path(percentEncoded: false)))
        let state = try #require(BadgeState.read(from: test.badges))
        #expect(state.badge(for: tracked.path)?.badge == .tracked)
        #expect(state.archive(containing: tracked.path + "/sketch.txt")?.root == test.root)

        _ = try test.handle.removeFromDesk(path: tracked.path, deleteCard: true)
        await test.agent.refresh()
        #expect(test.written?.desk.isEmpty == true)
    }

    @Test func searchingFindsItemsAsSearchDoes() async throws {
        let test = try await RequestSample()
        let reply = await test.handler.handle(.search(archive: nil, query: "lamp", limit: 10))
        #expect(reply.outcome == .done, "\(reply.message)")
        #expect(reply.items.contains { $0.title == "Desk lamp parts list" })
        let none = await test.handler.handle(
            .search(archive: nil, query: "zeppelin-quokka", limit: 10))
        #expect(none.items.isEmpty)
    }

    @Test func itemsCollectionsAndWhatsActiveAreListedForShortcuts() async throws {
        let test = try await RequestSample()
        let item = try test.sample.item(titled: "Desk lamp parts list")
        let found = await test.handler.handle(.items(archive: nil, ids: [item, "no-such-item"]))
        #expect(found.items.map(\.id) == [item])
        #expect(found.items.first?.path?.hasPrefix(test.root + "/") == true)

        let collections = await test.handler.handle(.collections(archive: nil))
        #expect(collections.outcome == .done)
        #expect(collections.items.map(\.id) == (try test.handle.collections()).map(\.path))

        let active = await test.handler.handle(.active(archive: nil))
        #expect(active.outcome == .done)
        #expect(active.items.map(\.id) == (try test.handle.active()).map(\.id))
        #expect(active.message.contains("checked out"))
    }

    @Test func openingShowsTheItemInTheApp() async throws {
        let test = try await RequestSample()
        let item = try test.sample.item(titled: "Desk lamp parts list")
        let reply = await test.handler.handle(.open(archive: test.root, item: item))
        #expect(reply.outcome == .done)
        #expect(test.opened.links == [AgentLink(archive: URL(filePath: test.root), item: item)])

        _ = await test.handler.handle(.open(archive: test.root, item: item, action: .returnItem))
        #expect(test.opened.links.last?.action == .returnItem, "Finder's Return… opens the sheet")
        _ = await test.handler.handle(.open(archive: test.root, item: nil, action: .ask))
        #expect(test.opened.links.last?.action == nil, "no item, nothing to start")
    }

    @Test func whatTheCoreCantDoSaysWhy() async throws {
        let test = try await RequestSample()
        let snapshot = await test.handler.handle(.snapshot(archive: nil))
        #expect(snapshot.outcome == .failed, "no backups are set up")
        #expect(!snapshot.message.isEmpty)
        let checkOut = await test.handler.handle(
            .checkOut(archive: nil, item: "no-such-item", folder: nil, due: nil))
        #expect(checkOut.outcome == .failed)
        #expect(!checkOut.message.isEmpty)
    }
}

@MainActor
@Suite struct AgentRefusals {
    @Test func aPausedAgentRefusesAndChangesNothing() async throws {
        let test = try await RequestSample()
        test.agent.pause()
        #expect(BadgeState.read(from: test.badges)?.paused == true)
        let item = try test.sample.item(titled: "Desk lamp parts list")
        for request in [
            AgentRequest.checkOut(archive: nil, item: item, folder: nil, due: nil),
            .search(archive: nil, query: "lamp", limit: 5),
        ] {
            let reply = await test.handler.handle(request)
            #expect(reply.outcome == .refused)
            #expect(reply.message.contains("paused"))
        }
        #expect(try !test.handle.active().contains { $0.id == item })
    }

    @Test func anArchiveTheAgentDoesntFollowIsRefused() async throws {
        let test = try await RequestSample()
        let reply = await test.handler.handle(
            .search(
                archive: test.sample.folder.appending(path: "Elsewhere").path, query: "a", limit: 1)
        )
        #expect(reply.outcome == .refused)
        #expect(reply.message.contains("isn’t open in Carrel"))

        await test.agent.follow([])
        let none = await test.handler.handle(.search(archive: nil, query: "a", limit: 1))
        #expect(none.outcome == .refused)
        #expect(none.message.contains("isn’t following"))
    }

    @Test func anotherAgentsArchiveIsReadButNotWritten() async throws {
        let test = try await RequestSample()
        let state = try #require(UserDefaults(suiteName: "carrelkit-other-\(UUID().uuidString)"))
        let other = Agent(
            options: .isolated(config: test.sample.folder.appending(path: "config")),
            state: state, watching: false)
        await other.follow([test.sample.root])
        let handler = AgentRequestHandler(
            agent: other, sent: test.sample.folder.appending(path: "Sent")
        ) { _ in }
        let item = try test.sample.item(titled: "Desk lamp parts list")
        let write = await handler.handle(
            .checkOut(archive: nil, item: item, folder: nil, due: nil))
        #expect(write.outcome == .refused)
        #expect(write.message.contains("Another Carrel agent"))
        let read = await handler.handle(.search(archive: nil, query: "lamp", limit: 5))
        #expect(read.outcome == .done)
    }

    @Test func anotherWritersLeaseIsRefusedWithItsReason() async throws {
        let test = try await RequestSample()
        try test.holdLeaseElsewhere()
        let item = try test.sample.item(titled: "Desk lamp parts list")
        let desktop = try test.folder("Desktop")
        let reply = await test.handler.handle(
            .checkOut(
                archive: nil, item: item, folder: desktop.path(percentEncoded: false), due: nil))
        #expect(reply.outcome == .refused, "\(reply.message)")
        #expect(!reply.message.isEmpty)
        #expect(try !test.handle.active().contains { $0.id == item })
    }
}

#if os(macOS)
@MainActor
@Suite struct AgentOverXPC {
    @Test func requestsAndRepliesCrossTheConnection() async throws {
        let test = try await RequestSample()
        let listener = AgentXPCListener(handler: test.handler, listener: .anonymous())
        listener.resume()
        defer { listener.invalidate() }
        let client = AgentClient(endpoint: listener.endpoint)
        let reply = await client.send(.search(archive: nil, query: "lamp", limit: 10))
        #expect(reply.outcome == .done, "\(reply.message)")
        #expect(reply.items.contains { $0.title == "Desk lamp parts list" })

        test.agent.pause()
        let refused = await client.send(.search(archive: nil, query: "lamp", limit: 10))
        #expect(refused.outcome == .refused)
    }

    @Test func anAgentThatIsntThereIsRefusedAsNotRunning() async throws {
        let client = AgentClient(machServiceName: AgentService.appGroup + ".NoSuchAgent")
        let reply = await client.send(.snapshot(archive: nil))
        #expect(reply == .notRunning)
    }
}
#endif

@Suite struct BadgeStates {
    let state = BadgeState(archives: [
        BadgeState.Archive(
            root: "/A", name: "A", sealed: ["/A/Code/repo"],
            checkouts: [
                .init(item: "i1", title: "Lamp", path: "/A/Projects/lamp", working: "/W/lamp")
            ],
            desk: [.init(item: "d1", title: "Notes", path: "/Desk/Notes")]),
        BadgeState.Archive(root: "/B", name: "B"),
    ])

    @Test func pathsAreBadgedByWhatTheyAre() {
        #expect(state.badge(for: "/A/Code/repo")?.badge == .sealed)
        #expect(state.badge(for: "/A/Projects/lamp")?.badge == .checkedOut)
        #expect(state.badge(for: "/W/lamp")?.item == "i1")
        #expect(state.badge(for: "/Desk/Notes")?.badge == .tracked)
        #expect(state.badge(for: "/A/Projects") == nil)
        #expect(state.folders == ["/A", "/W/lamp", "/Desk/Notes", "/B"])
        #expect(state.archive(containing: "/W/lamp/a.txt")?.root == "/A")
        #expect(state.archive(containing: "/Bee/x") == nil)
        #expect(state.archive(containing: "/B")?.name == "B")
    }

    @Test func itRoundTripsThroughItsFile() throws {
        let file = FileManager.default.temporaryDirectory
            .appending(path: "carrelkit-badges-\(UUID().uuidString)/\(BadgeState.fileName)")
        try state.write(to: file)
        #expect(BadgeState.read(from: file) == state)
        #expect(BadgeState.read(from: file.appending(path: "missing")) == nil)
    }
}
