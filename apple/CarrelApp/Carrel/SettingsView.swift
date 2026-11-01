// Settings (⌘,): this Mac's and the archive's settings, the models, the embedding index, the
// providers and file types, backups, and the agent's switch. The archive's are those of the
// window last in front.

import AppKit
import ServiceManagement
import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @State private var settings: LocalSettings?
    @State private var model: SettingsModel?

    var body: some View {
        TabView {
            Tab("General", systemImage: "gearshape") { GeneralTab(model: model) }
            Tab("Models", systemImage: "cpu") {
                ModelsTab(model: model, folder: settings?.modelsFolder)
            }
            Tab("Search", systemImage: "magnifyingglass") { SearchTab(model: model) }
            Tab("Providers", systemImage: "puzzlepiece.extension") { ProvidersTab(model: model) }
            Tab("Backups", systemImage: "externaldrive.badge.timemachine") {
                BackupsTab(settings: settings)
            }
            Tab("Agent", systemImage: "person.badge.clock") { AgentTab() }
        }
        .frame(width: 640, height: 540)
        .task(id: app.activeArchive) {
            let archive = app.activeArchive
            settings = await Task.detached { LocalSettings.read(archive: archive) }.value
            model = archive.map { SettingsModel(url: $0) }
            await model?.load()
        }
    }
}

private struct BackupsTab: View {
    let settings: LocalSettings?

    var body: some View {
        Form {
            Section("Kopia") {
                if let kopia = settings?.kopia {
                    LabeledContent("Installed", value: kopia.path(percentEncoded: false))
                } else {
                    Text("Kopia isn’t installed. Backups use the installed Kopia, as the CLI does.")
                        .foregroundStyle(.secondary)
                }
            }
            if let archive = settings?.archive {
                Section("“\(archive.name)”") {
                    LabeledContent(
                        "Backups on this Mac", value: archive.backupsSetUp ? "Set up" : "Not set up"
                    )
                }
            }
            Section {
                Text(
                    "Setting up backups comes to the app in a later build. Until then, use `carrel backup`."
                )
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

private struct AgentTab: View {
    @State private var loginItem = AgentLoginItem()

    var body: some View {
        Form {
            Section {
                Toggle(
                    "Run the Carrel agent at login",
                    isOn: Binding(get: { loginItem.isOn }, set: { loginItem.set($0) }))
                Text(
                    "The agent drafts new items, notices work sessions as they happen, keeps search current and takes scheduled backups, while the app is closed too. Its menu bar list shows what’s checked out and on the desk."
                )
                .foregroundStyle(.secondary)
                if !loginItem.isOn {
                    Text(
                        "With it off, Carrel drafts new items while the app is open, as the command line does, and work sessions are noticed when the app or the command line runs."
                    )
                    .foregroundStyle(.secondary)
                }
            }
            if loginItem.needsApproval {
                Section {
                    Text("Allow the Carrel agent in System Settings → General → Login Items.")
                    Button("Open Login Items Settings") {
                        SMAppService.openSystemSettingsLoginItems()
                    }
                }
            }
            if let problem = loginItem.problem {
                Section { Text(problem).foregroundStyle(.red) }
            }
        }
        .formStyle(.grouped)
        .onAppear { loginItem.refresh() }
    }
}

private struct RevealButton: View {
    let title: String
    let url: URL

    var body: some View {
        Button(title, systemImage: "folder") {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        }
        .disabled(!FileManager.default.fileExists(atPath: url.path(percentEncoded: false)))
    }
}
