// The agent's scheduling, as plain dates, then the agent itself against temporary copies of
// the sample archive, with configuration folders of their own.

import CarrelKit
import Foundation
import Testing

@Suite struct AgentPlanning {
    let root = "/Archive"
    let start = Date(timeIntervalSince1970: 1_800_000_000)

    @Test func changedPathsAreSortedByWhatTheyMean() {
        let plan = AgentPlan(
            root: "/Archive/", workingCopies: ["/Work/Lamp", "/Archive/Desk/Notes"])
        #expect(plan.classify("/Archive/_System/queue/intake/0193.json") == .queue)
        #expect(plan.classify("/Archive/Projects/Hardware/lamp.md") == .filed)
        #expect(plan.classify("/Archive/_System/devices/d1/desk.json") == .active)
        #expect(plan.classify("/Archive/_System/manifests/0193.checkout.tsv") == .active)
        #expect(plan.classify("/Work/Lamp/notes.txt") == .working)
        #expect(plan.classify("/Archive/Desk/Notes/today.md") == .working)
        for ignored in [
            "/Archive/_System/index/model/index.json", "/Archive/_System/history.git/HEAD",
            "/Archive/_System/cache.nosync/text/a", "/Archive/_System/writer.lock",
            "/Archive/_System/queue/review/b.json", "/Archive/Projects/card.md.tmp",
            "/Archive", "/Elsewhere/file", "/Archived/file", "/Work/Lampshade/x",
        ] {
            #expect(plan.classify(ignored) == .ignored, "\(ignored)")
        }
    }

    @Test func startingCatchesUpAtOnce() {
        var plan = AgentPlan(root: root)
        #expect(plan.due(at: start).isEmpty)
        #expect(plan.nextDue == nil)
        plan.start(at: start)
        #expect(plan.due(at: start) == [.work, .sessions, .remind])
    }

    @Test func remindersAreLookedForOnStartingAfterSessionsAndEveryQuarterHour() {
        var plan = AgentPlan(root: root)
        plan.start(at: start)
        plan.ran(.work, startedAt: start)
        plan.ran(.remind, startedAt: start)
        #expect(plan.remindDue == start + AgentPlan.remindEvery)
        plan.ran(.sessions, startedAt: start + 10)
        #expect(plan.due(at: start + 10) == [.remind], "a recorded session may need one")
        plan.ran(.remind, startedAt: start + 10)
        #expect(plan.remindDue == start + 10 + AgentPlan.remindEvery)
    }

    @Test func aBurstOfChangesRunsTheWorkerOnceItSettles() {
        var plan = AgentPlan(root: root)
        plan.noticed(["/Archive/_System/queue/intake/a.json"], at: start)
        plan.noticed(["/Archive/Projects/b.md"], at: start + 1)
        #expect(plan.due(at: start + 2).isEmpty, "still settling")
        #expect(plan.nextDue == start + 1 + AgentPlan.settle)
        #expect(plan.due(at: start + 3) == [.work])
        plan.ran(.work, startedAt: start + 3)
        #expect(plan.due(at: start + 10).isEmpty)
    }

    @Test func aChangeWhileTheWorkerRunsStaysDue() {
        var plan = AgentPlan(root: root)
        plan.start(at: start)
        plan.noticed(["/Archive/_System/queue/intake/late.json"], at: start + 5)
        plan.ran(.work, startedAt: start)
        #expect(plan.due(at: start + 10).contains(.work))
    }

    @Test func sessionsAreRecordedOnceWorkHasPausedLongEnough() {
        var plan = AgentPlan(root: root, workingCopies: ["/Work/Lamp"], idleMinutes: 30)
        plan.noticed(["/Work/Lamp/a.txt"], at: start)
        plan.noticed(["/Work/Lamp/a.txt"], at: start + 600)
        let idle: TimeInterval = 30 * 60
        #expect(!plan.due(at: start + idle + AgentPlan.margin).contains(.sessions), "work went on")
        #expect(plan.due(at: start + 600 + idle + AgentPlan.margin) == [.sessions])
        #expect(!plan.due(at: start).contains(.work), "work on a copy doesn't run the worker")
    }

