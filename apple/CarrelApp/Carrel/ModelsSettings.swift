// Settings → Models: which models draft and check cards on this Mac, and Carrel's catalog with
// sizes, memory needs and licenses, downloading or deleting one after a confirmation, as
// `carrel models` does.

import AppKit
import CarrelKit
import SwiftUI

struct ModelsTab: View {
    let model: SettingsModel?
    let folder: URL?

    var body: some View {
        SettingsForm(model: model) { model in
            DraftingSection(model: model)
            Section("Text models") {
                ForEach(model.catalog.filter { !$0.embedding }) {
                    ModelRow(entry: $0, model: model)
                }
            }
            Section("Embedding models, for search by meaning") {
                ForEach(model.catalog.filter(\.embedding)) { ModelRow(entry: $0, model: model) }
            }
            if let folder {
                Section {
                    Button("Show Models Folder", systemImage: "folder") {
                        NSWorkspace.shared.activateFileViewerSelecting([folder])
                    }
                    .disabled(
                        !FileManager.default.fileExists(atPath: folder.path(percentEncoded: false)))
                }
            }
        }
        .modelDownloadConfirmation(model)
        .confirmationDialog(
            "Delete \(model?.confirmingRemoval?.name ?? "") (\(model?.confirmingFormat.title ?? ""))?",
            isPresented: confirming(\.confirmingRemoval),
            presenting: model?.confirmingRemoval
        ) { _ in
            Button("Delete", role: .destructive) { Task { await model?.removeConfirmed() } }
        } message: { entry in
            Text("It’s deleted from this Mac’s models folder; you can download it again later.")
        }
    }

    private func confirming(_ path: ReferenceWritableKeyPath<SettingsModel, CatalogModel?>)
        -> Binding<Bool>
    {
        Binding(
            get: { model?[keyPath: path] != nil },
            set: { if !$0 { model?[keyPath: path] = nil } })
    }
}

extension View {
    /// Asks before downloading the model Settings is waiting to download, with its size and
    /// license, as Models and Search both offer.
    func modelDownloadConfirmation(_ model: SettingsModel?) -> some View {
        confirmationDialog(
            "Download \(model?.confirmingDownload?.name ?? "") (\(model?.confirmingFormat.title ?? ""))?",
            isPresented: Binding(
                get: { model?.confirmingDownload != nil },
                set: { if !$0 { model?.confirmingDownload = nil } }),
            presenting: model?.confirmingDownload
        ) { entry in
            let bytes = size(
                model.map { $0.size(of: entry, in: $0.confirmingFormat) } ?? entry.size)
            Button("Download \(bytes)") { Task { await model?.downloadConfirmed() } }
        } message: { entry in
            let bytes = size(
                model.map { $0.size(of: entry, in: $0.confirmingFormat) } ?? entry.size)
            Text(
                "\(entry.title). \(bytes) from the internet, under the \(entry.license) license; it needs about \(entry.memoryGb) GB of memory."
            )
        }
    }
}

private func size(_ bytes: UInt64) -> String {
    bytes.formatted(.byteCount(style: .file))
}

private struct DraftingSection: View {
    let model: SettingsModel

    var body: some View {
        Section {
            if let choices = model.choices {
                Picker(
                    "Drafts cards",
                    selection: Binding(
                        get: { choices.model },
                        set: { drafting in
                            Task {
                                await model.chooseModels(
                                    drafting: drafting, secondOpinion: choices.secondModel)
                            }
                        })
                ) {
                    Text("Carrel’s choice").tag(String?.none)
                    ForEach(choices.available, id: \.self) { Text($0).tag(String?.some($0)) }
                }
                Picker(
                    "Asked again",
                    selection: Binding(
                        get: { choices.secondModel },
                        set: { second in
                            Task {
                                await model.chooseModels(
                                    drafting: choices.model, secondOpinion: second)
                            }
                        })
                ) {
                    Text("Recommended, if installed").tag(String?.none)
                    Text("Off").tag(String?.some("off"))
                    ForEach(choices.available, id: \.self) { Text($0).tag(String?.some($0)) }
                }
                Picker(
                    "Librarian",
                    selection: Binding(
                        get: { model.device?.librarianModel },
                        set: { chosen in
                            Task { await model.setDevice("librarian.model", to: chosen ?? "") }
                        })
                ) {
                    Text("The strongest that fits").tag(String?.none)
                    ForEach(choices.available, id: \.self) { Text($0).tag(String?.some($0)) }
                }
                .help("The model the librarian answers with")
                LabeledContent("Drafting now", value: choices.modelInUse ?? "No model available")
                LabeledContent("Asked again now", value: choices.secondInUse ?? "None")
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
        } header: {
            Text("This Mac")
        } footer: {
            Text(
                "The second model is asked when the first isn’t sure where an item goes."
            )
            .foregroundStyle(.secondary)
        }
        .disabled(model.isWorking)
    }
}

/// A model in the catalog: what it is, its size, memory and license, and whether it's here.
private struct ModelRow: View {
    let entry: CatalogModel
    let model: SettingsModel

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(entry.name).font(.body.monospaced())
                    if entry.recommended {
                        Text("Recommended").font(.caption).foregroundStyle(.tint)
                    }
                }
                Text(entry.title).foregroundStyle(.secondary)
                Text(details).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            actions
        }
        .disabled(model.isWorking)
    }

    private var details: String {
        var parts: [String] = []
        if entry.size > 0 { parts.append(size(entry.size)) }
        parts.append("\(entry.memoryGb) GB memory")
        parts.append(entry.license)
        return parts.joined(separator: " · ")
    }

    @ViewBuilder private var actions: some View {
        switch entry.state {
        case .builtIn:
            Text("Comes with \(entry.comesWith ?? "a provider")").foregroundStyle(.secondary)
        case .downloaded:
            ModelFormatActions(entry: entry, model: model)
        case .partial(let bytes):
            VStack(alignment: .trailing) {
                Text("Stopped at \(size(bytes))").font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("Delete…") {
                        model.confirmingFormat = .gguf
                        model.confirmingRemoval = entry
                    }
                    Button("Carry On…") {
                        model.confirmingFormat = .gguf
                        model.confirmingDownload = entry
                    }
                    .disabled(!entry.downloadable)
                }
            }
        case .notDownloaded:
            ModelFormatActions(entry: entry, model: model)
                .disabled(!entry.downloadable)
        }
    }
}
