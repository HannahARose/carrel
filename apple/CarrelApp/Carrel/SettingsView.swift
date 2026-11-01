// Settings (⌘,): this Mac's and the archive's settings, the models, the embedding index, the
// providers and file types, backups, and the agent's switch. The archive's are those of the
// window last in front. On iPad and iPhone it's a sheet of the same pages, without the Mac's
// backups and agent, plus what this device does in the background.

import SwiftUI

#if os(macOS)
    import ServiceManagement
#endif

struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @State private var settings: LocalSettings?
    @State private var model: SettingsModel?

    var body: some View {
        pages
            .task(id: app.activeArchive) {
            let archive = app.activeArchive
            settings = await Task.detached { LocalSettings.read(archive: archive) }.value
            model = archive.map { SettingsModel(url: $0) }
            await model?.load()
        }
    }

    #if os(macOS)
        private var pages: some View {
            TabView {
                Tab("General", systemImage: "gearshape") { GeneralTab(model: model) }
                Tab("Models", systemImage: "cpu") {
                    ModelsTab(model: model, folder: settings?.modelsFolder)
                }
                Tab("Search", systemImage: "magnifyingglass") { SearchTab(model: model) }
                Tab("Providers", systemImage: "puzzlepiece.extension") {
                    ProvidersTab(model: model)
                }
                Tab("Backups", systemImage: "externaldrive.badge.timemachine") {
                    BackupsSettingsTab(model: model)
                }
                Tab("Agent", systemImage: "person.badge.clock") { AgentTab() }
            }
            .frame(width: 640, height: 540)
        }
    #else
        @Environment(\.dismiss) private var dismiss

        private var pages: some View {
            NavigationStack {
                List {
                    NavigationLink {
                        GeneralTab(model: model).navigationTitle("General")
                    } label: {
                        Label("General", systemImage: "gearshape")
                    }
                    NavigationLink {
                        ModelsTab(model: model, folder: settings?.modelsFolder)
                            .navigationTitle("Models")
                    } label: {
                        Label("Models", systemImage: "cpu")
                    }
                    NavigationLink {
                        SearchTab(model: model).navigationTitle("Search")
                    } label: {
                        Label("Search", systemImage: "magnifyingglass")
                    }
                    NavigationLink {
                        ProvidersTab(model: model).navigationTitle("Providers")
                    } label: {
                        Label("Providers", systemImage: "puzzlepiece.extension")
                    }
                    NavigationLink {
                        DeviceSettingsPage().navigationTitle("This Device")
                    } label: {
                        Label("This Device", systemImage: "battery.100percent.bolt")
                    }
                }
                .navigationTitle("Settings")
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { dismiss() }
                    }
                }
            }
        }
    #endif
}

#if os(macOS)
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
#endif

private struct RevealButton: View {
    let title: String
    let url: URL

    var body: some View {
        Button(title, systemImage: "folder") {
            revealInFiles([url])
        }
        .disabled(!FileManager.default.fileExists(atPath: url.path(percentEncoded: false)))
    }
}
