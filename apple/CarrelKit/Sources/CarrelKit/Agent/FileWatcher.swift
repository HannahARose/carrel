// File-system events for folders, through FSEvents: how the agent notices the intake queue,
// filed items and working copies change as it happens, instead of the CLI's five-minute check.

#if os(macOS)
import CoreServices
import Foundation

/// Watches folders and everything in them, calling back with the paths that changed.
public final class FileWatcher: @unchecked Sendable {
    private var stream: FSEventStreamRef?
    private let queue = DispatchQueue(label: "Carrel file watcher")
    private let changed: @Sendable ([String]) -> Void

    /// Starts watching `folders`, calling `changed` with the changed paths, a batch at a
    /// time, from a queue of its own. Changes are batched over `latency` seconds.
    public init?(
        folders: [URL], latency: TimeInterval = 0.5,
        changed: @escaping @Sendable ([String]) -> Void
    ) {
        self.changed = changed
        let paths = folders.map { $0.path(percentEncoded: false) } as CFArray
        var context = FSEventStreamContext(
            version: 0, info: Unmanaged.passUnretained(self).toOpaque(), retain: nil,
            release: nil, copyDescription: nil)
        let callback: FSEventStreamCallback = { _, info, count, paths, _, _ in
            guard let info else { return }
            let watcher = Unmanaged<FileWatcher>.fromOpaque(info).takeUnretainedValue()
            let array = Unmanaged<CFArray>.fromOpaque(paths).takeUnretainedValue()
            let changed = (array as? [String]) ?? []
            if !changed.isEmpty { watcher.changed(Array(changed.prefix(count))) }
        }
        let flags = FSEventStreamCreateFlags(
            kFSEventStreamCreateFlagUseCFTypes | kFSEventStreamCreateFlagFileEvents
                | kFSEventStreamCreateFlagNoDefer)
        guard
            let stream = FSEventStreamCreate(
                nil, callback, &context, paths,
                FSEventStreamEventId(kFSEventStreamEventIdSinceNow), latency, flags)
        else { return nil }
        self.stream = stream
        FSEventStreamSetDispatchQueue(stream, queue)
        guard FSEventStreamStart(stream) else {
            stop()
            return nil
        }
    }

    /// Stops watching. Nothing is called back afterwards.
    public func stop() {
        guard let stream else { return }
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
        self.stream = nil
    }

    deinit { stop() }
}
#endif
