// What the agent adds to Spotlight (docs/design/macos-integration.md, "Spotlight"): each
// archive's cards, collections and glossary facts, as much of them as `spotlight.index` says,
// and never restricted items. The core decides what goes in; this keeps Core Spotlight in step
// with it: entries that changed are indexed again, those gone are deleted, and an archive's
// entries are removed when the agent stops following it.

import Foundation

#if canImport(CoreSpotlight)
import CoreSpotlight
import UniformTypeIdentifiers
#endif

/// One entry as it's indexed: the archive's, and the link that opens it in Carrel.
public struct SpotlightIndexed: Sendable, Equatable {
    /// The identifier Spotlight knows it by, which opens it in Carrel: a `carrel://open`
    /// link to the item, or to the archive for a collection or a fact.
    public let identifier: String
    /// The archive's identifier.
    public let domain: String
    public let entry: SpotlightEntry
}

#if canImport(CoreSpotlight)
extension AgentLink {
    /// The link a Spotlight result opens, from the activity Spotlight continues in the agent
    /// when it's chosen; `nil` for another activity.
    public init?(spotlightResult activity: NSUserActivity) {
        guard activity.activityType == CSSearchableItemActionType,
            let identifier = activity.userInfo?[CSSearchableItemActivityIdentifier] as? String,
            let url = URL(string: identifier)
        else { return nil }
        self.init(url)
    }
}
#endif

/// Where the agent puts what Spotlight finds: Core Spotlight, or a test's own.
public protocol SpotlightIndexing: Sendable {
    func index(_ entries: [SpotlightIndexed]) async throws
    func delete(identifiers: [String]) async throws
    func deleteDomain(_ domain: String) async throws
}

@MainActor
final class SpotlightIndexer {
    /// How often an archive is looked over when nothing in it seems to have changed.
    nonisolated static let interval: TimeInterval = 600

    private let index: any SpotlightIndexing
    /// What's indexed for each archive, by entry: its version and identifier.
    private var indexed: [String: [String: (version: String, identifier: String)]] = [:]
    private var lastSynced: [String: Date] = [:]
    private var syncing: Task<Void, Never>?
    /// Archives to look over once the pass under way ends.
    private var pending: [String: (ArchiveHandle, URL)] = [:]
    /// Archives whose entries are to be removed, before any pass.
    private var removals: Set<String> = []

    init(index: any SpotlightIndexing) {
        self.index = index
    }

    /// Brings the archive `handle` at `root` up to date in Spotlight: at once when `changed`,
    /// else once `interval` has passed since the last time.
    func sync(_ handle: ArchiveHandle, root: URL, at date: Date, changed: Bool) {
        let domain = handle.id()
        let due =
            changed
            || lastSynced[domain].map { date.timeIntervalSince($0) >= Self.interval } ?? true
        guard due else { return }
        pending[domain] = (handle, root)
        lastSynced[domain] = date
        removals.remove(domain)
        runPending()
    }

    /// Removes the archive `domain`'s entries, when the agent stops following it.
    func remove(_ domain: String) {
        pending[domain] = nil
        lastSynced[domain] = nil
        indexed[domain] = nil
        removals.insert(domain)
        runPending()
    }

    /// Waits until every pass asked for has run.
    func settled() async {
        while let syncing { await syncing.value }
    }

    /// Runs the next removal or pass, one at a time, so a pass never adds back what a
    /// removal took out.
    private func runPending() {
        guard syncing == nil else { return }
        if let domain = removals.popFirst() {
            let index = index
            syncing = Task {
                try? await index.deleteDomain(domain)
                syncing = nil
                runPending()
            }
        } else if let (domain, (handle, root)) = pending.first {
            pending[domain] = nil
            syncing = Task {
                await pass(handle, root: root, domain: domain)
                syncing = nil
                runPending()
            }
        }
    }

