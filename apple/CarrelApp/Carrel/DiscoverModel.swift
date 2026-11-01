// Discover (docs/design/librarian.md, Discovery): views of the archive itself, as
// `carrel discover` shows them. The core computes each one; this model reads them off the
// main thread for the Discover section's pages, and keeps the walk through the stacks.

import CarrelKit
import Foundation
import Observation

/// A page of the Discover section in the sidebar.
enum DiscoverPage: String, CaseIterable, Hashable {
    case today
    case wander
    case timeline
    case map
    case activity

    var title: String {
        switch self {
        case .today: "On This Day"
        case .wander: "Wander"
        case .timeline: "Timeline"
        case .map: "Archive Map"
        case .activity: "Activity"
        }
    }

    var symbol: String {
        switch self {
        case .today: "calendar"
        case .wander: "figure.walk"
        case .timeline: "chart.bar.xaxis"
        case .map: "square.grid.3x3.square"
        case .activity: "square.grid.4x3.fill"
        }
    }
}

/// Which of an item's dates the timeline goes by.
enum TimelineDate: String, CaseIterable, Hashable {
    /// Each item's own: when it was made, else when it was filed.
    case own
    case created
    case filed
    case worked

    var title: String {
        switch self {
        case .own: "Its Date"
        case .created: "Created"
        case .filed: "Filed"
        case .worked: "Worked On"
        }
    }
}

/// How finely the timeline groups.
enum TimelineGrouping: String, CaseIterable, Hashable {
    case year
    case month
    case day

    var title: String { rawValue.capitalized }
}

@MainActor
@Observable
final class DiscoverModel {
    let archive: ArchiveModel

    /// The day On This Day looks back from.
    var day = Date.now
    private(set) var onThisDay: OnThisDay?

    /// The walk through the stacks, its steps so far (newest last), and whether it has
    /// visited everything it can reach.
    private(set) var walk: Wander?
    private(set) var steps: [WanderStep] = []
    private(set) var walkIsDone = false
    private(set) var isStepping = false

    /// The timeline's words and filters, which date it goes by and how it groups.
    var timelineQuery = ""
    var timelineDate = TimelineDate.own
    var timelineGrouping = TimelineGrouping.month
    private(set) var timeline: DiscoveryTimeline?

    private(set) var map: ArchiveMap?
    private(set) var activity: ArchiveActivity?
    /// A collection not opened in a long time, for Status.
    private(set) var resurfaced: Resurfaced?

    /// What went wrong with the last view read, for the page to say.
    var problem: String?

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    /// The step the walk is at.
    var currentStep: WanderStep? { steps.last }

    /// Reads `page` again.
    func refresh(_ page: DiscoverPage) async {
        switch page {
        case .today: await refreshToday()
        case .wander: if walk == nil { await startWalk() }
        case .timeline: await refreshTimeline()
        case .map:
            await read {
                try $0.archiveMap()
            } into: {
                self.map = $0
            }
        case .activity:
            await read {
                try $0.activity(from: nil, to: nil)
            } into: {
                self.activity = $0
            }
        }
    }

    func refreshToday() async {
        let date = Self.dayText(day)
        await read {
            try $0.onThisDay(date: date)
        } into: {
            self.onThisDay = $0
        }
    }

    func refreshTimeline() async {
        let query = timelineQuery
        let by = timelineDate == .own ? nil : timelineDate.rawValue
        let per = timelineGrouping.rawValue
        await read {
            try $0.timeline(query: query, by: by, per: per)
        } into: {
            self.timeline = $0
        }
    }

    /// Reads whether a collection has been forgotten, for Status.
    func refreshResurfaced() async {
        await read {
            try $0.resurfaced()
        } into: {
            self.resurfaced = $0
        }
    }

    /// Starts a new walk, from the item `start` or from somewhere not visited in a while, and
    /// takes its first step.
    func startWalk(from start: String? = nil, seed: UInt64? = nil) async {
        walk = nil
        steps = []
        walkIsDone = false
        do {
            walk = try await archive.call { try $0.startWander(start: start, seed: seed) }
        } catch {
            take(error)
            return
        }
        await step()
    }

    /// Takes the walk's next step.
    func step() async {
        guard let walk, !walkIsDone, !isStepping else { return }
        isStepping = true
        defer { isStepping = false }
        do {
            if let next = try await archive.call({ try $0.wanderStep(walk: walk) }) {
                steps.append(next)
                // Its question is in the queue now, for Questions and the badge.
                if next.question != nil { try? await archive.refreshQuestions() }
            } else {
                walkIsDone = true
            }
            problem = nil
        } catch {
            take(error)
        }
    }

    /// Records that `item`'s collection was opened on this Mac, as showing an item does, so
    /// wander and resurfacing know it's been seen.
    func opened(collection: String) {
        let handle = archive.handle
        Task.detached(priority: .utility) { try? handle.recordOpened(collection: collection) }
    }

    /// `date` as `YYYY-MM-DD`, in this Mac's time zone.
    static func dayText(_ date: Date) -> String {
        dayText(date, in: .current)
    }

    private func read<T: Sendable>(
        _ work: @escaping @Sendable (ArchiveHandle) throws -> T, into keep: (T) -> Void
    ) async {
        do {
            keep(try await archive.call(work))
            problem = nil
        } catch {
            take(error)
        }
    }

    private func take(_ error: Error) {
        archive.lastError = nil
        problem = message(for: error)
    }
}

extension WanderReason {
    /// Why the step came next, in words.
    var words: String {
        switch self {
        case .start(let collection):
            "A start in \(collection), which you haven’t opened in a while."
        case .linked(let linkType, let from, let back):
            back
                ? "It links to “\(from)” (\(linkType))."
                : "“\(from)” links to it (\(linkType))."
        case .near(let from, let score):
            "It’s close in meaning to “\(from)” (\(Int((score * 100).rounded()))% alike)."
        case .neighbour(let collection):
            "It’s beside the last item, in \(collection)."
        case .jump(let collection, let lastSeen):
            lastSeen.map { "A jump to \(collection), last seen \($0)." }
                ?? "A jump to \(collection), which you’ve never opened here."
        }
    }
}

extension ArchiveMap {
    /// The regions directly inside `parent`, or at the top for `nil`.
    func regions(in parent: String?) -> [MapRegion] {
        regions.filter { $0.parent == parent }
    }
}

extension ArchiveWindowModel {
    /// Records that the chosen item's collection was opened on this Mac.
    func recordOpened() {
        guard let item = selectedItem, let path = collectionPath(of: item) else { return }
        discover?.opened(collection: path)
    }
}
