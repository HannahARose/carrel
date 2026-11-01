// A card's fields, editable in place: sections for a grouped Form, with the problems the
// core's checks find beside the fields they're about. A filed card's links, location, URL and
// snapshots are among them, where its kind has them.

import CarrelKit
import SwiftUI

struct CardEditor: View {
    @Bindable var edit: CardEditModel
    /// The archive's items, to link to.
    var items: [ItemSummary] = []
    @State private var newTag = ""

    var body: some View {
        if let unavailable = edit.unavailable {
            Section {
                Text(unavailable).foregroundStyle(.secondary)
            }
        } else if edit.fields != nil {
            if edit.fieldsClash != nil {
                Section { FieldsClashNotice(edit: edit) }
            }
            if !edit.otherProblems.isEmpty {
                Section { Problems(edit.otherProblems) }
            }
            Section("Card") {
                TextField("Title", text: field(\.title))
                Problems(edit.problems(about: "title"))
                TextField("Summary", text: optional(\.summary), axis: .vertical)
                    .lineLimit(2...8)
                Problems(edit.problems(about: "summary"))
                if edit.needsLocation || edit.fields?.location != nil {
                    TextField("Location", text: optional(\.location), prompt: Text("Where it is"))
                    Problems(edit.problems(about: "location"))
                }
                if edit.needsURL || edit.fields?.url != nil {
                    TextField("URL", text: optional(\.url), prompt: Text("https://"))
                    Problems(edit.problems(about: "url"))
                }
                Picker("Sensitivity", selection: field(\.sensitivity)) {
                    ForEach(edit.sensitivities, id: \.self) { Text($0.name.capitalized).tag($0) }
                }
                Problems(edit.problems(about: "sensitivity"))
                if edit.takesSnapshots {
                    Toggle(
                        "Snapshot only at checkout and return",
                        isOn: field(\.snapshotOnReturnOnly)
                    )
                    .help("For large data: no snapshot after each work session")
                    Problems(edit.problems(about: "snapshot"))
                }
            }
            if !edit.linkTypes.isEmpty {
                CardLinksSection(edit: edit, items: items)
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
                    if edit.bodyClash != nil {
                        BodyClashNotice(edit: edit)
                    }
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

/// Says fields were saved another way while they were edited, and offers to keep the edit or
/// the card's as they are now.
private struct FieldsClashNotice: View {
    let edit: CardEditModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(
                "Saved another way while you edited them, so nothing was saved: \(names).",
                systemImage: "exclamationmark.triangle"
            )
            .foregroundStyle(.orange)
            HStack {
                Button("Keep Mine") { edit.keepMyFields() }
                    .help("Keep your changes, then save them over the card as it is now")
                Button("Reload") { edit.reloadFields() }
                    .help("Drop your changes to the fields, and edit the card as it is now")
            }
        }
    }

    private var names: String {
        (edit.fieldsClash?.fields ?? []).joined(separator: ", ")
    }
}

/// A card's links to other items: each one's type, the item and a note, and a menu to add one.
private struct CardLinksSection: View {
    @Bindable var edit: CardEditModel
    let items: [ItemSummary]

    var body: some View {
        Section("Links") {
            ForEach(Array((edit.fields?.links ?? []).enumerated()), id: \.offset) { index, link in
                HStack {
                    Picker("Type", selection: linkField(index, \.linkType)) {
                        ForEach(types(including: link.linkType), id: \.self) { Text($0).tag($0) }
                    }
                    .labelsHidden()
                    .fixedSize()
                    Text(title(of: link.item)).lineLimit(1)
                    TextField("Note", text: note(index))
                    Button("Remove Link", systemImage: "minus.circle") {
                        edit.fields?.links.remove(at: index)
                    }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                }
            }
            Menu("Add Link") {
                ForEach(linkable, id: \.id) { item in
                    Button(item.title) {
                        edit.fields?.links.append(
                            CardLink(linkType: "related", item: item.id, note: nil))
                    }
                }
            }
            .fixedSize()
            .disabled(linkable.isEmpty)
            Problems(edit.problems(about: "link"))
        }
    }

    /// The items it may link to: every other one, by title.
    private var linkable: [ItemSummary] {
        guard case .item(let own) = edit.target else { return [] }
        return items.filter { $0.id != own }
            .sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
    }

    /// The link types to choose from, with `type` among them though the archive no longer
    /// declares it, so the picker can show it.
    private func types(including type: String) -> [String] {
        edit.linkTypes.contains(type) ? edit.linkTypes : edit.linkTypes + [type]
    }

    private func title(of id: String) -> String {
        items.first { $0.id == id }?.title ?? id
    }

    private func linkField(_ index: Int, _ path: WritableKeyPath<CardLink, String>)
        -> Binding<String>
    {
        Binding {
            edit.fields.flatMap { $0.links.indices.contains(index) ? $0.links[index] : nil }?[
                keyPath: path] ?? ""
        } set: {
            guard edit.fields?.links.indices.contains(index) == true else { return }
            edit.fields?.links[index][keyPath: path] = $0
        }
    }

    private func note(_ index: Int) -> Binding<String> {
        Binding {
            edit.fields.flatMap { $0.links.indices.contains(index) ? $0.links[index] : nil }?
                .note ?? ""
        } set: {
            guard edit.fields?.links.indices.contains(index) == true else { return }
            edit.fields?.links[index].note = $0.isEmpty ? nil : $0
        }
    }
}

/// Says the body was saved another way while it was edited, on the same lines, and offers
/// both versions together or the body as it is now.
private struct BodyClashNotice: View {
    let edit: CardEditModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(
                "These notes were saved another way while you edited them, on the same lines, so nothing was saved.",
                systemImage: "exclamationmark.triangle"
            )
            .foregroundStyle(.orange)
            HStack {
                Button("Keep Both") { edit.keepBothBodies() }
                    .help("Edit both versions together, then save")
                Button("Reload") { edit.reloadBody() }
                    .help("Drop your changes to the notes, and edit the card as it is now")
            }
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
    /// The archive's items, to link to.
    var items: [ItemSummary] = []
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
            CardEditor(edit: edit, items: items)
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
