// App Intents for Shortcuts, Siri and Apple Intelligence (docs/design/macos-integration.md, "App
// Intents"). They run in the agent, through the same requests the extensions send, so they work
// with Carrel's window closed. Anything that needs a decision, such as approving a return, opens
// Carrel's sheet for it.

import AppIntents
import CarrelKit
import Foundation

/// Sends `request` to the agent in this process; a refusal or failure becomes the intent's error,
/// with the agent's reason.
@MainActor
private func answer(_ request: AgentRequest) async throws -> AgentReply {
    guard let host = AgentHost.current else {
        throw CarrelIntentError(AgentReply.notRunning.message)
    }
    let reply = await host.answer(request)
    guard reply.outcome == .done else { throw CarrelIntentError(reply.message) }
    return reply
}

/// Why an intent couldn't run, as the agent said.
struct CarrelIntentError: Error, CustomLocalizedStringResourceConvertible {
    let reason: String

    init(_ reason: String) {
        self.reason = reason
    }

    var localizedStringResource: LocalizedStringResource { "\(reason)" }
}

// MARK: - Entities

/// An item in the archive.
struct ItemEntity: AppEntity, IndexedEntity {
    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Archive Item"
    static let defaultQuery = ItemQuery()

    let id: String
    @Property(title: "Title") var title: String
    @Property(title: "Collection") var collection: String?

    init(_ item: AgentFoundItem) {
        id = item.id
        title = item.title
        collection = item.collection
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(title)", subtitle: collection.map { "\($0)" })
    }
}

/// Finds items by identifier, and by words as search does.
struct ItemQuery: EntityStringQuery {
    func entities(for identifiers: [String]) async throws -> [ItemEntity] {
        try await answer(.items(archive: nil, ids: identifiers)).items.map(ItemEntity.init)
    }

    func entities(matching string: String) async throws -> [ItemEntity] {
        try await answer(.search(archive: nil, query: string, limit: 20)).items.map(ItemEntity.init)
    }

    func suggestedEntities() async throws -> [ItemEntity] {
        try await answer(.active(archive: nil)).items.map(ItemEntity.init)
    }
}

/// A collection in the archive.
struct CollectionEntity: AppEntity, IndexedEntity {
    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Archive Collection"
    static let defaultQuery = CollectionQuery()

    /// Its folder, relative to the archive root.
    let id: String
    @Property(title: "Title") var title: String

    init(_ item: AgentFoundItem) {
        id = item.id
        title = item.title
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(title)", subtitle: "\(id)")
    }
}

struct CollectionQuery: EntityStringQuery {
    func entities(for identifiers: [String]) async throws -> [CollectionEntity] {
        try await suggestedEntities().filter { identifiers.contains($0.id) }
    }

    func entities(matching string: String) async throws -> [CollectionEntity] {
        try await suggestedEntities().filter {
            $0.title.localizedCaseInsensitiveContains(string)
                || $0.id.localizedCaseInsensitiveContains(string)
        }
    }

    func suggestedEntities() async throws -> [CollectionEntity] {
        try await answer(.collections(archive: nil)).items.map(CollectionEntity.init)
    }
}

// MARK: - Intents

struct AddToArchiveIntent: AppIntent {
    static let title: LocalizedStringResource = "Add to Archive"
    static let description = IntentDescription(
        "Adds files to the archive's intake queue, to be drafted and filed.")

    @Parameter(title: "Files") var files: [IntentFile]

    func perform() async throws -> some IntentResult & ProvidesDialog {
        var added = 0
        for file in files {
            let request: AgentRequest =
                if let url = file.fileURL {
                    .addFiles(
                        archive: nil, paths: [url.path(percentEncoded: false)], each: false,
                        whole: true)
                } else {
                    .addData(archive: nil, name: file.filename, data: file.data)
                }
            added += try await answer(request).items.count
        }
        return .result(dialog: "Added \(added) to the intake queue.")
    }
}

struct SearchArchiveIntent: AppIntent {
    static let title: LocalizedStringResource = "Search the Archive"
    static let description = IntentDescription("Finds items by their words and meaning.")

