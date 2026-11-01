// Settings → General: this Mac's file picker and editor, which the command line and the
// interactive mode use, and the archive's shared settings, each change recorded in History.

import CarrelKit
import SwiftUI

struct GeneralTab: View {
    let model: SettingsModel?

    var body: some View {
        SettingsForm(model: model) { model in
            Section {
                DeviceField(
                    title: "File picker", key: "picker", value: model.device?.picker,
                    prompt: "Carrel chooses", model: model)
                DeviceField(
                    title: "Card editor", key: "editor", value: model.device?.editor,
                    prompt: "$VISUAL or $EDITOR", model: model)
            } header: {
                Text("This Mac")
            } footer: {
                Text(
                    "The command line and the interactive mode use these: the picker that chooses what to add (a picker’s name, a command, or builtin), and the editor for cards."
                )
                .foregroundStyle(.secondary)
            }
            if let archive = model.archive {
                Section {
                    ForEach(model.shared) { setting in
                        SharedSettingRow(setting: setting, model: model)
                    }
                } header: {
                    Text("“\(archive.name)”, shared by every device")
                } footer: {
                    Text(
                        "Each change is recorded in History, where it can be undone. The embedding model is in Search."
                    )
                    .foregroundStyle(.secondary)
                }
            }
        }
    }
}

/// One of this device's preferences, saved when Return is pressed.
private struct DeviceField: View {
    let title: String
    let key: String
    let value: String?
    let prompt: String
    let model: SettingsModel
    @State private var text = ""

    var body: some View {
        TextField(title, text: $text, prompt: Text(prompt))
            .onSubmit { Task { await model.setDevice(key, to: text) } }
            .task(id: value) { text = value ?? "" }
    }
}

/// One of the archive's shared settings: a menu of its choices, or a whole number.
struct SharedSettingRow: View {
    let setting: Setting
    let model: SettingsModel
    @State private var number = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if setting.choices.isEmpty {
                TextField(Self.title(setting.key), text: $number)
                    .onSubmit { Task { await model.set(setting.key, to: number) } }
                    .task(id: setting.value) { number = setting.value }
            } else {
                Picker(
                    Self.title(setting.key),
                    selection: Binding(
                        get: { setting.value },
                        set: { value in Task { await model.set(setting.key, to: value) } })
                ) {
                    ForEach(setting.choices, id: \.self) { Text($0).tag($0) }
                }
            }
            Text(setting.description)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .disabled(model.isWorking)
    }

    /// The setting's name, for people; its key when there's none.
    static func title(_ key: String) -> String {
        switch key {
        case "ingest.originals": "Originals"
        case "compressed_files": "Compressed files"
        case "signals.crowded_items": "Crowded collections"
        case "outside_edits": "Changes made outside Carrel"
        default: key
        }
    }
}

/// A Settings tab's form, with what the last change did, what went wrong, and the progress of
/// a long operation at the end. Without an archive open, it says to open one.
struct SettingsForm<Content: View>: View {
    let model: SettingsModel?
    @ViewBuilder let content: (SettingsModel) -> Content

    var body: some View {
        Form {
            if let model {
                content(model)
                SettingsFeedback(model: model)
            } else {
                Text("Open an archive to change its settings, models and search.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

private struct SettingsFeedback: View {
    let model: SettingsModel

    var body: some View {
        if let progress = model.progress {
            Section { OperationProgressView(progress: progress) }
        }
        if model.note != nil || model.problem != nil {
            Section {
                if let note = model.note {
                    HStack {
                        Text(note)
                        Spacer()
                        if model.lastChange != nil {
                            Button("Undo") { Task { await model.undoLastChange() } }
                                .disabled(model.isWorking)
                        }
                    }
                }
                if let problem = model.problem {
                    Text(problem).foregroundStyle(.red)
                }
            }
        }
    }
}

/// A long operation's progress bar: items, or bytes for a download.
struct OperationProgressView: View {
    let progress: OperationProgress

    var body: some View {
        ProgressView(value: progress.fraction) {
            Text(progress.title)
        } currentValueLabel: {
            if progress.total > 0 {
                Text("\(amount(progress.done)) of \(amount(progress.total))")
            } else if let item = progress.item {
                Text(item)
            }
        }
    }

    private func amount(_ count: UInt64) -> String {
        progress.title.hasPrefix("Downloading")
            ? count.formatted(.byteCount(style: .file)) : count.formatted()
    }
}
