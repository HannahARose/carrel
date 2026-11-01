// The readings of what a proposal's item is, when the drafting model wasn't sure (#449): each
// with why the model read it so and what about the item doesn't fit it, and a button to use
// it, which opens the correction sheet with the card's title, summary and tags rewritten from
// it, as `carrel review pick` and `p` in the interactive mode do.

import CarrelKit
import SwiftUI

struct ReadingsSection: View {
    let proposal: Proposal
    let pick: (ProposalReading) -> Void

    var body: some View {
        Section {
            ForEach(proposal.readings, id: \.number) { reading in
                ReadingRow(reading: reading) { pick(reading) }
            }
        } header: {
            Text(proposal.readingsDoubted ? "What Is It?" : "Readings")
        } footer: {
            Text(
                "The model wasn’t sure what this is. Using another reading rewrites the card’s "
                    + "title, summary and tags; you see the change before it’s saved.")
        }
    }
}

/// One reading, with its button.
struct ReadingRow: View {
    let reading: ProposalReading
    let use: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text("\(reading.number). \(reading.title)").font(.headline)
                Spacer()
                if reading.onCard {
                    Text("On the card").font(.caption).foregroundStyle(.secondary)
                } else {
                    Button("Use This Reading", action: use)
                }
            }
            Text(reading.summary)
            if !reading.tags.isEmpty {
                Text(reading.tags.joined(separator: ", "))
                    .font(.caption).foregroundStyle(.secondary)
            }
            if !reading.reason.isEmpty {
                Text("Why: \(reading.reason)").font(.caption).foregroundStyle(.secondary)
            }
            ForEach(reading.against, id: \.self) { against in
                Label(Self.doesNotFit(against), systemImage: "exclamationmark.triangle")
                    .font(.caption).foregroundStyle(.orange)
            }
        }
        .textSelection(.enabled)
    }

    /// "the file name (…)" as "The file name (…) doesn't fit it".
    static func doesNotFit(_ against: String) -> String {
        against.prefix(1).uppercased() + against.dropFirst() + " doesn’t fit it"
    }
}

extension ArchiveWindowModel {
    /// Opens the correction sheet with `reading` of `proposal` picked, its change worked out
    /// (no model is needed) and shown, to save or not.
    func pickReading(_ reading: ProposalReading, of proposal: Proposal) {
        guard let archive else { return }
        let correction = CorrectionModel(
            archive: archive, target: .proposal(id: proposal.id, title: proposal.fields.title))
        correction.words = readingPickWords(number: reading.number)
        correction.saved = { [weak self] in await self?.refreshItems() }
        self.correction = correction
        Task { await correction.prepare() }
    }
}
