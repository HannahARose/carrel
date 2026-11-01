// The Restructure screen: the proposals waiting in the list, and the chosen one in the
// inspector, with the archive before and after side by side, each change with its reason and
// how invasive it is, chosen or left out, then approving the chosen changes, all of them, or
// rejecting the proposal.

import CarrelKit
import SwiftUI

struct RestructureView: View {
    @Bindable var restructure: RestructureModel
    /// The collections a section proposal can cover.
    let collections: [String]

    @State private var choosingSection = false

    /// The picker's entry for the whole archive.
    static let wholeArchive = "The Whole Archive"

    var body: some View {
        List(selection: $restructure.selection) {
            ForEach(restructure.proposals) { proposal in
                VStack(alignment: .leading, spacing: 2) {
                    Text(
                        proposal.rationale.isEmpty ? proposal.kind.capitalized : proposal.rationale
                    )
                    .lineLimit(2)
                    Text(proposal.summary).font(.caption).foregroundStyle(.secondary)
                }
                .tag(proposal.id)
            }
        }
        .overlay {
            if restructure.proposals.isEmpty {
                ContentUnavailableView {
                    Label("No Proposals", systemImage: "rectangle.3.group")
                } description: {
                    Text(
                        "Proposals to reorganize collections and tags appear here, for you to approve in whole or in part."
                    )
                } actions: {
                    Button("Propose Tag Changes") { Task { await restructure.proposeTags() } }
                        .disabled(restructure.archive.isWorking)
                }
            }
        }
        .toolbar {
            Button("Propose Changes…", systemImage: "rectangle.3.group") {
                choosingSection = true
            }
            .help("Look over a section of the archive for a better shape")
            .disabled(restructure.archive.isWorking)
            Button("Propose Tag Changes", systemImage: "tag") {
                Task { await restructure.proposeTags() }
            }
            .help("Look over the archive's tags for ones to merge, split or retire")
            .disabled(restructure.archive.isWorking)
        }
        .sheet(isPresented: $choosingSection) {
            CollectionPicker(
                title: "Propose changes to", collections: [Self.wholeArchive] + collections
            ) { collection in
                let section = collection == Self.wholeArchive ? "" : collection
                Task { await restructure.proposeSection(section) }
            }
        }
        .safeAreaInset(edge: .bottom) {
            if let report = restructure.report {
                ReportBar(text: report)
            }
        }
    }
}

