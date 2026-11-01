// Capture on iPad and iPhone (docs/design/sync-and-mobile.md, "iPad and iPhone"): photos from
// the library, a photo from the camera, a document scan (as one PDF), files from the document
// picker, and what the share sheet sent (`Carrel Share`). Each joins the intake queue, or this
// device's inbox while another device is the writer, as the Mac's share sheet does. Captured
// photos and scans are kept in the app's own folder, since the queue reads them from there.

#if os(iOS)
import CarrelKit
import Observation
import PhotosUI
import SwiftUI
import UIKit
import VisionKit

@Observable
final class CaptureModel {
    let archive: ArchiveModel
    /// What the last capture did, in words, until it's dismissed.
    var report: String?

    init(archive: ArchiveModel) {
        self.archive = archive
    }

    /// Where photos and scans taken here are kept until they're filed.
    static var folder: URL {
        URL.applicationSupportDirectory.appending(path: "Captured", directoryHint: .isDirectory)
    }

    /// What the share sheet left in the app group's folder for the app to add.
    static var sharedFolder: URL? {
        guard let group = Bundle.main.object(forInfoDictionaryKey: "CarrelAppGroup") as? String
        else { return nil }
        return FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)?
            .appending(path: "Shared", directoryHint: .isDirectory)
    }

    /// Keeps `data` as a new file named after `name` and the time.
    func keep(_ data: Data, named name: String, extension ext: String) throws -> URL {
        let folder = Self.folder
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let stamp = Date.now.formatted(
            .iso8601.year().month().day().time(includingFractionalSeconds: false)
        )
        .replacingOccurrences(of: ":", with: "-")
        var url = folder.appending(path: "\(name) \(stamp).\(ext)")
        var number = 2
        while FileManager.default.fileExists(atPath: url.path(percentEncoded: false)) {
            url = folder.appending(path: "\(name) \(stamp) \(number).\(ext)")
            number += 1
        }
        try data.write(to: url)
        return url
    }

    /// Sends the files at `urls` to the archive: the intake queue, or this device's inbox while
    /// another device writes.
    func send(_ urls: [URL], note: String? = nil) async {
        guard !urls.isEmpty else { return }
        let paths = urls.map { $0.path(percentEncoded: false) }
        do {
            let shared = try await archive.call {
                try $0.sharePaths(paths: paths, each: false, whole: false, note: note)
            }
            report = Self.describe(shared, count: urls.count)
            try? await archive.refreshIntake()
        } catch {
            report = message(for: error)
        }
        BackgroundWork.schedule()
    }

    /// Sends a web address, from the share sheet.
    func send(address: String, title: String?) async {
        do {
            let shared = try await archive.call {
                try $0.shareUrl(url: address, title: title, note: nil)
            }
            report = Self.describe(shared, count: 1)
            try? await archive.refreshIntake()
        } catch {
            report = message(for: error)
        }
    }

    private static func describe(_ shared: Shared, count: Int) -> String {
        if shared.inbox != nil {
            return "Sent to this device’s inbox, for the device that’s writing to file."
        }
        let added = shared.added.added.count
        let leftOut = shared.added.leftOut.map {
            "\(URL(filePath: $0.path).lastPathComponent): \($0.reason)"
        }
        var words =
            added == 1
            ? "Added 1 item to the intake queue." : "Added \(added) items to the intake queue."
        if !leftOut.isEmpty { words += "\n" + leftOut.joined(separator: "\n") }
        return words
    }

    // MARK: Sources

    /// Photos chosen from the library, as their original files where the library has them.
    func add(_ items: [PhotosPickerItem]) async {
        var urls: [URL] = []
        for item in items {
            guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
            let type = item.supportedContentTypes.first
            let ext = type?.preferredFilenameExtension ?? "jpeg"
            if let url = try? keep(data, named: "Photo", extension: ext) { urls.append(url) }
        }
        await send(urls)
    }

    /// A photo from the camera.
    func add(_ image: UIImage) async {
        guard let data = image.heicData() ?? image.jpegData(compressionQuality: 0.9),
            let url = try? keep(
                data, named: "Photo", extension: image.heicData() == nil ? "jpeg" : "heic")
        else { return }
        await send([url])
    }

    /// A document scan, as one PDF of its pages.
    func add(_ scan: VNDocumentCameraScan) async {
        let pdf = NSMutableData()
        let first = scan.imageOfPage(at: 0)
        UIGraphicsBeginPDFContextToData(pdf, CGRect(origin: .zero, size: first.size), nil)
        for index in 0..<scan.pageCount {
            let page = scan.imageOfPage(at: index)
            UIGraphicsBeginPDFPageWithInfo(CGRect(origin: .zero, size: page.size), nil)
            page.draw(at: .zero)
        }
        UIGraphicsEndPDFContext()
        let name = scan.title.isEmpty ? "Scan" : scan.title
        guard let url = try? keep(pdf as Data, named: name, extension: "pdf") else { return }
        await send([url])
    }

    /// What the share sheet left: files, and web addresses in `.url.txt` files. Each is moved
    /// into the app's own folder first, since the queue reads it from where it is.
    func addShared() async {
        guard let shared = Self.sharedFolder,
            let names = try? FileManager.default.contentsOfDirectory(
                atPath: shared.path(percentEncoded: false)),
            !names.isEmpty
        else { return }
        try? FileManager.default.createDirectory(at: Self.folder, withIntermediateDirectories: true)
        var files: [URL] = []
        for name in names.sorted() {
            let url = shared.appending(path: name)
            if name.hasSuffix(".url.txt") {
                if let address = try? String(contentsOf: url, encoding: .utf8) {
                    await send(
                        address: address.trimmingCharacters(in: .whitespacesAndNewlines), title: nil
                    )
                }
                try? FileManager.default.removeItem(at: url)
                continue
            }
            let kept = Self.folder.appending(path: name)
            if (try? FileManager.default.moveItem(at: url, to: kept)) != nil { files.append(kept) }
        }
        await send(files)
    }
}

