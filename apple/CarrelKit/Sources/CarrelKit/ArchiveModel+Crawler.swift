// The crawler (docs/design/restructure.md, "Crawlers"): its findings approved or rejected in
// Review, how far it has got, and the changes it made on its own, undone together.

import Foundation

extension ArchiveModel {
    /// Approves the finding `id`: its change is made as the owner's, through the corrections
    /// path, with `model` or this Mac's.
    public func approve(finding id: String, model: ModelProvider? = nil) async throws {
        _ = try await call(reporting: "Making the crawler’s change") { handle, _ in
            try handle.approveCrawlerFinding(id: id, model: model)
        }
        try await refreshCrawler()
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
    }

    /// Rejects the finding `id`: it leaves Review, and the crawler won't suggest it again.
    public func reject(finding id: String) async throws {
        try await call { try $0.rejectCrawlerFinding(id: id) }
        try await refreshCrawler()
    }

    /// How far the crawler has got through the archive.
    public func crawlerStatus() async throws -> CrawlerStatus {
        try await call { try $0.crawlerStatus() }
    }

    /// The changes the crawler made on its own since `since`, newest first.
    public func crawlerChanges(since: Date) async throws -> [Change] {
        let seconds = Int64(since.timeIntervalSince1970)
        return try await call { try $0.crawlerChanges(since: seconds) }
    }

    /// Undoes every change the crawler made on its own since `since`, newest first.
    public func undoCrawlerChanges(since: Date) async throws -> LibrarianUndone {
        let seconds = Int64(since.timeIntervalSince1970)
        let undone = try await call { try $0.undoCrawlerChanges(since: seconds) }
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
        return undone
    }

    /// A pass of the crawler now, of `items` items (its pace's share when `nil`), whatever
    /// its pace, with `model` or this Mac's strongest that fits in `memoryGB`.
    @discardableResult
    public func crawlNow(items: UInt32? = nil, model: ModelProvider? = nil, memoryGB: UInt64?)
        async throws -> CrawlerPassed
    {
        let passed = try await call(reporting: "Rechecking where items are filed") { handle, _ in
            try handle.crawlNow(items: items, model: model, memoryGb: memoryGB)
        }
        try await refreshCrawler()
        try await refreshStatus()
        if status?.hasHistory == true { try await refreshHistory() }
        return passed
    }
}
