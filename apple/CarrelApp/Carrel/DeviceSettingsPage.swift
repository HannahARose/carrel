// Settings → This Device, on iPad and iPhone: what this device does in the background and when,
// by the battery rules, where checked-out working copies go, and what only a Mac does.

#if os(iOS)
import SwiftUI
import UIKit

struct DeviceSettingsPage: View {
    @State private var refresh = UIApplication.shared.backgroundRefreshStatus
    @State private var heavyWork = PowerPolicy.heavyWorkWaits

    var body: some View {
        Form {
            Section {
                LabeledContent("Background App Refresh", value: refreshWords)
                LabeledContent(
                    "Drafting and the index", value: heavyWork == nil ? "Run now" : "Waiting")
                if let heavyWork { Text(heavyWork).foregroundStyle(.secondary) }
            } header: {
                Text("In the Background")
            } footer: {
                Text(
                    PowerPolicy.isPhone
                        ? "This iPhone drafts new items and rebuilds the index only while it’s charging. Reviewing, answering, browsing and searching work at any time."
                        : "This iPad drafts new items in the background when it has time, except in Low Power Mode."
                )
            }
            Section {
                LabeledContent("Working copies") {
                    Text("Files → On My \(PowerPolicy.isPhone ? "iPhone" : "iPad") → Carrel")
                }
                Button("Show in Files", systemImage: "folder") {
                    revealInFiles([MobileLibrary.documents])
                }
            } header: {
                Text("Checkouts")
            } footer: {
                Text(
                    "Checking out puts a working copy in Carrel’s folder in Files unless you choose another place, such as iCloud Drive."
                )
            }
            Section {
                Text(
                    "Backups (Kopia), the agent, and the Finder, Quick Look and Spotlight extensions run only on a Mac. What’s replaced here is kept in the archive’s holding folder until a Mac snapshots it."
                )
                .foregroundStyle(.secondary)
            } header: {
                Text("Done on a Mac")
            }
        }
        .onReceive(
            NotificationCenter.default.publisher(for: UIDevice.batteryStateDidChangeNotification)
        ) { _ in
            heavyWork = PowerPolicy.heavyWorkWaits
        }
        .onAppear {
            refresh = UIApplication.shared.backgroundRefreshStatus
            heavyWork = PowerPolicy.heavyWorkWaits
        }
    }

    private var refreshWords: String {
        switch refresh {
        case .available: "On"
        case .denied: "Off"
        case .restricted: "Restricted"
        @unknown default: "Unknown"
        }
    }
}
#endif
