// Status: the reminders due, what the archive holds, the intake queue and the worker's
// progress, Recent, problems and suggestions, as `carrel status` shows them.

import CarrelKit
import SwiftUI

struct StatusView: View {
    let model: ArchiveWindowModel

    var body: some View {
        let summary = model.summary
        Form {
            StatusRemindersSection(window: model)
            Section("Archive") {
                LabeledContent("Name", value: model.name)
                LabeledContent("Folder") {
                    Text(model.root.path(percentEncoded: false))
                        .textSelection(.enabled)
                        .lineLimit(1).truncationMode(.middle)
                }
                LabeledContent("Access", value: model.isReadOnly ? "Read only" : "Read and write")
                LabeledContent("Version", value: versionSummary())
                if let archive = model.archive, archive.status?.hasHistory == false {
                    LabeledContent {
                        Button("Start History") { Task { try? await archive.startHistory() } }
                            .disabled(model.isReadOnly)
                    } label: {
                        Text("History")
                        Text("This archive has no history yet, so its changes can’t be undone.")
                    }
                }
                if let writer = model.archive?.status?.writer {
                    LabeledContent(
                        "Being changed by",
                        value:
                            "\(writer.purpose), \(writer.thisDevice ? "on \(thisDevice)" : "on \(writer.device)")"
                            + (writer.expired ? " (stopped)" : ""))
                }
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
            StatusArchiveSections(model: model)
            StatusFlaggedSection(model: model)
            StatusDiscoverySections(model: model)
            StatusQuestionsSection(window: model)
            StatusCrawlerSection(window: model)
            StatusGapsSection(window: model)
        }
        .formStyle(.grouped)
        .overlay {
            if model.isLoading, model.items.isEmpty { ProgressView() }
        }
    }
}
