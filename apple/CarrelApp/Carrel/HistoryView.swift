// History: every change, newest first. `u` in the list, or History → Undo Change…, undoes
// the chosen one after a confirmation sheet.

import CarrelKit
import SwiftUI

struct HistoryView: View {
    @Bindable var model: HistoryScreenModel

    var body: some View {
        List(selection: $model.selectedID) {
            ForEach(model.changes, id: \.id) { change in
                VStack(alignment: .leading, spacing: 2) {
                    Text(change.summary).lineLimit(2)
                    HStack(spacing: 6) {
                        Text(change.when)
                        if let operation = change.operation { Text("· \(operation)") }
                        if let agent = change.agent { Text("· by the \(agent)") }
                    }
                    .font(.caption).foregroundStyle(.secondary)
                }
            }
            if model.canLoadMore {
                HStack {
                    Text("The newest \(model.readCount) changes")
                        .font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Load More") { Task { await model.loadMore() } }
                    Button("Load All") { Task { await model.loadAll() } }
                }
                .selectionDisabled()
            }
        }
        .toolbar {
            Picker("Show", selection: $model.librarianOnly) {
                Text("All Changes").tag(false)
                Text("The Librarian’s").tag(true)
            }
            .pickerStyle(.segmented)
            .help("Show every change, or only those the librarian made on its own")
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
            if model.changes.isEmpty, model.librarianOnly {
                ContentUnavailableView(
                    "No Changes by the Librarian", systemImage: "books.vertical",
                    description: Text(
                        model.canLoadMore
                            ? "None of the newest \(model.readCount) changes are the librarian’s."
                            : "The librarian hasn’t changed anything on its own."))
            } else if model.changes.isEmpty, model.archive.status?.hasHistory == false {
                ContentUnavailableView {
                    Label("No History Yet", systemImage: "clock.arrow.circlepath")
                } description: {
                    Text("Start it, and every change to the archive is kept here, to undo.")
                } actions: {
                    Button("Start History") { Task { try? await model.archive.startHistory() } }
                        .disabled(model.archive.handle.isReadOnly())
                }
            } else if model.changes.isEmpty {
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
                    if let agent = change.agent {
                        LabeledContent(
                            "Made by",
                            value: "The \(agent), on its own")
                    }
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
