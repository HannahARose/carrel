// Status's parts from v0.6, as `carrel status` shows them: now and then a collection not
// opened in a long time, with an item or two from it, and the changes the librarian made on
// its own this week, which can be undone together.

import CarrelKit
import SwiftUI

struct StatusDiscoverySections: View {
    let model: ArchiveWindowModel
    @State private var confirmingUndo = false

    var body: some View {
        Group {
            if let found = model.discover?.resurfaced {
                Section("Not Opened in a While") {
                    Button {
                        model.screen = .collection(found.collection)
                    } label: {
                        LabeledContent(found.title, value: "last seen \(found.lastSeen)")
                    }
                    .buttonStyle(.plain)
                    .help("Show \(found.collection) in Browse")
                    ForEach(found.items, id: \.id) { item in
                        Button {
                            model.open(item: item.id)
                        } label: {
                            Label(item.title, systemImage: item.kind.symbol)
                        }
                        .linkButtonStyle()
                    }
                }
            }
            if let librarian = model.librarian, !librarian.changes.isEmpty {
                Section {
                    ForEach(librarian.changes.prefix(5), id: \.id) { change in
                        LabeledContent(change.summary, value: change.when)
                    }
                    if librarian.changes.count > 5 {
                        Text("and \(librarian.changes.count - 5) more, in History")
                            .foregroundStyle(.secondary)
                    }
                    Button("Undo All of Them…") { confirmingUndo = true }
                        .disabled(model.isReadOnly)
                } header: {
                    Text("The Librarian’s Changes This Week")
                }
                .confirmationDialog(
                    "Undo the librarian’s \(librarian.changes.count) changes this week?",
                    isPresented: $confirmingUndo
                ) {
                    Button("Undo All") { Task { await librarian.undoAll() } }
                } message: {
                    Text(
                        "Each is undone as a new change, newest first. Any that can’t be are left and listed."
                    )
                }
            }
        }
        .task(id: model.discover.map(ObjectIdentifier.init)) {
            await model.discover?.refreshResurfaced()
            await model.librarian?.refreshChanges()
        }
    }
}
