// Settings, each test against its own temporary copy of the sample archive with a
// configuration folder (and so a models folder) of its own. Nothing is downloaded: models are
// stand-in files, and only the built-in provider runs.

import CarrelKit
import XCTest

@testable import Carrel

/// Settings for a temporary copy of the sample archive, with its history started.
@MainActor
struct SampleSettings {
    let folder: URL
    let model: SettingsModel

    var root: URL { folder.appending(path: "archive") }
    var config: URL { folder.appending(path: "config") }
    var models: URL { config.appending(path: "models") }

    /// Opens the copy, first letting `prepare` change it or the configuration folder.
    init(prepare: (_ root: URL, _ config: URL) throws -> Void = { _, _ in }) async throws {
        folder = FileManager.default.temporaryDirectory
            .appending(path: "carrel-settings-\(UUID().uuidString)")
        let root = folder.appending(path: "archive")
        let config = folder.appending(path: "config")
        try FileManager.default.createDirectory(at: config, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: sampleArchive, to: root)
        try prepare(root, config)
        let handle = try ArchiveHandle.open(root, options: .isolated(config: config))
        try handle.startHistory()
        model = SettingsModel(url: root, options: .isolated(config: config))
        await model.load()
        XCTAssertNil(model.problem)
    }

    func remove() {
        try? FileManager.default.removeItem(at: folder)
    }
}

@MainActor
final class SettingsTests: XCTestCase {
    func testASharedSettingChangesForEveryDeviceAndIsUndone() async throws {
        let sample = try await SampleSettings()
        defer { sample.remove() }
        let model = sample.model
        let originals = try XCTUnwrap(model.shared.first { $0.key == "ingest.originals" })
        XCTAssertEqual(originals.value, "move")
        XCTAssertEqual(originals.choices, ["move", "keep"])
        XCTAssertFalse(
            model.shared.contains { $0.key == SettingsModel.embeddingKey },
            "the embedding model is switched in Search")

        await model.set("ingest.originals", to: "keep")
        XCTAssertNil(model.problem)
        XCTAssertEqual(model.shared.first { $0.key == "ingest.originals" }?.value, "keep")
        XCTAssertNotNil(model.lastChange?.commit)
        let settings = try String(
            contentsOf: sample.root.appending(path: "_System/settings.json"), encoding: .utf8)
        XCTAssertTrue(settings.contains("keep"), settings)
        XCTAssertEqual(model.archive?.history.first?.summary.contains("ingest.originals"), true)

        await model.undoLastChange()
        XCTAssertNil(model.problem)
        XCTAssertNil(model.lastChange)
        XCTAssertEqual(model.shared.first { $0.key == "ingest.originals" }?.value, "move")
    }

    func testAWrongValueIsRefusedInTheCoresWords() async throws {
        let sample = try await SampleSettings()
        defer { sample.remove() }
        await sample.model.set("signals.crowded_items", to: "none")
        XCTAssertEqual(
            sample.model.problem, "signals.crowded_items is a whole number, at least 1")
        XCTAssertNil(sample.model.lastChange)
    }

    func testThisMacsPickerAndEditorAreSavedInItsOwnPreferences() async throws {
        let sample = try await SampleSettings()
        defer { sample.remove() }
        let model = sample.model
        XCTAssertNil(model.device?.editor)

        await model.setDevice("editor", to: "nvim")
        await model.setDevice("picker", to: "builtin")
        XCTAssertEqual(model.device?.editor, "nvim")
        XCTAssertEqual(model.device?.picker, "builtin")
        let prefs = try XCTUnwrap(
            JSONSerialization.jsonObject(
                with: Data(contentsOf: sample.config.appending(path: "cli.json")))
                as? [String: Any])
        XCTAssertEqual(prefs["editor"] as? String, "nvim")

        await model.setDevice("editor", to: " ")
        XCTAssertNil(model.device?.editor)
        XCTAssertEqual(model.note, "Cleared editor; Carrel chooses for itself.")
    }

