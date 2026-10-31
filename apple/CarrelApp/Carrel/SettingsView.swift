// Settings (⌘,): what's set today, read only. Choosing models, downloads, backups and the
// agent become editable as CarrelKit gains those operations (#233) and the agent arrives (#237).

import AppKit
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
    var body: some View {
        Form {
            Section {
                Toggle("Run the Carrel agent at login", isOn: .constant(false))
                    .disabled(true)
                Text(
                    "The agent drafts new items, notices work sessions as they happen and keeps search current while the app is closed. It arrives in a later build."
                )
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
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
