// Browse's sheets and confirmations, on the archive's window: a new collection, a rename and
// an expansion, each asked before anything changes.

import CarrelKit
import SwiftUI

extension View {
    /// Adds Browse's sheets for creating collections, renaming and expanding.
    @ViewBuilder
    func browseSheets(_ model: BrowseActionsModel?) -> some View {
        if let model { self.modifier(BrowseDialogs(model: model)) } else { self }
    }
}

private struct BrowseDialogs: ViewModifier {
    @Bindable var model: BrowseActionsModel

    func body(content: Content) -> some View {
        content
            .sheet(item: $model.creating) { draft in
                NewCollectionSheet(draft: draft) { draft in Task { await model.create(draft) } }
                    .id(draft.problem)
            }
            .sheet(item: $model.renaming) { draft in
                RenameSheet(draft: draft) { draft in Task { await model.rename(draft) } }
                    .id(draft.problem)
            }
            .confirmationDialog(
                model.expanding?.keptBecause == nil
                    ? "Expand “\(model.expanding?.title ?? "")”?"
                    : "Expand “\(model.expanding?.title ?? "")” all the same?",
                isPresented: Binding(
                    get: { model.expanding != nil }, set: { if !$0 { model.expanding = nil } }),
                presenting: model.expanding
            ) { request in
                Button(request.keptBecause == nil ? "Expand" : "Expand All the Same") {
                    Task { await model.expand(request) }
                }
                Button("Keep It Compressed", role: .cancel) { model.expanding = nil }
            } message: { request in
                if let reason = request.keptBecause {
                    Text(
                        "It’s kept compressed: \(reason). What it holds takes its place in the collection, its card follows, and the compressed file goes to the Trash. Undo it from History."
                    )
                } else {
                    Text(
                        "What it holds takes its place in the collection, its card follows, and the compressed file goes to the Trash. Undo it from History."
                    )
                }
            }
            .alert(
                "Couldn’t Do That",
                isPresented: Binding(
                    get: { model.problem != nil }, set: { if !$0 { model.problem = nil } })
            ) {
                Button("OK") { model.problem = nil }
            } message: {
                Text(model.problem ?? "")
            }
            .flagDialogs(model)
    }
}

/// A new collection's path, what belongs there and its tags.
private struct NewCollectionSheet: View {
    @State var draft: CollectionDraft
    let confirm: (CollectionDraft) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("New Collection").font(.headline).padding([.horizontal, .top], 20)
            Form {
                TextField("Path", text: $draft.path, prompt: Text("Finances/Tax"))
                TextField(
                    "What belongs there", text: $draft.description,
                    prompt: Text("Optional"), axis: .vertical
                )
                .lineLimit(2...4)
                Section {
                    ForEach($draft.tags) { $tag in
                        HStack {
                            TextField("Tag", text: $tag.name, prompt: Text("tag-name"))
                                .labelsHidden()
                                .frame(width: 140)
                            TextField("Meaning", text: $tag.meaning, prompt: Text("What it means"))
                                .labelsHidden()
                            Button("Remove Tag", systemImage: "minus.circle") {
                                draft.tags.removeAll { $0.id == tag.id }
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.borderless)
                        }
                    }
                    Button("Add Tag", systemImage: "plus") { draft.tags.append(TagDraft()) }
                } header: {
                    Text("Tags")
                } footer: {
                    Text(
                        "Folder names separated by /, inside an existing collection. Its design document is where you say what belongs there, and what doesn’t: models read it before filing anything. Undo it from History."
                    )
                    .font(.caption).foregroundStyle(.secondary)
                }
                if let problem = draft.problem {
                    Label(problem, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                }
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Create") { confirm(draft) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(draft.trimmedPath.isEmpty)
            }
            .padding(20)
        }
        .frame(width: 480)
    }
}

/// An item's new name, starting from one that works on every system.
private struct RenameSheet: View {
    @State var draft: RenameDraft
    let confirm: (RenameDraft) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Rename “\(draft.title)”").font(.headline).padding([.horizontal, .top], 20)
            Form {
                LabeledContent("Now") { Text(draft.current).textSelection(.enabled) }
                TextField("New name", text: $draft.name)
                Text(
                    "The item and its card are renamed together, and links to it follow. The name suggested works on every system. Undo it from History."
                )
                .font(.caption).foregroundStyle(.secondary)
                if let problem = draft.problem {
                    Label(problem, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                }
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Rename") { confirm(draft) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!draft.canRename)
            }
            .padding(20)
        }
        .frame(width: 440)
    }
}