    @Parameter(title: "Words") var words: String

    func perform() async throws -> some IntentResult & ReturnsValue<[ItemEntity]> {
        let reply = try await answer(.search(archive: nil, query: words, limit: 20))
        return .result(value: reply.items.map(ItemEntity.init))
    }
}

struct AskLibrarianIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask the Librarian"
    static let description = IntentDescription(
        "Asks the librarian about the archive, returning its answer and the items it's about.")

    @Parameter(title: "Question") var question: String

    func perform() async throws -> some IntentResult & ReturnsValue<[ItemEntity]> & ProvidesDialog {
        let reply = try await answer(.ask(archive: nil, question: question))
        return .result(
            value: reply.items.map(ItemEntity.init), dialog: "\(reply.answer ?? reply.message)")
    }
}

struct CheckOutIntent: AppIntent {
    static let title: LocalizedStringResource = "Check Out"
    static let description = IntentDescription(
        "Checks an item out to the checkout folder, optionally due back on a date.")

    @Parameter(title: "Item") var item: ItemEntity
    @Parameter(title: "Due") var due: Date?

    func perform() async throws -> some IntentResult & ReturnsValue<String> & ProvidesDialog {
        let dueString = due.map {
            $0.formatted(.iso8601.year().month().day().dateSeparator(.dash))
        }
        let reply = try await answer(
            .checkOut(archive: nil, item: item.id, folder: nil, due: dueString))
        return .result(value: reply.working ?? "", dialog: "\(reply.message)")
    }
}

/// Returning shows what changed and asks, so it opens Carrel's return preview.
struct ReturnIntent: AppIntent {
    static let title: LocalizedStringResource = "Return"
    static let description = IntentDescription(
        "Opens Carrel's return preview for a checked-out item, to see what changed and return it.")

    @Parameter(title: "Item") var item: ItemEntity

    func perform() async throws -> some IntentResult {
        _ = try await answer(.open(archive: nil, item: item.id, action: .returnItem))
        return .result()
    }
}

struct WhatsCheckedOutIntent: AppIntent {
    static let title: LocalizedStringResource = "What’s Checked Out"
    static let description = IntentDescription("Lists what's checked out and on the desk.")

    func perform() async throws -> some IntentResult & ReturnsValue<[ItemEntity]> & ProvidesDialog {
        let reply = try await answer(.active(archive: nil))
        return .result(value: reply.items.map(ItemEntity.init), dialog: "\(reply.message)")
    }
}

struct TakeSnapshotIntent: AppIntent {
    static let title: LocalizedStringResource = "Take a Snapshot"
    static let description = IntentDescription("Backs up the whole archive now.")

    func perform() async throws -> some IntentResult & ProvidesDialog {
        .result(dialog: "\(try await answer(.snapshot(archive: nil)).message)")
    }
}

struct OpenInCarrelIntent: AppIntent {
    static let title: LocalizedStringResource = "Open in Carrel"
    static let description = IntentDescription("Shows the archive, or an item in it, in Carrel.")

    @Parameter(title: "Item") var item: ItemEntity?

    func perform() async throws -> some IntentResult {
        _ = try await answer(.open(archive: nil, item: item?.id))
        return .result()
    }
}

struct CarrelShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: SearchArchiveIntent(), phrases: ["Search \(.applicationName)"],
            shortTitle: "Search the Archive", systemImageName: "magnifyingglass")
        AppShortcut(
            intent: AskLibrarianIntent(), phrases: ["Ask \(.applicationName)"],
            shortTitle: "Ask the Librarian", systemImageName: "text.bubble")
        AppShortcut(
            intent: WhatsCheckedOutIntent(), phrases: ["What’s checked out in \(.applicationName)"],
            shortTitle: "What’s Checked Out", systemImageName: "arrow.up.forward.circle")
        AppShortcut(
            intent: TakeSnapshotIntent(), phrases: ["Take a snapshot in \(.applicationName)"],
            shortTitle: "Take a Snapshot", systemImageName: "externaldrive")
    }
}
