// The Backups screen: where backups stand, the snapshots taken with each change, newest first,
// and Set Up, Snapshot Now and Verify in the toolbar. The inspector shows the chosen snapshot's
// change, with Restore.

#if os(macOS)
import AppKit
import CarrelKit
import SwiftUI

struct BackupsView: View {
    @Bindable var backups: BackupsModel

    var body: some View {
        List(selection: $backups.selection) {
            if let notice = backups.notice {
                Section { Label(notice, systemImage: noticeSymbol).foregroundStyle(.secondary) }
            }
            if backups.working != nil || backups.report != nil {
                Section { BackupActivity(backups: backups) }
            }
            Section("Snapshots") {
                ForEach(backups.snapshots) { change in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(change.summary).lineLimit(2)
                        Text(change.when).font(.caption).foregroundStyle(.secondary)
                    }
                    .tag(change.id)
                }
            }
        }
        .overlay {
            if backups.state != nil, backups.snapshots.isEmpty, backups.notice == nil {
                ContentUnavailableView(
                    "No Snapshots Yet", systemImage: "externaldrive.badge.timemachine",
                    description: Text("Approving a batch takes one, or choose Snapshot Now."))
            }
        }
        .toolbar {
            ToolbarItemGroup {
                Button("Set Up Backups…", systemImage: "externaldrive.badge.plus") {
                    chooseFolder()
                }
                .help("Choose a folder or drive for the backups, or connect to one already there")
                Button("Snapshot Now", systemImage: "camera") {
                    Task { await backups.snapshotNow() }
                }
                .disabled(!backups.isSetUp || backups.working != nil)
                Button("Verify…", systemImage: "checkmark.shield") {
                    backups.choosingVerify = true
                }
                .help("Check every snapshot, reading back as much of the files as you choose")
                .disabled(!backups.isSetUp || backups.working != nil)
            }
        }
        .sheet(item: $backups.setup) { _ in BackupSetupSheet(backups: backups) }
        .sheet(isPresented: $backups.choosingVerify) { VerifySheet(backups: backups) }
        .task { await backups.refresh() }
    }

    private var noticeSymbol: String {
        backups.state?.driveMissing == true ? "externaldrive.badge.xmark" : "info.circle"
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Choose"
        panel.message =
            "Choose the folder for the backups: an empty one for new backups, or an existing repository."
        if panel.runModal() == .OK, let url = panel.url { backups.choose(folder: url) }
    }
}

extension BackupSetup: Identifiable {
    var id: URL { folder }
}

/// What's under way, or what the last operation did.
private struct BackupActivity: View {
    let backups: BackupsModel

    var body: some View {
        if let working = backups.working {
            HStack {
                ProgressView().controlSize(.small)
                Text(working)
            }
        } else if let report = backups.report {
            VStack(alignment: .leading, spacing: 6) {
                Text(report).textSelection(.enabled)
                if let restored = backups.restored {
                    Button("Show in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting([restored])
                    }
                }
            }
        }
    }
}

/// The password for the folder chosen: twice for a new repository, once to connect.
private struct BackupSetupSheet: View {
    @Bindable var backups: BackupsModel

    var body: some View {
        if let setup = Binding($backups.setup) {
            Form {
                Section {
                    LabeledContent("Folder") {
                        Text(setup.wrappedValue.folder.path(percentEncoded: false))
                            .lineLimit(1).truncationMode(.middle)
                    }
                    TextField(
                        "Name", text: setup.description,
                        prompt: Text(setup.wrappedValue.folder.lastPathComponent))
                } header: {
                    Text(setup.wrappedValue.isNew ? "New Backups" : "Connect to Backups")
                }
                Section {
                    SecureField("Password", text: setup.password)
                    if setup.wrappedValue.isNew {
                        SecureField("Same password again", text: setup.confirmation)
                    }
                } footer: {
                    Text(
                        setup.wrappedValue.isNew
                            ? "Backups are encrypted with this password, and Kopia keeps it in this Mac’s keychain. Keep it in a password manager too: if it’s lost, the backups can’t be read."
                            : "The password the backups were set up with. Kopia keeps it in this Mac’s keychain."
                    )
                    .foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
            .frame(width: 460)
            .disabled(backups.working != nil)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { backups.setup = nil }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(setup.wrappedValue.isNew ? "Create" : "Connect") {
                        Task { await backups.setUp() }
                    }
                    .disabled(setup.wrappedValue.problem != nil || backups.working != nil)
                }
                ToolbarItem(placement: .status) {
                    if let working = backups.working {
                        HStack {
                            ProgressView().controlSize(.small)
                            Text(working)
                        }
                    }
                }
            }
        }
    }
}

