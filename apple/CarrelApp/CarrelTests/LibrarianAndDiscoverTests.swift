// The Librarian panel and Discover, each test against its own temporary copy of the sample
// archive with a configuration folder of its own (`.isolated` options), and the librarian on a
// scripted model written in Swift: no real model, and nothing outside the copy is written.

import CarrelKit
import XCTest

@testable import Carrel

/// The sample's note on coupled oscillators, in Reading, made on 2024-03-01.
let sampleNote = "01a18701-0500-776a-9867-0805d715b667"
let sampleNoteCard = "Reading/Coupled oscillators note.pdf.md"

/// A model that gives the librarian's answers from a script, one per request, and records
/// what it was asked.
final class ScriptedLibrarianModel: ModelProvider, @unchecked Sendable {
    private let lock = NSLock()
    private var script: [String]
    private(set) var asked: [ModelRequest] = []

    init(_ script: [[String: Any]]) {
        self.script = script.map {
            String(decoding: try! JSONSerialization.data(withJSONObject: $0), as: UTF8.self)
        }
    }

    func complete(request: ModelRequest) throws -> ModelAnswer {
        lock.lock()
        defer { lock.unlock() }
        asked.append(request)
        guard !script.isEmpty else {
            throw CarrelError.Callback(message: "the script has run out")
        }
        return ModelAnswer(text: script.removeFirst(), model: "swift/scripted")
    }

    /// The librarian asking to run `tool` with `input`.
    static func tool(_ tool: String, _ input: String) -> [String: Any] {
        [
            "action": tool, "input": input, "answer": "", "items": [], "passages": [],
            "suggestions": [],
        ]
    }

    /// The librarian answering, with the items it names and the changes it suggests.
    static func answer(
        _ text: String, items: [String] = [], suggestions: [[String: Any]] = []
    ) -> [String: Any] {
        [
            "action": "answer", "input": "", "answer": text, "items": items, "passages": [],
            "suggestions": suggestions,
        ]
    }
}

@MainActor
final class LibrarianTests: XCTestCase {
    private var sample: SampleWindow!

    override func setUp() async throws {
        sample = try await SampleWindow()
    }

    override func tearDown() async throws {
        sample.remove()
    }

    private func librarian(_ script: [[String: Any]]) -> (LibrarianModel, ScriptedLibrarianModel) {
        let model = ScriptedLibrarianModel(script)
        return (LibrarianModel(archive: sample.archive, provider: model, memoryGB: 16), model)
    }

    func testAnswersWithTheItemsItFoundAndTheModelThatAnswered() async throws {
        let (librarian, model) = librarian([
            ScriptedLibrarianModel.tool("search", "oscillators"),
            ScriptedLibrarianModel.answer(
                "You have a note on coupled oscillators.", items: [sampleNote, "Nowhere/at-all.md"]),
        ])
        librarian.draft = "What do I have about oscillators?"
        XCTAssertTrue(librarian.canAsk)
        await librarian.ask()
        XCTAssertNil(librarian.problem)
        XCTAssertEqual(librarian.answers.count, 1)
        let answer = try XCTUnwrap(librarian.answers.first)
        XCTAssertEqual(answer.question, "What do I have about oscillators?")
        XCTAssertEqual(answer.items.map(\.id), [sampleNote], "an item that isn't there is dropped")
        XCTAssertEqual(answer.items.first?.card, sampleNoteCard)
        XCTAssertEqual(answer.steps.map(\.tool), ["search"])
        XCTAssertFalse(answer.model.isEmpty)
        XCTAssertEqual(librarian.modelLabel, answer.model)
        XCTAssertEqual(librarian.draft, "", "the question went")
        XCTAssertFalse(librarian.isAsking)
        XCTAssertNil(librarian.progress)
        XCTAssertEqual(model.asked.count, 2)
    }

