// Status's Intake section: what waits in the intake queue on this device, each entry with
// Remove, Move to Front and Try Again as `carrel ingest` and the interactive mode's queue
// offer them, failures with their reasons, and the worker's progress drafting it, read from
// the worker's status file.

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
            ForEach(Array(intake.queue.waiting.enumerated()), id: \.element.id) { index, entry in
                QueuedRow(entry: entry, failed: false)
                    .contextMenu {
                        if !isReadOnly {
                            QueuedActions(intake: intake, entry: entry, first: index == 0)
                        }
                    }
            }
            if !intake.queue.failed.isEmpty {
                LabeledContent("Failed", value: intake.queue.failed.count, format: .number)
                ForEach(intake.queue.failed, id: \.id) { entry in
                    QueuedRow(entry: entry, failed: true)
                        .contextMenu {
                            if !isReadOnly {
                                QueuedActions(intake: intake, entry: entry, first: false)
                            }
                        }
                }
            }
            if intake.canDraft, !isReadOnly {
                let waits = PowerPolicy.heavyWorkWaits
                LabeledContent {
                    Button("Draft Now") { Task { await intake.draft() } }
                        .disabled(waits != nil)
                } label: {
                    Text(
                        waits
                            ?? (isMac
                                ? "The agent isn’t running to draft what’s waiting."
                                : "Drafting also runs in the background when this device has time.")
                    )
                    .foregroundStyle(.secondary)
                }
            }
            if intake.queue.waiting.isEmpty, intake.worker == nil, !isReadOnly {
                Text(
                    isMac
                        ? "Drop files or folders on the window, or choose File → Add to Archive…"
                        : "Add photos, scans and files with the + button, or share them to Carrel."
                )
                .foregroundStyle(.secondary)
            }
        }
    }
}

/// An entry in the intake queue: its name, when it was added, and why drafting it failed.
private struct QueuedRow: View {
    let entry: Queued
    let failed: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Label(
                URL(filePath: entry.path).lastPathComponent,
                systemImage: failed ? "exclamationmark.triangle" : "doc"
            )
            .lineLimit(1).truncationMode(.middle)
            .help(entry.path)
            Group {
                if let failure = entry.failure {
                    Text(
                        "\(failure) (tried \(entry.attempts) time\(entry.attempts == 1 ? "" : "s"))"
                    )
                    .foregroundStyle(failed ? .red : .secondary)
                } else {
                    Text("Added \(entry.added)").foregroundStyle(.secondary)
                }
            }
            .font(.caption)
            .textSelection(.enabled)
        }
        .padding(.leading, 12)
    }
}

/// What can be done with an entry in the queue.
private struct QueuedActions: View {
    let intake: IntakeModel
    let entry: Queued
    /// Whether it's drafted next already.
    let first: Bool

    var body: some View {
        if entry.failure != nil {
            Button("Try Again") { Task { await intake.tryAgain(entry) } }
        }
        Button("Move to Front") { Task { await intake.moveToFront(entry) } }
            .disabled(first)
        Divider()
        Button("Remove from Queue", role: .destructive) { Task { await intake.remove(entry) } }
        Button("Show in Finder") {
            revealInFiles([URL(filePath: entry.path)])
        }
    }
}
