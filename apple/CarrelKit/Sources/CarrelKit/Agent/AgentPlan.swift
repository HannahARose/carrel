// When the Mac app's agent does what for one archive (docs/design/mac-app.md, "The background
// agent"): what a changed file means, and when each job is due. It's plain values and dates,
// so the scheduling is tested without waiting.

import Foundation

/// A job the agent runs for an archive.
public enum AgentJob: Sendable, Equatable, CaseIterable {
    /// Draft the intake queue, then bring the embedding index up to date: the worker loop.
    case work
    /// Record the work sessions that have finished on checked-out and desk items.
    case sessions
    /// Take a scheduled snapshot of the archive.
    case backup
    /// Let the crawler recheck a few items, when nothing else is due and the Mac is idle and
    /// on power (its pace decides how often and how many).
    case crawl
}

/// What a changed path means to the agent.
public enum AgentChange: Sendable, Equatable {
    /// Something joined or left the intake queue.
    case queue
    /// A filed item or card changed, so the index may be behind.
    case filed
    /// What's checked out or on the desk changed, so the working copies to watch did too.
    case active
    /// A working copy changed: a work session is going on.
    case working
    /// Nothing the agent acts on, such as its own index or the history repository.
    case ignored
}

/// One archive's schedule.
public struct AgentPlan: Sendable, Equatable {
    /// How long changes settle before the worker runs, so a burst of them runs it once.
    public static let settle: TimeInterval = 2
    /// How long after a session goes idle it's recorded, past `sessions.idle_minutes`.
    public static let margin: TimeInterval = 60
    /// How often a scheduled snapshot is taken.
    public static let backupEvery: TimeInterval = 24 * 60 * 60
    /// How soon a snapshot that failed, as when the drive isn't plugged in, is tried again.
    public static let backupRetry: TimeInterval = 60 * 60
    /// How often the agent asks the crawler whether a pass is due, and how long it waits
    /// after a change it noticed: the crawler works only when the owner isn't.
    public static let crawlEvery: TimeInterval = 5 * 60

    /// The archive's root folder, as a path ending without a slash.
    public let root: String
    /// The working copies of what's checked out and on the desk, as paths.
    public var workingCopies: [String]
    /// How long a pause ends a work session.
    public var idle: TimeInterval
    /// Whether backups are set up for the archive on this Mac.
    public var backupsSetUp: Bool
    /// When the last scheduled snapshot was taken, or tried and failed.
    public var lastBackup: Date?
    /// Whether the last scheduled snapshot failed.
    public var backupFailed = false

    /// When the worker is next due, if it is.
    public private(set) var workDue: Date?
    /// When sessions are next due to be recorded, if they are.
    public private(set) var sessionsDue: Date?
    /// When the crawler is next asked whether a pass is due.
    public private(set) var crawlDue: Date?

    public init(
        root: String, workingCopies: [String] = [], idleMinutes: UInt32 = 30,
        backupsSetUp: Bool = false, lastBackup: Date? = nil
    ) {
        self.root = Self.folder(root)
        self.workingCopies = workingCopies.map(Self.folder)
        self.idle = TimeInterval(idleMinutes) * 60
        self.backupsSetUp = backupsSetUp
        self.lastBackup = lastBackup
    }

    private static func folder(_ path: String) -> String {
        path.count > 1 && path.hasSuffix("/") ? String(path.dropLast()) : path
    }

    private static func path(_ path: String, isIn folder: String) -> Bool {
        path == folder || path.hasPrefix(folder + "/")
    }

    /// What a change at `path` means.
    public func classify(_ path: String) -> AgentChange {
        // A working copy inside the archive is still the owner's work.
        if workingCopies.contains(where: { Self.path(path, isIn: $0) }) { return .working }
        guard Self.path(path, isIn: root) else { return .ignored }
        let relative = path == root ? "" : String(path.dropFirst(root.count + 1))
        if relative.hasSuffix(".tmp") || relative.contains(".tmp-") { return .ignored }
        guard relative == "_System" || relative.hasPrefix("_System/") else {
            return relative.isEmpty ? .ignored : .filed
        }
        if relative.hasPrefix("_System/queue/intake") { return .queue }
        if relative.hasPrefix("_System/devices/") && relative.hasSuffix("/desk.json") {
            return .active
        }
        if relative.hasPrefix("_System/manifests/") && relative.hasSuffix(".checkout.tsv") {
            return .active
        }
        return .ignored
    }

    /// Notes the changes at `paths`, noticed at `now`, and returns whether what's checked out
    /// or on the desk changed, so the caller reads the working copies again.
    @discardableResult
    public mutating func noticed(_ paths: [String], at now: Date) -> Bool {
        var active = false
        for change in paths.map(classify) {
            if change != .ignored {
                // Anything the owner does puts the crawler off a while.
                crawlDue = max(crawlDue ?? .distantPast, now.addingTimeInterval(Self.crawlEvery))
            }
            switch change {
            case .queue, .filed:
                workDue = now.addingTimeInterval(Self.settle)
            case .active:
                active = true
                workDue = now.addingTimeInterval(Self.settle)
                sessionsDue = min(sessionsDue ?? .distantFuture, now)
            case .working:
                // Recorded once the session has been idle long enough; each change pushes
                // that back.
                sessionsDue = now.addingTimeInterval(idle + Self.margin)
            case .ignored:
                break
            }
        }
        return active
    }

    /// On starting, or resuming after a pause: catch up on everything at once.
    public mutating func start(at now: Date) {
        workDue = now
        sessionsDue = now
        crawlDue = now.addingTimeInterval(Self.crawlEvery)
    }

    /// When the next scheduled snapshot is due, if backups are set up.
    public var backupDue: Date? {
        guard backupsSetUp else { return nil }
        guard let lastBackup else { return .distantPast }
        return lastBackup.addingTimeInterval(backupFailed ? Self.backupRetry : Self.backupEvery)
    }

    /// The jobs due at `now`, in the order to run them.
    public func due(at now: Date) -> [AgentJob] {
        var jobs: [AgentJob] = []
        if let workDue, workDue <= now { jobs.append(.work) }
        if let sessionsDue, sessionsDue <= now { jobs.append(.sessions) }
        if let backupDue, backupDue <= now { jobs.append(.backup) }
        // The crawler only gets the time nothing else needs.
        if jobs.isEmpty, let crawlDue, crawlDue <= now { jobs.append(.crawl) }
        return jobs
    }

    /// Notes that `job`, started at `started`, has run. A change noticed while it ran stays
    /// due.
    public mutating func ran(_ job: AgentJob, startedAt started: Date, succeeded: Bool = true) {
        switch job {
        case .work:
            if let due = workDue, due <= started { workDue = nil }
        case .sessions:
            if let due = sessionsDue, due <= started { sessionsDue = nil }
        case .backup:
            lastBackup = started
            backupFailed = !succeeded
        case .crawl:
            crawlDue = started.addingTimeInterval(Self.crawlEvery)
        }
    }

    /// Puts `job` off until `later`, as when another writer holds the archive.
    public mutating func postpone(_ job: AgentJob, until later: Date) {
        switch job {
        case .work: workDue = later
        case .sessions: sessionsDue = later
        case .backup: break
        case .crawl: crawlDue = later
        }
    }

    /// When something is next due, if anything is.
    public var nextDue: Date? {
        [workDue, sessionsDue, backupDue, crawlDue].compactMap { $0 }.min()
    }
}