/// The capture menu in the archive's toolbar.
struct CaptureMenu: View {
    let capture: CaptureModel
    @State private var choosingPhotos = false
    @State private var photos: [PhotosPickerItem] = []
    @State private var takingPhoto = false
    @State private var scanning = false

    var body: some View {
        Menu("Add", systemImage: "plus") {
            Button("Photos…", systemImage: "photo.on.rectangle") { choosingPhotos = true }
            Button("Take Photo…", systemImage: "camera") { takingPhoto = true }
                .disabled(!UIImagePickerController.isSourceTypeAvailable(.camera))
            Button("Scan Document…", systemImage: "doc.viewfinder") { scanning = true }
                .disabled(!VNDocumentCameraViewController.isSupported)
            Button("Files…", systemImage: "folder") {
                FileChoosing.shared.choose(
                    .init(folders: true, files: true, multiple: true, prompt: "Add") { urls in
                        Task { await capture.send(urls) }
                    })
            }
        }
        .photosPicker(
            isPresented: $choosingPhotos, selection: $photos,
            matching: .any(of: [.images, .videos])
        )
        .onChange(of: photos) {
            let chosen = photos
            photos = []
            Task { await capture.add(chosen) }
        }
        .fullScreenCover(isPresented: $takingPhoto) {
            CameraView { image in Task { await capture.add(image) } }
                .ignoresSafeArea()
        }
        .fullScreenCover(isPresented: $scanning) {
            ScannerView { scan in Task { await capture.add(scan) } }
                .ignoresSafeArea()
        }
        .alert(
            "Added to the Archive",
            isPresented: Binding(
                get: { capture.report != nil }, set: { if !$0 { capture.report = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(capture.report ?? "")
        }
    }
}

/// The camera, for one photo.
private struct CameraView: UIViewControllerRepresentable {
    let taken: (UIImage) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ picker: UIImagePickerController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, UIImagePickerControllerDelegate,
        UINavigationControllerDelegate
    {
        let parent: CameraView
        init(_ parent: CameraView) { self.parent = parent }

        func imagePickerController(
            _ picker: UIImagePickerController,
            didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
        ) {
            if let image = info[.originalImage] as? UIImage { parent.taken(image) }
            parent.dismiss()
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            parent.dismiss()
        }
    }
}

/// The document scanner.
private struct ScannerView: UIViewControllerRepresentable {
    let scanned: (VNDocumentCameraScan) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> VNDocumentCameraViewController {
        let scanner = VNDocumentCameraViewController()
        scanner.delegate = context.coordinator
        return scanner
    }

    func updateUIViewController(_ scanner: VNDocumentCameraViewController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, VNDocumentCameraViewControllerDelegate {
        let parent: ScannerView
        init(_ parent: ScannerView) { self.parent = parent }

        func documentCameraViewController(
            _ controller: VNDocumentCameraViewController, didFinishWith scan: VNDocumentCameraScan
        ) {
            if scan.pageCount > 0 { parent.scanned(scan) }
            parent.dismiss()
        }

        func documentCameraViewControllerDidCancel(_ controller: VNDocumentCameraViewController) {
            parent.dismiss()
        }

        func documentCameraViewController(
            _ controller: VNDocumentCameraViewController, didFailWithError error: any Error
        ) {
            parent.dismiss()
        }
    }
}
#endif
