// A proposal in the inspector: its card, editable in place, where it would go and why, what
// needs settling, and the decisions on it.

import CarrelKit
import SwiftUI

struct ProposalInspector: View {
    let proposal: Proposal
    let review: ReviewModel
    let correct: () -> Void

    @State private var edit: CardEditModel?

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label(
                        "\(proposal.kind.name) · proposal \(proposal.shortId)",
                        systemImage: proposal.kind.symbol
                    )
                    .font(.caption).foregroundStyle(.secondary)
                    Text(proposal.fields.title).font(.title2.bold())
                    Text(proposal.source).font(.caption).foregroundStyle(.secondary)
                }
                .textSelection(.enabled)
            }
            if proposal.draftAgain {
                DraftAgainSection(proposal: proposal, review: review)
            } else if !proposal.problems.isEmpty {
                Section("Needs Settling") {
                    ForEach(proposal.problems, id: \.self) { problem in
                        Label(problem, systemImage: "exclamationmark.circle")
                            .foregroundStyle(.orange)
                    }
                }
            }
            Section("Destination") {
                LabeledContent("Collection", value: proposal.destination)
                LabeledContent("Confidence", value: proposal.confidence.name.capitalized)
                if let tag = proposal.newTag {
                    LabeledContent("New tag", value: tag)
                }
                ForEach(proposal.reasons, id: \.self) { Text($0).foregroundStyle(.secondary) }
            }
            if let compressed = proposal.compressed {
                CompressedSection(proposal: proposal, compressed: compressed, review: review)
            }
            if !proposal.duplicates.isEmpty {
                DuplicatesSection(proposal: proposal, review: review)
            }
            if !proposal.sensitivityReasons.isEmpty {
                Section("Why It’s \(proposal.fields.sensitivity.name.capitalized)") {
                    ForEach(proposal.sensitivityReasons, id: \.self) { Text($0) }
                }
            }
            if let edit, edit.target == .proposal(proposal.id) {
                CardEditor(edit: edit)
            }
            Section {
                HStack {
                    Button("Correct…", action: correct)
                    Spacer()
                    ReviewActions(review: review, ids: [proposal.id])
                }
            }
        }
        .formStyle(.grouped)
        .task(id: proposal.id) {
            let edit = CardEditModel(archive: review.archive, target: .proposal(proposal.id))
            self.edit = edit
            await edit.load()
        }
    }
}

/// The archived items a proposal duplicates, each side by side with it, to file it as a
/// version of one or beside it.
private struct DuplicatesSection: View {
    let proposal: Proposal
    let review: ReviewModel
    @State private var comparisons: [Comparison] = []

    var body: some View {
        ForEach(comparisons, id: \.duplicate.id) { comparison in
            Section(
                "Duplicates \(comparison.duplicate.card.isEmpty ? comparison.duplicate.id : comparison.duplicate.card)"
            ) {
                Text(comparison.duplicate.why.joined(separator: ", ")).foregroundStyle(.secondary)
                Grid(
                    alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 4
                ) {
                    GridRow {
                        Text("")
                        Text("Proposed").font(.caption).foregroundStyle(.secondary)
                        Text("Archived").font(.caption).foregroundStyle(.secondary)
                    }
                    ForEach(comparison.rows, id: \.label) { row in
                        GridRow {
                            Text(row.label.capitalized).foregroundStyle(.secondary)
                            Text(row.proposed).fontWeight(row.differs ? .semibold : .regular)
                            Text(row.archived).fontWeight(row.differs ? .semibold : .regular)
                        }
                    }
                }
                .textSelection(.enabled)
                HStack {
                    Spacer()
                    Button("File as Version") { keep(comparison, asVersion: true) }
                    Button("Keep Both") { keep(comparison, asVersion: false) }
                }
            }
        }
        .task(id: proposal) { comparisons = await review.comparisons(of: proposal.id) }
    }

    private func keep(_ comparison: Comparison, asVersion: Bool) {
        let id = proposal.id
        let target = comparison.duplicate.id
        Task { await review.keepDuplicate(id, asVersion: asVersion, of: target) }
    }
}

/// What approving does with a compressed file, and the choice when it asks.
private struct CompressedSection: View {
    let proposal: Proposal
    let compressed: CompressedChoice
    let review: ReviewModel

    var body: some View {
        Section("Compressed") {
            LabeledContent(
                "On approval",
                value: compressed.asking
                    ? "Asks first" : compressed.expand ? "Expands" : "Stays compressed")
            if let files = compressed.files {
                LabeledContent("Files inside", value: files.formatted())
            }
            if let reason = compressed.reason {
                Text(reason).foregroundStyle(.secondary)
            }
            HStack {
                Button("Expand") { choose(expand: true) }
                    .disabled(compressed.expand && !compressed.asking)
                Button("Keep Compressed") { choose(expand: false) }
                    .disabled(!compressed.expand && !compressed.asking)
            }
        }
    }

    private func choose(expand: Bool) {
        let id = proposal.id
        Task { await review.chooseCompressed([id], expand: expand) }
    }
}

/// A proposal whose card or collection couldn't be settled, as when no text model could be
/// reached: what went wrong, with Draft Again first, before writing the card by hand.
private struct DraftAgainSection: View {
    let proposal: Proposal
    let review: ReviewModel

    var body: some View {
        Section("Needs Settling") {
            ForEach(proposal.problems, id: \.self) { problem in
                Label(problem, systemImage: "exclamationmark.circle")
                    .foregroundStyle(.orange)
            }
            HStack {
                Text("Carrel can draft it again with the models \(thisDevice) has now.")
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Draft Again") {
                    let id = proposal.id
                    Task { await review.redraft([id]) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(review.archive.progress != nil)
            }
        }
    }
}
