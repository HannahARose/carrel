// What the agent knows about the Mac for the crawler (docs/design/restructure.md, "Crawlers"):
// how long since the keyboard or pointer was used, whether it's on mains power, and its memory.
// The core has no platform code, so the agent reads these and hands them over.

import Foundation

#if os(macOS)
import IOKit
import IOKit.ps
#endif

/// The Mac's state at a moment.
struct MacState: Sendable {
    /// Seconds since the keyboard or pointer was last used, when known.
    var idle: UInt64?
    /// Whether it's on mains power rather than battery, when known.
    var onPower: Bool?

    /// The state now.
    static func now() -> MacState {
        MacState(idle: idleSeconds(), onPower: onMainsPower())
    }

    /// This Mac's memory, rounded to whole gigabytes.
    static var memoryGB: UInt64? {
        let bytes = ProcessInfo.processInfo.physicalMemory
        return bytes > 0 ? (bytes + (1 << 29)) >> 30 : nil
    }

    /// Seconds since the keyboard or pointer was last used, from the HID system.
    static func idleSeconds() -> UInt64? {
        #if os(macOS)
        let service = IOServiceGetMatchingService(
            kIOMainPortDefault, IOServiceMatching("IOHIDSystem"))
        guard service != 0 else { return nil }
        defer { IOObjectRelease(service) }
        guard
            let property = IORegistryEntryCreateCFProperty(
                service, "HIDIdleTime" as CFString, kCFAllocatorDefault, 0)?
                .takeRetainedValue() as? NSNumber
        else { return nil }
        return property.uint64Value / 1_000_000_000
        #else
        return nil
        #endif
    }

    /// Whether the Mac draws from mains power; a Mac with no battery always does.
    static func onMainsPower() -> Bool? {
        #if os(macOS)
        guard let info = IOPSCopyPowerSourcesInfo()?.takeRetainedValue() else { return nil }
        guard
            let type = IOPSGetProvidingPowerSourceType(info)?.takeUnretainedValue() as String?
        else { return nil }
        return type == kIOPMACPowerKey
        #else
        return nil
        #endif
    }
}

/// Stops a crawler pass once the owner uses the Mac again.
final class StopWhenUsed: CrawlStop {
    func shouldStop() -> Bool {
        MacState.idleSeconds().map { $0 < 30 } ?? false
    }
}