    func testModelsShowTheirSizesAndLicensesAndOneIsDeletedOnceConfirmed() async throws {
        let sample = try await SampleSettings { _, config in
            let models = config.appending(path: "models")
            try FileManager.default.createDirectory(at: models, withIntermediateDirectories: true)
            try Data("stand-in".utf8).write(to: models.appending(path: "granite-4.2-3b.gguf"))
        }
        defer { sample.remove() }
        let model = sample.model
        let granite = try XCTUnwrap(model.catalog.first { $0.name == "granite-4.2-3b" })
        XCTAssertEqual(granite.state, .downloaded)
        XCTAssertGreaterThan(granite.size, 1_000_000)
        XCTAssertFalse(granite.license.isEmpty)
        XCTAssertGreaterThan(granite.memoryGb, 0)
        let other = try XCTUnwrap(model.catalog.first { $0.name == "qwen3.5-4b" })
        XCTAssertEqual(other.state, .notDownloaded)
        XCTAssertTrue(model.catalog.contains { $0.embedding })

        // Asking is all a download button does; nothing is fetched until it's confirmed.
        model.confirmingDownload = other
        model.confirmingDownload = nil
        await model.downloadConfirmed()
        XCTAssertNil(model.progress)

        model.confirmingRemoval = granite
        await model.removeConfirmed()
        XCTAssertNil(model.problem)
        XCTAssertNil(model.confirmingRemoval)
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: sample.models.appending(path: "granite-4.2-3b.gguf")
                    .path(percentEncoded: false)))
        XCTAssertEqual(model.catalog.first { $0.name == "granite-4.2-3b" }?.state, .notDownloaded)

        model.confirmingRemoval = granite
        await model.removeConfirmed()
        XCTAssertEqual(model.problem?.contains("granite-4.2-3b"), true)
    }

    func testTheProvidersAndTheFileTypesThisMacCouldntRead() async throws {
        let sample = try await SampleSettings { _, config in
            let log: [String: Any] = [
                "entries": [
                    [
                        "media_type": "application/x-7z-compressed", "extension": "7z",
                        "why": "no-reader", "count": 3, "first_seen": "2026-10-01",
                        "last_seen": "2026-10-30",
                    ]
                ]
            ]
            try JSONSerialization.data(withJSONObject: log)
                .write(to: config.appending(path: "unreadable.json"))
        }
        defer { sample.remove() }
        let model = sample.model
        XCTAssertEqual(model.providers.first?.name, "built-in")
        let formats = try XCTUnwrap(model.formats)
        XCTAssertTrue(formats.reads.contains { $0.mediaType == "text/plain" })
        let unread = try XCTUnwrap(formats.unread.first)
        XCTAssertEqual(unread.extension, "7z")
        XCTAssertEqual(unread.count, 3)
        XCTAssertEqual(unread.why, "no reader")
        XCTAssertNotNil(unread.hint)
    }

    func testTheIndexSaysWhatItUsesAndASwitchNeedsAProviderHere() async throws {
        let sample = try await SampleSettings()
        defer { sample.remove() }
        let model = sample.model
        let index = try XCTUnwrap(model.index)
        XCTAssertEqual(index.model, "sample/embedding-model-v1")
        XCTAssertFalse(index.exists)
        XCTAssertGreaterThan(index.items, 0)
        XCTAssertNil(index.switch)
        XCTAssertEqual(model.embedding?.here, [])
        XCTAssertTrue(
            model.embedding?.elsewhere.contains { $0.model == "qwen3-embedding-0.6b" } == true)

        model.confirmingSwitch = "qwen3-embedding-0.6b"
        await model.switchConfirmed()
        XCTAssertEqual(
            model.problem?.hasPrefix("no provider on this device offers qwen3-embedding-0.6b"),
            true, model.problem ?? "")
        XCTAssertNil(model.index?.switch)

        await model.rebuildIndex()
        XCTAssertNotNil(model.problem)
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: sample.root.appending(path: "_System/index").path(percentEncoded: false)),
            "nothing written")
    }

    func testASwitchUnderWayIsShownAndStopped() async throws {
        let sample = try await SampleSettings { root, _ in
            let file = root.appending(path: "_System/settings.json")
            var settings = try XCTUnwrap(
                JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
            settings["embedding"] = [
                "model": "sample/embedding-model-v1", "switching_to": "qwen3-embedding-0.6b",
            ]
            try JSONSerialization.data(withJSONObject: settings).write(to: file)
        }
        defer { sample.remove() }
        let model = sample.model
        let switching = try XCTUnwrap(model.index?.switch)
        XCTAssertEqual(switching.to, "qwen3-embedding-0.6b")
        XCTAssertEqual(switching.from, "sample/embedding-model-v1")
        XCTAssertEqual(switching.embedded, 0)
        XCTAssertGreaterThan(switching.behind, 0)

        await model.stopSwitch()
        XCTAssertNil(model.problem)
        XCTAssertNil(model.index?.switch)
        XCTAssertEqual(
            model.note,
            "Stopped switching to qwen3-embedding-0.6b; the archive keeps sample/embedding-model-v1."
        )
    }

    func testEverySharedSettingHasATitleAndTheRemindersTakeSeveral() async throws {
        let sample = try await SampleSettings()
        defer { sample.remove() }
        let model = sample.model
        for key in [
            "checkout.destination", "checkout.after_return", "sessions.idle_minutes", "reminders",
        ] {
            XCTAssertTrue(model.shared.contains { $0.key == key }, key)
        }
        XCTAssertFalse(model.shared.contains { $0.title == $0.key })
        XCTAssertEqual(
            model.shared.first { $0.key == "questions.skip_days" }?.title,
            "Skipped questions rest")
        let reminders = try XCTUnwrap(model.shared.first { $0.key == "reminders" })
        XCTAssertTrue(reminders.takesSeveral)
        XCTAssertEqual(reminders.value, "idle, weekly, menu-bar")

        await model.set("reminders", to: "idle, due-date")
        XCTAssertNil(model.problem)
        XCTAssertEqual(model.shared.first { $0.key == "reminders" }?.value, "idle, due-date")
        await model.set("sessions.idle_minutes", to: "0")
        XCTAssertEqual(model.problem, "sessions.idle_minutes is a whole number, at least 1")
    }

    func testThePickerAndEditorInUseAreShownWithWhy() async throws {
        let sample = try await SampleSettings()
        defer { sample.remove() }
        let model = sample.model
        XCTAssertTrue(model.device?.knownPickers.contains { $0.name == "yazi" } == true)
        await model.setDevice("picker", to: "builtin")
        XCTAssertEqual(model.device?.pickerInUse, "the built-in browser (chosen)")
        await model.setDevice("editor", to: "nvim")
        XCTAssertEqual(model.device?.editorInUse, "nvim (Carrel's editor setting)")
    }

    func testAModelIsDeletedInTheFormatChosen() async throws {
        let sample = try await SampleSettings { _, config in
            let models = config.appending(path: "models")
            try FileManager.default.createDirectory(
                at: models.appending(path: "mlx/granite-4.2-3b"), withIntermediateDirectories: true)
            try Data("stand-in".utf8).write(to: models.appending(path: "granite-4.2-3b.gguf"))
        }
        defer { sample.remove() }
        let model = sample.model
        let formats = try XCTUnwrap(model.modelFormats["granite-4.2-3b"])
        XCTAssertEqual(formats.here, [.gguf, .mlx])
        let granite = try XCTUnwrap(model.catalog.first { $0.name == "granite-4.2-3b" })

        model.confirmingFormat = .mlx
        model.confirmingRemoval = granite
        await model.removeConfirmed()
        XCTAssertNil(model.problem)
        XCTAssertEqual(model.modelFormats["granite-4.2-3b"]?.here, [.gguf])
        XCTAssertEqual(model.note, "Deleted granite-4.2-3b (MLX).")
    }
}
