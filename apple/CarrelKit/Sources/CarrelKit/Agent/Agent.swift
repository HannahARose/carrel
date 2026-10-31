// The Mac app's background agent (docs/design/mac-app.md, "The background agent"): for each
// archive it follows, it claims the archive so the CLI's worker steps aside, watches its files
// and working copies, and runs the jobs its AgentPlan says are due, one at a time: the
// worker loop, recording work sessions, and scheduled snapshots. It coordinates with the app
// and the CLI through the archive's files, the writer lease and the worker's status file.

import Foundation
import Observation

/// What the menu bar list shows for one archive.
public struct AgentArchive: Identifiable, Sendable, Equatable {
    /// The archive's root folder.
    public let root: URL
    public let name: String
    /// What's checked out and on this Mac's desk, with their pending history entries.
    public var active: [ActiveItem] = []
    /// How many items wait in the intake queue on this Mac.
    public var queued = 0
    /// What went wrong last, if the last job failed.
    public var problem: String?

    public var id: URL { root }

    /// How many pending history entries wait for approval.
    public var pending: Int { active.reduce(0) { $0 + $1.pending.count } }
}

@MainActor
@Observable
public final class Agent {
    /// The archives followed, in the order given.
    public private(set) var archives: [AgentArchive] = []
    /// What the agent is doing now, for people, or `nil` when it's waiting.
    public private(set) var activity: String?
    /// Whether it's paused: it lets go of its archives, so the CLI's worker drafts again.
    public private(set) var isPaused = false

    private struct Followed {
        let handle: ArchiveHandle
        var claim: AgentClaim?
        var plan: AgentPlan
        var watcher: AnyObject?
    }

    private var followed: [URL: Followed] = [:]
    private let options: OpenOptions?
    private let state: UserDefaults
    private let watching: Bool
    private let now: @Sendable () -> Date
    private var sleeper: Task<Void, Never>?

    /// An agent opening archives with `options` (the device's own when `nil`), keeping when
    /// it last backed each up in `state`. It watches files only when `watching` says so; tests
    /// tell it what changed instead.
    public init(
        options: OpenOptions? = nil, state: UserDefaults = .standard, watching: Bool = true,
        now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.options = options
        self.state = state
        self.watching = watching
        self.now = now
    }

    // MARK: - Which archives

    /// Follows the archives at `roots`, and stops following the others. An archive that
    /// can't be opened, or that another agent has claimed, is shown with what's wrong.
    public func follow(_ roots: [URL]) async {
        let roots = roots.map { URL(filePath: Self.realPath($0.path(percentEncoded: false))) }
        for root in followed.keys where !roots.contains(root) {
            drop(root)
        }
        var shown: [AgentArchive] = []
        var opened: [URL] = []
        for root in roots {
            if followed[root] == nil {
                do {
                    try await open(root)
                    opened.append(root)
                } catch {
                    shown.append(
                        AgentArchive(
                            root: root, name: root.lastPathComponent,
                            problem: (error as? CarrelError)?.message
                                ?? error.localizedDescription))
                    continue
                }
            }
            if let existing = archives.first(where: { $0.root == root }) {
                shown.append(existing)
            } else if let handle = followed[root]?.handle {
                shown.append(AgentArchive(root: root, name: handle.name()))
            }
        }
        archives = shown
        // An archive another agent had is tried again, in case that agent has gone.
        for root in roots where followed[root] != nil && followed[root]?.claim == nil && !isPaused {
            claim(root)
        }
        for root in opened { await refresh(root) }
        if !opened.isEmpty { nudge() }
    }

    /// Reads what every archive followed shows again.
    public func refresh() async {
        for archive in archives where followed[archive.root] != nil {
            await refresh(archive.root)
        }
    }

