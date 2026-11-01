// The crawler's findings in Review, "From the crawler": the list, each finding in the
// inspector with why and how sure the model was, approved or rejected; and on Status, how far
// the crawler has got and the changes it made on its own this week.

import CarrelKit
import SwiftUI

/// Review's switch between the proposals and the crawler's findings.
struct ReviewSourcePicker: View {
    @Bindable var crawler: CrawlerModel
    let proposals: Int

    var body: some View {
        Picker("Show", selection: $crawler.isShown) {
            Text(proposals == 1 ? "1 Proposal" : "\(proposals) Proposals").tag(false)
            Text("From the Crawler (\(crawler.findings.count))").tag(true)
        }
        .pickerStyle(.segmented)
        .help("Switch between drafted proposals and the crawler’s findings (f)")
    }
}

struct CrawlerFindingsView: View {
    @Bindable var crawler: CrawlerModel

    var body: some View {
        List(selection: $crawler.selection) {
            Section("From the crawler") {
                ForEach(crawler.findings) { finding in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(finding.title).lineLimit(1)
                        Text(finding.change).font(.caption).foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    .tag(finding.id)
                }
            }
        }
        .contextMenu(forSelectionType: CrawlerFinding.ID.self) { ids in
            let ids = crawler.findings.map(\.id).filter(ids.contains)
            Button("Approve") { Task { await crawler.approve(ids) } }
            Button("Reject") { Task { await crawler.reject(ids) } }
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "far"), phases: .down) { press in
            guard press.modifiers.isEmpty else { return .ignored }
            return crawler.key(press.characters) ? .handled : .ignored
        }
        .overlay {
            if crawler.findings.isEmpty {
                ContentUnavailableView(
                    "Nothing From the Crawler", systemImage: "binoculars",
                    description: Text(
                        "The crawler has found nothing to review. It rechecks where items are filed when this Mac is idle."
                    ))
            }
        }
        .safeAreaInset(edge: .bottom) {
            if let report = crawler.report {
                ReportBar(text: report)
            }
        }
    }
}

struct CrawlerFindingInspector: View {
    let finding: CrawlerFinding
    let crawler: CrawlerModel
    let window: ArchiveWindowModel

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Label("From the crawler", systemImage: "binoculars")
                        .font(.caption).foregroundStyle(.secondary)
                    Text(finding.title).font(.title3.bold())
                    Button("Show in Browse") { window.open(item: finding.item) }
                        .buttonStyle(.link)
                }
                .textSelection(.enabled)
            }
            Section("What It Would Change") {
                if let from = finding.from, let to = finding.to {
                    LabeledContent("Collection") {
                        HStack {
                            Text(from).strikethrough().foregroundStyle(.secondary)
                            Image(systemName: "arrow.right").foregroundStyle(.secondary)
                            Text(to)
                        }
                    }
                } else if let field = finding.field, let value = finding.value {
                    LabeledContent(field.capitalized, value: value)
                }
                LabeledContent("Why") { Text(finding.reason).textSelection(.enabled) }
                LabeledContent(
                    "How sure",
                    value: finding.confidence.formatted(.percent.precision(.fractionLength(0))))
                LabeledContent("Model", value: finding.model)
            }
            Section {
                HStack {
                    Button("Reject") { Task { await crawler.reject([finding.id]) } }
                    Spacer()
                    Button("Approve") { Task { await crawler.approve([finding.id]) } }
                        .keyboardShortcut(.defaultAction)
                }
                .disabled(window.isReadOnly || crawler.archive.isWorking)
            } footer: {
                Text(
                    "Approving makes the change as yours, undone from History; rejecting changes nothing, and the crawler won’t suggest it again."
                )
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

/// Status's crawler section: where it has got, and its own changes this week.
struct StatusCrawlerSection: View {
    let window: ArchiveWindowModel
    @State private var confirmingUndo = false

    var body: some View {
        Group {
            if let crawler = window.crawler, let status = crawler.status {
                Section {
                    Text(status.line)
                    if let report = crawler.report {
                        Text(report).font(.callout).foregroundStyle(.secondary)
                    }
                    ForEach(status.waiting, id: \.stage) { stage in
                        LabeledContent(stage.stage.capitalized, value: "\(stage.items) due")
                    }
                    if status.findings > 0 {
                        Button(
                            status.findings == 1
                                ? "1 finding to review" : "\(status.findings) findings to review"
                        ) {
                            window.screen = .review
                            crawler.isShown = true
                        }
                        .buttonStyle(.link)
                    }
                    ForEach(crawler.changes.prefix(5), id: \.id) { change in
                        LabeledContent(change.summary, value: change.when)
                    }
                    if crawler.changes.count > 5 {
                        Text("and \(crawler.changes.count - 5) more, in History")
                            .foregroundStyle(.secondary)
                    }
                    HStack {
                        Button("Check Some Now") { Task { await crawler.checkNow() } }
                            .help("Let the crawler check a few items now, whatever its pace")
                        if !crawler.changes.isEmpty {
                            Button("Undo All of Them…") { confirmingUndo = true }
                        }
                    }
                    .disabled(window.isReadOnly || crawler.archive.isWorking)
                } header: {
                    Text(
                        crawler.changes.isEmpty ? "The Crawler" : "The Crawler’s Changes This Week")
                }
                .confirmationDialog(
                    "Undo the \(crawler.changes.count) changes the crawler made this week?",
                    isPresented: $confirmingUndo
                ) {
                    Button("Undo All") { Task { await crawler.undoAll() } }
                } message: {
                    Text(
                        "Each is undone as a new change, newest first. Any that can’t be are left and listed."
                    )
                }
            }
        }
        .task(id: window.crawler.map(ObjectIdentifier.init)) { await window.crawler?.refresh() }
    }
}
