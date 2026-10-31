// The Mac app: one window per archive, a welcome window when none is open, and Settings.

import CarrelKit
import SwiftUI

@main
struct CarrelApp: App {
    @State private var app = AppModel()

    var body: some Scene {
        Window("Welcome to Carrel", id: WelcomeView.windowID) {
            WelcomeView()
                .environment(app)
        }
        .windowResizability(.contentSize)
        // Links from the agent's menu reach an open window, or this one when none is open.
        .handlesExternalEvents(matching: [AgentLink.scheme])

        WindowGroup("Archive", id: ArchiveWindow.windowID, for: URL.self) { $url in
            if let url {
                ArchiveWindow(url: url)
                    .environment(app)
            }
        }
        .handlesExternalEvents(matching: [])
        .commands {
            ArchiveCommands(app: app)
        }

        Settings {
            SettingsView()
                .environment(app)
        }
    }
}

/// File menu commands for opening and creating archives.
struct ArchiveCommands: Commands {
    let app: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Archive…") { app.chooseNewArchive(then: show) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            Button("Open Archive…") { app.chooseArchive(then: show) }
                .keyboardShortcut("o", modifiers: [.command, .shift])
            Menu("Open Recent") {
                ForEach(app.recents.urls, id: \.self) { url in
                    Button(url.lastPathComponent) { app.open(url, then: show) }
                }
                Divider()
                Button("Clear Menu") { app.recents.clear() }
                    .disabled(app.recents.urls.isEmpty)
            }
        }
    }

    private func show(_ root: URL) {
        openWindow(id: ArchiveWindow.windowID, value: root)
    }
}
