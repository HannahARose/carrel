// Add to Archive, from anywhere (docs/design/macos-integration.md, "Finder"): the agent offers it
// as a service, so it's in Finder's Quick Actions menu, the Services menu and the Touch Bar for any
// file or folder. It adds them to the intake queue through the agent's own request handler, as
// dropping them onto Carrel's window does, and says what happened in a notification.

import AppKit
import CarrelKit
import UserNotifications

/// The services in the agent's Info.plist (`NSServices`), by their `NSMessage`.
final class AgentQuickActions: NSObject {
    private let handler: AgentRequestHandler

    init(handler: AgentRequestHandler) {
        self.handler = handler
    }

    /// Add to Archive: each folder as one item.
    @objc func addToArchive(
        _ pasteboard: NSPasteboard, userData: String?,
        error: AutoreleasingUnsafeMutablePointer<NSString?>
    ) {
        add(from: pasteboard, each: false)
    }

    /// Add to Archive Entry by Entry: each entry inside a folder as its own item.
    @objc func addToArchiveEntryByEntry(
        _ pasteboard: NSPasteboard, userData: String?,
        error: AutoreleasingUnsafeMutablePointer<NSString?>
    ) {
        add(from: pasteboard, each: true)
    }

    private func add(from pasteboard: NSPasteboard, each: Bool) {
        let urls =
            pasteboard.readObjects(
                forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
            ?? []
        let paths = urls.map { $0.path(percentEncoded: false) }
        guard !paths.isEmpty else { return }
        let handler = handler
        Task {
            let reply = await handler.handle(
                .addFiles(archive: nil, paths: paths, each: each, whole: !each))
            await Self.tell(reply)
        }
    }

    /// Says how a request went: a notification, or an alert if notifications are off.
    static func tell(_ reply: AgentReply) async {
        let center = UNUserNotificationCenter.current()
        let allowed = (try? await center.requestAuthorization(options: [.alert])) ?? false
        let title =
            switch reply.outcome {
            case .done: "Added to the archive"
            case .refused, .failed: "Couldn’t add to the archive"
            }
        guard allowed else {
            let alert = NSAlert()
            alert.messageText = title
            alert.informativeText = reply.message
            NSApp.activate()
            alert.runModal()
            return
        }
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = reply.message
        try? await center.add(
            UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }
}
