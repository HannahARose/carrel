// Search (⌘F): the same search as `carrel search`, with its filters. Words come back as you
// type, over the text already read; the whole search, by meaning too, runs once typing
// pauses, so typing never waits on the model.

import CarrelKit
import Foundation
import Observation

@Observable
final class SearchModel {
    /// The most results listed.
    static let limit: UInt32 = 50

    let archive: ArchiveModel
    /// How long typing pauses before the search by meaning runs.
    let pause: Duration

    /// What's typed: words, and filters such as `tag:receipts`.
    var text = "" {
        didSet { if text != oldValue { typed() } }
    }
    /// The latest results: by words while typing, then by meaning too.
    private(set) var results: SearchResults?
    /// Whether the search by meaning is running for what's typed.
    private(set) var isSearchingByMeaning = false
    /// Why what's typed can't be searched, such as a filter that can't be used.
    private(set) var problem: String?
    /// Counts the requests to put the cursor in the search field, for ⌘F.
    private(set) var focusRequests = 0

    /// What's typed is searched in turn; a newer search drops the results of older ones.
    private var generation = 0
    private var running: Task<Void, Never>?

    init(archive: ArchiveModel, pause: Duration = .milliseconds(500)) {
        self.archive = archive
        self.pause = pause
    }

    /// Whether anything is typed, so the content column lists results.
    var isActive: Bool { !text.trimmingCharacters(in: .whitespaces).isEmpty }

    var hits: [SearchHit] { results?.hits ?? [] }

    /// How many were found, as the list's heading says it.
    var heading: String? {
        guard let results else { return nil }
        let shown = results.hits.count
        let total = Int(results.total)
        return switch total {
        case 0: "Nothing found"
        case 1: "1 result"
        case shown: "\(total) results"
        default: "The best \(shown) of \(total) results"
        }
    }

    /// What the search by meaning couldn't do, in `carrel search`'s words: the index is
    /// missing, behind, or has no model on this device.
    var notice: String? {
        guard let results else { return nil }
        if let why = results.wordsOnly { return why }
        guard results.behind > 0 else { return nil }
        let items = results.behind == 1 ? "1 item" : "\(results.behind) items"
        return
            "The meaning index is behind by \(items); they’re found by their words until it catches up."
    }

    /// Puts the cursor in the search field.
    func focus() {
        focusRequests += 1
    }

    /// Clears what's typed and its results.
    func clear() {
        text = ""
    }

    /// Searches what's typed: by words now, by meaning after a pause.
    private func typed() {
        running?.cancel()
        generation += 1
        isSearchingByMeaning = false
        guard isActive else {
            results = nil
            problem = nil
            return
        }
        let query = text
        let generation = generation
        running = Task { await search(query, generation: generation) }
    }

    /// Waits until the search started for what's typed now has finished, for tests.
    func settled() async {
        await running?.value
    }

    private func search(_ query: String, generation: Int) async {
        let handle = archive.handle
        let limit = Self.limit
        let current = { generation == self.generation }
        do {
            let words = try await Task.detached(priority: .userInitiated) {
                try handle.searchWords(query: query, limit: limit)
            }.value
            guard current() else { return }
            results = words
            problem = nil
        } catch {
            guard current() else { return }
            results = nil
            problem = message(for: error)
            return
        }
        do { try await Task.sleep(for: pause) } catch { return }
        guard current() else { return }
        isSearchingByMeaning = true
        defer { if current() { isSearchingByMeaning = false } }
        // The words' results stay if the whole search fails; `lastError` says why.
        if let whole = try? await archive.search(query, limit: Self.limit), current() {
            results = whole
        }
    }
}

extension SearchHit {
    /// The matched passage, with the query's words marked.
    var highlightedPassage: AttributedString {
        passage.reduce(into: AttributedString()) { text, part in
            var run = AttributedString(part.text)
            if part.highlighted { run.inlinePresentationIntent = .stronglyEmphasized }
            text += run
        }
    }

    /// The mark beside a sensitive or restricted item, as `carrel search` marks it.
    var sensitivityMark: String? {
        switch sensitivity {
        case .restricted: "Restricted"
        case .sensitive: "Sensitive"
        default: nil
        }
    }
}
