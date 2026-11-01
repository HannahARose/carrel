// Status's parts about syncing (FORMAT.md §7.8 and §7.12): this Mac's inbox, where changes
// go while another device is the writer, with what the writer skipped and why; and the
// holding folder, where a device without backups keeps the versions its changes replace
// until a snapshot has them, each of which can be restored to a folder.

import AppKit
import CarrelKit
import SwiftUI

/// A problem alert's title: `title`, or, when what it shows is a change that went into this
/// Mac's inbox for the writer instead of being made, a title saying so.
func problemTitle(_ title: String, _ archive: ArchiveModel?, showing problem: String? = nil)
    -> String
{
    guard let error = archive?.lastError, error.isSentToInbox,
        problem == nil || problem == error.message
    else { return title }
    return "Sent to the Writer"
}

/// This Mac's inbox, then the holding folder, when there's anything to say about them.
struct InboxStatusSection: View {
    let model: ArchiveWindowModel

    var body: some View {
        if let status = model.archive?.status {
            InboxSection(inbox: status.inbox)
            if status.holding.count > 0, let archive = model.archive {
                HoldingSection(archive: archive, holding: status.holding)
            }
        }
    }
}

/// Why changes made here go into the inbox, what waits there, and what the writer skipped.
private struct InboxSection: View {
    let inbox: InboxStatus

    var body: some View {
        if inbox.sending != nil || !inbox.waiting.isEmpty || !inbox.skipped.isEmpty {
            Section("This Mac’s Inbox") {
                if let sending = inbox.sending {
                    Label(sending, systemImage: "tray.and.arrow.up")
                }
                if !inbox.waiting.isEmpty {
                    LabeledContent(
                        "Waiting for the writer", value: inbox.waiting.count, format: .number)
                    ForEach(Array(inbox.waiting.enumerated()), id: \.offset) { _, waiting in
                        LabeledContent(waiting.what.capitalizedFirst, value: day(waiting.at))
                    }
                }
                ForEach(Array(inbox.skipped.enumerated()), id: \.offset) { _, skipped in
                    Label {
                        Text("Skipped “\(skipped.what)” (\(day(skipped.at)))")
                        Text(skipped.reason)
                    } icon: {
                        Image(systemName: "exclamationmark.triangle")
                    }
                }
            }
        }
    }
}

/// What waits in the holding folder for a snapshot, with its warnings, and each version, to
/// restore to a folder the owner chooses.
private struct HoldingSection: View {
    let archive: ArchiveModel
    let holding: HoldingStatus
    @State private var held: [HeldVersion] = []
    @State private var restored: URL?
    @State private var problem: String?

    var body: some View {
        Section {
            ForEach(holding.warnings, id: \.self) { warning in
                Label(warning, systemImage: "exclamationmark.triangle")
            }
            LabeledContent(
                "Waiting for a snapshot",
                value: "\(holding.count) version\(holding.count == 1 ? "" : "s"), "
                    + ByteCountFormatter.string(
                        fromByteCount: Int64(holding.size), countStyle: .file))
            ForEach(held, id: \.id) { version in
                LabeledContent {
                    Button("Restore…") { Task { await restore(version) } }
                } label: {
                    Text(version.title)
                    Text(
                        "\(version.name), \(version.why), \(day(version.at)) · "
                            + ByteCountFormatter.string(
                                fromByteCount: Int64(version.size), countStyle: .file))
                }
                .help(version.path)
            }
        } header: {
            Text("Holding Folder")
        } footer: {
            Text(
                "Versions replaced on a device without backups wait here until Carrel on a Mac with backups snapshots them and clears the folder."
            )
        }
        .task(id: holding.count) {
            held = (try? await archive.call { try $0.heldVersions() }) ?? []
        }
        .alert(
            "Restored",
            isPresented: Binding(get: { restored != nil }, set: { if !$0 { restored = nil } }),
            presenting: restored
        ) { url in
            Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([url]) }
            Button("OK", role: .cancel) {}
        } message: { url in
            Text("A copy is at \(url.path(percentEncoded: false)). The archive is unchanged.")
        }
        .alert(
            "Couldn’t Restore",
            isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })
        ) {
            Button("OK") { problem = nil }
        } message: {
            Text(problem ?? "")
        }
    }

    /// Asks for a folder, then copies `version` into it under its own name.
    private func restore(_ version: HeldVersion) async {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Restore Here"
        panel.message = "Choose a folder for a copy of “\(version.name)”."
        guard panel.runModal() == .OK, let folder = panel.url else { return }
        let to = folder.appending(path: version.name).path(percentEncoded: false)
        do {
            let path = try await archive.call { try $0.restoreHeld(id: version.id, to: to) }
            restored = URL(filePath: path)
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
    }
}

/// The date of an RFC 3339 time, as `YYYY-MM-DD`.
private func day(_ time: String) -> String {
    String(time.prefix(10))
}

extension String {
    /// The string with its first letter capitalized.
    fileprivate var capitalizedFirst: String {
        prefix(1).uppercased() + dropFirst()
    }
}
