// The menu commands for Review, Questions, corrections and editing, acting on the key
// window's archive, so every action is in the menus.

import CarrelKit
import SwiftUI

struct ReviewCommands: Commands {
    @FocusedValue(ArchiveWindowModel.self) private var window

    var body: some Commands {
        CommandGroup(after: .pasteboard) {
            Divider()
            Button("Correct…") { window?.correct() }
                .keyboardShortcut("k")
                .disabled(window?.correctionTarget == nil)
            Button("Edit Card") { window?.editSelectedItem() }
                .keyboardShortcut("e")
                .disabled(window?.canEditSelectedItem != true)
        }
        CommandMenu("Review") {
            if let review = window?.screen == .review ? window?.review : nil {
                ReviewActions(review: review, ids: review.selected.map(\.id))
                Divider()
                ReviewChoices(review: review, ids: review.selected.map(\.id))
                Divider()
                Button("Approve All High Confidence") { Task { await review.approveAllHigh() } }
                    .disabled(!review.proposals.contains(where: \.high))
            } else {
                Button("Approve") {}.disabled(true)
                Button("Approve All High Confidence") {}.disabled(true)
            }
        }
        CommandMenu("Questions") {
            let questions = window?.screen == .questions ? window?.questions : nil
            let id = questions?.selection
            Button("Answer") {
                if let id { Task { await questions?.answer(id) } }
            }
            .disabled(id.map { questions?.answer(for: $0).isEmpty ?? true } ?? true)
            Button("Dismiss") {
                if let id { Task { await questions?.dismiss(id) } }
            }
            .disabled(id == nil)
        }
    }
}

extension ArchiveWindowModel {
    /// What ⌘K would correct: the proposal shown in Review's inspector, or the chosen item.
    var correctionTarget: CorrectionModel.Target? {
        if screen == .review {
            return review?.inspected.map { .proposal(id: $0.id, title: $0.fields.title) }
        }
        return selectedItem.map { .item(id: $0.id, title: $0.title) }
    }

    /// Opens the correction sheet for what's chosen.
    func correct() {
        guard let archive, let target = correctionTarget else { return }
        let correction = CorrectionModel(archive: archive, target: target)
        correction.saved = { [weak self] in await self?.refreshItems() }
        self.correction = correction
    }

    var canEditSelectedItem: Bool {
        archive != nil && !isReadOnly && selectedItem != nil && editing == nil
    }

    /// Edits the chosen item's card in the inspector.
    func editSelectedItem() {
        guard let archive, let item = selectedItem else { return }
        let edit = CardEditModel(archive: archive, target: .item(item.id))
        edit.saved = { [weak self] in await self?.refreshItems() }
        editing = edit
    }
}

extension CorrectionModel: Identifiable {}

extension View {
    /// Shows the core's error from the last call that failed.
    func archiveErrorAlert(_ archive: ArchiveModel?) -> some View {
        alert(
            "Something Went Wrong",
            isPresented: Binding {
                archive?.lastError != nil
            } set: {
                if !$0 { archive?.lastError = nil }
            },
            presenting: archive?.lastError
        ) { _ in
            Button("OK") { archive?.lastError = nil }
        } message: { error in
            Text(error.message)
        }
    }
}
