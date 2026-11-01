// iCloud Drive's on-demand files (docs/design/sync-and-mobile.md, "iCloud Drive's files"):
// whether a file's contents are on this device, from its URL resource values, and downloading
// them through file coordination, which waits until they're here. The app and the agent
// register it with the core once, at launch, in place of the core's own placeholder checks.

import Foundation

/// iCloud Drive's on-demand files, for the core: `NSURL` resource values say whether a file is
/// downloaded, and `NSFileCoordinator` downloads it.
public final class CloudFiles: OnDemandFilesProvider {
    /// The provider the app and the agent register.
    public static let shared = CloudFiles()

    /// Whether `register()` has run in this process.
    nonisolated(unsafe) private static var registered = false
    private static let lock = NSLock()

    /// How long a download may take before it's reported as failed.
    let timeout: TimeInterval

    init(timeout: TimeInterval = 600) {
        self.timeout = timeout
    }

    /// Registers the shared provider with the core, once per process.
    public static func register() {
        lock.lock()
        defer { lock.unlock() }
        guard !registered else { return }
        registered = true
        setOnDemandFiles(provider: shared)
    }

    // MARK: - OnDemandFilesProvider

    public func state(path: String) -> FileDownloadState {
        Self.state(of: URL(filePath: path))
    }

    public func download(path: String) throws {
        let url = URL(filePath: path)
        switch Self.state(of: url) {
        case .downloaded:
            return
        case .missing:
            throw CarrelError.Callback(message: "\(path) isn’t here")
        case .notDownloaded, .downloading:
            break
        }
        // Asks iCloud for the contents; a file another provider evicted is fetched by reading
        // it, below. Neither is an error yet: coordination says if it can't be read.
        try? FileManager.default.startDownloadingUbiquitousItem(at: url)
        var coordinationError: NSError?
        var readError: Error?
        // A coordinated read without `.withoutChanges` waits for the download to finish.
        NSFileCoordinator(filePresenter: nil).coordinate(
            readingItemAt: url, options: [], error: &coordinationError
        ) { readable in
            do {
                // Reading the first byte makes the system materialize a dataless file.
                let handle = try FileHandle(forReadingFrom: readable)
                defer { try? handle.close() }
                _ = try handle.read(upToCount: 1)
            } catch {
                readError = error
            }
        }
        if let error = coordinationError ?? readError {
            throw CarrelError.Callback(
                message: "\(url.lastPathComponent) couldn’t be downloaded: "
                    + error.localizedDescription)
        }
        try waitUntilDownloaded(url)
    }

    /// Waits for iCloud to report the file at `url` downloaded, since a coordinated read can
    /// end as the contents arrive and before the resource values say so.
    private func waitUntilDownloaded(_ url: URL) throws {
        let deadline = Date().addingTimeInterval(timeout)
        while true {
            switch Self.state(of: url) {
            case .downloaded:
                return
            case .missing:
                throw CarrelError.Callback(message: "\(url.lastPathComponent) isn’t here")
            case .notDownloaded, .downloading:
                guard Date() < deadline else {
                    throw CarrelError.Callback(
                        message: "\(url.lastPathComponent) is still downloading")
                }
                Thread.sleep(forTimeInterval: 0.25)
            }
        }
    }

    // MARK: - Reading the state

    /// macOS's and iOS's flag for a file whose contents aren't on the device (`SF_DATALESS`).
    private static let dataless: UInt32 = 0x4000_0000

    /// What the file system says about a file, for `state(_:)`.
    struct Observed: Equatable {
        /// Whether it's in iCloud Drive, if known.
        var isUbiquitous: Bool?
        /// iCloud's downloading status, for a file in iCloud Drive.
        var status: URLUbiquitousItemDownloadingStatus?
        /// Whether iCloud is downloading it now.
        var isDownloading: Bool?
        /// Whether something is at its path.
        var exists: Bool
        /// Whether a `.<name>.icloud` placeholder stands in for it.
        var hasPlaceholder: Bool
        /// Whether it's dataless: a file another provider evicted.
        var isDataless: Bool
    }

    /// Whether iCloud evicted the file at `url`'s contents, or is downloading them: to say so
    /// before opening or previewing it, which downloads it first.
    public static func isEvicted(_ url: URL) -> Bool {
        switch state(of: url) {
        case .notDownloaded, .downloading: true
        case .downloaded, .missing: false
        }
    }

    /// Whether the file at `url` is downloaded.
    static func state(of url: URL) -> FileDownloadState {
        state(observe(url))
    }

    /// The state `observed` means: iCloud's own status for a file in iCloud Drive, else
    /// whether the file, a dataless file or a `.icloud` placeholder is there, as the core's
    /// own checks decide.
    static func state(_ observed: Observed) -> FileDownloadState {
        if observed.isUbiquitous == true, let status = observed.status {
            if observed.isDownloading == true { return .downloading }
            switch status {
            case .current, .downloaded: return .downloaded
            case .notDownloaded: return .notDownloaded
            default: return observed.exists ? .downloaded : .notDownloaded
            }
        }
        if observed.exists { return observed.isDataless ? .notDownloaded : .downloaded }
        return observed.hasPlaceholder ? .notDownloaded : .missing
    }

    /// Reads what `state(_:)` needs about the file at `url`.
    static func observe(_ url: URL) -> Observed {
        let path = url.path(percentEncoded: false)
        var info = stat()
        let exists = lstat(path, &info) == 0
        let placeholder = url.deletingLastPathComponent()
            .appending(path: ".\(url.lastPathComponent).icloud")
        let values = try? url.resourceValues(forKeys: [
            .isUbiquitousItemKey, .ubiquitousItemDownloadingStatusKey,
            .ubiquitousItemIsDownloadingKey,
        ])
        return Observed(
            isUbiquitous: values?.isUbiquitousItem,
            status: values?.ubiquitousItemDownloadingStatus,
            isDownloading: values?.ubiquitousItemIsDownloading,
            exists: exists,
            hasPlaceholder: !exists
                && FileManager.default.fileExists(atPath: placeholder.path(percentEncoded: false)),
            isDataless: exists && (info.st_mode & S_IFMT) == S_IFREG
                && info.st_flags & dataless != 0)
    }
}
