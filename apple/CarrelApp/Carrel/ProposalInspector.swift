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
            if !proposal.problems.isEmpty {
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
                Section("Duplicates") {
                    ForEach(proposal.duplicates, id: \.id) { duplicate in
                        LabeledContent(
                            duplicate.card.isEmpty ? duplicate.id : duplicate.card,
                            value: duplicate.why.joined(separator: ", "))
                    }
                }
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
        Task { try? await review.archive.chooseCompressed(id, expand: expand) }
    }
}
