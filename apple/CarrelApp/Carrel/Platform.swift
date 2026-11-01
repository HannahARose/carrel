// What the Mac app and the iPad and iPhone app do in their own ways: choosing files and folders
// (an open panel on the Mac, the document picker on iPad and iPhone), showing a file where it
// is (Finder, or Files), and opening a link. Screens call these, so they're shared unchanged.

import CarrelKit
import Observation
import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
    import AppKit
#else
    import UIKit
#endif

/// A request to choose files or folders, which the Mac answers with an open panel at once and
/// iPad and iPhone with the document picker the archive's root view shows.
@MainActor
@Observable
final class FileChoosing {
    static let shared = FileChoosing()

    struct Request: Identifiable {
        let id = UUID()
        var folders = false
        var files = true
        var multiple = false
        /// The panel's button, on the Mac.
        var prompt = "Choose"
        var message: String?
        var start: URL?
        let chosen: ([URL]) -> Void
    }

    /// The request the document picker is showing, on iPad and iPhone.
    var pending: Request?

    /// Asks for files or folders, and calls `chosen` with them unless it's cancelled.
    func choose(_ request: Request) {
        #if os(macOS)
            let panel = NSOpenPanel()
            panel.canChooseDirectories = request.folders
            panel.canChooseFiles = request.files
            panel.canCreateDirectories = request.folders
            panel.allowsMultipleSelection = request.multiple
            panel.prompt = request.prompt
            if let message = request.message { panel.message = message }
            if let start = request.start { panel.directoryURL = start }
            guard panel.runModal() == .OK, !panel.urls.isEmpty else { return }
            request.chosen(panel.urls)
        #else
            pending = request
        #endif
    }

    /// The document picker's answer: each URL is opened for access, since it's outside the
    /// app's container, and stays open while the app runs.
    func finish(_ result: Result<[URL], any Error>) {
        guard let request = pending else { return }
        pending = nil
        guard case .success(let urls) = result, !urls.isEmpty else { return }
        for url in urls { _ = url.startAccessingSecurityScopedResource() }
        request.chosen(urls)
    }
}

/// Chooses one folder, then calls `chosen` with it.
@MainActor
func pickFolder(
    prompt: String = "Choose", message: String? = nil, start: URL? = nil,
    _ chosen: @escaping (URL) -> Void
) {
    FileChoosing.shared.choose(
        .init(
            folders: true, files: false, prompt: prompt, message: message, start: start
        ) { chosen($0[0]) })
}

/// Chooses one file or folder, then calls `chosen` with it.
@MainActor
func pickFileOrFolder(
    prompt: String = "Choose", message: String? = nil, _ chosen: @escaping (URL) -> Void
) {
    FileChoosing.shared.choose(
        .init(folders: true, files: true, prompt: prompt, message: message) { chosen($0[0]) })
}

extension View {
    /// Shows the document picker for a `FileChoosing` request, on iPad and iPhone; on the Mac
    /// the open panel has already answered it.
    func fileChoosing() -> some View {
        modifier(FileChoosingPicker())
    }
}

private struct FileChoosingPicker: ViewModifier {
    @State private var choosing = FileChoosing.shared

    func body(content: Content) -> some View {
        #if os(macOS)
            content
        #else
            content.fileImporter(
                isPresented: Binding(
                    get: { choosing.pending != nil },
                    set: { if !$0, choosing.pending != nil { choosing.finish(.success([])) } }),
                allowedContentTypes: allowedTypes,
                allowsMultipleSelection: choosing.pending?.multiple ?? false
            ) { choosing.finish($0.map { $0 }) }
        #endif
    }

    private var allowedTypes: [UTType] {
        guard let request = choosing.pending else { return [.item] }
        switch (request.folders, request.files) {
        case (true, false): return [.folder]
        case (false, true): return [.item]
        default: return [.item, .folder]
        }
    }
}

/// Shows `urls` where they are: selected in Finder on the Mac, or their folder in Files on
/// iPad and iPhone.
@MainActor
func revealInFiles(_ urls: [URL]) {
    #if os(macOS)
        NSWorkspace.shared.activateFileViewerSelecting(urls)
    #else
        guard let url = urls.first else { return }
        // Files opens a folder at a `shareddocuments` address; it shows the folder a file is in.
        var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        components?.scheme = "shareddocuments"
        if let address = components?.url { UIApplication.shared.open(address) }
    #endif
}

/// Opens `url` in the app that handles it, such as a web page in the browser.
@MainActor
func openExternally(_ url: URL) {
    #if os(macOS)
        NSWorkspace.shared.open(url)
    #else
        UIApplication.shared.open(url)
    #endif
}

/// What a sheet's fixed width is on the Mac; on iPad and iPhone the sheet sizes itself.
extension View {
    func sheetWidth(_ width: CGFloat, height: CGFloat? = nil) -> some View {
        #if os(macOS)
            frame(width: width, height: height)
        #else
            self
        #endif
    }
}

/// Whether this device is a Mac, which has Kopia, the agent and the Finder, Quick Look and
/// Spotlight extensions; iPad and iPhone don't.
let isMac: Bool = {
    #if os(macOS)
        true
    #else
        false
    #endif
}()

/// Two panes side by side: a split view the owner can resize on the Mac, and a plain row on
/// iPad and iPhone.
struct SideBySide<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        #if os(macOS)
        HSplitView { content }
        #else
        HStack(spacing: 0) { content }
        #endif
    }
}

/// A button that opens Settings: its window on the Mac, its sheet on iPad and iPhone.
struct OpenSettingsButton: View {
    let title: String
    #if os(iOS)
    @Environment(MobileLibrary.self) private var library: MobileLibrary?
    #endif

    var body: some View {
        #if os(macOS)
        SettingsLink { Text(title) }
        #else
        Button(title) { library?.showingSettings = true }
        #endif
    }
}