    private func open(_ root: URL) async throws {
        let options = options
        let handle = try await Task.detached {
            if let options { try ArchiveHandle.open(root, options: options) } else {
                try ArchiveHandle.open(root)
            }
        }.value
        var plan = AgentPlan(
            root: root.path(percentEncoded: false), idleMinutes: handle.idleMinutes(),
            backupsSetUp: handle.backupsSetUp(), lastBackup: lastBackup(of: handle))
        plan.backupFailed = state.bool(forKey: Self.key("backupFailed", handle))
        plan.start(at: now())
        followed[root] = Followed(handle: handle, plan: plan)
    }

    private func drop(_ root: URL) {
        followed[root]?.claim?.release()
        stopWatching(root)
        followed[root] = nil
    }

    /// Claims `root` and watches it, unless another agent has it.
    private func claim(_ root: URL) {
        guard let handle = followed[root]?.handle else { return }
        do {
            guard let claim = try handle.claimForAgent() else {
                problem(root, "Another Carrel agent looks after this archive.")
                return
            }
            followed[root]?.claim = claim
            followed[root]?.plan.start(at: now())
            watch(root)
        } catch {
            problem(root, (error as? CarrelError)?.message ?? error.localizedDescription)
        }
    }

    // MARK: - Pausing

    /// Pauses: lets go of every archive, so the CLI's worker drafts again, and stops
    /// watching. A job under way finishes first.
    public func pause() {
        isPaused = true
        for root in followed.keys {
            followed[root]?.claim?.release()
            followed[root]?.claim = nil
            stopWatching(root)
        }
        nudge()
    }

    /// Resumes, catching up on everything at once.
    public func resume() {
        isPaused = false
        for root in followed.keys { claim(root) }
        nudge()
    }

    // MARK: - Noticing changes

    /// Notes that the files at `paths` changed, in or around the archive at `root`.
    public func noticed(_ paths: [String], in root: URL) {
        guard followed[root] != nil, !isPaused else { return }
        let paths = paths.map(Self.realPath)
        if followed[root]?.plan.noticed(paths, at: now()) == true {
            Task { await refresh(root) }
        }
        nudge()
    }

    private func watch(_ root: URL) {
        #if os(macOS)
            guard watching, let plan = followed[root]?.plan else { return }
            stopWatching(root)
            let outside = plan.workingCopies.filter {
                !($0 == plan.root || $0.hasPrefix(plan.root + "/"))
            }
            let folders = ([plan.root] + outside).map { URL(filePath: $0) }
            followed[root]?.watcher = FileWatcher(folders: folders) { [weak self] paths in
                Task { @MainActor in self?.noticed(paths, in: root) }
            }
        #endif
    }

    private func stopWatching(_ root: URL) {
        #if os(macOS)
            (followed[root]?.watcher as? FileWatcher)?.stop()
        #endif
        followed[root]?.watcher = nil
    }

    // MARK: - Running jobs

    /// When a job is next due, for any archive followed.
    public var nextDue: Date? {
        guard !isPaused else { return nil }
        return followed.values.filter { $0.claim != nil }.compactMap(\.plan.nextDue).min()
    }

    /// The jobs due at `date`, by archive, in the order followed.
    public func due(at date: Date) -> [(URL, AgentJob)] {
        guard !isPaused else { return [] }
        return archives.flatMap { archive -> [(URL, AgentJob)] in
            guard let followed = followed[archive.root], followed.claim != nil else { return [] }
            return followed.plan.due(at: date).map { (archive.root, $0) }
        }
    }

    /// Runs every job due now, one at a time.
    public func runDue() async {
        for (root, job) in due(at: now()) {
            guard !isPaused else { return }
            await run(job, for: root)
        }
    }

    /// Runs the agent until its task is cancelled: the jobs as they fall due, and the
    /// archives `roots` returns, read again every minute.
    public func run(following roots: @escaping @MainActor () -> [URL]) async {
        var refreshed = Date.distantPast
        while !Task.isCancelled {
            await follow(roots())
            await runDue()
            // Pending entries approved from the CLI, say, show within a minute.
            if now().timeIntervalSince(refreshed) >= 60 {
                await refresh()
                refreshed = now()
            }
            let wait = min(max(nextDue.map { $0.timeIntervalSince(now()) } ?? 60, 0.2), 60)
            let sleeper = Task<Void, Never> { try? await Task.sleep(for: .seconds(wait)) }
            self.sleeper = sleeper
            await sleeper.value
        }
        for root in followed.keys { drop(root) }
    }

