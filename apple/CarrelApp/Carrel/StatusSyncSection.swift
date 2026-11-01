// Status's Sync section (docs/design/sync-and-mobile.md, "In each interface"), as `carrel sync
// status` shows it: who holds the writer lock and until when, each device's inbox backlog, the
// holding folder waiting for a snapshot, re-checks waiting for a stronger device, the files
// iCloud evicted from this Mac, the collections kept downloaded, and iCloud's conflicted
// copies, each settled by keeping the original or the copy. An archive left behind by a move
// into iCloud Drive says where it went; one outside iCloud Drive offers to move there.

import CarrelKit
import SwiftUI

/// The sync status, read through the core, and settling conflicted copies.
@Observable
final class SyncStatusModel {
    let archive: ArchiveModel
    /// The status, once read.
    private(set) var status: SyncStatus?
    /// Why it couldn't be read, or a copy couldn't be settled.
    private(set) var problem: String?
    /// The conflicted copies being settled.
    private(set) var settling: Set<String> = []

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    /// Reads the status again.
    func load() async {
        do {
            status = try await archive.syncStatus()
            problem = nil
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
    }

    /// Settles the conflicted copy `conflict`, keeping the copy (`keepCopy`) or the original;
    /// the other goes to the Trash.
    func settle(_ conflict: ConflictedCopy, keepCopy: Bool) async {
        settling.insert(conflict.copy)
        defer { settling.remove(conflict.copy) }
        do {
            try await archive.resolveConflict(conflict.copy, keepCopy: keepCopy)
            problem = nil
        } catch {
            archive.lastError = nil
            problem = message(for: error)
            return
        }
        await load()
    }
}

struct StatusSyncSection: View {
    let window: ArchiveWindowModel

    @State private var model: SyncStatusModel?

    var body: some View {
        Section("Sync") {
            if let model {
                if let status = model.status {
                    SyncStatusRows(window: window, model: model, status: status)
                }
                if let problem = model.problem {
                    Label(problem, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
                }
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
        }
        // Read again whenever Status is, as after a change.
        .task(id: window.archive?.status) {
            guard let archive = window.archive else { return }
            if model?.archive !== archive { model = SyncStatusModel(archive: archive) }
            await model?.load()
        }
    }
}

/// The sync status's rows.
private struct SyncStatusRows: View {
    let window: ArchiveWindowModel
    let model: SyncStatusModel
    let status: SyncStatus

    var body: some View {
        if let movedTo = status.movedTo {
            Label {
                Text("This is the old copy. The archive was moved to \(movedTo).")
                    .textSelection(.enabled)
            } icon: {
                Image(systemName: "exclamationmark.triangle").foregroundStyle(.yellow)
            }
        }
        LabeledContent("Synced Folder") {
            if status.synced {
                Text("Yes")
            } else {
                Button("Move to iCloud Drive…") { window.startICloudMove() }
                    .disabled(window.isReadOnly || window.archive == nil)
            }
        }
        LabeledContent("Writer Lock", value: syncWriterLockText(status.writer))
        ForEach(status.inboxes, id: \.device) { inbox in
            LabeledContent(
                inbox.thisDevice ? "Inbox on This Mac" : "Inbox on \(inbox.device)",
                value: "\(inbox.waiting) waiting")
        }
        holding
        if status.rechecks > 0 {
            LabeledContent("Re-checks", value: "\(status.rechecks) waiting for a stronger device")
        }
        notDownloaded
        if !status.keptDownloaded.isEmpty {
            LabeledContent("Kept Downloaded", value: status.keptDownloaded.joined(separator: ", "))
        }
        ForEach(status.conflicts, id: \.copy) { conflict in
            ConflictRow(window: window, model: model, conflict: conflict)
        }
    }

    @ViewBuilder private var holding: some View {
        if status.holdingFiles > 0 {
            LabeledContent {
                Text(
                    "\(status.holdingFiles) files, "
                        + status.holdingBytes.formatted(.byteCount(style: .file)))
            } label: {
                if status.holdingWarn {
                    Label("Holding Folder", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.orange)
                } else {
                    Text("Holding Folder")
                }
                Text(
                    "Waiting for a snapshot on a Mac"
                        + (status.holdingOldest.map { ", since \($0)" } ?? ""))
            }
        }
    }

    @ViewBuilder private var notDownloaded: some View {
        if !status.notDownloaded.isEmpty {
            DisclosureGroup {
                ForEach(status.notDownloaded, id: \.self) { path in
                    Text(path).lineLimit(1).truncationMode(.middle).help(path)
                }
            } label: {
                LabeledContent(
                    "Not Downloaded",
                    value: "\(status.notDownloaded.count) files"
                        + (status.notDownloadedBytes > 0
                            ? ", " + status.notDownloadedBytes.formatted(.byteCount(style: .file))
                            : ""))
            }
        }
    }
}

/// Who holds the writer lock, and until when, as the Sync section says it.
func syncWriterLockText(_ writer: WriterLock?) -> String {
    guard let writer else { return "Free" }
    let who = writer.thisDevice ? "This Mac" : writer.device
    let until =
        (try? Date(writer.expires, strategy: .iso8601))
        .map { $0.formatted(date: .abbreviated, time: .shortened) } ?? writer.expires
    return writer.expired
        ? "\(who), \(writer.purpose) (expired \(until))"
        : "\(who), \(writer.purpose), until \(until)"
}

/// A conflicted copy, with Keep Original and Keep Copy.
private struct ConflictRow: View {
    let window: ArchiveWindowModel
    let model: SyncStatusModel
    let conflict: ConflictedCopy

    var body: some View {
        LabeledContent {
            HStack {
                Button("Keep Original") { Task { await model.settle(conflict, keepCopy: false) } }
                Button("Keep Copy") { Task { await model.settle(conflict, keepCopy: true) } }
            }
            .disabled(window.isReadOnly || model.settling.contains(conflict.copy))
        } label: {
            Label(conflict.original, systemImage: "doc.on.doc")
            Text("Conflicted copy: \(conflict.copy). The other goes to the Trash.")
        }
        .help(conflict.copy)
    }
}
