// Settings → Backups: where the archive's backups stand on this Mac, with a button to the
// Backups screen, which sets them up, takes snapshots, checks and restores them.

#if os(macOS)
import CarrelKit
import SwiftUI

struct BackupsSettingsTab: View {
    let model: SettingsModel?
    @Environment(AppModel.self) private var app
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        SettingsForm(model: model) { model in
            Section {
                if let state = model.backups {
                    LabeledContent("Backups on this Mac", value: Self.status(state))
                    if let description = state.description {
                        LabeledContent("Repository", value: description)
                    }
                    if let repository = state.repository {
                        LabeledContent("Folder", value: repository)
                    }
                    LabeledContent("Snapshots", value: state.snapshots.count.formatted())
                    if let last = state.snapshots.first {
                        LabeledContent("Last snapshot", value: last.when)
                    }
                } else {
                    ProgressView().frame(maxWidth: .infinity)
                }
            } header: {
                Text(model.archive.map { "“\($0.name)”" } ?? "Backups")
            } footer: {
                Text(
                    "Backups use Kopia, as the command line does. Set them up, take a snapshot, check them or restore an item on the Backups screen."
                )
                .foregroundStyle(.secondary)
            }
            Section {
                Button("Open Backups", systemImage: "externaldrive.badge.timemachine") {
                    app.show(.backups, in: model.url)
                    openWindow(id: ArchiveWindow.windowID, value: model.url)
                }
            }
        }
    }

    /// Where backups stand, in a few words.
    static func status(_ state: BackupsState) -> String {
        if !state.installed { return "Kopia isn’t installed" }
        if !state.setUpHere {
            return state.description == nil ? "Not set up" : "Not connected on this Mac"
        }
        return state.reachable ? "Set up" : "Set up; the drive isn’t connected"
    }
}
#endif
