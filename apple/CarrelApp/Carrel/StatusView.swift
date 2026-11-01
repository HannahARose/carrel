// Status: what the archive holds. The worker's progress, Recent and suggestions join it when
// the core's Status summary reaches CarrelKit (#233).

import CarrelKit
import SwiftUI

struct StatusView: View {
    let model: ArchiveWindowModel

    var body: some View {
        let summary = model.summary
        Form {
            Section("Archive") {
                LabeledContent("Name", value: model.name)
                LabeledContent("Folder") {
                    Text(model.root.path(percentEncoded: false))
                        .textSelection(.enabled)
                        .lineLimit(1).truncationMode(.middle)
                }
                LabeledContent("Access", value: model.isReadOnly ? "Read only" : "Read and write")
                LabeledContent("Version", value: versionSummary())
            }
            Section("Contents") {
                LabeledContent("Items", value: summary.itemCount, format: .number)
                LabeledContent("Collections", value: summary.collectionCount, format: .number)
                LabeledContent("In Holding", value: summary.holdingCount, format: .number)
                ForEach(summary.kinds, id: \.kind) { entry in
                    LabeledContent {
                        Text(entry.count, format: .number)
                    } label: {
                        Label(entry.kind.name, systemImage: entry.kind.symbol)
                    }
                }
            }
            if let intake = model.intake {
                IntakeSection(intake: intake, isReadOnly: model.isReadOnly)
            }
            StatusDiscoverySections(model: model)
            Section("Activity") {
                Text(
                    "The worker’s progress, recent changes and suggestions appear here in a later build."
                )
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .overlay {
            if model.isLoading, model.items.isEmpty { ProgressView() }
        }
    }
}
