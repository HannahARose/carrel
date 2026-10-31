// The sheets and confirmations of Active and History, on the archive's window, and the
// window for their menus.

import AppKit
import CarrelKit
import SwiftUI

extension FocusedValues {
    /// The key archive window, for Active's and History's menu commands.
    @Entry var activeWindow: ArchiveWindowModel?
}

extension View {
    /// Adds Active's and History's sheets, and publishes them to the menus.
    func activeAndHistorySheets(_ model: ArchiveWindowModel) -> some View {
        modifier(ActiveSheets(model: model))
    }
}

private struct ActiveSheets: ViewModifier {
    let model: ArchiveWindowModel

    func body(content: Content) -> some View {
        content
            .focusedSceneValue(\.activeWindow, model)
            .modifier(OptionalModifier(value: model.activeScreen) { ActiveDialogs(model: $0) })
            .modifier(OptionalModifier(value: model.historyScreen) { HistoryDialogs(model: $0) })
    }
}

/// Whether `value` is set, as a sheet's presentation: dismissing it clears the value.
private func present<Value>(_ value: Binding<Value?>) -> Binding<Bool> {
    Binding(get: { value.wrappedValue != nil }, set: { if !$0 { value.wrappedValue = nil } })
}

/// Applies a modifier made from `value` when there is one.
private struct OptionalModifier<Value, Modifier: ViewModifier>: ViewModifier {
    let value: Value?
    let make: (Value) -> Modifier

    func body(content: Content) -> some View {
        if let value { content.modifier(make(value)) } else { content }
    }
}

private struct ActiveDialogs: ViewModifier {
    @Bindable var model: ActiveScreenModel

    func body(content: Content) -> some View {
        content
            .sheet(isPresented: present($model.returning)) {
                if let preview = model.returning {
                    ReturnSheet(preview: preview) { Task { await model.confirmReturn(preview) } }
                }
            }
            .sheet(item: $model.checkingOut) { draft in
                CheckoutSheet(draft: draft) { draft in Task { await model.checkOut(draft) } }
            }
            .sheet(item: $model.editing) { draft in
                EntryEditSheet(draft: draft) { draft in Task { await model.save(draft) } }
            }
            .confirmationDialog(
                "Cancel the checkout of “\(model.cancelling?.title ?? "")”?",
                isPresented: present($model.cancelling),
                presenting: model.cancelling
            ) { item in
                Button("Cancel Checkout", role: .destructive) {
                    Task { await model.confirmCancel(item) }
                }
                Button("Keep It Checked Out", role: .cancel) { model.cancelling = nil }
            } message: { _ in
                Text(
                    "The archive copy is unlocked as it was. Changes in the working copy aren’t brought back, and the working copy stays where it is."
                )
            }
            .alert(
                "Couldn’t Do That",
                isPresented: present($model.problem)
            ) {
                Button("OK") { model.problem = nil }
            } message: {
                Text(model.problem ?? "")
            }
    }
}

private struct HistoryDialogs: ViewModifier {
    @Bindable var model: HistoryScreenModel

    func body(content: Content) -> some View {
        content
            .sheet(isPresented: present($model.undoing)) {
                if let change = model.undoing {
                    UndoSheet(change: change) { Task { await model.undo(change) } }
                }
            }
            .alert(
                "Couldn’t Undo",
                isPresented: present($model.problem)
            ) {
                Button("OK") { model.problem = nil }
            } message: {
                Text(model.problem ?? "")
            }
    }
}

/// Asks before returning, showing what changed, the commits since checkout and the entry the
/// return adds.
private struct ReturnSheet: View {
    let preview: ReturnPreview
    let confirm: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Return “\(preview.title)”?").font(.headline).padding([.horizontal, .top], 20)
            Form {
                LabeledContent("From") {
                    Text(preview.working).textSelection(.enabled).truncationMode(.middle)
                }
                ChangesSection(changes: preview.changes)
                if !preview.commits.isEmpty || preview.uncommitted {
                    Section("Commits") {
                        ForEach(preview.commits, id: \.self) { Text($0) }
                        if preview.uncommitted {
                            Label(
                                "There are changes not committed; they come back too.",
                                systemImage: "exclamationmark.triangle")
                        }
                    }
                }
                Section("The Entry It Adds") {
                    Text(preview.entry.joined(separator: "\n")).textSelection(.enabled)
                }
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Return") {
                    dismiss()
                    confirm()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(20)
        }
        .frame(width: 480, height: 460)
    }
}

/// Where to check an item out to, and when it's due back.
private struct CheckoutSheet: View {
    @State var draft: CheckoutDraft
    let confirm: (CheckoutDraft) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var hasDue = false
    @State private var due = Calendar.current.date(byAdding: .day, value: 14, to: .now) ?? .now

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Check Out “\(draft.title)”").font(.headline)
                .padding([.horizontal, .top], 20)
            Form {
                LabeledContent("To") {
                    HStack {
                        Text(draft.folder.path(percentEncoded: false))
                            .lineLimit(1).truncationMode(.middle)
                        Button("Choose…") { chooseFolder() }
                    }
                }
                Toggle("Due back", isOn: $hasDue)
                if hasDue {
                    DatePicker("On", selection: $due, displayedComponents: .date)
                }
                Text(
                    "A copy is made there to work on. The archive copy is read only until you return it."
                )
                .font(.caption).foregroundStyle(.secondary)
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Check Out") {
                    draft.due = hasDue ? due : nil
                    dismiss()
                    confirm(draft)
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(20)
        }
        .frame(width: 460)
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Choose"
        panel.directoryURL = draft.folder
        if panel.runModal() == .OK, let url = panel.url { draft.folder = url }
    }
}

/// Edits a pending entry's text; it stays pending.
private struct EntryEditSheet: View {
    @State var draft: EntryDraft
    let save: (EntryDraft) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Edit Entry").font(.headline)
            TextEditor(text: $draft.text)
                .font(.body.monospaced())
                .frame(minHeight: 140)
            Text("It stays pending until you approve it.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save") {
                    dismiss()
                    save(draft)
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 460)
    }
}
