import CarrelKit
import Foundation
import Testing

/// The sample archive at the repository root's fixtures/.
let sampleArchive = URL(filePath: #filePath)
    .deletingLastPathComponent()
    .appending(path: "../../../../fixtures/sample-archive")
    .standardizedFileURL

/// A model written in Swift, called from Rust.
final class RecordingModel: TextModel, @unchecked Sendable {
    private(set) var prompts: [String] = []

    func complete(prompt: String) throws -> String {
        prompts.append(prompt)
        return "  An archive of \(prompt.split(separator: "\n").count) lines.  "
    }
}

final class UnavailableModel: TextModel {
    func complete(prompt: String) throws -> String {
        throw CarrelError.Callback(message: "model offline")
    }
}

@Test func versionNamesTheArchiveFormat() {
    #expect(versionSummary().contains("archive format 1.0"))
}

@Test func opensAndListsTheSampleArchive() throws {
    let archive = try ArchiveHandle.open(sampleArchive.appending(path: "Reading"))
    #expect(archive.name() == "Sample archive")
    #expect(!archive.isReadOnly())
    #expect(try archive.collections().contains { $0.path == "Projects/Hardware" })
    let items = try archive.items()
    #expect(items.count == 12)
    let book = try #require(items.first { $0.card == "Reading/On the Origin of Species.md" })
    #expect(book.kind == .physical)
    #expect(book.item == nil)
}

@Test func foldersOutsideArchivesThrow() {
    #expect(throws: CarrelError.self) {
        try ArchiveHandle.open(FileManager.default.temporaryDirectory)
    }
}

@Test func rustCallsASwiftModel() throws {
    let archive = try ArchiveHandle.open(sampleArchive)
    let model = RecordingModel()
    let answer = try archive.describe(model: model)
    #expect(answer.hasPrefix("An archive of "))
    #expect(!answer.hasSuffix(" "), "Rust trims the answer")
    #expect(model.prompts.count == 1)
    #expect(model.prompts[0].contains("Archive: Sample archive"))
    #expect(model.prompts[0].contains("On the Origin of Species"))
}

@Test func swiftModelErrorsReachTheCaller() throws {
    let archive = try ArchiveHandle.open(sampleArchive)
    #expect(throws: CarrelError.Callback(message: "model offline")) {
        try archive.describe(model: UnavailableModel())
    }
}
