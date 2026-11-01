// Active: what's checked out and on this Mac's desk, each with its pending history entries.
// The single letters work in the list as in the interactive mode: r return, x cancel,
// o open, a approve, c combine, e edit, d drop.

import AppKit
import CarrelKit
import SwiftUI

struct ActiveView: View {
    @Bindable var model: ActiveScreenModel

    var body: some View {
        List(selection: $model.selection) {
            ForEach(model.items, id: \.id) { item in
                Section {
                    ActiveItemRow(item: item).tag(ActiveRow.item(item.id))
                    ForEach(Array(item.pending.enumerated()), id: \.offset) { index, entry in
                        PendingEntryRow(entry: entry)
                            .tag(ActiveRow.entry(item.id, UInt32(index + 1)))
                    }
                }
            }
        }
        .contextMenu(forSelectionType: ActiveRow.self) { _ in
            ActiveActions(model: model)
        }
        .onKeyPress(characters: .init(charactersIn: "rxoaced")) { press in
            key(press.characters) ? .handled : .ignored
        }
        .overlay {
            if model.items.isEmpty {
                ContentUnavailableView {
                    Label("Nothing Active", systemImage: "arrow.up.forward.square")
                } description: {
                    Text("Check out an item from Browse, or put a folder on the desk.")
                } actions: {
                    Button("Add to Desk…") { chooseDeskItem(for: model) }
                }
            }
        }
        .toolbar {
            ToolbarItem {
                Button("Add to Desk…", systemImage: "plus") { chooseDeskItem(for: model) }
            }
        }
    }

    /// The interactive mode's letters, for what's chosen.
    private func key(_ characters: String) -> Bool {
        guard let item = model.selectedItem else { return false }
        switch characters {
        case "r" where model.canReturn: Task { await model.askToReturn() }
        case "x" where model.canReturn: model.askToCancel()
        case "o": openWorkingCopy(item, of: model)
        case "a" where model.canDecide: Task { await model.decide(.approve) }
        case "c" where model.canCombine: Task { await model.decide(.combine) }
        case "d" where model.canDecide: Task { await model.decide(.drop) }
        case "e" where model.canEdit: model.startEditing()
        default: return false
        }
        return true
    }
}

private struct ActiveItemRow: View {
    let item: ActiveItem

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Label(
                item.title,
                systemImage: item.kind == .checkedOut ? "arrow.up.forward.square" : "tray")
            HStack(spacing: 6) {
                Text(item.kind == .checkedOut ? "Checked out" : "On the desk")
                if let due = item.due { Text("· due \(due)") }
                if item.state != "unchanged" { Text("· \(item.state)") }
            }
            .font(.caption).foregroundStyle(.secondary)
        }
        .badge(item.pending.count)
    }
}

private struct PendingEntryRow: View {
    let entry: CardHistoryEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(entry.heading).font(.caption).foregroundStyle(.secondary)
            Text(entry.lines.joined(separator: "\n")).lineLimit(3)
        }
        .padding(.leading, 22)
    }
}

extension CardHistoryEntry {
    /// Its date and length, as the card's heading shows them.
    var heading: String {
        guard let minutes else { return date }
        let length =
            minutes >= 60
            ? "\(minutes / 60)h" + (minutes % 60 > 0 ? " \(minutes % 60)m" : "")
            : "\(minutes)m"
        return "\(date) · \(length)"
    }
}

/// The actions for what's chosen in Active, for the context menu and the inspector.
struct ActiveActions: View {
    let model: ActiveScreenModel

