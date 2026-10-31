// Finding MLX models: each is a folder in `mlx/` inside Carrel's models folder, as
// `carrel models download NAME --format mlx` leaves it (docs/design/mlx.md).

import Foundation

/// An MLX model on disk: a folder with `config.json`, the tokenizer and the weights.
public struct ModelFolder: Sendable, Equatable {
    /// The folder's name, which is the model's offer, such as `qwen3.5-4b`.
    public var name: String
    public var url: URL
    /// The files' total size in bytes, about the memory the model needs once loaded.
    public var size: UInt64

    public init(name: String, url: URL, size: UInt64) {
        self.name = name
        self.url = url
        self.size = size
    }

    /// Carrel's models folder: `CARREL_MODELS_DIR` if set, otherwise `models` in Carrel's
    /// folder in Application Support, as `carrel_core::local::models_dir` has it.
    public static func modelsRoot(environment: [String: String] = ProcessInfo.processInfo.environment)
        -> URL?
    {
        if let dir = environment["CARREL_MODELS_DIR"], !dir.isEmpty {
            return URL(filePath: dir, directoryHint: .isDirectory)
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first?.appending(path: "Carrel/models", directoryHint: .isDirectory)
    }

    /// The models in `mlx/` inside `root`, by name. A folder without `config.json`, such as
    /// one still downloading, is left out.
    public static func discover(in root: URL) -> [ModelFolder] {
        let dir = root.appending(path: "mlx", directoryHint: .isDirectory)
        let fm = FileManager.default
        guard let names = try? fm.contentsOfDirectory(atPath: dir.path(percentEncoded: false)) else {
            return []
        }
        return names.sorted().compactMap { name in
            let url = dir.appending(path: name, directoryHint: .isDirectory)
            guard !name.hasPrefix("."),
                fm.fileExists(atPath: url.appending(path: "config.json").path(percentEncoded: false))
            else { return nil }
            return ModelFolder(name: name, url: url, size: Self.size(of: url))
        }
    }

    static func size(of url: URL) -> UInt64 {
        guard let files = FileManager.default.enumerator(
            at: url, includingPropertiesForKeys: [.fileSizeKey, .isRegularFileKey])
        else { return 0 }
        var total: UInt64 = 0
        for case let file as URL in files {
            let values = try? file.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey])
            if values?.isRegularFile == true {
                total += UInt64(values?.fileSize ?? 0)
            }
        }
        return total
    }

    /// Why the model can't run on a machine with `memory` bytes, or nil if it can: it needs
    /// twice its size, for the weights, the context and everything else running.
    public func tooBig(forMemory memory: UInt64) -> String? {
        guard size > 0, memory < size * 2 else { return nil }
        let gb = { (bytes: UInt64) in String(format: "%.0f GB", Double(bytes) / 1e9) }
        return "\(name) needs about \(gb(size * 2)) of memory; this Mac has \(gb(memory))"
    }
}
