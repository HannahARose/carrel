// Status's parts from the core's summary, as `carrel status` and the interactive mode show
// them: what was filed, changed or worked on lately; cards with problems; names that won't
// work on every system, with the name to rename them to; signs the structure needs a
// restructure; and the desk's suggestions, folders to put on it and items to archive.

import CarrelKit
import SwiftUI

struct StatusArchiveSections: View {
    let model: ArchiveWindowModel

    var body: some View {
        if let status = model.archive?.status {
            RecentSection(model: model, recent: status.recent)
            if !status.problems.isEmpty {
                Section("Cards with Problems") {
                    ForEach(status.problems, id: \.card) { problem in
                        CardProblemRow(model: model, problem: problem)
                    }
                }
            }
            if !status.names.isEmpty {
                Section {
                    ForEach(status.names, id: \.path) { name in
                        NameProblemRow(model: model, name: name)
                    }
                } header: {
                    Text("Names That Won’t Work Everywhere")
                }
            }
            if !status.signals.isEmpty {
                Section("The Structure") {
                    ForEach(status.signals, id: \.self) { signal in
                        Label(signal, systemImage: "rectangle.3.group")
                    }
                }
            }
            DeskSuggestionsSection(model: model, status: status)
        }
    }
}

/// What was filed, changed or worked on lately, newest first. A change opens in History; work
/// opens its item.
private struct RecentSection: View {
    let model: ArchiveWindowModel
    let recent: [RecentEntry]

    var body: some View {
        Section("Recent") {
            if recent.isEmpty {
                Text("Nothing filed, changed or worked on lately.").foregroundStyle(.secondary)
            }
            ForEach(Array(recent.prefix(10).enumerated()), id: \.offset) { _, entry in
                Button {
                    open(entry)
                } label: {
                    LabeledContent {
                        Text(entry.when)
                    } label: {
                        Label(entry.title, systemImage: entry.change == nil ? "hammer" : "clock")
                        if let detail = detail(of: entry) { Text(detail) }
                    }
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func detail(of entry: RecentEntry) -> String? {
        if let duration = entry.duration {
            return [duration, entry.note].compactMap(\.self).joined(separator: " · ")
        }
        return entry.change?.operation
    }

    private func open(_ entry: RecentEntry) {
        if let change = entry.change {
            model.screen = .history
            model.historyScreen?.selectedID = change.id
        } else if let item = entry.item {
            model.open(item: item)
        }
    }
}

/// A card whose fields don't conform, with Edit to fix it.
private struct CardProblemRow: View {
    let model: ArchiveWindowModel
    let problem: CardProblem

    var body: some View {
        let item = model.items.first { $0.card == problem.card }
        LabeledContent {
            if let item {
                Button("Show") { model.open(item: item.id) }
            }
        } label: {
            Label(item?.title ?? problem.card, systemImage: "exclamationmark.triangle")
            Text(problem.problems.joined(separator: "; "))
        }
        .help(problem.card)
    }
}

/// A path whose name won't work on every system, with Rename when it's an item's.
private struct NameProblemRow: View {
    let model: ArchiveWindowModel
    let name: NameProblem

    var body: some View {
        let item = model.items.first { $0.item == name.path || $0.card == name.path }
        LabeledContent {
            if let item, name.suggestion != nil, let browse = model.browse {
                Button("Rename…") { Task { await browse.startRename(of: item) } }
                    .disabled(!model.canChange)
            }
        } label: {
            Text(name.path).lineLimit(1).truncationMode(.middle)
            Text(
                ([name.problems.joined(separator: "; ")]
                    + (name.suggestion.map { ["rename it to “\($0)”"] } ?? []))
                    .joined(separator: " · "))
        }
        .help(name.path)
    }
}

/// The desk's suggestions: folders that look like active work, to put on the desk, and desk
/// items unchanged for a long time, to archive.
private struct DeskSuggestionsSection: View {
    let model: ArchiveWindowModel
    let status: StatusSummary

    var body: some View {
        if !status.toTrack.isEmpty || !status.toArchive.isEmpty {
            Section("Desk Suggestions") {
                ForEach(status.toTrack, id: \.path) { folder in
                    LabeledContent {
                        Button("Add to Desk…") {
                            model.screen = .active
                            model.activeScreen?.startAddingToDesk(URL(filePath: folder.path))
                        }
                        .disabled(model.isReadOnly)
                    } label: {
                        Label(
                            URL(filePath: folder.path).lastPathComponent,
                            systemImage: folder.git
                                ? "chevron.left.forwardslash.chevron.right" : "folder")
                        Text(
                            "Looks like active work"
                                + (folder.changed.map { ", changed \($0)" } ?? ""))
                    }
                    .help(folder.path)
                }
                ForEach(status.toArchive, id: \.path) { desk in
                    LabeledContent {
                        if let item = model.activeScreen?.items.first(where: {
                            $0.kind == .desk && $0.working == desk.path
                        }) {
                            Button("Archive…") {
                                model.screen = .active
                                model.activeScreen?.selection = [.item(item.id)]
                                model.activeScreen?.askToTakeOff()
                            }
                            .disabled(model.isReadOnly)
                        }
                    } label: {
                        Label(URL(filePath: desk.path).lastPathComponent, systemImage: "tray")
                        Text("On the desk, unchanged since \(desk.changed)")
                    }
                    .help(desk.path)
                }
            }
        }
    }
}