    @Test func checkingOutLooksForSessionsAndReadsTheWorkingCopiesAgain() {
        var plan = AgentPlan(root: root)
        let desk = plan.noticed(["/Archive/_System/devices/d1/desk.json"], at: start)
        #expect(desk)
        let filed = plan.noticed(["/Archive/Projects/b.md"], at: start)
        #expect(!filed)
        #expect(plan.due(at: start + AgentPlan.settle) == [.work, .sessions])
    }

    @Test func busyJobsArePutOff() {
        var plan = AgentPlan(root: root)
        plan.start(at: start)
        plan.postpone(.sessions, until: start + 30)
        plan.postpone(.remind, until: start + 30)
        #expect(plan.due(at: start + 1) == [.work])
        #expect(plan.due(at: start + 30) == [.work, .sessions, .remind])
    }

    @Test func snapshotsAreDailyOnceSetUpAndRetriedHourlyAfterAFailure() {
        var plan = AgentPlan(root: root)
        #expect(plan.backupDue == nil, "no backups set up")
        plan.backupsSetUp = true
        #expect(plan.due(at: start) == [.backup], "never taken, so due now")
        plan.ran(.backup, startedAt: start, succeeded: false)
        #expect(plan.backupDue == start + AgentPlan.backupRetry)
        plan.ran(.backup, startedAt: start + AgentPlan.backupRetry)
        #expect(plan.backupDue == start + AgentPlan.backupRetry + AgentPlan.backupEvery)
        #expect(plan.due(at: start + AgentPlan.backupEvery).isEmpty)
    }

    @Test func theCrawlerOnlyGetsTimeNothingElseNeedsAndWaitsAfterChanges() {
        var plan = AgentPlan(root: root)
        plan.start(at: start)
        let every = AgentPlan.crawlEvery
        #expect(plan.due(at: start) == [.work, .sessions, .remind], "not straight away")
        plan.ran(.work, startedAt: start)
        plan.ran(.sessions, startedAt: start)
        plan.ran(.remind, startedAt: start)
        #expect(plan.due(at: start + every) == [.crawl])
        plan.noticed(["/Archive/_System/queue/intake/a.json"], at: start + every)
        #expect(
            plan.due(at: start + every + AgentPlan.settle) == [.work],
            "the worker first, and the crawler waits")
        plan.ran(.work, startedAt: start + every + AgentPlan.settle)
        #expect(plan.due(at: start + every + 60).isEmpty, "it waits after a change")
        #expect(plan.due(at: start + 2 * every) == [.crawl])
        plan.ran(.crawl, startedAt: start + 2 * every)
        #expect(plan.nextDue == start + 3 * every)
        plan.postpone(.crawl, until: start + 4 * every)
        #expect(plan.due(at: start + 3 * every) == [.remind], "only the quarter-hour look")
    }
}

/// Remembers the reminders it's asked to post.
final class FakeReminderPoster: ReminderPoster, @unchecked Sendable {
    private let lock = NSLock()
    private var posted: [String] = []

    var texts: [String] { lock.withLock { posted } }

    func post(_ reminder: Reminder, archive: URL, name: String) async -> Bool {
        lock.withLock { posted.append(reminder.text) }
        return true
    }
}

/// A clock the tests move.
final class Clock: @unchecked Sendable {
    private let lock = NSLock()
    private var date = Date()

    var now: Date { lock.withLock { date } }

    func advance(_ seconds: TimeInterval) {
        lock.withLock { date += seconds }
    }
}

/// An agent for a temporary copy of the sample archive, sharing its configuration folder.
@MainActor
struct AgentSample {
    let sample: Sample
    let clock = Clock()
    let state: UserDefaults
    let agent: Agent

