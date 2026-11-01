// Status's Intake section: what waits in the intake queue on this device, and the worker's
// progress drafting it, read from the worker's status file.

import CarrelKit
import SwiftUI

struct IntakeSection: View {
    let intake: IntakeModel
    let isReadOnly: Bool

    var body: some View {
        Section("Intake") {
            if let line = intake.workerLine, let worker = intake.worker {
                LabeledContent("Worker") {
                    Text(line).lineLimit(1).truncationMode(.middle)
                }
                ProgressView(
                    value: Double(worker.done),
                    total: Double(max(worker.done + worker.waiting, 1))
                )
            } else if intake.isDrafting, let progress = intake.archive.progress {
                LabeledContent("Worker", value: progress.item ?? progress.title)
                if let fraction = progress.fraction {
                    ProgressView(value: fraction)
                } else {
                    ProgressView()
                }
            } else {
                LabeledContent("Worker", value: "Not running")
            }
            LabeledContent("Waiting", value: intake.queue.waiting.count, format: .number)
            if !intake.queue.failed.isEmpty {
                LabeledContent("Failed", value: intake.queue.failed.count, format: .number)
                    .help(
                        intake.queue.failed.compactMap(\.failure).joined(separator: "\n"))
            }
            if intake.canDraft, !isReadOnly {
                LabeledContent {
                    Button("Draft Now") { Task { await intake.draft() } }
                } label: {
                    Text("The agent isn’t running to draft what’s waiting.")
                        .foregroundStyle(.secondary)
                }
            }
            if intake.queue.waiting.isEmpty, intake.worker == nil, !isReadOnly {
                Text("Drop files or folders on the window, or choose File → Add to Archive…")
                    .foregroundStyle(.secondary)
            }
        }
    }
}
