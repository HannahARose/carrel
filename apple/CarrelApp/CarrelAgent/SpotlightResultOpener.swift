// Spotlight's results (docs/design/macos-integration.md, "Spotlight"). The agent indexed
// them, so Spotlight hands a chosen one to the agent; it opens the link the result was indexed
// by in the app, which shows the item.

import AppKit
import CarrelKit

final class SpotlightResultOpener: NSObject, NSApplicationDelegate {
    func application(
        _ application: NSApplication, continue userActivity: NSUserActivity,
        restorationHandler: @escaping ([any NSUserActivityRestoring]) -> Void
    ) -> Bool {
        guard let link = AgentLink(spotlightResult: userActivity) else { return false }
        if let app = AgentHost.app {
            NSWorkspace.shared.open(
                [link.url], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration())
        } else {
            NSWorkspace.shared.open(link.url)
        }
        return true
    }
}
