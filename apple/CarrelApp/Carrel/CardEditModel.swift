// A card's fields edited in place in the inspector: a proposal's before it's filed, or a filed
// item's after Edit (⌘E), whose every field the owner sets, links, location, URL and snapshots
// among them, and whose body, Notes and all, are edited too. Both are saved against the card
// as the editor opened it, so what was saved another way meanwhile isn't overwritten. Saving runs the checks
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
    /// The link types it may have links of; none for a proposal, whose links are set once
    /// it's filed.
    private(set) var linkTypes: [String] = []
    /// Whether it needs a location, a URL, and whether it may take snapshots, by its kind.
    private(set) var needsLocation = false
    private(set) var needsURL = false
    private(set) var takesSnapshots = false
    /// A filed card's body as saved: its Notes and Carrel's sections.
    private(set) var originalBody: String?
    /// The body as edited.
    var body: String?
    /// Why the last save didn't check out.
    private(set) var problems: [String] = []
    /// The body as saved another way while it was being edited, on the same lines as the
    /// edit, and both versions together: nothing was saved, and the owner chooses.
    private(set) var bodyClash: (current: String, both: String)?
    /// The fields saved another way while they were being edited, to other values than the
    /// edit's, and the card's fields as they are now: nothing was saved, and the owner
    /// chooses.
    private(set) var fieldsClash: (fields: [String], current: CardFields)?
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
            linkTypes = editable.linkTypes
            needsLocation = editable.needsLocation
            needsURL = editable.needsUrl
            takesSnapshots = editable.takesSnapshots
            problems = []
            bodyClash = nil
            fieldsClash = nil
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
        let fields = [
            "title", "summary", "tag", "date", "sensitivity", "link", "location", "url",
            "snapshot",
        ]
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
            if fields != original, let original {
                outcome = try? await archive.edit(item: id, base: original, fields: fields)
            }
            switch outcome {
            case .problems, .fieldsChanged, nil: break
            default:
                guard bodyChanged, let body else { break }
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
        case .fieldsChanged(let changed, let current):
            problems = []
            fieldsClash = (changed, current)
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
        fieldsClash = nil
    }

    /// After a clash of fields, keeps the edit, to save again against the fields as they are
    /// now.
    func keepMyFields() {
        guard let clash = fieldsClash else { return }
        original = clash.current
        fieldsClash = nil
    }

    /// After a clash of fields, drops the edit of the fields for the card's as they are now.
    func reloadFields() {
        guard let clash = fieldsClash else { return }
        original = clash.current
        fields = clash.current
        fieldsClash = nil
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