    var body: some View {
        if let item = model.selectedItem {
            if item.kind == .checkedOut {
                Button("Return…") { Task { await model.askToReturn() } }
                Button("Return From…") { chooseMovedWorkingCopy(for: model) }
                    .help("Return it from where its working copy has moved to")
                Button("Cancel Checkout…", role: .destructive) { model.askToCancel() }
            }
            Button("Show in Finder") { openWorkingCopy(item, of: model) }
            if model.canDecide {
                Divider()
                Button(model.selectedEntries.isEmpty ? "Approve All Entries" : "Approve") {
                    Task { await model.decide(.approve) }
                }
                Button("Combine") { Task { await model.decide(.combine) } }
                    .disabled(!model.canCombine)
                Button("Edit Entry…") { model.startEditing() }
                    .disabled(!model.canEdit)
                Button(model.selectedEntries.isEmpty ? "Drop All Entries" : "Drop") {
                    Task { await model.decide(.drop) }
                }
            }
            if item.kind == .desk {
                Divider()
                Button("Take Off Desk…") { model.askToTakeOff() }
            }
        }
    }
}

/// Shows the working copy, or the desk item, in Finder.
func openWorkingCopy(_ item: ActiveItem, of model: ActiveScreenModel) {
    NSWorkspace.shared.activateFileViewerSelecting([model.workingCopy(of: item)])
}

/// Asks for a file or folder to put on the desk.
func chooseDeskItem(for model: ActiveScreenModel) {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = true
    panel.allowsMultipleSelection = false
    panel.prompt = "Add to Desk"
    panel.message = "Choose a folder or file to work on where it is."
    guard panel.runModal() == .OK, let url = panel.url else { return }
    model.startAddingToDesk(url)
}

/// Asks where the chosen checkout's working copy has moved to, then what returning it from
/// there brings back.
func chooseMovedWorkingCopy(for model: ActiveScreenModel) {
    guard let item = model.selectedItem, item.kind == .checkedOut else { return }
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = true
    panel.allowsMultipleSelection = false
    panel.prompt = "Return From Here"
    panel.message = "Choose where the working copy of “\(item.title)” is now."
    guard panel.runModal() == .OK, let url = panel.url else { return }
    Task { await model.askToReturn(from: url) }
}

/// The inspector for Active: the chosen item, what changed, and its pending entries.
struct ActiveInspector: View {
    let model: ActiveScreenModel

    var body: some View {
        if let item = model.selectedItem {
            Form {
                Section {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(item.kind == .checkedOut ? "Checked out" : "On the desk")
                            .font(.caption).foregroundStyle(.secondary)
                        Text(item.title).font(.title2.bold())
                    }
                    .textSelection(.enabled)
                }
                Section {
                    LabeledContent(item.kind == .checkedOut ? "Working copy" : "Where") {
                        Text(item.working).textSelection(.enabled)
                            .lineLimit(2).truncationMode(.middle)
                    }
                    LabeledContent("Since", value: item.since)
                    if let due = item.due { LabeledContent("Due back", value: due) }
                    LabeledContent("State", value: item.state.capitalized)
                }
                if let changes = item.changes { ChangesSection(changes: changes) }
                if !item.pending.isEmpty {
                    Section("Pending Entries") {
                        ForEach(Array(item.pending.enumerated()), id: \.offset) { _, entry in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(entry.heading).font(.caption).foregroundStyle(.secondary)
                                Text(entry.lines.joined(separator: "\n")).textSelection(.enabled)
                            }
                        }
                    }
                }
                Section {
                    HStack { ActiveActions(model: model) }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                }
            }
            .formStyle(.grouped)
            .id(item.id)
        } else {
            ContentUnavailableView(
                "Nothing Selected", systemImage: "sidebar.right",
                description: Text("Choose an item or one of its entries."))
        }
    }
}

/// The files that changed in a working copy.
struct ChangesSection: View {
    let changes: Changes

    var body: some View {
        let groups = [
            ("Changed", changes.changed), ("Added", changes.added),
            ("Removed", changes.removed), ("Renamed", changes.renamed),
        ].filter { !$0.1.isEmpty }
        Section("What Changed") {
            if groups.isEmpty {
                Text("Nothing has changed.").foregroundStyle(.secondary)
            }
            ForEach(groups, id: \.0) { name, files in
                LabeledContent(name) {
                    Text(files.joined(separator: "\n")).textSelection(.enabled)
                }
            }
        }
    }
}
