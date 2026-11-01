// A card's fields, editable in place: sections for a grouped Form, with the problems the
// core's checks find beside the fields they're about.

import CarrelKit
import SwiftUI

struct CardEditor: View {
    @Bindable var edit: CardEditModel
    @State private var newTag = ""

    var body: some View {
        if let unavailable = edit.unavailable {
            Section {
                Text(unavailable).foregroundStyle(.secondary)
            }
        } else if edit.fields != nil {
            if !edit.otherProblems.isEmpty {
                Section { Problems(edit.otherProblems) }
            }
            Section("Card") {
                TextField("Title", text: field(\.title))
                Problems(edit.problems(about: "title"))
                TextField("Summary", text: optional(\.summary), axis: .vertical)
                    .lineLimit(2...8)
                Problems(edit.problems(about: "summary"))
                Picker("Sensitivity", selection: field(\.sensitivity)) {
                    ForEach(edit.sensitivities, id: \.self) { Text($0.name.capitalized).tag($0) }
                }
                Problems(edit.problems(about: "sensitivity"))
            }
            Section("Tags") {
                ForEach(edit.fields?.tags ?? [], id: \.self) { tag in
                    HStack {
                        Text(tag)
                        Spacer()
                        Button("Remove \(tag)", systemImage: "minus.circle") {
                            edit.fields?.tags.removeAll { $0 == tag }
                        }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                    }
                }
                HStack {
                    TextField("New tag", text: $newTag)
                        .onSubmit(addTag)
                    Menu("Add") {
                        ForEach(edit.addableTags, id: \.self) { tag in
                            Button(tag) { edit.fields?.tags.append(tag) }
                        }
                    }
                    .fixedSize()
                    .disabled(edit.addableTags.isEmpty)
                }
                Problems(edit.problems(about: "tag"))
            }
            Section("Dates") {
                TextField("Created", text: optional(\.dates.created), prompt: Text("YYYY-MM-DD"))
                TextField("Modified", text: optional(\.dates.modified), prompt: Text("YYYY-MM-DD"))
                TextField("Added", text: optional(\.dates.added), prompt: Text("YYYY-MM-DD"))
                TextField("Worked on", text: optional(\.dates.worked), prompt: Text("YYYY-MM-DD"))
                TextField("Document", text: optional(\.dates.document), prompt: Text("YYYY-MM-DD"))
                Problems(edit.problems(about: "date"))
            }
            if edit.body != nil {
                Section {
                    TextEditor(
                        text: Binding {
                            edit.body ?? ""
                        } set: {
                            edit.body = $0
                        }
                    )
                    .font(.body.monospaced())
                    .frame(minHeight: 160)
                } header: {
                    Text("Notes and Body")
                } footer: {
                    Text(
                        "Write anything outside Carrel’s sections, the parts between <!-- carrel:… --> markers, such as the history; they have to stay as they are."
                    )
                }
            }
            Section {
                HStack {
                    Spacer()
                    Button("Revert") { edit.revert() }
                        .disabled(!edit.hasChanges)
                    Button("Save Card") { Task { await edit.save() } }
                        .keyboardShortcut("s")
                        .disabled(!edit.hasChanges)
                }
            }
        } else {
            Section { ProgressView().frame(maxWidth: .infinity) }
        }
    }

    private func addTag() {
        let tag = newTag.trimmingCharacters(in: .whitespaces)
        guard !tag.isEmpty, edit.fields?.tags.contains(tag) == false else { return }
        edit.fields?.tags.append(tag)
        newTag = ""
    }

    private func field<T>(_ path: WritableKeyPath<CardFields, T>) -> Binding<T> where T: Equatable {
        let fallback = edit.fields?[keyPath: path] ?? edit.original![keyPath: path]
        return Binding {
            edit.fields?[keyPath: path] ?? fallback
        } set: {
            edit.fields?[keyPath: path] = $0
        }
    }

    /// An optional text field as text, empty for none.
    private func optional(_ path: WritableKeyPath<CardFields, String?>) -> Binding<String> {
        Binding {
            edit.fields?[keyPath: path] ?? ""
        } set: {
            edit.fields?[keyPath: path] = $0.isEmpty ? nil : $0
        }
    }
}

/// What's wrong with a field, in red, when anything is.
private struct Problems: View {
    let problems: [String]

    init(_ problems: [String]) {
        self.problems = problems
    }

    var body: some View {
        ForEach(problems, id: \.self) { problem in
            Label(problem, systemImage: "exclamationmark.triangle")
                .font(.callout)
                .foregroundStyle(.red)
        }
    }
}

/// A filed item's card being edited in the inspector (⌘E).
struct ItemEditor: View {
    let item: ItemSummary
    let edit: CardEditModel
    let done: () -> Void

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label("Editing \(item.kind.name)", systemImage: item.kind.symbol)
                        .font(.caption).foregroundStyle(.secondary)
                    Text(item.title).font(.title2.bold())
                }
            }
            CardEditor(edit: edit)
            Section {
                HStack {
                    Spacer()
                    Button("Done", action: done)
                        .keyboardShortcut(.cancelAction)
                }
            }
        }
        .formStyle(.grouped)
        .task(id: edit.target) { await edit.load() }
    }
}
