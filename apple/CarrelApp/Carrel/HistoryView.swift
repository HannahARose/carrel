// History: every change, newest first. `u` in the list, or History → Undo Change…, undoes
// the chosen one after a confirmation sheet.

import CarrelKit
import SwiftUI

struct HistoryView: View {
    @Bindable var model: HistoryScreenModel

    var body: some View {
        List(model.changes, id: \.id, selection: $model.selectedID) { change in
            VStack(alignment: .leading, spacing: 2) {
                Text(change.summary).lineLimit(2)
                HStack(spacing: 6) {
                    Text(change.when)
                    if let operation = change.operation { Text("· \(operation)") }
                }
                .font(.caption).foregroundStyle(.secondary)
            }
        }
        .contextMenu(forSelectionType: String.self) { _ in
            Button("Undo Change…") { model.askToUndo() }
        }
        .onKeyPress("u") {
            guard model.selected != nil else { return .ignored }
            model.askToUndo()
            return .handled
        }
        .overlay {
            if model.changes.isEmpty {
                ContentUnavailableView(
                    "No History", systemImage: "clock.arrow.circlepath",
                    description: Text("Changes to the archive appear here."))
            }
        }
    }
}

/// The inspector for History: what the chosen change did.
struct ChangeInspector: View {
    let model: HistoryScreenModel
    /// An item's title, from its identifier.
    let title: (String) -> String?

    var body: some View {
        if let change = model.selected {
            Form {
                Section {
                    Text(change.summary).font(.title3.bold()).textSelection(.enabled)
                }
                Section {
                    LabeledContent("When", value: change.when)
                    if let operation = change.operation {
                        LabeledContent("Operation", value: operation)
                    }
                    if let device = change.device { LabeledContent("Device", value: device) }
                    LabeledContent("Change") {
                        Text(change.id.prefix(12)).monospaced().textSelection(.enabled)
                    }
                    if let reverts = change.reverts {
                        LabeledContent("Undoes") {
                            Text(reverts.prefix(12)).monospaced().textSelection(.enabled)
                        }
                    }
                    if let snapshot = change.snapshot {
                        LabeledContent("Backup snapshot") {
                            Text(snapshot).monospaced().textSelection(.enabled)
                        }
                    }
                }
                if !change.items.isEmpty {
                    Section("Items") {
                        ForEach(change.items, id: \.self) { id in
                            Text(title(id) ?? id).textSelection(.enabled)
                        }
                    }
                }
                Section {
                    Button("Undo Change…", systemImage: "arrow.uturn.backward") {
                        model.askToUndo()
                    }
                }
            }
            .formStyle(.grouped)
            .id(change.id)
        } else {
            ContentUnavailableView(
                "Nothing Selected", systemImage: "sidebar.right",
                description: Text("Choose a change to see what it did."))
        }
    }
}

/// Asks before undoing, saying what undoing restores and what it doesn't.
struct UndoSheet: View {
    let change: Change
    let undo: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let explanation = UndoExplanation(change)
        VStack(alignment: .leading, spacing: 12) {
            Text("Undo “\(change.summary)”?").font(.headline)
            Label(explanation.restores, systemImage: "arrow.uturn.backward")
            Label(explanation.keeps, systemImage: "doc")
            if let command = explanation.command {
                Text(command)
                    .monospaced().textSelection(.enabled)
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.quaternary, in: .rect(cornerRadius: 6))
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Undo") {
                    dismiss()
                    undo()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(20)
        .frame(width: 440)
    }
}
