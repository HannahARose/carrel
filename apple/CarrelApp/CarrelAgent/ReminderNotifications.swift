// The agent's reminders as notifications (docs/design/reminders.md): permission is asked the
// first time a reminder is due, not at launch, and choosing one opens its item in the app,
// through the same carrel:// links the menu bar uses.

import CarrelKit
import Foundation
import UserNotifications

/// Posts reminders through the Notification Center, and opens what a chosen one is about.
final class ReminderNotifications: NSObject, ReminderPoster, UNUserNotificationCenterDelegate,
    @unchecked Sendable
{
    private static let archiveKey = "archive"
    private static let itemKey = "item"

    private let open: @Sendable (AgentLink) -> Void

    /// Notifications whose choosing calls `open` with the reminder's link.
    init(open: @escaping @Sendable (AgentLink) -> Void) {
        self.open = open
        super.init()
        UNUserNotificationCenter.current().delegate = self
    }

    func post(_ reminder: Reminder, archive: URL, name: String) async -> Bool {
        let center = UNUserNotificationCenter.current()
        // Asked once; afterwards the owner's answer stands.
        guard (try? await center.requestAuthorization(options: [.alert, .sound])) == true else {
            return false
        }
        let content = UNMutableNotificationContent()
        content.title = reminder.title
        content.subtitle = name
        content.body = reminder.text
        content.userInfo = [
            Self.archiveKey: archive.path(percentEncoded: false),
            Self.itemKey: reminder.item ?? "",
        ]
        let request = UNNotificationRequest(
            identifier: reminder.key, content: content, trigger: nil)
        do {
            try await center.add(request)
            return true
        } catch {
            return false
        }
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse
    ) async {
        let info = response.notification.request.content.userInfo
        guard let archive = info[Self.archiveKey] as? String else { return }
        let item = (info[Self.itemKey] as? String).flatMap { $0.isEmpty ? nil : $0 }
        open(AgentLink(archive: URL(filePath: archive), item: item))
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .list]
    }
}
