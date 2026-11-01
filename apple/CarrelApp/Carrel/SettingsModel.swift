// What Settings shows and changes (docs/design/mac-app.md, "Settings"): this device's
// preferences and the archive's shared settings, the models, the providers and the file types
// this device couldn't read, and the embedding index. Everything goes through the core, as
// `carrel settings`, `models`, `providers`, `formats` and `index` do.

import CarrelKit
import Foundation
import Observation

@Observable
final class SettingsModel {
    /// The archive whose window was last in front.
    let url: URL
    private let options: OpenOptions?

    private(set) var archive: ArchiveModel?
    /// This device's file picker and editor.
    private(set) var device: DeviceSettings?
    /// This device's choice of text models, and what's in use.
    private(set) var choices: ModelChoices?
    /// Carrel's model catalog, with what's downloaded here.
    private(set) var catalog: [CatalogModel] = []
    /// The formats each downloadable catalog model comes in, and which are here, by name.
    private(set) var modelFormats: [String: ModelFormats] = [:]
    /// Where backups stand, for Settings → Backups.
    private(set) var backups: BackupsState?
    /// The providers on this device.
    private(set) var providers: [ProviderInfo] = []
    /// The file types this device reads, and those it couldn't.
    private(set) var formats: Formats?
    /// The embedding models to switch to.
    private(set) var embedding: EmbeddingModels?
    private(set) var isLoading = false

    /// The last change to a shared setting, which Undo takes back.
    private(set) var lastChange: SettingChanged?
    /// What the last change did, in words.
    private(set) var note: String?
    /// What went wrong, in the core's words.
    var problem: String?

    /// A model waiting for the owner to confirm downloading it, having seen its size.
    var confirmingDownload: CatalogModel?
    /// A model waiting for the owner to confirm deleting it.
    var confirmingRemoval: CatalogModel?
    /// The format of the model waiting to be downloaded or deleted.
    var confirmingFormat: ModelFormat = .gguf
    /// An embedding model waiting for the owner to confirm switching to it.
    var confirmingSwitch: String?

    init(url: URL, options: OpenOptions? = nil) {
        self.url = url
        self.options = options
    }

    /// The archive's shared settings, but the embedding model, which Search switches.
    var shared: [Setting] {
        (archive?.settings ?? []).filter { $0.key != Self.embeddingKey }
    }

    static let embeddingKey = "embedding.model"

    /// The long operation under way, such as a download or a rebuild.
    var progress: OperationProgress? { archive?.progress }

    var isWorking: Bool { archive?.isWorking ?? false }

    /// The index, once read.
    var index: IndexStatus? { archive?.index }

    // MARK: - Reading

    /// Opens the archive and reads everything Settings shows, off the main thread.
    func load() async {
        isLoading = true
        defer { isLoading = false }
        let url = url
        let options = options
        do {
            let handle = try await Task.detached {
                if let options {
                    try ArchiveHandle.open(url, options: options)
                } else {
                    try ArchiveHandle.open(url)
                }
            }.value
            let archive = ArchiveModel(handle: handle)
            self.archive = archive
            try await archive.refreshSettings()
            await refreshDevice()
            try await archive.refreshIndex()
        } catch {
            problem = message(for: error)
        }
    }

    /// Reads this device's preferences, models, providers and file types again.
    func refreshDevice() async {
        guard let archive else { return }
        do {
            device = try await archive.call { try $0.deviceSettings() }
            choices = try await archive.call { try $0.modelChoices() }
            catalog = try await archive.call { $0.modelCatalog() }
            let formats = try await archive.call { $0.modelFormats() }
            modelFormats = Dictionary(formats.map { ($0.name, $0) }) { first, _ in first }
            backups = try? await archive.backups()
            providers = try await archive.call { $0.providers() }
            formats = try await archive.call { $0.formats() }
            embedding = try await archive.call { $0.embeddingModels() }
        } catch {
            problem = message(for: error)
        }
    }

