// The Active and History menus: every action on those screens, with a key, for the key
// archive window.

import CarrelKit
import SwiftUI

struct ActiveCommands: Commands {
    @FocusedValue(\.activeWindow) private var window

    var body: some Commands {
        CommandMenu("Active") {
            ActiveMenu(window: window)
        }
        CommandMenu("History") {
            Button("Undo Change…") { window?.historyScreen?.askToUndo() }
                .keyboardShortcut("z", modifiers: [.command, .option])
                .disabled(window?.screen != .history || window?.historyScreen?.selected == nil)
        }
    }
}

private struct ActiveMenu: View {
    let window: ArchiveWindowModel?

    var body: some View {
        let browsed = window?.selectedItem
        let active = window?.screen == .active ? window?.activeScreen : nil
        let item = active?.selectedItem
        Button("Check Out…") {
            if let browsed {
                window?.activeScreen?.startCheckout(of: browsed.id, titled: browsed.title)
            }
        }
        .keyboardShortcut("c", modifiers: [.command, .option])
        .disabled(browsed == nil)
        Button("Add to Desk…") {
            if let screen = window?.activeScreen { chooseDeskItem(for: screen) }
        }
        .keyboardShortcut("d", modifiers: [.command, .option])
        .disabled(window?.activeScreen == nil)
        Divider()
        Button("Return…") { Task { await active?.askToReturn() } }
            .keyboardShortcut("r", modifiers: [.command, .option])
            .disabled(!(active?.canReturn ?? false))
        Button("Return From…") { if let active { chooseMovedWorkingCopy(for: active) } }
            .disabled(!(active?.canReturn ?? false))
        Button("Take Off Desk…") { active?.askToTakeOff() }
            .disabled(item?.kind != .desk)
        Button("Cancel Checkout…") { active?.askToCancel() }
            .keyboardShortcut(.delete, modifiers: [.command, .option])
            .disabled(!(active?.canReturn ?? false))
        Button("Show Working Copy in \(fileBrowser)") {
            if let active, let item { openWorkingCopy(item, of: active) }
        }
        .keyboardShortcut("o", modifiers: [.command, .option])
        .disabled(item == nil)
        Divider()
        Button("Approve Entries") { Task { await active?.decide(.approve) } }
            .keyboardShortcut("a", modifiers: [.command, .option])
            .disabled(!(active?.canDecide ?? false))
        Button("Combine Entries") { Task { await active?.decide(.combine) } }
            .keyboardShortcut("j", modifiers: [.command, .option])
            .disabled(!(active?.canCombine ?? false))
        Button("Edit Entry…") { active?.startEditing() }
            .keyboardShortcut("e", modifiers: [.command, .option])
            .disabled(!(active?.canEdit ?? false))
        Button("Drop Entries") { Task { await active?.decide(.drop) } }
            .keyboardShortcut(.delete, modifiers: [.command, .option, .shift])
            .disabled(!(active?.canDecide ?? false))
    }
}

/// Browse's quick actions for an item: checking it out, renaming and expanding it; and
/// creating a collection, with or without one chosen.
struct BrowseActions: View {
    let model: ArchiveWindowModel
    let ids: Set<ItemSummary.ID>

    var body: some View {
        if ids.count == 1, let id = ids.first, let item = model.items.first(where: { $0.id == id })
        {
            Button("Check Out…") { model.activeScreen?.startCheckout(of: id, titled: item.title) }
            ItemActions(model: model, item: item)
            Divider()
        }
        NewCollectionButton(model: model)
    }
}
