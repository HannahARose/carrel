// What the agent keeps for the Quick Look extension (docs/design/macos-integration.md,
// "Quick Look"): Carrel's drawings of STEP, IGES and 3MF items, which come from providers the
// sandboxed extension can't run, and the roots of the archives followed, to name a card's
// collection by. Restricted items' drawings are never kept.

import Foundation

@MainActor
final class QuickLookDrawingsKeeper {
    /// How often the drawings are looked over when nothing drawn has changed.
    nonisolated static let interval: TimeInterval = 600

    let store: QuickLookStore
    private var lastKept: Date?
    private var keeping: Task<Void, Never>?
    /// The archives to look over again once the pass under way ends.
    private var pending: [ArchiveHandle]?
    private var pendingRoots: [URL] = []

    init(store: QuickLookStore) {
        self.store = store
    }

    /// Keeps the store current for the archives `handles`, at `roots`: at once when
    /// `changed`, else once `interval` has passed. One pass runs at a time; a change during
    /// one runs another after it.
    func keep(_ handles: [ArchiveHandle], roots: [URL], at date: Date, changed: Bool) {
        try? store.write(roots: roots.map { $0.path(percentEncoded: false) })
        let due = changed || lastKept.map { date.timeIntervalSince($0) >= Self.interval } ?? true
        guard due else { return }
        guard keeping == nil else {
            if changed {
                pending = handles
                pendingRoots = roots
            }
            return
        }
        lastKept = date
        let drawings = store.drawings.path(percentEncoded: false)
        keeping = Task {
            await Task.detached(priority: .background) {
                var keep: [String] = []
                for handle in handles {
                    // An archive that can't be read now keeps nothing until the next pass.
                    keep += (try? handle.keepQuickLookDrawings(store: drawings)) ?? []
                }
                pruneQuickLookDrawings(store: drawings, keep: keep)
            }.value
            keeping = nil
            if let next = pending {
                pending = nil
                keep(next, roots: pendingRoots, at: date, changed: true)
            }
        }
    }

    /// Waits for the pass under way, if any.
    func settled() async {
        while let keeping { await keeping.value }
    }
}