    // MARK: - Settings

    /// Sets the shared setting `key` to `value`, as one change Undo takes back.
    func set(_ key: String, to value: String) async {
        await perform {
            let changed = try await $0.set(key, to: value)
            let title = archive?.settings.first { $0.key == key }?.title ?? key
            if changed.commit != nil {
                lastChange = changed
                note = "Set \(title) to \(changed.to) (was \(changed.from)) for every device."
            } else {
                note = "\(title) is already \(changed.to)."
            }
        }
    }

    /// Undoes the last change to a shared setting, as a new change in History.
    func undoLastChange() async {
        guard let commit = lastChange?.commit else { return }
        await perform {
            try await $0.undo(commit)
            lastChange = nil
            note = "Undone."
        }
    }

    /// Sets this device's `picker`, `editor` or `librarian.model`; empty lets Carrel choose.
    func setDevice(_ key: String, to value: String) async {
        let value = value.trimmingCharacters(in: .whitespaces)
        await perform {
            device = try await $0.setDevice(key, to: value.isEmpty ? nil : value)
            note =
                value.isEmpty
                ? "Cleared \(key); Carrel chooses for itself." : "Set \(key) on this Mac."
        }
    }

    /// Chooses the model that drafts cards (`nil` lets Carrel choose) and the one asked for a
    /// second opinion (`off`, or `nil` for a recommended one).
    func chooseModels(drafting: String?, secondOpinion: String?) async {
        await perform {
            choices = try await $0.chooseModels(drafting: drafting, secondOpinion: secondOpinion)
        }
    }

    // MARK: - Models

    /// Downloads the model the owner confirmed.
    func downloadConfirmed() async {
        guard let model = confirmingDownload else { return }
        confirmingDownload = nil
        let format = confirmingFormat
        await perform {
            try await $0.download(model: model.name, in: format)
            note = "Downloaded \(model.name) (\(format.title))."
        }
        await refreshDevice()
    }

    /// Deletes the model the owner confirmed.
    func removeConfirmed() async {
        guard let model = confirmingRemoval else { return }
        confirmingRemoval = nil
        let format = confirmingFormat
        await perform {
            try await $0.remove(model: model.name, in: format)
            note = "Deleted \(model.name) (\(format.title))."
        }
        await refreshDevice()
    }

    // MARK: - The embedding index

    /// Embeds every item again with the archive's embedding model.
    func rebuildIndex() async {
        await perform {
            let built = try await $0.rebuildIndex()
            note = "Embedded \(built.embedded) items with \(built.model)."
        }
    }

    /// Switches to the embedding model the owner confirmed, or carries on with the switch
    /// under way.
    func switchConfirmed() async {
        guard let model = confirmingSwitch else { return }
        confirmingSwitch = nil
        await switchIndex(to: model)
    }

    /// Carries on with the switch under way.
    func carryOnSwitch() async {
        guard let to = index?.switch?.to else { return }
        await switchIndex(to: to)
    }

    /// Stops the switch under way; the archive keeps its model.
    func stopSwitch() async {
        guard let model = index?.model else { return }
        await switchIndex(to: model)
    }

    private func switchIndex(to model: String) async {
        await perform {
            switch try await $0.switchIndex(to: model) {
            case .already:
                note = "This archive is already searched with \(model)."
            case .stopped(let to):
                note = "Stopped switching to \(to); the archive keeps \(model)."
            case .done(_, _, let built):
                note =
                    "Embedded \(built.embedded) items with \(model). This archive is now searched with \(model), on every device that has it."
            }
        }
    }

    // MARK: -

    /// Runs `work` on the archive, keeping its error as `problem`.
    private func perform(_ work: (ArchiveModel) async throws -> Void) async {
        guard let archive else { return }
        problem = nil
        do {
            try await work(archive)
        } catch {
            problem = message(for: error)
        }
    }
}
