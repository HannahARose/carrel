// Carrel Share, the iPad and iPhone share extension (docs/design/sync-and-mobile.md, "iPad and
// iPhone"): files, photos, text and web addresses from any app's share sheet are left in the
// app group's `Shared` folder, and Carrel adds them to the archive that's open the next time it
// comes to the front: to the intake queue, or this device's inbox while another device writes.
// It never opens the archive itself, so it stays small and quick, as the Mac's does.

import SwiftUI
import UIKit
import UniformTypeIdentifiers

final class ShareViewController: UIViewController {
    private let model = SharedForCarrel()

    override func viewDidLoad() {
        super.viewDidLoad()
        let host = UIHostingController(rootView: SharedForCarrelView(model: model))
        addChild(host)
        host.view.frame = view.bounds
        host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(host.view)
        host.didMove(toParent: self)
        model.finish = { [weak self] in
            self?.extensionContext?.completeRequest(returningItems: nil)
        }
        let items = (extensionContext?.inputItems as? [NSExtensionItem]) ?? []
        Task { await model.keep(items.flatMap { $0.attachments ?? [] }) }
    }
}

/// What's being left for Carrel, and how it went.
@MainActor
@Observable
final class SharedForCarrel {
    private(set) var kept: [String] = []
    private(set) var leftOut: [String] = []
    private(set) var done = false
    var finish: () -> Void = {}

    /// The app group's folder the app reads, from the extension's Info.plist.
    private var folder: URL? {
        guard let group = Bundle.main.object(forInfoDictionaryKey: "CarrelAppGroup") as? String
        else { return nil }
        return FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)?
            .appending(path: "Shared", directoryHint: .isDirectory)
    }

    func keep(_ providers: [NSItemProvider]) async {
        guard let folder else {
            leftOut.append("Carrel’s shared folder isn’t available.")
            done = true
            return
        }
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for provider in providers {
            if let name = await keep(provider, in: folder) {
                kept.append(name)
            } else {
                leftOut.append("\(provider.suggestedName ?? "Something") can’t be archived.")
            }
        }
        done = true
    }

    /// Leaves what `provider` holds in `folder`: a web address as a `.url.txt` file, anything
    /// else as a copy of its file. Returns its name.
    private func keep(_ provider: NSItemProvider, in folder: URL) async -> String? {
        if provider.hasItemConformingToTypeIdentifier(UTType.url.identifier),
            !provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier),
            let url = try? await provider.loadItem(forTypeIdentifier: UTType.url.identifier)
                as? URL,
            url.scheme == "http" || url.scheme == "https"
        {
            let file = unique(in: folder, name: "Link", ext: "url.txt")
            guard
                (try? url.absoluteString.write(to: file, atomically: true, encoding: .utf8)) != nil
            else { return nil }
            return url.host() ?? url.absoluteString
        }
        if provider.hasItemConformingToTypeIdentifier(UTType.plainText.identifier),
            provider.registeredTypeIdentifiers.count == 1,
            let text = try? await provider.loadItem(forTypeIdentifier: UTType.plainText.identifier)
                as? String
        {
            let file = unique(in: folder, name: "Note", ext: "txt")
            guard (try? text.write(to: file, atomically: true, encoding: .utf8)) != nil else {
                return nil
            }
            return file.lastPathComponent
        }
        guard let type = provider.registeredTypeIdentifiers.first else { return nil }
        return await withCheckedContinuation { continuation in
            _ = provider.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
                guard let url else { return continuation.resume(returning: nil) }
                let name =
                    provider.suggestedName.map {
                        URL(filePath: $0).pathExtension.isEmpty
                            ? "\($0).\(url.pathExtension)" : $0
                    } ?? url.lastPathComponent
                let base = URL(filePath: name).deletingPathExtension().lastPathComponent
                let destination = self.unique(
                    in: folder, name: base, ext: URL(filePath: name).pathExtension)
                // The file is removed when this returns, so it's copied now.
                let copied = (try? FileManager.default.copyItem(at: url, to: destination)) != nil
                continuation.resume(returning: copied ? destination.lastPathComponent : nil)
            }
        }
    }

    nonisolated private func unique(in folder: URL, name: String, ext: String) -> URL {
        let suffix = ext.isEmpty ? "" : ".\(ext)"
        var url = folder.appending(path: name + suffix)
        var number = 2
        while FileManager.default.fileExists(atPath: url.path(percentEncoded: false)) {
            url = folder.appending(path: "\(name) \(number)\(suffix)")
            number += 1
        }
        return url
    }
}

struct SharedForCarrelView: View {
    let model: SharedForCarrel

    var body: some View {
        NavigationStack {
            List {
                if !model.done {
                    ProgressView("Getting it ready…")
                } else {
                    if !model.kept.isEmpty {
                        Section {
                            ForEach(model.kept, id: \.self) { Label($0, systemImage: "doc") }
                        } footer: {
                            Text("Carrel adds these to your archive next time you open it.")
                        }
                    }
                    if !model.leftOut.isEmpty {
                        Section {
                            ForEach(model.leftOut, id: \.self) {
                                Text($0).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Send to Carrel")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { model.finish() }.disabled(!model.done)
                }
            }
        }
    }
}
