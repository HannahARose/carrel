// Flags through ArchiveModel (FORMAT.md §5.9), against a temporary copy of the sample archive
// with a configuration folder of its own.

import CarrelKit
import Foundation
import Testing

@MainActor
@Suite struct Flags {
    @Test func anItemAndACollectionAreFlaggedListedAndCleared() async throws {
        let sample = try Sample()
        let model = sample.model
        try await model.refreshAll()
        let book = try #require(
            try model.handle.items().first { $0.card == "Reading/On the Origin of Species.md" })

        let flagged = try await model.flag(item: book.id, note: "wrong title")
        #expect(flagged.changed)
        #expect(try await model.flag(of: book.id)?.note == "wrong title")
        #expect(model.status?.flagged.map(\.path) == [book.card], "Status follows")
        #expect(model.history.first?.summary.contains("needing work") == true)

        try await model.flag(collection: "Projects", note: nil)
        #expect(model.status?.flagged.first?.isCollection == true)
        #expect(try model.handle.collections().first { $0.path == "Projects" }?.flag != nil)

        try await model.unflag(item: book.id)
        try await model.unflag(collection: "Projects")
        #expect(try await model.flag(of: book.id) == nil)
        #expect(model.status?.flagged.isEmpty == true)
    }
}
