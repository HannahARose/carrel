// A collection in the inspector, when Browse shows it with no item chosen: what it holds, and
// how eager Carrel is to rearrange it (docs/design/restructure.md, "Eagerness"): the archive's
// level, one inherited from a collection it's inside, or its own, set in its design document
// as one change History can undo. From here, Propose Changes looks over it for a better shape.

import CarrelKit
import SwiftUI

struct CollectionInspector: View {
    let collection: CollectionSummary
    let window: ArchiveWindowModel

    @State private var eagerness: EagernessAt?
    @State private var problem: String?

    /// The levels to choose, least eager first.
    static let levels = ["settled", "balanced", "eager"]

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label("Collection", systemImage: "folder")
                        .font(.caption).foregroundStyle(.secondary)
                    Text(collection.title).font(.title2.bold())
                    if let description = collection.description {
                        Text(description).foregroundStyle(.secondary)
                    }
                }
                .textSelection(.enabled)
                LabeledContent("Folder", value: collection.path)
                LabeledContent(
                    "Items", value: window.items(in: collection.path).count, format: .number)
            }
            CollectionFlagSection(model: window, collection: collection)
            Section {
                if let eagerness {
                    Picker("Eagerness", selection: chosen(eagerness)) {
                        Text("As the archive (\(eagerness.archiveLevel.capitalized))").tag("")
                        ForEach(Self.levels, id: \.self) { Text($0.capitalized).tag($0) }
                    }
                    .disabled(window.isReadOnly)
                    if let from = eagerness.fromCollection, from != collection.path {
                        Text("\(eagerness.level.capitalized), as \(from) sets it.")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                }
                if let problem {
                    Label(problem, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red).font(.callout)
                }
                Button("Propose Changes Here") {
                    guard let restructure = window.restructure else { return }
                    let path = collection.path
                    window.screen = .restructure
                    Task { await restructure.proposeSection(path) }
                }
                .disabled(window.isReadOnly || window.archive?.isWorking == true)
            } header: {
                Text("Rearranging")
            } footer: {
                Text(
                    "How readily Carrel proposes reorganizing this collection: Settled needs plenty of evidence, Eager less. Every proposal still waits for you."
                )
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .task(id: collection.path) { await refresh() }
    }

    /// The picker's value: the collection's own level, or empty for the archive's or one
    /// inherited.
    private func chosen(_ eagerness: EagernessAt) -> Binding<String> {
        Binding {
            eagerness.fromCollection == collection.path ? eagerness.level : ""
        } set: { level in
            Task { await set(level.isEmpty ? nil : level) }
        }
    }

    private func set(_ level: String?) async {
        guard let archive = window.archive else { return }
        problem = nil
        do {
            try await archive.setEagerness(of: collection.path, to: level)
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
        await refresh()
    }

    private func refresh() async {
        eagerness = try? await window.archive?.eagerness(at: collection.path)
    }
}