    func testAConversationGoesOnAndStartsAgain() async throws {
        let (librarian, model) = librarian([
            ScriptedLibrarianModel.answer("Two notes."),
            ScriptedLibrarianModel.answer("The newer one."),
        ])
        librarian.draft = "How many notes?"
        await librarian.ask()
        librarian.draft = "Which is newer?"
        await librarian.ask()
        XCTAssertEqual(librarian.answers.map(\.text), ["Two notes.", "The newer one."])
        XCTAssertTrue(
            model.asked[1].prompt.contains("How many notes?"), "the second question sees the first")
        librarian.clear()
        XCTAssertTrue(librarian.answers.isEmpty)
    }

    func testAFailedQuestionIsKeptToAskAgain() async throws {
        let (librarian, _) = librarian([])
        librarian.draft = "Anything?"
        await librarian.ask()
        XCTAssertNotNil(librarian.problem)
        XCTAssertEqual(librarian.draft, "Anything?")
        XCTAssertTrue(librarian.answers.isEmpty)
        XCTAssertNil(sample.archive.lastError, "the panel says it, not an alert")
    }

    func testSaysWhichModelAnswers() async throws {
        let (librarian, _) = librarian([])
        await librarian.refresh()
        XCTAssertNotNil(librarian.modelLabel)
        XCTAssertNil(try XCTUnwrap(librarian.modelChoice).problem)
    }

    func testWithNoModelItOffersOneToDownloadWithItsSize() async throws {
        // The sample's window starts no provider programs, so this Mac has no text model.
        let librarian = LibrarianModel(archive: sample.archive, memoryGB: 8)
        await librarian.refresh()
        let choice = try XCTUnwrap(librarian.modelChoice)
        XCTAssertNil(choice.label)
        XCTAssertNil(librarian.modelLabel)
        let download = try XCTUnwrap(choice.download)
        XCTAssertEqual(download.name, "qwen3.5-4b")
        XCTAssertGreaterThan(download.size, 0)
        let problem = try XCTUnwrap(choice.problem)
        XCTAssertTrue(problem.contains("needs a text model"), problem)
        librarian.draft = "Anything?"
        await librarian.ask()
        XCTAssertEqual(librarian.problem, problem, "asking says the same")
    }