/// The inspector for a snapshot: the change it was taken with, and restoring an item from it.
struct BackupInspector: View {
    let change: Change
    let backups: BackupsModel
    /// The archive's items, to choose one to restore.
    let items: [ItemSummary]
    @State private var choosing = false

    var body: some View {
        Form {
            Section("Taken With") {
                Text(change.summary).textSelection(.enabled)
                LabeledContent("When", value: change.when)
                if let snapshot = change.snapshot {
                    LabeledContent("Snapshot") {
                        Text(snapshot).textSelection(.enabled).lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                LabeledContent("Change") {
                    Text(change.id.prefix(10)).textSelection(.enabled).monospaced()
                }
            }
            Section {
                Button("Restore an Item…", systemImage: "arrow.uturn.backward.circle") {
                    choosing = true
                }
                .disabled(!backups.isSetUp || backups.working != nil)
            } footer: {
                Text(
                    "Restores an item’s contents as they were then into a new folder beside the archive, or one you choose. Nothing in the archive changes."
                )
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .sheet(isPresented: $choosing) {
            RestoreItemSheet(items: items) { id, folder in
                choosing = false
                Task { await backups.restore(item: id, to: folder) }
            }
        }
    }
}

/// How much of the files Verify reads back, from 1% to all of them.
private struct VerifySheet: View {
    let backups: BackupsModel
    @Environment(\.dismiss) private var dismiss
    @State private var percent = Double(BackupsModel.defaultVerifyPercent)

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Verify the Backups").font(.headline)
            Text(
                "Every snapshot’s structure is checked, and this much of the files’ contents is read back and compared. More takes longer."
            )
            .fixedSize(horizontal: false, vertical: true)
            HStack {
                Slider(value: $percent, in: 1...100, step: 1) { Text("Read back") }
                Stepper(
                    "\(Int(percent))%", value: $percent, in: 1...100, step: 1
                )
                .monospacedDigit()
                .fixedSize()
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Verify") {
                    let chosen = UInt8(percent)
                    dismiss()
                    Task { await backups.verify(percent: chosen) }
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 420)
    }
}

/// Choosing the item to restore, among those with contents, and where it goes: a new folder
/// beside the archive, or a new place the owner names.
private struct RestoreItemSheet: View {
    let items: [ItemSummary]
    let restore: (String, URL?) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var filter = ""
    @State private var chosen: ItemSummary.ID?
    @State private var place: URL?

    private var shown: [ItemSummary] {
        items.filter { [.file, .folder, .bundle].contains($0.kind) }
            .filter { filter.isEmpty || $0.title.localizedCaseInsensitiveContains(filter) }
            .sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
    }

    var body: some View {
        VStack(alignment: .leading) {
            Text("Restore Which Item?").font(.headline)
            TextField("Filter", text: $filter)
            List(shown, selection: $chosen) { item in
                Label(item.title, systemImage: item.kind.symbol)
            }
            .frame(minHeight: 260)
            LabeledContent("Restore to") {
                HStack {
                    Text(place?.path(percentEncoded: false) ?? "A new folder beside the archive")
                        .lineLimit(1).truncationMode(.middle)
                    Button("Choose…") { choosePlace() }
                        .disabled(chosen == nil)
                    if place != nil {
                        Button("Beside the Archive") { place = nil }
                    }
                }
            }
        }
        .padding()
        .frame(width: 420)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("Restore") { if let chosen { restore(chosen, place) } }
                    .disabled(chosen == nil)
            }
        }
    }

    /// Asks where the restored copy goes, under a name that isn't taken.
    private func choosePlace() {
        let panel = NSSavePanel()
        panel.canCreateDirectories = true
        panel.prompt = "Restore Here"
        panel.message = "Choose where the restored copy goes. Nothing in the archive changes."
        panel.nameFieldStringValue =
            items.first { $0.id == chosen }
            .map { URL(filePath: $0.item ?? $0.card).lastPathComponent } ?? ""
        if panel.runModal() == .OK, let url = panel.url { place = url }
    }
}
#endif
