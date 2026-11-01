// What Settings can read today, from the files Carrel keeps on this Mac and in the archive.
// The folders follow carrel-core's `local` module. CarrelKit's settings and models bindings
// (#233) replace this, and bring editing with them.

import CarrelKit
import Foundation

nonisolated struct LocalSettings: Equatable {
    /// `CARREL_CONFIG_DIR`, or `~/Library/Application Support/Carrel`.
    var configFolder: URL
    /// `CARREL_MODELS_DIR`, or `models` in the configuration folder.
    var modelsFolder: URL
    /// What's in the models folder.
    var installedModels: [String]
    /// The CLI's choice of drafting model (`carrel models use`), if any.
    var draftingModel: String?
    /// The model asked again when the first isn't sure, if chosen.
    var secondModel: String?
    /// The `kopia` program, if installed.
    var kopia: URL?
    var archive: ArchiveSettings?

    struct ArchiveSettings: Equatable {
        var name: String
        var embeddingModel: String?
        var backupsSetUp: Bool
    }

    static func read(
        archive: URL?, environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> LocalSettings {
        let files = FileManager.default
        let config =
            environment["CARREL_CONFIG_DIR"].map { URL(filePath: $0) }
            ?? homeFolder(files).appending(path: "Library/Application Support/Carrel")
        let models =
            environment["CARREL_MODELS_DIR"].map { URL(filePath: $0) }
            ?? config.appending(path: "models")
        let installed =
            ((try? files.contentsOfDirectory(atPath: models.path(percentEncoded: false))) ?? [])
            .filter { !$0.hasPrefix(".") }
            .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        let prefs = json(config.appending(path: "cli.json"))
        return LocalSettings(
            configFolder: config,
            modelsFolder: models,
            installedModels: installed,
            draftingModel: prefs?["model"] as? String,
            secondModel: prefs?["second_model"] as? String,
            kopia: program("kopia", environment: environment),
            archive: archive.flatMap { archiveSettings($0, config: config) })
    }

    private static func archiveSettings(_ url: URL, config: URL) -> ArchiveSettings? {
        guard let handle = try? ArchiveHandle.open(url) else { return nil }
        let root = URL(filePath: handle.root())
        let settings = json(root.appending(path: "_System/settings.json"))
        let embedding = settings?["embedding"] as? [String: Any]
        let backups = config.appending(path: "backups/\(handle.id()).config")
        return ArchiveSettings(
            name: handle.name(),
            embeddingModel: embedding?["model"] as? String,
            backupsSetUp: FileManager.default.fileExists(
                atPath: backups.path(percentEncoded: false)))
    }

    private static func json(_ url: URL) -> [String: Any]? {
        (try? Data(contentsOf: url))
            .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    }

    /// `name` on `PATH`, or where Homebrew installs it (apps don't get the shell's `PATH`).
    private static func program(_ name: String, environment: [String: String]) -> URL? {
        let path = (environment["PATH"] ?? "").split(separator: ":").map(String.init)
        return (path + ["/opt/homebrew/bin", "/usr/local/bin"])
            .map { URL(filePath: $0).appending(path: name) }
            .first { FileManager.default.isExecutableFile(atPath: $0.path(percentEncoded: false)) }
    }
}

/// This person's home folder; on iPad and iPhone, the app's container.
nonisolated private func homeFolder(_ files: FileManager) -> URL {
    #if os(macOS)
    files.homeDirectoryForCurrentUser
    #else
    URL(filePath: NSHomeDirectory(), directoryHint: .isDirectory)
    #endif
}