    func testASettingNamingAModelThatsGoneIsSaidAndCleared() async throws {
        let config = sample.folder.appending(path: "config")
        try FileManager.default.createDirectory(at: config, withIntermediateDirectories: true)
        try Data(#"{"librarian_model": "llama/gone-7b"}"#.utf8)
            .write(to: config.appending(path: "cli.json"))
        let librarian = LibrarianModel(archive: sample.archive, memoryGB: 16)
        await librarian.refresh()
        let choice = try XCTUnwrap(librarian.modelChoice)
        XCTAssertEqual(choice.gone, "llama/gone-7b")
        XCTAssertTrue(choice.problem?.contains("llama/gone-7b") == true)
        await librarian.useStrongest()
        XCTAssertNil(librarian.modelChoice?.gone, "the setting is cleared")
        XCTAssertNotNil(librarian.modelChoice?.download, "and there's a model to download")
    }

    func testAskAboutThisOpensThePanelWithTheItem() throws {
        let librarian = try XCTUnwrap(sample.model.librarian)
        let item = try XCTUnwrap(sample.model.items.first { $0.id == sampleNote })
        XCTAssertFalse(librarian.isShown)
        librarian.ask(about: item)
        XCTAssertTrue(librarian.isShown)
        XCTAssertTrue(librarian.draft.contains(item.title))
        XCTAssertTrue(librarian.draft.contains(sampleNoteCard))
        XCTAssertEqual(librarian.focusRequests, 1)
    }

    func testSavesTheConversationToTheJournal() async throws {
        let (librarian, _) = librarian([
            ScriptedLibrarianModel.answer("The note.", items: [sampleNote])
        ])
        await librarian.save()
        XCTAssertNil(librarian.savedTo, "nothing to save yet")
        librarian.draft = "Oscillators?"
        await librarian.ask()
        await librarian.save()
        XCTAssertNil(librarian.problem)
        let path = try XCTUnwrap(librarian.savedTo)
        XCTAssertTrue(path.hasPrefix("Journal/"))
        let text = try String(contentsOf: sample.root.appending(path: path), encoding: .utf8)
        XCTAssertTrue(text.contains("Oscillators?"))
        XCTAssertTrue(text.contains("The note."))
    }

    func testWithChangesOffASuggestionOpensTheCorrectionSheetWorkedOut() async throws {
        let (librarian, _) = librarian([
            ScriptedLibrarianModel.answer(
                "It's a project note, really.",
                suggestions: [
                    [
                        "change": "refile", "item": sampleNote, "field": "",
                        "value": "Projects/Hardware", "why": "It's about the hardware project.",
                        "confidence": 0.95,
                    ]
                ])
        ])
        librarian.draft = "Is the note filed right?"
        await librarian.ask()
        let suggestion = try XCTUnwrap(librarian.answers.first?.suggestions.first)
        XCTAssertTrue(suggestion.isProposed, "the librarian only suggests by default")
        guard case .refile(let item, let to) = suggestion.change else {
            return XCTFail("a refile: \(suggestion.change)")
        }
        XCTAssertEqual(item.id, sampleNote)
        XCTAssertEqual(to, "Projects/Hardware")

        await sample.model.confirm(suggestion)
        let correction = try XCTUnwrap(sample.model.correction)
        XCTAssertEqual(correction.words, "collection: Projects/Hardware")
        XCTAssertEqual(correction.changes?.movedTo, "Projects/Hardware", "worked out, to confirm")
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: sample.root.appending(
                    path: "Projects/Hardware/Coupled oscillators note.pdf"
                )
                .path(percentEncoded: false)),
            "nothing moves until it's confirmed")
    }

    func testRenameAndCollectionSuggestionsOpenTheirSheets() async throws {
        let rename = LibrarianSuggestion(
            change: .rename(
                item: LibrarianItem(
                    id: sampleNote, title: "A note on coupled oscillators", card: sampleNoteCard,
                    collection: "Reading", sensitivity: .public),
                name: "oscillators-note.pdf"),
            description: "rename it", why: "", confidence: 0.5,
            outcome: .proposed(reason: "off"))
        await sample.model.confirm(rename)
        XCTAssertEqual(sample.model.browse?.renaming?.name, "oscillators-note.pdf")
        XCTAssertEqual(sample.model.browse?.renaming?.item, sampleNote)

        let collection = LibrarianSuggestion(
            change: .collection(path: "Reading/Physics", description: "Notes on physics"),
            description: "create it", why: "", confidence: 0.5,
            outcome: .proposed(reason: "off"))
        await sample.model.confirm(collection)
        XCTAssertEqual(sample.model.browse?.creating?.path, "Reading/Physics")
        XCTAssertEqual(sample.model.browse?.creating?.description, "Notes on physics")
    }

    func testWithChangesOnItsChangesAreListedAndUndoneTogether() async throws {
        try await sample.archive.set("librarian.changes", to: "on")
        let (librarian, _) = librarian([
            ScriptedLibrarianModel.answer(
                "Summarized.",
                suggestions: [
                    [
                        "change": "correct", "item": sampleNote, "field": "summary",
                        "value": "The locking condition for two coupled oscillators.",
                        "why": "Says what it's about.", "confidence": 0.97,
                    ]
                ])
        ])
        var changed = 0
        librarian.didChange = { changed += 1 }
        await librarian.refresh()
        XCTAssertTrue(librarian.makesChanges)
        XCTAssertTrue(librarian.changes.isEmpty)

        librarian.draft = "Give the note a better summary."
        await librarian.ask()
        let suggestion = try XCTUnwrap(librarian.answers.first?.suggestions.first)
        XCTAssertTrue(suggestion.isMade, "\(suggestion.outcome)")
        XCTAssertEqual(librarian.changes.count, 1)
        XCTAssertEqual(librarian.changes.first?.agent, "librarian")
        XCTAssertEqual(changed, 1, "the window follows the change")

        await librarian.undoAll()
        XCTAssertNil(librarian.problem)
        XCTAssertTrue(librarian.changes.isEmpty)
        let card = try String(
            contentsOf: sample.root.appending(path: sampleNoteCard), encoding: .utf8)
        XCTAssertTrue(card.contains("An invented two-page note"), "the summary is back")
    }

    func testReportsProgressAsItWorks() async throws {
        let (librarian, _) = librarian([
            ScriptedLibrarianModel.tool("search", "oscillators"),
            ScriptedLibrarianModel.answer("The note."),
        ])
        var seen: [LibrarianProgress] = []
        let watching = Task { @MainActor in
            while !Task.isCancelled {
                if let progress = librarian.progress, progress != seen.last {
                    seen.append(progress)
                }
                await Task.yield()
            }
        }
        librarian.draft = "Oscillators?"
        await librarian.ask()
        watching.cancel()
        XCTAssertTrue(librarian.answers.count == 1)
        XCTAssertTrue(seen.contains { $0.step == 0 }, "it says it's thinking at once")
        XCTAssertNil(librarian.progress, "and stops once it has answered")
    }
}

