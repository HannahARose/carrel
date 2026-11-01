// Heavy work and the battery (docs/design/sync-and-mobile.md, "iPad and iPhone"): drafting and
// rebuilding the index run on a Mac whenever they're asked for, on an iPad unless Low Power
// Mode is on or it's too hot, and on an iPhone only while it's charging. On iPad and iPhone,
// `BackgroundWork` drains the intake queue and applies other devices' inboxes in background
// tasks, which the system runs when the device has time (and, on an iPhone, power).

import CarrelKit
import Foundation

#if os(iOS)
import BackgroundTasks
import UIKit
#endif

/// Whether this device may do heavy work now, by the battery rules.
enum PowerPolicy {
    /// Whether this device is an iPhone, whose battery rules are the strictest.
    static var isPhone: Bool {
        #if os(iOS)
        UIDevice.current.userInterfaceIdiom == .phone
        #else
        false
        #endif
    }

    /// Why heavy work, such as drafting or rebuilding the index, waits now, or `nil` when it
    /// may run.
    @MainActor
    static var heavyWorkWaits: String? {
        #if os(iOS)
        let device = UIDevice.current
        device.isBatteryMonitoringEnabled = true
        let charging = device.batteryState == .charging || device.batteryState == .full
        if isPhone, !charging {
            return "Drafting and rebuilding the index wait until this iPhone is charging."
        }
        if ProcessInfo.processInfo.isLowPowerModeEnabled, !charging {
            return "Drafting and rebuilding the index wait while Low Power Mode is on."
        }
        if ProcessInfo.processInfo.thermalState.rawValue
            >= ProcessInfo.ThermalState.serious.rawValue
        {
            return "Drafting and rebuilding the index wait until this device cools down."
        }
        return nil
        #else
        return nil
        #endif
    }
}

#if os(iOS)
/// The app's background tasks on iPad and iPhone. The system calls their handlers off the main
/// thread.
nonisolated enum BackgroundWork {
    /// Drafts what's waiting in the intake queue, which also applies other devices' inboxes
    /// when this device can write: heavy, so on an iPhone it waits for power.
    static let intake = identifier("intake")
    /// Applies other devices' inboxes, a short task.
    static let inbox = identifier("inbox")

    private static func identifier(_ name: String) -> String {
        "\(Bundle.main.bundleIdentifier ?? "io.github.hannaharose.Carrel").\(name)"
    }

    /// Registers the tasks' handlers; called once, as the app launches.
    static func register() {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: intake, using: nil) { task in
            run(task) { handle in _ = try handle.draftQueued(progress: nil) }
        }
        BGTaskScheduler.shared.register(forTaskWithIdentifier: inbox, using: nil) { task in
            run(task) { handle in try applyInboxes(handle) }
        }
    }

    /// Asks for both tasks to run when the system next has time, such as when the app goes
    /// into the background.
    @MainActor
    static func schedule() {
        let drafting = BGProcessingTaskRequest(identifier: intake)
        drafting.requiresExternalPower = PowerPolicy.isPhone
        drafting.requiresNetworkConnectivity = false
        try? BGTaskScheduler.shared.submit(drafting)

        let applying = BGAppRefreshTaskRequest(identifier: inbox)
        applying.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        try? BGTaskScheduler.shared.submit(applying)
    }

    /// Runs `work` on the archive that was open last, off the main thread, then schedules the
    /// next run. The system ends the task if it runs out of time, and the work carries on from
    /// where it stopped next time, as the worker always does.
    private static func run(
        _ task: BGTask, _ work: @escaping @Sendable (ArchiveHandle) throws -> Void
    ) {
        let job = Task.detached(priority: .background) {
            guard let handle = lastArchive() else { return true }
            do {
                try work(handle)
                return true
            } catch {
                return false
            }
        }
        nonisolated(unsafe) let task = task
        task.expirationHandler = { job.cancel() }
        Task {
            let done = await job.value
            task.setTaskCompleted(success: done)
            await schedule()
        }
    }

    /// The archive that was open last, opened for access from its bookmark if it's outside the
    /// app's container.
    private static func lastArchive() -> ArchiveHandle? {
        guard let path = UserDefaults.app.string(forKey: MobileLibrary.openKey) else { return nil }
        let url = URL(filePath: path, directoryHint: .isDirectory)
        MobileLibrary.access(url, defaults: .app)
        return try? ArchiveHandle.open(url)
    }

    /// Applies other devices' inboxes when this device can write.
    ///
    /// TODO(#466, #467): once the CloudKit coordinator and the inbox's coverage of every
    /// operation land, take the CloudKit lock here for this batch, apply the inboxes through
    /// the core, record the new history head and release the lock. Until then the worker's
    /// own pass applies them under the writer lease, which the intake task runs (and which
    /// the app runs while it's open, through Draft Now).
    private static func applyInboxes(_ handle: ArchiveHandle) throws {
        _ = handle
    }
}
#endif