    init() throws {
        sample = try Sample()
        state = try #require(UserDefaults(suiteName: "carrelkit-agent-\(UUID().uuidString)"))
        let clock = clock
        agent = Agent(
            options: .isolated(config: sample.folder.appending(path: "config")), state: state,
            watching: false, now: { clock.now })
    }

    var handle: ArchiveHandle { sample.model.handle }
    var root: URL { URL(filePath: realPath(sample.root.path(percentEncoded: false))) }
}

func realPath(_ path: String) -> String {
    guard let resolved = realpath(path, nil) else { return path }
    defer { free(resolved) }
    return String(cString: resolved)
}

@MainActor
@Suite struct TheAgent {
    @Test func itClaimsWhatItFollowsSoOtherWorkersStepAside() async throws {
        let test = try AgentSample()
        #expect(!test.handle.agentRunning())
        await test.agent.follow([test.sample.root])
        #expect(test.agent.archives.map(\.name) == [test.handle.name()])
        #expect(test.agent.archives.first?.problem == nil)
        #expect(test.handle.agentRunning())
        let drafted = try await test.sample.model.draftQueued()
        #expect(drafted.alreadyRunning, "the app's own worker steps aside")

        test.agent.pause()
        #expect(!test.handle.agentRunning(), "paused, the CLI's worker drafts again")
        #expect(test.agent.due(at: test.clock.now).isEmpty)
        test.agent.resume()
        #expect(test.handle.agentRunning())

        await test.agent.follow([])
        #expect(!test.handle.agentRunning())
        #expect(test.agent.archives.isEmpty)
    }

    @Test func aSecondAgentLeavesTheArchiveToTheFirst() async throws {
        let test = try AgentSample()
        await test.agent.follow([test.sample.root])
        let other = Agent(
            options: .isolated(config: test.sample.folder.appending(path: "config")),
            state: test.state, watching: false)
        await other.follow([test.sample.root])
        #expect(other.archives.first?.problem?.contains("Another Carrel agent") == true)
        #expect(other.due(at: .distantFuture).isEmpty)
    }

    @Test func anArchiveThatCantBeOpenedSaysWhy() async throws {
        let test = try AgentSample()
        await test.agent.follow([test.sample.folder.appending(path: "missing")])
        #expect(test.agent.archives.first?.problem != nil)
        #expect(test.agent.nextDue == nil)
    }

    @Test func queuedItemsAreDraftedOnceTheQueueSettles() async throws {
        let test = try AgentSample()
        await test.agent.follow([test.sample.root])
        await test.agent.runDue()
        let before = try test.handle.review().batches.flatMap(\.proposals).count

        let note = test.sample.folder.appending(path: "a note.txt")
        try "A note about the lamp.\n".write(to: note, atomically: true, encoding: .utf8)
        let added = try test.handle.queue(
            paths: [note.path(percentEncoded: false)], each: false, whole: false)
        #expect(added.added.count == 1)
        await test.agent.refresh()
        #expect(test.agent.archives.first?.queued == 1)

        let intake = test.root.appending(path: "_System/queue/intake").path(percentEncoded: false)
        test.agent.noticed([intake + "/entry.json"], in: test.root)
        #expect(test.agent.due(at: test.clock.now).isEmpty, "settling")
        test.clock.advance(AgentPlan.settle)
        #expect(test.agent.due(at: test.clock.now).map(\.1) == [.work])
        await test.agent.runDue()
        #expect(try test.handle.intake().waiting.isEmpty)
        #expect(try test.handle.review().batches.flatMap(\.proposals).count == before + 1)
        #expect(test.agent.archives.first?.queued == 0)
        #expect(test.agent.activity == nil)
        #expect(test.agent.due(at: test.clock.now).isEmpty)
    }