@MainActor
final class DiscoverTests: XCTestCase {
    private var sample: SampleWindow!
    private var discover: DiscoverModel { sample.model.discover! }

    override func setUp() async throws {
        sample = try await SampleWindow()
    }

    override func tearDown() async throws {
        sample.remove()
    }

    func testDiscoverIsInTheSidebar() {
        sample.model.screen = .discover(.wander)
        XCTAssertEqual(sample.model.screenTitle, "Wander")
    }

    func testOnThisDayLooksBackFromTheDayChosen() async throws {
        discover.day = try XCTUnwrap(
            Calendar.current.date(from: DateComponents(year: 2026, month: 3, day: 1, hour: 12)))
        await discover.refreshToday()
        XCTAssertNil(discover.problem)
        let day = try XCTUnwrap(discover.onThisDay)
        XCTAssertEqual(day.date, "2026-03-01")
        let year = try XCTUnwrap(day.years.first { $0.year == 2024 })
        XCTAssertEqual(year.yearsAgo, 2)
        XCTAssertTrue(year.items.contains { $0.item.id == sampleNote && $0.what == ["created"] })
    }

    func testWandersOneItemAtATimeAndRemembersWhatItOpened() async throws {
        await discover.startWalk(from: sampleNote, seed: 7)
        XCTAssertNil(discover.problem)
        let first = try XCTUnwrap(discover.currentStep)
        XCTAssertEqual(first.number, 1)
        XCTAssertNotEqual(first.item.id, sampleNote, "it moves on from where it starts")
        XCTAssertFalse(first.reason.words.isEmpty)
        await discover.step()
        XCTAssertEqual(discover.steps.map(\.number), [1, 2])
        XCTAssertNotEqual(discover.steps[1].item.id, first.item.id)
        let visits = sample.folder.appending(path: "config/discover")
        let saved = try FileManager.default.contentsOfDirectory(atPath: visits.path())
        XCTAssertEqual(saved.count, 1, "the walk and what it opened, in the tests' own folder")
    }

    func testAWalkWithAnUnknownStartSaysSo() async throws {
        await discover.startWalk(from: "no-such-item")
        XCTAssertNotNil(discover.problem)
        XCTAssertNil(discover.currentStep)
    }

    func testTheTimelineGroupsAndNarrows() async throws {
        discover.timelineGrouping = .year
        await discover.refreshTimeline()
        let all = try XCTUnwrap(discover.timeline)
        XCTAssertFalse(all.periods.isEmpty)
        XCTAssertTrue(all.periods.allSatisfy { $0.period.count == 4 })
        let placed = all.periods.flatMap(\.items)
        XCTAssertTrue(placed.contains { $0.item.id == sampleNote && $0.date == "2024-03-01" })

        discover.timelineQuery = "oscillators"
        await discover.refreshTimeline()
        let narrowed = try XCTUnwrap(discover.timeline).periods.flatMap(\.items)
        XCTAssertTrue(narrowed.contains { $0.item.id == sampleNote })
        XCTAssertLessThan(narrowed.count, placed.count)

        discover.timelineDate = .filed
        discover.timelineGrouping = .day
        discover.timelineQuery = ""
        await discover.refreshTimeline()
        XCTAssertTrue(
            try XCTUnwrap(discover.timeline).periods.flatMap(\.items).allSatisfy {
                $0.what == "filed"
            })
    }

