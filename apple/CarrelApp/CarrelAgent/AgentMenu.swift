// The menu bar list: what's checked out and on the desk in each archive, with pending
// entries, the crawler's findings and its last pass, what's waiting to be drafted, what the
// agent is doing now, and Pause. Choosing an
// item opens it in the app.

import CarrelKit
import SwiftUI

struct AgentMenu: View {
    let host: AgentHost

    private var agent: Agent { host.agent }

    var body: some View {
        if agent.archives.isEmpty {
            Text("No archives yet. Open one in Carrel.")
        }
        ForEach(agent.archives) { archive in
            Section(archive.name) {
                ArchiveItems(archive: archive, open: host.open)
            }
        }
        Divider()
        Text(status)
        Button(agent.isPaused ? "Resume" : "Pause") { host.togglePause() }
        Divider()
        Button("Open Carrel") { host.openApp() }
        Button("Quit Carrel Agent") { NSApplication.shared.terminate(nil) }
            .keyboardShortcut("q")
    }

    private var status: String {
        if agent.isPaused { return "Paused: the command line drafts and indexes" }
        return agent.activity ?? "Waiting for changes"
    }
}

private struct ArchiveItems: View {
    let archive: AgentArchive
    let open: (AgentLink) -> Void

    var body: some View {
        if let problem = archive.problem {
            Text(problem)
        }
        let checkedOut = archive.active.filter { $0.kind == .checkedOut }
        let desk = archive.active.filter { $0.kind == .desk }
        if checkedOut.isEmpty && desk.isEmpty && archive.problem == nil {
            Text("Nothing checked out or on the desk")
        }
        ForEach(checkedOut) { item in
            row(item, systemImage: "arrow.up.forward.square")
        }
        ForEach(desk) { item in
            row(item, systemImage: "tray")
        }
        if archive.pending > 0 {
            Button(
                archive.pending == 1
                    ? "1 entry to approve" : "\(archive.pending) entries to approve"
            ) {
                open(AgentLink(archive: archive.root))
            }
        }
        if archive.findings > 0 {
            Button(
                archive.findings == 1
                    ? "1 finding from the crawler" : "\(archive.findings) findings from the crawler"
            ) {
                open(AgentLink(archive: archive.root))
            }
        }
        if let crawled = archive.crawled {
            Text(crawled)
        }
        if archive.queued > 0 {
            Text(
                archive.queued == 1
                    ? "1 item waiting to be drafted"
                    : "\(archive.queued) items waiting to be drafted")
        }
    }

    private func row(_ item: ActiveItem, systemImage: String) -> some View {
        Button {
            open(AgentLink(archive: archive.root, item: item.id))
        } label: {
            Label(
                item.pending.isEmpty ? item.title : "\(item.title) (\(item.pending.count) pending)",
                systemImage: systemImage)
        }
    }
}
