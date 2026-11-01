// The Review screen's list: the proposals waiting, under the collections they'd go to, with
// the decisions on a selection from the context menu, the Review menu and single letters.

import CarrelKit
import SwiftUI

struct ReviewView: View {
    @Bindable var review: ReviewModel
    /// The collections a proposal can be moved to.
    let collections: [String]

    var body: some View {
        List(selection: $review.selection) {
            ForEach(review.groups) { group in
                Section(group.destination) {
                    ForEach(group.proposals) { proposal in
                        ProposalRow(proposal: proposal).tag(proposal.id)
                    }
                }
            }
        }
        .contextMenu(forSelectionType: Proposal.ID.self) { ids in
            ReviewActions(review: review, ids: review.ordered(ids))
            Divider()
            ReviewChoices(review: review, ids: review.ordered(ids))
        }
        .onKeyPress(characters: Self.keys, phases: .down) { press in
            guard press.modifiers.isEmpty else { return .ignored }
            return review.key(press.characters) ? .handled : .ignored
        }
        .overlay {
            if review.groups.isEmpty {
                ContentUnavailableView(
                    "Nothing to Review", systemImage: "tray",
                    description: Text("Proposals appear here once new items are drafted."))
            }
        }
        .safeAreaInset(edge: .bottom) {
            if let report = review.report {
                ReportBar(text: report)
            }
        }
        .sheet(isPresented: review.isMoving) {
            CollectionPicker(
                title: review.moving?.count == 1
                    ? "Move the proposal to" : "Move \(review.moving?.count ?? 0) proposals to",
                collections: collections
            ) { collection in
                let ids = review.moving ?? []
                Task { await review.move(ids, to: collection) }
            }
        }
    }

    /// The single letters the list answers to: approve, hold, move, reject, draft again,
    /// expand or keep compressed, a duplicate filed as a version or beside the original, and
    /// the crawler's findings instead.
    static let keys = CharacterSet(charactersIn: "ahmrdzvbf")
}

extension ReviewModel {
    /// `ids` in the list's order.
    func ordered(_ ids: Set<Proposal.ID>) -> [Proposal.ID] {
        groups.flatMap(\.proposals).map(\.id).filter(ids.contains)
    }

    /// Whether the collection picker is open, for a sheet.
    var isMoving: Binding<Bool> {
        Binding {
            self.moving != nil
        } set: {
            if !$0 { self.moving = nil }
        }
    }

    /// Acts on a single letter typed in the list; returns whether it meant something.
    func key(_ characters: String) -> Bool {
        if characters == "f" {
            showCrawler()
            return true
        }
        let ids = selected.map(\.id)
        guard !ids.isEmpty else { return false }
        switch characters {
        case "a": Task { await approve(ids) }
        case "h": Task { await hold(ids) }
        case "m": moving = ids
        case "r": Task { await reject(ids) }
        case "d": Task { await redraft(ids) }
        case "z":
            guard selected.first?.compressed != nil else { return false }
            Task { await toggleCompressed(ids) }
        case "v", "b":
            guard ids.count == 1, selected.first?.duplicates.isEmpty == false else { return false }
            Task { await keepDuplicate(ids[0], asVersion: characters == "v") }
        default: return false
        }
        return true
    }
}

/// A proposal in the list: its title, kind, and what's notable about it.
private struct ProposalRow: View {
    let proposal: Proposal

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(proposal.fields.title).lineLimit(1)
                Text(proposal.flags.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
        } icon: {
            Image(
                systemName: proposal.state == .needsOwner
                    ? "exclamationmark.circle" : proposal.kind.symbol
            )
            .foregroundStyle(
                proposal.state == .needsOwner ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
        }
        .help(proposal.source)
    }
}

