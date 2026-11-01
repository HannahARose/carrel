// The Mac app: one window per archive, a welcome window when none is open, and Settings. As the
// host of the unit tests it's a bare app instead, with no windows, decision log or real defaults.

import AppKit
import CarrelKit
import SwiftUI

@main
enum CarrelMain {
    /// Whether this process hosts the unit tests, which XCTest says in the environment.
    static let isHostingTests = ProcessInfo.processInfo.environment.keys.contains {
        $0 == "XCTestConfigurationFilePath" || $0 == "XCTestBundlePath"
    }

    static func main() {
        guard isHostingTests else { return CarrelApp.main() }
        NSApplication.shared.setActivationPolicy(.prohibited)
        NSApplication.shared.run()
    }
}

extension UserDefaults {
    /// The app's defaults: its own domain, or while it hosts the tests a suite of their own,
    /// emptied at each run, so tests never read or write a developer's settings or recents.
    static let app: UserDefaults = {
        guard CarrelMain.isHostingTests else { return .standard }
        let suite = "io.github.hannaharose.Carrel.test-host"
        UserDefaults.standard.removePersistentDomain(forName: suite)
        return UserDefaults(suiteName: suite) ?? .standard
    }()
}

struct CarrelApp: App {
    @State private var app = AppModel()

    init() {
        // This device's decision log for the usage report, unless `report.log` is off.
        startDecisionLog()
    }

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
            ReviewCommands()
            ActiveCommands()
            ArchiveWindowCommands()
            BrowseCommands()
            LibrarianCommands()
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