    private func pass(_ handle: ArchiveHandle, root: URL, domain: String) async {
        let read = await Task.detached(priority: .utility) { try? handle.spotlightEntries() }.value
        // The archive couldn't be read: leave what's indexed until the next pass.
        guard let entries = read else { return }
        let first = indexed[domain] == nil
        let before = indexed[domain] ?? [:]
        let now = entries.map {
            SpotlightIndexed(
                identifier: Self.identifier(of: $0, in: root), domain: domain, entry: $0)
        }
        let changed = first ? now : now.filter { before[$0.entry.id]?.version != $0.entry.version }
        let ids = Set(entries.map(\.id))
        let gone = before.filter { !ids.contains($0.key) }.map(\.value.identifier)
        do {
            // The first pass since the agent started replaces whatever an earlier run left.
            if first { try await index.deleteDomain(domain) }
            if !gone.isEmpty { try await index.delete(identifiers: gone) }
            if !changed.isEmpty { try await index.index(changed) }
        } catch {
            // Spotlight isn't taking entries now: try everything again next time.
            indexed[domain] = nil
            return
        }
        // The archive was let go of while this ran.
        guard lastSynced[domain] != nil else { return }
        indexed[domain] = Dictionary(
            uniqueKeysWithValues: now.map { ($0.entry.id, ($0.entry.version, $0.identifier)) })
    }

    /// The link that opens `entry` in Carrel, which is also its identifier in Spotlight.
    nonisolated static func identifier(of entry: SpotlightEntry, in root: URL) -> String {
        var components = URLComponents(
            url: AgentLink(archive: root, item: entry.item).url, resolvingAgainstBaseURL: false)!
        // Collections and facts open the archive; these keep their identifiers apart.
        switch entry.kind {
        case .item: break
        case .collection:
            components.queryItems?.append(
                URLQueryItem(name: "collection", value: entry.collectionPath))
        case .fact:
            components.queryItems?.append(URLQueryItem(name: "fact", value: entry.title))
        }
        return components.url!.absoluteString
    }
}

#if canImport(CoreSpotlight)
/// The device's Core Spotlight index, in an index of Carrel's own.
public struct CoreSpotlightIndex: SpotlightIndexing {
    private let name: String

    public init(name: String = "Carrel") {
        self.name = name
    }

    private var searchable: CSSearchableIndex { CSSearchableIndex(name: name) }

    public func index(_ entries: [SpotlightIndexed]) async throws {
        try await searchable.indexSearchableItems(entries.map(Self.item))
    }

    public func delete(identifiers: [String]) async throws {
        try await searchable.deleteSearchableItems(withIdentifiers: identifiers)
    }

    public func deleteDomain(_ domain: String) async throws {
        try await searchable.deleteSearchableItems(withDomainIdentifiers: [domain])
    }

    /// `entry` as Spotlight shows it: a sensitive item's title and nothing else.
    static func item(_ indexed: SpotlightIndexed) -> CSSearchableItem {
        let entry = indexed.entry
        let attributes = CSSearchableItemAttributeSet(contentType: .text)
        attributes.title = entry.title
        attributes.displayName = entry.title
        if !entry.titleOnly {
            attributes.contentDescription =
                [entry.description, entry.collection.isEmpty ? nil : entry.collection]
                .compactMap { $0 }.joined(separator: " · ")
            attributes.keywords = entry.keywords
            attributes.kind = entry.itemKind ?? Self.kind(entry.kind)
            attributes.contentCreationDate = entry.documentDate.flatMap(Self.date)
            attributes.addedDate = entry.added.flatMap(Self.date)
        }
        return CSSearchableItem(
            uniqueIdentifier: indexed.identifier, domainIdentifier: indexed.domain,
            attributeSet: attributes)
    }

    private static func kind(_ kind: SpotlightKind) -> String {
        switch kind {
        case .item: "Carrel item"
        case .collection: "Carrel collection"
        case .fact: "Carrel glossary fact"
        }
    }

    /// A card's date: `2026-10-29`, or a full timestamp.
    private static func date(_ text: String) -> Date? {
        if let date = try? Date(text, strategy: .iso8601) { return date }
        return try? Date(String(text.prefix(10)), strategy: .iso8601.year().month().day())
    }
}
#endif
