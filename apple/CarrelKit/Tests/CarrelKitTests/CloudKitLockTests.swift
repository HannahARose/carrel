// The CloudKit writer lock's store, without CloudKit itself: the test runner has no iCloud
// entitlement, so live CloudKit is checked by hand (the pull request's checklist).

import CloudKit
import Foundation
import Testing

@testable import CarrelKit

@Suite struct CloudKitLockTests {
    @Test func withoutTheEntitlementThereIsNoStore() {
        #expect(CloudKitLockStore.forBundle() == nil)
        #expect(!CloudKitLockStore.isEntitled(to: CloudKitLockStore.placeholderContainer))
    }

    @Test func anUnsetBuildSettingNamesNoContainer() throws {
        for (value, expected) in [
            ("$(CARREL_CLOUDKIT_CONTAINER)", nil as String?),
            ("", nil),
            (CloudKitLockStore.placeholderContainer, CloudKitLockStore.placeholderContainer),
        ] {
            // A new bundle each time: bundles keep the Info.plist they first read.
            let folder = FileManager.default.temporaryDirectory.appending(
                path: "CloudKitLockTests-\(UUID().uuidString).bundle")
            defer { try? FileManager.default.removeItem(at: folder) }
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try (["CarrelCloudKitContainer": value] as NSDictionary)
                .write(to: folder.appending(path: "Info.plist"))
            let bundle = try #require(Bundle(url: folder))
            #expect(CloudKitLockStore.containerIdentifier(bundle) == expected)
        }
    }

    @Test func aLockRecordKeepsTheLeaseAndHistoryHeadOnly() {
        let lock = CloudLockRecord(
            token: "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b",
            person: "01928f39-eeee-7fff-8000-000000000005",
            device: "01928f3a-0000-7000-8000-00000000d001", purpose: "ingest",
            program: "carrel 0.8.0", acquired: "2026-11-01T09:00:00-04:00",
            expires: "2026-11-01T09:05:00-04:00", head: "3f9a", tag: "")
        let record = CKRecord(
            recordType: CloudKitLockStore.recordType,
            recordID: CKRecord.ID(recordName: CloudKitLockStore.recordName("archive")))
        CloudKitLockStore.fill(record, from: lock)
        #expect(
            Set(record.allKeys()) == [
                "token", "person", "device", "purpose", "program", "acquired", "expires", "head",
            ])
        #expect(CloudKitLockStore.lockRecord(record) == lock)
        #expect(record.recordID.recordName == "archive-archive")
    }
}