    @Test func workOnTheDeskIsWatchedAndItsSessionsRecorded() async throws {
        let test = try AgentSample()
        let desk = test.sample.folder.appending(path: "Desk/Lamp notes")
        try FileManager.default.createDirectory(at: desk, withIntermediateDirectories: true)
        try "Sketches\n".write(
            to: desk.appending(path: "sketch.txt"), atomically: true, encoding: .utf8)
        _ = try test.handle.addToDesk(
            path: desk.path(percentEncoded: false), title: "Lamp notes", summary: nil)

        await test.agent.follow([test.sample.root])
        #expect(test.agent.archives.first?.active.map(\.title).contains("Lamp notes") == true)
        await test.agent.runDue()
        #expect(test.agent.archives.first?.problem == nil)

        let working = realPath(desk.path(percentEncoded: false))
        test.agent.noticed([working + "/sketch.txt"], in: test.root)
        let idle = TimeInterval(test.handle.idleMinutes()) * 60
        test.clock.advance(idle)
        // The crawler and the reminders' look may be due by now too.
        let others = {
            test.agent.due(at: test.clock.now).map(\.1).filter { $0 != .crawl && $0 != .remind }
        }
        #expect(others().isEmpty, "the session may still be going")
        test.clock.advance(AgentPlan.margin)
        #expect(others() == [.sessions])
        await test.agent.runDue()
        #expect(test.agent.archives.first?.problem == nil)
        #expect(others().isEmpty)
    }

    @Test func remindersArePostedOnceAndTheMenuBarFollowsTheSetting() async throws {
        let sample = try Sample()
        let poster = FakeReminderPoster()
        let clock = Clock()
        let agent = Agent(
            options: .isolated(config: sample.folder.appending(path: "config")),
            state: try #require(UserDefaults(suiteName: "carrelkit-agent-\(UUID().uuidString)")),
            watching: false, reminders: poster, now: { clock.now })
        await agent.follow([sample.root])
        await agent.runDue()
        #expect(
            poster.texts.contains(
                "You worked on “oscillator-sim” for 2h 10m. Its entry is waiting for you."),
            "\(poster.texts)")
        let posted = poster.texts.count
        clock.advance(AgentPlan.remindEvery)
        await agent.runDue()
        #expect(poster.texts.count == posted, "each is posted once")
        #expect(try sample.model.handle.reminders().count == posted, "Status still lists them")

        #expect(agent.showsMenuBar)
        _ = try sample.model.handle.setSetting(key: "reminders", value: "idle")
        await agent.refresh()
        #expect(!agent.showsMenuBar)
    }

    #if os(macOS)
    @Test func theFileWatcherSeesFilesChange() async throws {
        let folder = FileManager.default.temporaryDirectory
            .appending(path: "carrelkit-watch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let (stream, continuation) = AsyncStream<[String]>.makeStream()
        let watcher = try #require(
            FileWatcher(folders: [folder], latency: 0.1) { continuation.yield($0) })
        defer { watcher.stop() }
        try await Task.sleep(for: .milliseconds(300))
        try "hello\n".write(
            to: folder.appending(path: "new.txt"), atomically: false, encoding: .utf8)
        let seen = await withTaskGroup(of: [String]?.self) { group in
            group.addTask {
                for await paths in stream where paths.contains(where: { $0.hasSuffix("/new.txt") })
                {
                    return paths
                }
                return nil
            }
            group.addTask {
                try? await Task.sleep(for: .seconds(10))
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            return first
        }
        #expect(seen != nil, "the new file was noticed")
    }
    #endif
}

@Suite struct Links {
    @Test func aLinkToAnItemRoundTrips() throws {
        let link = AgentLink(archive: URL(filePath: "/Users/me/My Archive"), item: "0193abcd")
        let url = link.url
        #expect(url.scheme == "carrel")
        #expect(AgentLink(url) == link)
        #expect(AgentLink(AgentLink(archive: link.archive).url)?.item == nil)
        #expect(AgentLink(URL(string: "carrel://open")!) == nil)
        #expect(AgentLink(URL(string: "https://open?archive=/a")!) == nil)
    }
}
