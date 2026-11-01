// Carrel Agent: the login item that keeps working while the app is closed (docs/design/
// mac-app.md, "The background agent"). It follows the archives the app has open or opened
// recently, runs CarrelKit's Agent for them, and shows the menu bar list. It also answers the
// macOS extensions' requests over XPC and keeps their badge state file (docs/design/
// macos-integration.md).

import AppKit
import CarrelKit
import SwiftUI

@main
struct CarrelAgentApp: App {
    @State private var agent = AgentHost()
    @NSApplicationDelegateAdaptor private var spotlightResults: SpotlightResultOpener

    var body: some Scene {
        MenuBarExtra {
            AgentMenu(host: agent)
        } label: {
            Image(systemName: agent.agent.activity == nil ? "archivebox" : "archivebox.fill")
        }
    }
}

/// The agent, the archives it follows, and whether it's paused, kept between launches.
@Observable
final class AgentHost {
    /// The app's defaults, which hold its recent archives.
    static let appDomain = "io.github.hannaharose.Carrel"
    /// Set to archive folders separated by `:` to follow those instead of the app's recent
    /// archives, as in manual checks against a temporary archive.
    static let archivesEnv = "CARREL_AGENT_ARCHIVES"
    /// Set to offer the XPC service even with `CARREL_AGENT_ARCHIVES`.
    static let serviceEnv = "CARREL_AGENT_SERVICE"
    private static let pausedKey = "paused"

    let agent: Agent
    private var loop: Task<Void, Never>?
    private var service: AgentXPCListener?

    init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        agent = Agent(
            options: .app(helpers: Self.helpers), badges: AgentService.badgeStateFile,
            spotlight: CoreSpotlightIndex())
        let override = environment[Self.archivesEnv].map {
            $0.split(separator: ":").map { URL(filePath: String($0)) }
        }
        if UserDefaults.standard.bool(forKey: Self.pausedKey) { agent.pause() }
        // Run from the build rather than as a login item, it has no Mach service to offer.
        if override == nil || environment[Self.serviceEnv] != nil {
            let service = AgentXPCListener(
                handler: AgentRequestHandler(agent: agent) { [weak self] in self?.open($0) })
            service.resume()
            self.service = service
        }
        loop = Task { [agent] in
            await agent.run { override ?? Self.recentArchives() }
        }
    }

    /// The archives the app has open or opened recently, newest first.
    static func recentArchives() -> [URL] {
        (UserDefaults(suiteName: appDomain)?.stringArray(forKey: "recentArchives") ?? [])
            .map { URL(filePath: $0) }
    }

    /// The app bundle the agent is inside: `Carrel.app/Contents/Library/LoginItems/Carrel
    /// Agent.app`.
    static var app: URL? {
        let url = Bundle.main.bundleURL
            .deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        return url.pathExtension == "app" ? url : nil
    }

    /// The provider helpers bundled in the app.
    static var helpers: URL? {
        app?.appending(path: "Contents/Helpers")
    }

    func togglePause() {
        if agent.isPaused { agent.resume() } else { agent.pause() }
        UserDefaults.standard.set(agent.isPaused, forKey: Self.pausedKey)
    }

    /// Shows `link` in the app.
    func open(_ link: AgentLink) {
        if let app = Self.app {
            NSWorkspace.shared.open(
                [link.url], withApplicationAt: app,
                configuration: NSWorkspace.OpenConfiguration())
        } else {
            NSWorkspace.shared.open(link.url)
        }
    }

    func openApp() {
        if let app = Self.app {
            NSWorkspace.shared.openApplication(
                at: app, configuration: NSWorkspace.OpenConfiguration())
        }
    }
}