struct RestructureInspector: View {
    let proposal: RestructureProposal
    @Bindable var restructure: RestructureModel

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label(proposal.summary, systemImage: "rectangle.3.group")
                        .font(.caption).foregroundStyle(.secondary)
                    Text(proposal.rationale).font(.title3.bold()).textSelection(.enabled)
                }
            }
            if let assessment = proposal.assessment {
                Section("How It Was Judged") {
                    AssessmentView(assessment: assessment)
                }
            }
            if let preview = restructure.preview {
                Section("Before and After") {
                    BeforeAndAfter(preview: preview)
                }
            }
            Section("Changes") {
                ForEach(proposal.changes) { change in
                    ChangeRow(change: change, proposal: proposal, restructure: restructure)
                }
            }
            Section {
                let blocked = restructure.blocked(in: proposal)
                if !blocked.isEmpty {
                    Text(
                        "Approving drops \(blocked.map { "change \($0.id)" }.joined(separator: ", ")) too: each needs a change left out."
                    )
                    .font(.callout).foregroundStyle(.secondary)
                }
                HStack {
                    Button("Reject", role: .destructive) { Task { await restructure.reject() } }
                    Spacer()
                    Button("Approve All") { Task { await restructure.approve(all: true) } }
                    Button("Approve Chosen") { Task { await restructure.approve() } }
                        .keyboardShortcut(.defaultAction)
                        .disabled(restructure.chosen(in: proposal).isEmpty)
                }
                .disabled(restructure.archive.isWorking)
            } footer: {
                Text("Approving is one change in History, which undoes it as a whole.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

/// One change: chosen or left out, what it does, why, how invasive, and what it touches.
private struct ChangeRow: View {
    let change: RestructureChange
    let proposal: RestructureProposal
    let restructure: RestructureModel

    var body: some View {
        Toggle(
            isOn: Binding(
                get: { restructure.isChosen(change, in: proposal) },
                set: { restructure.choose(change, in: proposal, $0) })
        ) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline) {
                    Text("\(change.id). \(change.description)").bold()
                    Spacer()
                    InvasivenessBadge(invasiveness: change.invasiveness)
                        .help(change.invasivenessWords)
                }
                Text(change.reason).foregroundStyle(.secondary)
                if !change.items.isEmpty {
                    DisclosureGroup(
                        change.items.count == 1 ? "1 item" : "\(change.items.count) items"
                    ) {
                        ForEach(change.items, id: \.id) { Text($0.title).font(.callout) }
                    }
                    .font(.callout)
                }
                ForEach(change.held, id: \.id) { held in
                    Text("Leaves “\(held.title)” alone: \(held.why ?? "")")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if !change.documents.isEmpty {
                    Text("Rewrites \(change.documents.joined(separator: ", "))")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if !change.dependsOn.isEmpty {
                    Text("Needs change \(change.dependsOn.joined(separator: ", "))")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .toggleStyle(.checkbox)
    }
}

/// How invasive a change is, from 1 to 5 dots.
struct InvasivenessBadge: View {
    let invasiveness: Invasiveness

    var body: some View {
        HStack(spacing: 2) {
            ForEach(1...5, id: \.self) { step in
                Circle()
                    .fill(step <= invasiveness.rank ? Color.orange : Color.secondary.opacity(0.25))
                    .frame(width: 6, height: 6)
            }
            Text(invasiveness.name).font(.caption).foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(invasiveness.name), \(invasiveness.rank) of 5")
    }
}

/// The collections, and the tags the proposal touches, before and after, side by side.
private struct BeforeAndAfter: View {
    let preview: RestructurePreview

    var body: some View {
        Grid(alignment: .topLeading, horizontalSpacing: 16, verticalSpacing: 8) {
            GridRow {
                Text("Now").font(.caption.bold()).foregroundStyle(.secondary)
                Text("After").font(.caption.bold()).foregroundStyle(.secondary)
            }
            GridRow {
                Tree(collections: preview.before, mark: "minus.circle")
                Tree(collections: preview.after, mark: "plus.circle")
            }
            if !preview.tagsBefore.isEmpty || !preview.tagsAfter.isEmpty {
                GridRow {
                    Tags(tags: preview.tagsBefore, other: preview.tagsAfter)
                    Tags(tags: preview.tagsAfter, other: preview.tagsBefore)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private struct Tree: View {
        let collections: [PreviewedCollection]
        let mark: String

        var body: some View {
            VStack(alignment: .leading, spacing: 2) {
                ForEach(collections, id: \.path) { collection in
                    let depth = collection.path.split(separator: "/").count - 1
                    HStack(spacing: 4) {
                        Image(systemName: collection.changed ? mark : "folder")
                            .foregroundStyle(collection.changed ? Color.accentColor : .secondary)
                        Text(collection.path.split(separator: "/").last.map(String.init) ?? "")
                            .fontWeight(collection.changed ? .semibold : .regular)
                        Text("\(collection.items)").font(.caption).foregroundStyle(.secondary)
                    }
                    .padding(.leading, CGFloat(depth) * 14)
                    .help(collection.path)
                }
            }
        }
    }

    private struct Tags: View {
        let tags: [PreviewedTag]
        let other: [PreviewedTag]

        var body: some View {
            VStack(alignment: .leading, spacing: 2) {
                ForEach(tags, id: \.tag) { tag in
                    let same = other.contains(tag)
                    Text("#\(tag.tag) (\(tag.items))")
                        .font(.callout)
                        .foregroundStyle(same ? .secondary : .primary)
                        .fontWeight(same ? .regular : .semibold)
                }
                if tags.isEmpty {
                    Text("None of these tags").font(.callout).foregroundStyle(.secondary)
                }
            }
        }
    }
}

/// How a section proposal was judged: the eagerness, benefit and cost, the findability test,
/// and what it held back.
private struct AssessmentView: View {
    let assessment: RestructureAssessment

    var body: some View {
        LabeledContent("Eagerness") {
            Text(
                assessment.eagernessFrom.map { "\(assessment.eagerness.capitalized), set by \($0)" }
                    ?? "\(assessment.eagerness.capitalized), the archive’s")
        }
        LabeledContent(
            "Benefit and cost",
            value: String(format: "%.2f against %.2f", assessment.benefit, assessment.cost))
        if let tried = assessment.tried, let now = assessment.foundNow,
            let after = assessment.foundAfter
        {
            LabeledContent("Findability") {
                Text("\(now) of \(tried) found now, \(after) after")
            }
        } else if let untested = assessment.untested {
            LabeledContent("Findability") { Text("Untested: \(untested)") }
        }
        ForEach(assessment.heldBack, id: \.self) { held in
            Text("Held back: \(held)").font(.callout).foregroundStyle(.secondary)
        }
    }
}
