// A minimal SwiftUI app over CarrelKit: open an archive, list its collections and items, and
// ask a Swift-implemented model, through Rust, to describe it.
//
//   swift run CarrelPoC [archive-folder]

import CarrelKit
import SwiftUI

#if os(macOS)
import AppKit
#endif

@main
struct CarrelPoCApp: App {
    init() {
        #if os(macOS)
        // Run from the command line, the app needs telling that it has windows.
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate()
        #endif
    }

    var body: some Scene {
        WindowGroup("Carrel") {
            ContentView(initialPath: CommandLine.arguments.dropFirst().first)
        }
    }
}

struct ContentView: View {
    let initialPath: String?

    @State private var archive: ArchiveHandle?
    @State private var collections: [CollectionSummary] = []
    @State private var items: [ItemSummary] = []
    @State private var summary: String?
    @State private var problem: String?
    @State private var choosing = false
    @State private var describing = false

    var body: some View {
        NavigationSplitView {
            List(collections) { collection in
                VStack(alignment: .leading) {
                    Text(collection.title)
                    if let description = collection.description {
                        Text(description).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle(archive?.name() ?? "Carrel")
        } detail: {
            if archive == nil {
                ContentUnavailableView(
                    "No archive open", systemImage: "archivebox",
                    description: Text(versionSummary()))
            } else {
                List {
                    if describing || summary != nil {
                        Section("Description") {
                            if let summary { Text(summary) } else { ProgressView() }
                        }
                    }
                    Section("Items") {
                        ForEach(items) { item in
                            Label(item.title, systemImage: symbol(for: item.kind))
                        }
                    }
                }
            }
        }
        .toolbar {
            Button("Open Archive…", systemImage: "folder") { choosing = true }
            Button("Describe", systemImage: "sparkles") { describe() }
                .disabled(archive == nil || describing)
        }
        .fileImporter(isPresented: $choosing, allowedContentTypes: [.folder]) { result in
            switch result {
            case .success(let url):
                _ = url.startAccessingSecurityScopedResource()
                open(url)
            case .failure(let error):
                problem = error.localizedDescription
            }
        }
        .alert("Carrel", isPresented: .constant(problem != nil)) {
            Button("OK") { problem = nil }
        } message: {
            Text(problem ?? "")
        }
        .onAppear {
            if let initialPath { open(URL(filePath: initialPath)) }
        }
    }

    private func open(_ url: URL) {
        do {
            let opened = try ArchiveHandle.open(url)
            collections = try opened.collections()
            items = try opened.items()
            archive = opened
            summary = nil
        } catch {
            problem = error.localizedDescription
        }
    }

    private func describe() {
        guard let archive else { return }
        describing = true
        summary = nil
        Task.detached {
            // Off the main thread: the model may wait for an answer.
            let result = Result { try archive.describe(model: availableModel()) }
            await MainActor.run {
                describing = false
                switch result {
                case .success(let text): summary = text
                case .failure(let error): problem = error.localizedDescription
                }
            }
        }
    }
}

private func symbol(for kind: ItemKind) -> String {
    switch kind {
    case .file: "doc"
    case .folder: "folder"
    case .bundle: "shippingbox"
    case .physical: "books.vertical"
    case .url: "link"
    case .external: "externaldrive"
    case .tombstone: "archivebox"
    }
}