    /// Wakes the loop to look at what's due.
    private func nudge() {
        sleeper?.cancel()
    }

    private func run(_ job: AgentJob, for root: URL) async {
        guard let handle = followed[root]?.handle else { return }
        let name = handle.name()
        let started = now()
        var succeeded = true
        do {
            switch job {
            case .work:
                activity = "Drafting in “\(name)”"
                let relay = ProgressRelay { [weak self] _, _, item in
                    guard let item else { return }
                    Task { @MainActor in self?.activity = "Working on \(item) in “\(name)”" }
                }
                let drafted = try await Task.detached { try handle.draftQueued(progress: relay) }
                    .value
                if let failure = drafted.failures.last {
                    problem(root, "Couldn’t draft \(failure.path): \(failure.reason)")
                } else {
                    problem(root, nil)
                }
            case .sessions:
                activity = "Looking for work sessions in “\(name)”"
                _ = try await Task.detached { try handle.recordSessions() }.value
                problem(root, nil)
            case .backup:
                activity = "Backing up “\(name)”"
                _ = try await Task.detached { try handle.backUp(description: "Scheduled snapshot") }
                    .value
            }
        } catch CarrelError.Busy {
            // Another writer has the archive; try again shortly.
            followed[root]?.plan.postpone(job, until: started.addingTimeInterval(30))
            activity = nil
            return
        } catch {
            succeeded = false
            problem(root, (error as? CarrelError)?.message ?? error.localizedDescription)
        }
        activity = nil
        followed[root]?.plan.ran(job, startedAt: started, succeeded: succeeded)
        if job == .backup {
            state.set(started, forKey: Self.key("lastBackup", handle))
            state.set(!succeeded, forKey: Self.key("backupFailed", handle))
        }
        await refresh(root)
    }

    // MARK: - What the menu bar shows

    /// Reads what's active and queued in `root` again, and the working copies to watch.
    private func refresh(_ root: URL) async {
        guard let handle = followed[root]?.handle else { return }
        let read = await Task.detached {
            (
                try? handle.active(), (try? handle.intake())?.waiting.count ?? 0,
                handle.idleMinutes(), handle.backupsSetUp()
            )
        }.value
        guard followed[root] != nil else { return }
        let active = read.0 ?? []
        let working = active.map { Self.realPath($0.working) }
        let moved = followed[root]?.plan.workingCopies != working
        followed[root]?.plan.workingCopies = working
        followed[root]?.plan.idle = TimeInterval(read.2) * 60
        followed[root]?.plan.backupsSetUp = read.3
        if let index = archives.firstIndex(where: { $0.root == root }) {
            archives[index].active = active
            archives[index].queued = read.1
        }
        if moved, followed[root]?.claim != nil { watch(root) }
    }

    private func problem(_ root: URL, _ message: String?) {
        if let index = archives.firstIndex(where: { $0.root == root }) {
            archives[index].problem = message
        }
    }

    // MARK: - State

    private static func key(_ name: String, _ handle: ArchiveHandle) -> String {
        "\(name).\(handle.id())"
    }

    private func lastBackup(of handle: ArchiveHandle) -> Date? {
        state.object(forKey: Self.key("lastBackup", handle)) as? Date
    }

    /// `path` with symbolic links resolved, as file-system events report it.
    nonisolated static func realPath(_ path: String) -> String {
        guard let resolved = realpath(path, nil) else {
            return path.count > 1 && path.hasSuffix("/") ? String(path.dropLast()) : path
        }
        defer { free(resolved) }
        return String(cString: resolved)
    }
}
