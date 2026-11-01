// The CloudKit writer lock (docs/design/sync-and-mobile.md, "The writer lock"): one record per
// archive in the owner's private CloudKit database, holding the writing device, its lease's
// expiry and the history head. Never file contents. The core decides who may write
// (coordinator/cloud.rs); this only keeps the record. The Mac agent serves it to the app and the
// command line (`serveCloudLock`), and the iOS app uses it directly (`useCloudLock`).

#if canImport(CloudKit)
import CloudKit
import Foundation
import Security

/// Lock records in CloudKit, as the core's `CloudLockStore`. The core calls it from a background
/// thread and waits, so each call blocks until CloudKit answers.
public final class CloudKitLockStore: CloudLockStore, @unchecked Sendable {
    /// The Info.plist key naming the CloudKit container, set from the `CARREL_CLOUDKIT_CONTAINER`
    /// build setting.
    public static let containerKey = "CarrelCloudKitContainer"
    /// The container until the owner creates the real one in the Apple Developer account.
    public static let placeholderContainer = "iCloud.io.github.hannaharose.Carrel"
    /// The record type.
    public static let recordType = "WriterLock"
    /// How long to wait for CloudKit before calling it unreachable.
    static let timeout: DispatchTimeInterval = .seconds(30)

    private let database: CKDatabase
    private let lock = NSLock()
    /// The records as last seen, whose change tags conditional saves need.
    private var seen: [String: CKRecord] = [:]

    /// The store in `container`'s private database.
    public init(container: CKContainer) {
        database = container.privateCloudDatabase
    }

    /// The store for `bundle`'s container, when this build can use CloudKit: the bundle names a
    /// container and, on a Mac, is signed with the iCloud entitlement for it. `nil` otherwise,
    /// since CloudKit stops a process that uses a container it isn't entitled to; builds and
    /// tests without the entitlement then never touch CloudKit, and archives in iCloud Drive
    /// keep their lock file, as before v0.8.
    public static func forBundle(_ bundle: Bundle = .main) -> CloudKitLockStore? {
        guard let identifier = containerIdentifier(bundle), isEntitled(to: identifier) else {
            return nil
        }
        return CloudKitLockStore(container: CKContainer(identifier: identifier))
    }

    /// The container `bundle` names, if any.
    static func containerIdentifier(_ bundle: Bundle) -> String? {
        guard let identifier = bundle.object(forInfoDictionaryKey: containerKey) as? String,
            !identifier.isEmpty, !identifier.hasPrefix("$(")
        else { return nil }
        return identifier
    }

    /// Whether this process is signed for `container`. On iOS an app that names a container is
    /// built with its entitlement.
    static func isEntitled(to container: String) -> Bool {
        #if os(macOS)
        guard let task = SecTaskCreateFromSelf(nil),
            let value = SecTaskCopyValueForEntitlement(
                task, "com.apple.developer.icloud-container-identifiers" as CFString, nil),
            let containers = value as? [String]
        else { return false }
        return containers.contains(container)
        #else
        return true
        #endif
    }

    static func recordName(_ archiveId: String) -> String {
        "archive-\(archiveId)"
    }

    // MARK: CloudLockStore

    public func fetch(archiveId: String) throws -> CloudLockRecord? {
        try fetchRecord(Self.recordName(archiveId)).map(Self.lockRecord)
    }

    public func save(
        archiveId: String, record: CloudLockRecord, expectedTag: String?
    ) throws -> CloudLockSave {
        let name = Self.recordName(archiveId)
        let target: CKRecord
        if let expectedTag {
            let known = lock.withLock { seen[name] }
            let current =
                known?.recordChangeTag == expectedTag ? known : try fetchRecord(name)
            guard let current, current.recordChangeTag == expectedTag else {
                return .changed(current: current.map(Self.lockRecord))
            }
            target = current
        } else {
            target = CKRecord(recordType: Self.recordType, recordID: CKRecord.ID(recordName: name))
        }
        Self.fill(target, from: record)
        switch modify(target) {
        case .success(let saved):
            remember(saved)
            return .saved(tag: saved.recordChangeTag ?? "")
        case .failure(let error as CKError) where error.code == .serverRecordChanged:
            let current = error.serverRecord
            if let current { remember(current) }
            return .changed(current: current.map(Self.lockRecord))
        case .failure(let error):
            throw CarrelError.Callback(message: Self.describe(error))
        }
    }

    // MARK: Records

    static func fill(_ target: CKRecord, from record: CloudLockRecord) {
        target["token"] = record.token
        target["person"] = record.person
        target["device"] = record.device
        target["purpose"] = record.purpose
        target["program"] = record.program
        target["acquired"] = record.acquired
        target["expires"] = record.expires
        target["head"] = record.head
    }

    static func lockRecord(_ record: CKRecord) -> CloudLockRecord {
        func text(_ key: String) -> String { record[key] as? String ?? "" }
        return CloudLockRecord(
            token: text("token"), person: text("person"), device: text("device"),
            purpose: text("purpose"), program: text("program"), acquired: text("acquired"),
            expires: text("expires"), head: record["head"] as? String,
            tag: record.recordChangeTag ?? "")
    }

    private func remember(_ record: CKRecord) {
        lock.withLock { seen[record.recordID.recordName] = record }
    }

    private func fetchRecord(_ name: String) throws -> CKRecord? {
        let result = wait { done in
            self.database.fetch(withRecordID: CKRecord.ID(recordName: name)) { record, error in
                done(record.map { .success($0) } ?? .failure(error ?? CKError(.internalError)))
            }
        }
        switch result {
        case .success(let record):
            remember(record)
            return record
        case .failure(let error as CKError) where error.code == .unknownItem:
            return nil
        case .failure(let error):
            throw CarrelError.Callback(message: Self.describe(error))
        }
    }

    private func modify(_ record: CKRecord) -> Result<CKRecord, Error> {
        wait { done in
            let operation = CKModifyRecordsOperation(recordsToSave: [record])
            operation.savePolicy = .ifServerRecordUnchanged
            operation.qualityOfService = .userInitiated
            operation.perRecordSaveBlock = { _, result in done(result) }
            self.database.add(operation)
        }
    }

    /// Runs `start` and waits for the first result it gives, or times out.
    private func wait<T>(_ start: (@escaping @Sendable (Result<T, Error>) -> Void) -> Void)
        -> Result<T, Error>
    {
        let box = ResultBox<T>()
        let semaphore = DispatchSemaphore(value: 0)
        start { result in
            if box.set(result) { semaphore.signal() }
        }
        if semaphore.wait(timeout: .now() + Self.timeout) == .timedOut {
            return .failure(CKError(.networkUnavailable))
        }
        return box.get() ?? .failure(CKError(.internalError))
    }

    static func describe(_ error: Error) -> String {
        guard let error = error as? CKError else { return error.localizedDescription }
        switch error.code {
        case .notAuthenticated: return "this device isn’t signed in to iCloud"
        case .networkUnavailable, .networkFailure: return "iCloud can’t be reached"
        case .quotaExceeded: return "the iCloud account is full"
        default: return error.localizedDescription
        }
    }
}

/// The first result a CloudKit callback gives.
private final class ResultBox<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var result: Result<T, Error>?

    /// Keeps `result` if it's the first, saying so.
    func set(_ result: Result<T, Error>) -> Bool {
        lock.withLock {
            guard self.result == nil else { return false }
            self.result = result
            return true
        }
    }

    func get() -> Result<T, Error>? {
        lock.withLock { result }
    }
}
#endif
