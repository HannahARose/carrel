// The window shown when no archive is open: create or open one, or pick a recent one.

import CarrelKit
import SwiftUI

struct WelcomeView: View {
    static let windowID = "welcome"

    @Environment(AppModel.self) private var app
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow

    var body: some View {
        HStack(spacing: 0) {
            VStack(spacing: 16) {
                Image(systemName: "archivebox")
                    .font(.system(size: 56))
                    .foregroundStyle(.tint)
                Text("Carrel").font(.largeTitle.bold())
                Text(versionSummary()).font(.callout).foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 8) {
                    Button("New Archive…", systemImage: "plus") {
                        app.chooseNewArchive(then: show)
                    }
                    Button("Open Archive…", systemImage: "folder") {
                        app.chooseArchive(then: show)
                    }
                }
                .buttonStyle(.borderless)
                .padding(.top)
            }
            .frame(width: 300)
            .padding(32)

            List(app.recents.urls, id: \.self) { url in
                Button {
                    app.open(url, then: show)
                } label: {
                    VStack(alignment: .leading) {
                        Text(url.lastPathComponent)
                        Text(url.deletingLastPathComponent().path(percentEncoded: false))
                            .font(.caption).foregroundStyle(.secondary)
                            .lineLimit(1).truncationMode(.head)
                    }
                }
                .buttonStyle(.plain)
            }
            .overlay {
                if app.recents.urls.isEmpty {
                    ContentUnavailableView("No Recent Archives", systemImage: "clock")
                }
            }
            .frame(width: 280)
        }
        .frame(height: 360)
        .onOpenURL { app.open(link: $0, then: show) }
    }

    private func show(_ root: URL) {
        openWindow(id: ArchiveWindow.windowID, value: root)
        dismissWindow(id: Self.windowID)
    }
}
