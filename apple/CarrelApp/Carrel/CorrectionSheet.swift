// The correction sheet (⌘K): type the correction, see the changes the model works out, then
// confirm them.

import CarrelKit
import SwiftUI

struct CorrectionSheet: View {
    @Bindable var correction: CorrectionModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Correct “\(correction.target.title)”").font(.headline)
            switch correction.stage {
            case .writing:
                Text("Say what’s wrong, in your own words: a title, a date, where it belongs.")
                    .foregroundStyle(.secondary)
                TextField("Correction", text: $correction.words, axis: .vertical)
                    .lineLimit(3...8)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await correction.prepare() } }
            case .preparing:
                ProgressView {
                    Text(correction.archive.progress?.title ?? "Working out the correction")
                }
                .frame(maxWidth: .infinity, minHeight: 80)
            case .prepared(_, let changes):
                ChangesView(changes: changes, learnFact: $correction.learnFact)
            case .saved(let saved):
                Label(
                    saved.filed ? "Saved; undo it from History." : "Saved to the proposal.",
                    systemImage: "checkmark.circle")
            }
            if let problem = correction.problem {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
            }
            buttons
        }
        .padding(20)
        .frame(width: 480)
    }

    @ViewBuilder private var buttons: some View {
        HStack {
            switch correction.stage {
            case .writing, .preparing:
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Work Out Changes") { Task { await correction.prepare() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!correction.canPrepare)
            case .prepared(_, let changes):
                Button("Change Words") { correction.rewrite() }
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Confirm") { Task { await correction.confirm() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(changes.isEmpty)
            case .saved:
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}

/// What a correction would change: each field before and after, and a move.
private struct ChangesView: View {
    let changes: CorrectionChanges
    @Binding var learnFact: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if changes.isEmpty {
                Text("That changes nothing anyone would see, so there’s nothing to save.")
                    .foregroundStyle(.secondary)
            }
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 6) {
                ForEach(changes.changed, id: \.self) { field in
                    GridRow {
                        Text(field.capitalized).foregroundStyle(.secondary)
                        Text(changes.before.describe(field)).strikethrough()
                            .foregroundStyle(.secondary)
                        Image(systemName: "arrow.right").foregroundStyle(.secondary)
                        Text(changes.after.describe(field))
                    }
                }
                if let from = changes.movedFrom, let to = changes.movedTo {
                    GridRow {
                        Text("Collection").foregroundStyle(.secondary)
                        Text(from).strikethrough().foregroundStyle(.secondary)
                        Image(systemName: "arrow.right").foregroundStyle(.secondary)
                        Text(to)
                    }
                }
            }
            .textSelection(.enabled)
            if !changes.dropped.isEmpty {
                Text(
                    "Leaves off \(changes.dropped.joined(separator: ", ")), which don’t apply there."
                )
                .font(.callout)
            }
            if changes.keptSensitive {
                Text("Kept at sensitive: a rule finds a restricted value in the item.")
                    .font(.callout)
            }
            if let fact = changes.fact {
                Toggle(isOn: $learnFact) {
                    Text("Add to the glossary: \(fact.fact)")
                }
            }
            if let model = changes.model {
                Text("Worked out by \(model)").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

extension CardFields {
    /// The field `name`'s value, in words, for showing a change.
    func describe(_ name: String) -> String {
        switch name {
        case "title": title
        case "summary": summary ?? "none"
        case "tags": tags.isEmpty ? "none" : tags.joined(separator: ", ")
        case "sensitivity": sensitivity.name
        case "dates":
            [
                ("created", dates.created), ("modified", dates.modified),
                ("added", dates.added), ("worked", dates.worked), ("document", dates.document),
            ]
            .compactMap { label, value in value.map { "\(label) \($0)" } }
            .joined(separator: ", ")
        default: ""
        }
    }
}
