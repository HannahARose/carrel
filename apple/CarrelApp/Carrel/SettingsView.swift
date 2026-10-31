// Settings (⌘,): what's set today, read only, and the agent's switch. Choosing models,
// downloads and backups become editable as CarrelKit gains those operations (#233).

import AppKit
import ServiceManagement
import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @State private var settings: LocalSettings?

    var body: some View {
        TabView {
            Tab("Models", systemImage: "cpu") { ModelsTab(settings: settings) }
            Tab("Search", systemImage: "magnifyingglass") { SearchTab(settings: settings) }
            Tab("Backups", systemImage: "externaldrive.badge.timemachine") {
                BackupsTab(settings: settings)
            }
            Tab("Agent", systemImage: "person.badge.clock") { AgentTab() }
        }
        .frame(width: 520, height: 380)
        .task(id: app.activeArchive) {
            let archive = app.activeArchive
            settings = await Task.detached { LocalSettings.read(archive: archive) }.value
        }
    }
}

private struct ModelsTab: View {
    let settings: LocalSettings?

    var body: some View {
        Form {
            Section("Drafting") {
                LabeledContent("Drafts cards", value: settings?.draftingModel ?? "Best available")
                LabeledContent(
                    "Asked again", value: settings?.secondModel ?? "Recommended, if installed")
            }
            Section("Installed") {
                if let settings, !settings.installedModels.isEmpty {
                    ForEach(settings.installedModels, id: \.self) { Text($0) }
                } else {
                    Text("No models downloaded yet.").foregroundStyle(.secondary)
                }
                if let folder = settings?.modelsFolder {
                    RevealButton(title: "Show Models Folder", url: folder)
                }
            }
            Section {
                Text(
                    "Choosing models and downloading them come to the app in a later build. Until then, use `carrel models`."
                )
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

private struct SearchTab: View {
    let settings: LocalSettings?

    var body: some View {
        Form {
            if let archive = settings?.archive {
                Section("Embedding model for “\(archive.name)”") {
                    LabeledContent("Model", value: archive.embeddingModel ?? "Not chosen")
                    Text("Every device searches this archive with the same embedding model.")
                        .foregroundStyle(.secondary)
                }
            } else {
                Text("Open an archive to see its embedding model.").foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
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
