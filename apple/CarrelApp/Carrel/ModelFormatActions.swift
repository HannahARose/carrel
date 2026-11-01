// Settings → Models' Download… and Delete… for a model in either format: one GGUF file for
// llama.cpp, or a folder for MLX on Apple silicon, as `carrel models download --format` and
// `carrel models remove --format` offer. Each asks first.

import CarrelKit
import SwiftUI

extension ModelFormat {
    /// Its name, for people.
    var title: String {
        switch self {
        case .gguf: "GGUF"
        case .mlx: "MLX"
        }
    }

    /// Whether this Mac can run it: MLX needs Apple silicon.
    var runsHere: Bool {
        switch self {
        case .gguf: true
        case .mlx:
            #if arch(arm64)
            true
            #else
            false
            #endif
        }
    }
}

extension ModelFormats {
    /// The formats that are here.
    var here: [ModelFormat] {
        (ggufHere ? [.gguf] : []) + (mlxHere ? [.mlx] : [])
    }

    /// The formats that can be downloaded to this Mac and aren't here yet.
    var downloadable: [ModelFormat] {
        let offered: [ModelFormat] = [.gguf] + (mlxSize == nil ? [] : [.mlx])
        return offered.filter { $0.runsHere && !here.contains($0) }
    }
}

/// The Download… and Delete… buttons for a catalog model, a menu of formats when there's a
/// choice.
struct ModelFormatActions: View {
    let entry: CatalogModel
    let model: SettingsModel

    private var formats: ModelFormats? { model.modelFormats[entry.name] }

    var body: some View {
        HStack {
            let here = formats?.here ?? [.gguf]
            let missing = formats?.downloadable ?? []
            if !here.isEmpty { choice("Delete…", formats: here, delete: true) }
            if !missing.isEmpty && entry.downloadable {
                choice("Download…", formats: missing, delete: false)
            }
        }
    }

    @ViewBuilder
    private func choice(_ title: String, formats: [ModelFormat], delete: Bool) -> some View {
        if formats.count == 1, let format = formats.first {
            Button(formats == [.gguf] ? title : "\(title.dropLast()) \(format.title)…") {
                ask(format, delete: delete)
            }
        } else {
            Menu(title) {
                ForEach(formats, id: \.self) { format in
                    Button(label(format)) { ask(format, delete: delete) }
                }
            }
            .fixedSize()
        }
    }

    private func label(_ format: ModelFormat) -> String {
        let bytes = model.size(of: entry, in: format)
        return bytes > 0
            ? "\(format.title), \(bytes.formatted(.byteCount(style: .file)))" : format.title
    }

    private func ask(_ format: ModelFormat, delete: Bool) {
        model.confirmingFormat = format
        if delete { model.confirmingRemoval = entry } else { model.confirmingDownload = entry }
    }
}

extension SettingsModel {
    /// The size of `entry`'s download in `format`, in bytes.
    func size(of entry: CatalogModel, in format: ModelFormat) -> UInt64 {
        switch format {
        case .gguf: entry.size
        case .mlx: modelFormats[entry.name]?.mlxSize ?? 0
        }
    }
}