    func testTheMapIsAFlatTreeForTheTreemap() async throws {
        await discover.refresh(.map)
        let map = try XCTUnwrap(discover.map)
        XCTAssertEqual(
            map.totalItems, UInt64(sample.model.items.count { $0.kind != .tombstone }),
            "every item but removed ones")
        let top = map.regions(in: nil)
        XCTAssertTrue(top.contains { $0.path == "Reading" })
        let hardware = try XCTUnwrap(map.regions.first { $0.path == "Projects/Hardware" })
        XCTAssertEqual(hardware.parent, "Projects")
        XCTAssertEqual(hardware.depth, 1)
        XCTAssertEqual(map.totalItems, top.reduce(0) { $0 + $1.totalItems })
    }

    func testActivityFillsWholeWeeks() async throws {
        await discover.refresh(.activity)
        let activity = try XCTUnwrap(discover.activity)
        let weeks = HeatmapWeeks(activity: activity)
        XCTAssertGreaterThanOrEqual(weeks.columns.count, 52)
        XCTAssertTrue(weeks.columns.allSatisfy { $0.count == 7 })
        XCTAssertTrue(activity.days.allSatisfy { (0...4).contains($0.level) })

        let october = try sample.archive.handle.activity(from: "2026-10-01", to: "2026-10-31")
        XCTAssertTrue(october.days.contains { $0.date == "2026-10-20" && $0.filed > 0 })
        let busiest = try XCTUnwrap(
            october.days.max { $0.filed + $0.sessions < $1.filed + $1.sessions })
        XCTAssertEqual(busiest.level, 4)
        XCTAssertThrowsError(
            try sample.archive.handle.activity(from: "2026-10-31", to: "2026-10-01"))
    }

    func testChoosingAnItemRecordsItsCollectionAsOpened() async throws {
        sample.model.screen = .collection("Reading")
        sample.model.selectedItemID = sampleNote
        let file = sample.folder.appending(path: "config/discover")
        for _ in 0..<50 where !FileManager.default.fileExists(atPath: file.path()) {
            try await Task.sleep(for: .milliseconds(20))
        }
        let saved = try FileManager.default.contentsOfDirectory(
            at: file, includingPropertiesForKeys: nil)
        let text = try String(contentsOf: try XCTUnwrap(saved.first), encoding: .utf8)
        XCTAssertTrue(text.contains("\"Reading\""))
    }

    func testResurfacingReadsWithoutFailing() async throws {
        await discover.refreshResurfaced()
        XCTAssertNil(discover.problem)
    }
}

@MainActor
final class TreemapTests: XCTestCase {
    func testTilesFillTheSpaceInProportion() {
        let rect = CGRect(x: 0, y: 0, width: 600, height: 400)
        let values: [(id: String, value: Double)] = [
            ("a", 6), ("b", 6), ("c", 4), ("d", 3), ("e", 2), ("f", 2), ("g", 1), ("none", 0),
        ]
        let tiles = Treemap.layout(values, in: rect)
        XCTAssertEqual(tiles.map(\.id).sorted(), ["a", "b", "c", "d", "e", "f", "g"])
        let total = values.reduce(0) { $0 + $1.value }
        for tile in tiles {
            let value = values.first { $0.id == tile.id }!.value
            let area = tile.rect.width * tile.rect.height
            XCTAssertEqual(area, rect.width * rect.height * value / total, accuracy: 1, tile.id)
            XCTAssertTrue(rect.insetBy(dx: -0.01, dy: -0.01).contains(tile.rect), tile.id)
        }
        for (i, a) in tiles.enumerated() {
            for b in tiles[(i + 1)...] {
                XCTAssertLessThan(
                    a.rect.intersection(b.rect).width * a.rect.intersection(b.rect).height, 0.01,
                    "\(a.id) and \(b.id) overlap")
            }
        }
    }

    func testNothingToLayOut() {
        XCTAssertTrue(
            Treemap.layout([("a", 0)], in: CGRect(x: 0, y: 0, width: 10, height: 10)).isEmpty)
        XCTAssertTrue(Treemap.layout([("a", 1)], in: .zero).isEmpty)
    }
}
