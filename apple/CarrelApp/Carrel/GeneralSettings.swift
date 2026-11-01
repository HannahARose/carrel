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
                    prompt: "Carrel chooses", inUse: model.device?.pickerInUse, model: model
                ) {
                    PickerMenu(pickers: model.device?.knownPickers ?? [], model: model)
                }
                DeviceField(
                    title: "Card editor", key: "editor", value: model.device?.editor,
                    prompt: "$VISUAL or $EDITOR", inUse: model.device?.editorInUse, model: model
                ) { EmptyView() }
            } header: {
                Text("This Mac")
            } footer: {
                Text(
                    "The command line and the interactive mode use these: the picker that chooses what to add (a picker’s name, a command, or builtin), and the editor for cards. Unset, Carrel uses the first picker installed, and the terminal’s $VISUAL or $EDITOR before the system’s editor."
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

/// One of this device's preferences, saved when Return is pressed, with what's in use and
/// why, and any choices beside it.
private struct DeviceField<Choices: View>: View {
    let title: String
    let key: String
    let value: String?
    let prompt: String
    let inUse: String?
    let model: SettingsModel
    @ViewBuilder let choices: () -> Choices
    @State private var text = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                TextField(title, text: $text, prompt: Text(prompt))
                    .onSubmit { Task { await model.setDevice(key, to: text) } }
                    .task(id: value) { text = value ?? "" }
                choices()
            }
            if let inUse {
                Text("In use: \(inUse)").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// The pickers Carrel knows, each with what it's like and whether it's installed, to choose
/// one, or to go back to Carrel's choice.
private struct PickerMenu: View {
    let pickers: [KnownPicker]
    let model: SettingsModel

    var body: some View {
        Menu("Choose") {
            Button("Carrel’s Choice") { Task { await model.setDevice("picker", to: "") } }
            Button("The Built-in Browser") {
                Task { await model.setDevice("picker", to: "builtin") }
            }
            Divider()
            ForEach(pickers, id: \.name) { picker in
                Button {
                    Task { await model.setDevice("picker", to: picker.name) }
                } label: {
                    Text(picker.name)
                    Text("\(picker.about) · \(picker.state)")
                }
            }
        }
        .fixedSize()
        .help("Carrel never installs a picker itself; each says how to install it.")
    }
}

/// One of the archive's shared settings: a menu of its choices, a switch for each when it
/// takes several, or a field for a number or a folder.
struct SharedSettingRow: View {
    let setting: Setting
    let model: SettingsModel
    @State private var number = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if setting.takesSeveral {
                SeveralChoices(setting: setting, model: model)
            } else if setting.choices.isEmpty {
                TextField(setting.title, text: $number)
                    .onSubmit { Task { await model.set(setting.key, to: number) } }
                    .task(id: setting.value) { number = setting.value }
            } else {
                Picker(
                    setting.title,
                    selection: Binding(
                        get: { setting.value },
                        set: { value in Task { await model.set(setting.key, to: value) } })
                ) {
                    ForEach(setting.choices, id: \.self) { Text($0).tag($0) }
                }
            }
            Text(
                setting.choices.isEmpty
                    ? "\(setting.description). It takes \(setting.takes)." : setting.description
            )
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .disabled(model.isWorking)
    }
}

/// A setting that takes several of its choices, such as the reminders: a switch for each.
private struct SeveralChoices: View {
    let setting: Setting
    let model: SettingsModel

    private var chosen: [String] {
        setting.value.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
    }

    var body: some View {
        LabeledContent(setting.title) {
            VStack(alignment: .leading) {
                ForEach(setting.choices, id: \.self) { choice in
                    Toggle(
                        choice,
                        isOn: Binding(
                            get: { chosen.contains(choice) },
                            set: { on in
                                let now = setting.choices.filter {
                                    $0 == choice ? on : chosen.contains($0)
                                }
                                let value = now.isEmpty ? "none" : now.joined(separator: ", ")
                                Task { await model.set(setting.key, to: value) }
                            }))
                }
            }
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
