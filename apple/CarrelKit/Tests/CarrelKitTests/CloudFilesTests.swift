// iCloud Drive's on-demand files: what the provider makes of iCloud's resource values and of
// files outside iCloud Drive, and downloading an item through ArchiveModel. iCloud itself
// can't be faked, so a real eviction is checked by hand (docs/v0.8-gate.md).

import Foundation
import Testing

@testable import CarrelKit

@Suite struct CloudFilesState {
    private func observed(
        ubiquitous: Bool? = nil, status: URLUbiquitousItemDownloadingStatus? = nil,
        downloading: Bool? = nil, exists: Bool = true, placeholder: Bool = false,
        dataless: Bool = false
    ) -> CloudFiles.Observed {
        CloudFiles.Observed(
            isUbiquitous: ubiquitous, status: status, isDownloading: downloading, exists: exists,
            hasPlaceholder: placeholder, isDataless: dataless)
    }

    @Test func iCloudsOwnStatusDecidesForAFileInICloudDrive() {
        #expect(CloudFiles.state(observed(ubiquitous: true, status: .current)) == .downloaded)
        #expect(CloudFiles.state(observed(ubiquitous: true, status: .downloaded)) == .downloaded)
        #expect(
            CloudFiles.state(observed(ubiquitous: true, status: .notDownloaded, exists: false))
                == .notDownloaded)
        #expect(
            CloudFiles.state(
                observed(ubiquitous: true, status: .notDownloaded, downloading: true))
                == .downloading)
    }

    @Test func aFileOutsideICloudDriveIsThereOrNot() {
        #expect(CloudFiles.state(observed(ubiquitous: false)) == .downloaded)
        #expect(CloudFiles.state(observed(exists: false)) == .missing)
        #expect(CloudFiles.state(observed(exists: false, placeholder: true)) == .notDownloaded)
        #expect(CloudFiles.state(observed(dataless: true)) == .notDownloaded)
    }

    @Test func realFilesArePresentPlaceholdersOrMissing() throws {
        let folder = FileManager.default.temporaryDirectory
            .appending(path: "carrelkit-cloud-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let here = folder.appending(path: "here.txt")
        try Data("here".utf8).write(to: here)
        try Data().write(to: folder.appending(path: ".away.pdf.icloud"))
        let files = CloudFiles(timeout: 1)

        #expect(files.state(path: here.path(percentEncoded: false)) == .downloaded)
        #expect(!CloudFiles.isEvicted(here))
        let away = folder.appending(path: "away.pdf")
        #expect(files.state(path: away.path(percentEncoded: false)) == .notDownloaded)
        #expect(CloudFiles.isEvicted(away))
        let gone = folder.appending(path: "gone.pdf").path(percentEncoded: false)
        #expect(files.state(path: gone) == .missing)

        try files.download(path: here.path(percentEncoded: false))
        #expect(throws: CarrelError.self) { try files.download(path: gone) }
    }
}

@MainActor
@Suite struct CloudFilesDownloads {
    @Test func aDownloadedItemIsReadyWhereItIs() async throws {
        let sample = try Sample()
        defer { try? FileManager.default.removeItem(at: sample.folder) }
        let pdf = try sample.item(titled: "A note on coupled oscillators")
        let url = try await sample.model.download(item: pdf)
        #expect(url.lastPathComponent == "Coupled oscillators note.pdf")
        #expect(FileManager.default.fileExists(atPath: url.path(percentEncoded: false)))
        #expect(try await sample.model.download(collection: "Reading") == 0)
        let status = try await sample.model.syncStatus()
        #expect(status.notDownloaded.isEmpty)
        #expect(status.movedTo == nil)
    }
}
