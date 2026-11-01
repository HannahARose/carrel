// Settings → Search and → Providers: the embedding index (its model, how many items are
// embedded and how many are behind, rebuilding it, and switching models), as `carrel index`
// shows it; and the providers in use with the file types this Mac reads and couldn't read, as
// `carrel providers` and `carrel formats` show them.

import CarrelKit
import SwiftUI

struct SearchTab: View {
    let model: SettingsModel?

    var body: some View {
        SettingsForm(model: model) { model in
            if let index = model.index {
                IndexSection(index: index, model: model)
                if let switching = index.switch {
                    SwitchSection(switching: switching, model: model)
                }
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
            if let embedding = model.embedding {
                ChoicesSection(embedding: embedding, current: model.index?.model, model: model)
            }
        }
        .confirmationDialog(
            "Switch this archive to \(model?.confirmingSwitch ?? "")?",
            isPresented: Binding(
                get: { model?.confirmingSwitch != nil },
                set: { if !$0 { model?.confirmingSwitch = nil } }),
            presenting: model?.confirmingSwitch
        ) { _ in
            Button("Switch") { Task { await model?.switchConfirmed() } }
        } message: { chosen in
            Text(
                "Every item is embedded again with \(chosen), while search keeps using \(model?.index?.model ?? "words only"). Once it’s done, every device searches this archive with \(chosen), and the old index is removed."
            )
        }
    }
}

private struct IndexSection: View {
    let index: IndexStatus
    let model: SettingsModel

    var body: some View {
        Section {
            LabeledContent("Model", value: index.model ?? "None yet")
            if index.model == nil {
                Text("This archive is searched by words only. Choose a model below.")
                    .foregroundStyle(.secondary)
            } else if index.exists {
                LabeledContent(
                    "Embedded", value: "\(index.embedded) of \(index.items) items")
                LabeledContent("Behind", value: "\(index.behind)")
                if index.rebuilding {
                    Text("A rebuild stopped part way; rebuilding carries on from there.")
                        .foregroundStyle(.secondary)
                } else if index.behind > 0 {
                    Text("The agent or the worker embeds them after the next change.")
                        .foregroundStyle(.secondary)
                }
            } else {
                Text("There’s no index for it yet, so search uses words only.")
                    .foregroundStyle(.secondary)
            }
            if index.model != nil {
                Button(index.rebuilding ? "Carry On Rebuilding" : "Rebuild Index") {
                    Task { await model.rebuildIndex() }
                }
                .disabled(model.isWorking)
            }
        } header: {
            Text("Search by meaning in “\(model.archive?.name ?? "")”")
        } footer: {
            Text("Every device searches this archive with the same embedding model.")
                .foregroundStyle(.secondary)
        }
    }
}

private struct SwitchSection: View {
    let switching: IndexSwitch
    let model: SettingsModel

    var body: some View {
        Section("Switching to \(switching.to)") {
            ProgressView(
                value: Double(switching.embedded), total: Double(max(switching.items, 1))
            ) {
                Text("\(switching.embedded) of \(switching.items) items embedded")
            }
            Text(
                "Search uses \(switching.from ?? "words only") until it’s done. Any device that has \(switching.to) can carry on."
            )
            .foregroundStyle(.secondary)
            HStack {
                Button("Carry On") { Task { await model.carryOnSwitch() } }
                Button("Stop Switching", role: .destructive) {
                    Task { await model.stopSwitch() }
                }
            }
            .disabled(model.isWorking)
        }
    }
}

private struct ChoicesSection: View {
    let embedding: EmbeddingModels
    let current: String?
    let model: SettingsModel

    var body: some View {
        Section("Embedding models") {
            if embedding.here.isEmpty {
                Text("No provider on this Mac embeds.").foregroundStyle(.secondary)
            }
            ForEach(embedding.here) { choice in
                HStack {
                    VStack(alignment: .leading) {
                        Text(choice.model).font(.body.monospaced())
                        Text(details(choice)).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if choice.model == current {
                        Text("In use").foregroundStyle(.secondary)
                    } else {
                        Button("Switch…") { model.confirmingSwitch = choice.model }
                            .disabled(model.isWorking)
                    }
                }
            }
            ForEach(embedding.elsewhere, id: \.model) { other in
                VStack(alignment: .leading) {
                    Text(other.model).font(.body.monospaced())
                    Text("Not on this Mac: \(other.how)")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }

    private func details(_ choice: EmbeddingChoice) -> String {
        let numbers = choice.dimensions.map { ", \($0) numbers" } ?? ""
        return "through \(choice.provider)\(numbers)"
    }
}

struct ProvidersTab: View {
    let model: SettingsModel?

    var body: some View {
        SettingsForm(model: model) { model in
            Section("Providers") {
                ForEach(model.providers) { ProviderRow(provider: $0) }
            }
            if let formats = model.formats {
                Section("File types this Mac reads") {
                    ForEach(formats.reads) { type in
                        LabeledContent(type.mediaType, value: type.readers.joined(separator: ", "))
                    }
                }
                Section("File types it couldn’t read") {
                    if formats.unread.isEmpty {
                        Text("None so far.").foregroundStyle(.secondary)
                    }
                    ForEach(formats.unread) { UnreadRow(type: $0) }
                }
            }
        }
    }
}

private struct ProviderRow: View {
    let provider: ProviderInfo

    var body: some View {
        DisclosureGroup {
            ForEach(provider.offers, id: \.self) { offer in
                LabeledContent {
                    Text(
                        offer.available
                            ? "priority \(offer.priority)" : offer.reason ?? "unavailable")
                } label: {
                    Text(offer.label)
                    Text(offer.capability)
                }
                .foregroundStyle(offer.available ? .primary : .secondary)
            }
        } label: {
            VStack(alignment: .leading) {
                Text(provider.name)
                if let error = provider.error {
                    Text(error).font(.caption).foregroundStyle(.red)
                } else if let path = provider.path {
                    Text([provider.version, path].compactMap(\.self).joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    Text("Built in").font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}

private struct UnreadRow: View {
    let type: UnreadType

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(type.extension.isEmpty ? "(no extension)" : ".\(type.extension)")
                    .font(.body.monospaced())
                Text(type.mediaType).foregroundStyle(.secondary)
                Spacer()
                Text(type.count == 1 ? "1 item" : "\(type.count) items")
            }
            Text("\(type.why), since \(type.firstSeen)")
                .font(.caption).foregroundStyle(.secondary)
            if let hint = type.hint {
                Text(hint).font(.caption)
            }
        }
    }
}
