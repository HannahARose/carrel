// Where the agent and the macOS extensions meet (docs/design/macos-integration.md): the app group
// they share, the agent's Mach service in it, and the files the agent keeps in its container.

import Foundation

public enum AgentService {
    /// The app group the app, the agent and the extensions share.
    public static let appGroup = "io.github.hannaharose.Carrel"
    /// The agent's Mach service: its bundle identifier, inside the app group.
    public static let machServiceName = appGroup + ".Agent"

    /// The app group's container, where the badge state file is kept.
    public static var container: URL {
        if let shared = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: appGroup)
        {
            return shared
        }
        #if os(macOS)
        return FileManager.default.homeDirectoryForCurrentUser
            .appending(path: "Library/Group Containers/\(appGroup)")
        #else
        return FileManager.default.temporaryDirectory.appending(path: appGroup)
        #endif
    }

    /// Where the agent keeps text and data the extensions send, for the intake queue.
    public static var sentFolder: URL {
        container.appending(path: "Sent")
    }

    /// The badge state file.
    public static var badgeStateFile: URL {
        container.appending(path: BadgeState.fileName)
    }

    static func encode<T: Encodable>(_ value: T) -> Data {
        (try? JSONEncoder().encode(value)) ?? Data()
    }
}