extension Proposal {
    /// What `carrel review` notes beside it: confidence, state, sensitivity, compression and
    /// duplicates.
    var flags: [String] {
        var flags = [confidence.name]
        if state == .needsOwner { flags.append("needs you") }
        if draftAgain { flags.append("draft again?") }
        if let readingsFlag { flags.append(readingsFlag) }
        if [.sensitive, .restricted].contains(fields.sensitivity) {
            flags.append(fields.sensitivity.name)
        }
        if let compressed {
            flags.append(
                compressed.asking ? "expand?" : compressed.expand ? "expands" : "kept compressed")
        }
        if let first = duplicates.first {
            flags.append(first.why == ["same checksum"] ? "duplicate" : "likely duplicate")
        }
        return flags
    }
}

/// The decisions on proposals, for the context menu and the Review menu.
struct ReviewActions: View {
    let review: ReviewModel
    let ids: [Proposal.ID]

    var body: some View {
        Button("Approve") { Task { await review.approve(ids) } }
            .disabled(ids.isEmpty)
        Button("Approve Batch") {
            if let id = ids.first { Task { await review.approveBatch(of: id) } }
        }
        .disabled(ids.count != 1)
        Button("Hold") { Task { await review.hold(ids) } }
            .disabled(ids.isEmpty)
        Button("Move To…") { review.moving = ids }
            .disabled(ids.isEmpty)
        Button("Reject") { Task { await review.reject(ids) } }
            .disabled(ids.isEmpty)
        Button("Draft Again") { Task { await review.redraft(ids) } }
            .disabled(ids.isEmpty)
    }
}

/// The choices for compressed files and duplicates, for the Review menu and the context menu.
struct ReviewChoices: View {
    let review: ReviewModel
    let ids: [Proposal.ID]

    private var chosen: [Proposal] { review.proposals.filter { ids.contains($0.id) } }

    var body: some View {
        let compressed = !chosen.isEmpty && chosen.allSatisfy { $0.compressed != nil }
        Button("Expand When Filed") { Task { await review.chooseCompressed(ids, expand: true) } }
            .disabled(!compressed)
        Button("Keep Compressed") { Task { await review.chooseCompressed(ids, expand: false) } }
            .disabled(!compressed)
        let duplicate = chosen.count == 1 && chosen[0].duplicates.isEmpty == false
        Button("File as Version") {
            if let id = ids.first { Task { await review.keepDuplicate(id, asVersion: true) } }
        }
        .disabled(!duplicate)
        Button("Keep Both") {
            if let id = ids.first { Task { await review.keepDuplicate(id, asVersion: false) } }
        }
        .disabled(!duplicate)
    }
}

/// What the last action did, along the bottom of a list.
struct ReportBar: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.callout)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.bar)
            .textSelection(.enabled)
    }
}

/// Chooses a collection, by path, filtering as you type.
struct CollectionPicker: View {
    let title: String
    let collections: [String]
    let choose: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var filter = ""
    @State private var chosen: String?

    private var shown: [String] {
        filter.isEmpty
            ? collections : collections.filter { $0.localizedCaseInsensitiveContains(filter) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.headline)
            TextField("Filter", text: $filter)
                .textFieldStyle(.roundedBorder)
                .onSubmit { if shown.count == 1 { pick(shown[0]) } }
            List(shown, id: \.self, selection: $chosen) { Text($0) }
                .contextMenu(forSelectionType: String.self) { _ in
                } primaryAction: { paths in
                    if let path = paths.first { pick(path) }
                }
                .frame(minHeight: 240)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Move") { chosen.map(pick) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(chosen == nil)
            }
        }
        .padding()
        .frame(width: 380)
    }

    private func pick(_ path: String) {
        dismiss()
        choose(path)
    }
}

extension Confidence {
    var name: String {
        switch self {
        case .low: "low"
        case .medium: "medium"
        case .high: "high"
        }
    }
}

extension Sensitivity {
    /// Least sensitive first.
    static let ordered: [Sensitivity] = [.public, .personal, .sensitive, .restricted]

    var name: String {
        switch self {
        case .public: "public"
        case .personal: "personal"
        case .sensitive: "sensitive"
        case .restricted: "restricted"
        }
    }
}
