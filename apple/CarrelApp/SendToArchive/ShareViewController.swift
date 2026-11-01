// Send to Archive, the Share extension (docs/design/macos-integration.md, "Share"): files, images
// and text from any app's share sheet join the intake queue, and a web address becomes a `url`
// item, with an optional note for their cards. It's sandboxed and never writes the archive: it
// sends each one to Carrel Agent over XPC and shows what the agent says, including when another
// device is the writer and it went into this Mac's inbox. It doesn't wait for drafting.

import AppKit
import CarrelKit
import SwiftUI
import UniformTypeIdentifiers

final class ShareViewController: NSViewController {
    private let model = SendToArchiveModel()

    override func loadView() {
        model.finish = { [weak self] sent in
            if sent {
                self?.extensionContext?.completeRequest(returningItems: nil)
            } else {
                self?.extensionContext?.cancelRequest(withError: CocoaError(.userCancelled))
            }
        }
        let view = NSHostingView(rootView: SendToArchiveView(model: model))
        view.frame.size = NSSize(width: 380, height: 260)
        self.view = view
        let items = (extensionContext?.inputItems as? [NSExtensionItem]) ?? []
        Task { await model.read(items.flatMap { $0.attachments ?? [] }) }
    }
}

/// What's being sent, and how it went.
@MainActor
@Observable
final class SendToArchiveModel {
    /// One thing shared, as a request to the agent.
    struct Shared: Identifiable {
        let id = UUID()
        let name: String
        let request: AgentRequest
    }

    private(set) var shared: [Shared] = []
    /// What couldn't be sent, and why.
    private(set) var leftOut: [String] = []
    /// The owner's note, given to the drafts and kept in the cards.
    var note = ""
    /// The archive it goes to, from the agent's state file.
    let archive = BadgeState.read(from: AgentService.badgeStateFile)?.archives.first?.name
    private(set) var reading = true
    private(set) var sending = false
    /// The agent's answers, once sent.
    private(set) var replies: [AgentReply] = []
    var finish: (Bool) -> Void = { _ in }

    var sent: Bool { !replies.isEmpty }

    func read(_ providers: [NSItemProvider]) async {
        for provider in providers {
            if let item = await Self.shared(from: provider) {
                shared.append(item)
            } else {
                leftOut.append("\(provider.suggestedName ?? "Something") can’t be archived.")
            }
        }
        reading = false
    }

    func send() async {
        sending = true
        let client = AgentClient()
        let note = note.trimmingCharacters(in: .whitespacesAndNewlines)
        for item in shared {
            replies.append(await client.send(item.request.noted(note.isEmpty ? nil : note)))
        }
        sending = false
    }

    /// The request for what `provider` holds: a file, an image, a web address or text.
    private static func shared(from provider: NSItemProvider) async -> Shared? {
        if let url = await fileURL(from: provider) {
            return Shared(
                name: url.lastPathComponent,
                request: .addFiles(
                    archive: nil, paths: [url.path(percentEncoded: false)], each: false,
                    whole: true))
        }
        if let type = provider.registeredContentTypes.first(where: { $0.conforms(to: .image) }),
            let data = await data(of: type, from: provider)
        {
            let name =
                (provider.suggestedName ?? "Image")
                + (type.preferredFilenameExtension.map { "." + $0 } ?? "")
            return Shared(name: name, request: .addData(archive: nil, name: name, data: data))
        }
        if let url = await webAddress(from: provider) {
            let title = provider.suggestedName
            return Shared(
                name: title ?? url.absoluteString,
                request: .addURL(archive: nil, url: url.absoluteString, title: title))
        }
        if let text = await text(from: provider), !text.isEmpty {
            return Shared(name: "Text", request: .addText(archive: nil, text: text, title: nil))
        }
        return nil
    }

    private static func fileURL(from provider: NSItemProvider) async -> URL? {
        let type = UTType.fileURL.identifier
        guard provider.hasItemConformingToTypeIdentifier(type) else { return nil }
        let item = try? await provider.loadItem(forTypeIdentifier: type)
        if let url = item as? URL { return url }
        return (item as? Data).flatMap { URL(dataRepresentation: $0, relativeTo: nil) }
    }

    /// A shared web address: an `http` or `https` URL that isn't a file.
    private static func webAddress(from provider: NSItemProvider) async -> URL? {
        let type = UTType.url.identifier
        guard provider.hasItemConformingToTypeIdentifier(type) else { return nil }
        let item = try? await provider.loadItem(forTypeIdentifier: type)
        let url =
            (item as? URL)
            ?? (item as? Data).flatMap { URL(dataRepresentation: $0, relativeTo: nil) }
        guard let url, ["http", "https"].contains(url.scheme?.lowercased() ?? "") else {
            return nil
        }
        return url
    }

    private static func text(from provider: NSItemProvider) async -> String? {
        let type = UTType.plainText.identifier
        guard provider.hasItemConformingToTypeIdentifier(type) else { return nil }
        let item = try? await provider.loadItem(forTypeIdentifier: type)
        return (item as? String) ?? (item as? Data).flatMap { String(data: $0, encoding: .utf8) }
    }

    private static func data(of type: UTType, from provider: NSItemProvider) async -> Data? {
        await withCheckedContinuation { continuation in
            _ = provider.loadDataRepresentation(for: type) { data, _ in
                continuation.resume(returning: data)
            }
        }
    }
}

struct SendToArchiveView: View {
    @Bindable var model: SendToArchiveModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Send to \(model.archive.map { "“\($0)”" } ?? "the archive")")
                .font(.headline)
            if model.reading {
                ProgressView()
            } else if model.sent {
                ForEach(Array(model.replies.enumerated()), id: \.offset) { _, reply in
                    Label(
                        reply.outcome == .done && !reply.message.contains("inbox")
                            ? "Added to the intake queue" : reply.message,
                        systemImage: reply.outcome == .done
                            ? "checkmark.circle" : "exclamationmark.triangle")
                }
            } else {
                ForEach(model.shared) { Label($0.name, systemImage: "doc") }
                TextField("Note for the card (optional)", text: $model.note, axis: .vertical)
                    .lineLimit(1...3)
                ForEach(model.leftOut, id: \.self) {
                    Label($0, systemImage: "exclamationmark.triangle").foregroundStyle(.secondary)
                }
                if model.shared.isEmpty && model.leftOut.isEmpty {
                    Text("There’s nothing here Carrel can archive.").foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 0)
            HStack {
                Spacer()
                if model.sent {
                    Button("Done") { model.finish(true) }.keyboardShortcut(.defaultAction)
                } else {
                    Button("Cancel") { model.finish(false) }.keyboardShortcut(.cancelAction)
                    Button("Send") { Task { await model.send() } }
                        .keyboardShortcut(.defaultAction)
                        .disabled(model.shared.isEmpty || model.sending)
                }
            }
        }
        .padding()
        .frame(width: 380, height: 260)
    }
}
