// A card's fields edited in place in the inspector: a proposal's before it's filed, or a filed
// item's after Edit (⌘E), whose body, Notes and all, is edited too. Saving runs the checks
// `carrel edit` runs, in the core; problems come back to show beside the fields.

import CarrelKit
import Foundation
import Observation

@Observable
final class CardEditModel {
    /// Whose card it is.
    enum Target: Hashable {
        case proposal(String)
        case item(String)
    }

    let archive: ArchiveModel
    let target: Target

    /// The fields as saved.
    private(set) var original: CardFields?
    /// The fields as edited.
    var fields: CardFields?
    /// The tags that apply where it is or goes, to choose from.
    private(set) var tags: [String] = []
    /// The lowest sensitivity it may be given.
    private(set) var floor: Sensitivity = .public
    /// A filed card's body as saved: its Notes and Carrel's sections.
    private(set) var originalBody: String?
    /// The body as edited.
    var body: String?
    /// Why the last save didn't check out.
    private(set) var problems: [String] = []
    /// The body as saved another way while it was being edited, on the same lines as the
    /// edit, and both versions together: nothing was saved, and the owner chooses.
    private(set) var bodyClash: (current: String, both: String)?
    /// Why the card can't be edited, when it can't be read.
    private(set) var unavailable: String?
    /// Called after a save, so the lists show the change.
    @ObservationIgnored var saved: () async -> Void = {}

    init(archive: ArchiveModel, target: Target) {
        self.archive = archive
        self.target = target
    }

    /// Reads the card's fields to edit.
    func load() async {
        let target = target
        do {
            let editable = try await archive.call { handle in
                switch target {
                case .proposal(let id): try handle.editableProposal(id: id)
                case .item(let id): try handle.editableCard(item: id)
                }
            }
            let body: String? = try await archive.call { handle in
                switch target {
                case .proposal: nil
                case .item(let id): try handle.cardBody(item: id)
                }
            }
            original = editable.fields
            fields = editable.fields
            originalBody = body
            self.body = body
            tags = editable.tags
            floor = editable.floor
            problems = []
            bodyClash = nil
            unavailable = nil
        } catch {
            archive.lastError = nil
            unavailable = message(for: error)
        }
    }

    var hasChanges: Bool { (fields != nil && fields != original) || bodyChanged }

    private var bodyChanged: Bool { body != nil && body != originalBody }

    /// The sensitivities it may be given, least first.
    var sensitivities: [Sensitivity] {
        Array(Sensitivity.ordered.drop { $0 != floor })
    }

    /// The tags it doesn't have yet, to add.
    var addableTags: [String] {
        tags.filter { !(fields?.tags.contains($0) ?? true) }
    }

    /// The problems that name `field`, to show beside it.
    func problems(about field: String) -> [String] {
        problems.filter { $0.localizedCaseInsensitiveContains(field) }
    }

    /// The problems that name none of the fields, to show above them.
    var otherProblems: [String] {
        let fields = ["title", "summary", "tag", "date", "sensitivity"]
        return problems.filter { problem in
            !fields.contains { problem.localizedCaseInsensitiveContains($0) }
        }
    }

    /// Saves the edited fields, checked as `carrel edit` checks them. Returns whether they
    /// were saved (or nothing had changed).
    @discardableResult
    func save() async -> Bool {
        guard let fields, hasChanges else { return true }
        var outcome: EditOutcome? = .unchanged
        switch target {
        case .proposal(let id):
            outcome = try? await archive.edit(proposal: id, fields: fields)
        case .item(let id):
            if fields != original { outcome = try? await archive.edit(item: id, fields: fields) }
            if case .problems = outcome {
            } else if outcome != nil, bodyChanged, let body {
                // The fields are saved, whatever becomes of the body.
                original = fields
                outcome = try? await archive.edit(item: id, base: originalBody ?? "", body: body)
            }
        }
        switch outcome {
        case .problems(let problems):
            self.problems = problems
            return false
        case .bodyChanged(let current, let both):
            problems = []
            bodyClash = (current, both)
            return false
        case .saved, .unchanged:
            problems = []
            await load()
            await saved()
            return true
        case nil:
            return false
        }
    }

    /// Puts the fields back as they're saved.
    func revert() {
        fields = original
        body = originalBody
        problems = []
        bodyClash = nil
    }

    /// After a clash, edits both versions together, to tidy and save against the body as it
    /// is now.
    func keepBothBodies() {
        guard let clash = bodyClash else { return }
        originalBody = clash.current
        body = clash.both
        bodyClash = nil
    }

    /// After a clash, drops the edit for the body as it is now.
    func reloadBody() {
        guard let clash = bodyClash else { return }
        originalBody = clash.current
        body = clash.current
        bodyClash = nil
    }
}
