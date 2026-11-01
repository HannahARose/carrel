// Status's Gaps (#455): what Carrel couldn't do on this Mac, as `carrel report gaps` lists it.
// Report… shows the issue draft, with room for the owner's own words, and opens it in the
// browser to edit and submit there; Carrel never sends it. The issue's address, once given
// back, marks the gap as reported. Report a Missing Feature… records one in the owner's words.

import AppKit
import CarrelKit
import SwiftUI

struct StatusGapsSection: View {
    let window: ArchiveWindowModel

    @State private var gaps: [GapRecord] = []
    @State private var reporting: ReportingGap?
    @State private var problem: String?

    var body: some View {
        Section {
            if gaps.isEmpty {
                Text("Carrel has done all it was asked on this Mac so far.")
                    .foregroundStyle(.secondary)
            }
            ForEach(gaps, id: \.number) { gap in
                LabeledContent {
                    Button(gap.reported == nil ? "Report…" : "Reported…") {
                        reporting = ReportingGap(gap: gap)
                    }
                } label: {
                    Text(gap.summary)
                    Text(Self.detail(of: gap))
                }
            }
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
            }
        } header: {
            HStack {
                Text("Gaps")
                Spacer()
                Button("Report a Missing Feature…") {
                    window.missingFeature = MissingFeatureModel(item: nil)
                }
                .buttonStyle(.link)
            }
        } footer: {
            Text(
                "No titles, text, paths or file names are kept. "
                    + "Nothing is sent unless you submit a draft in your browser."
            )
            .font(.footnote).foregroundStyle(.secondary)
        }
        .sheet(item: $reporting, onDismiss: { Task { await load() } }) {
            GapReportSheet(window: window, gap: $0.gap)
        }
        .task(
            id:
                "\(window.handle != nil)-\(window.summary.itemCount)-\(window.missingFeature == nil)"
        ) {
            await load()
        }
    }

    private func load() async {
        do {
            gaps = try await window.archive?.gaps() ?? []
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }

    /// A gap's counts, versions and report, in a line.
    static func detail(of gap: GapRecord) -> String {
        var parts = [
            "\(gap.items) item\(gap.items == 1 ? "" : "s"), \(gap.firstSeen) to \(gap.lastSeen), "
                + "Carrel \(gap.lastVersion)"
        ]
        if let words = gap.words { parts.append("“\(words)”") }
        if let reported = gap.reported {
            parts.append("Reported: \(reported)")
        }
        if gap.notSeenSinceUpdate {
            parts.append(
                "not seen since Carrel \(gap.lastVersion), so this version may have addressed it")
        }
        return parts.joined(separator: " · ")
    }
}

/// The gap Report… is open for.
struct ReportingGap: Identifiable {
    let gap: GapRecord
    var id: UInt32 { gap.number }
}

/// The issue draft for a gap: what it holds, words of the owner's own to add, and opening it
/// in the browser; then the address it was reported at.
struct GapReportSheet: View {
    let window: ArchiveWindowModel
    let gap: GapRecord

    @Environment(\.dismiss) private var dismiss
    @State private var added = ""
    @State private var draft: GapIssueDraft?
    @State private var address = ""
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Report to Development").font(.headline)
            Text(
                "The draft opens on GitHub in your browser, to read, edit and submit there. "
                    + "Carrel sends nothing itself."
            )
            .foregroundStyle(.secondary)
            TextField("Anything to add, in your own words", text: $added, axis: .vertical)
                .lineLimit(2...5)
                .textFieldStyle(.roundedBorder)
            if let draft {
                GroupBox(draft.title) {
                    ScrollView {
                        Text(draft.body).font(.callout.monospaced()).textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 160)
                }
            }
            LabeledContent("Reported at") {
                TextField("The issue’s address, once submitted", text: $address)
                    .textFieldStyle(.roundedBorder)
            }
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
            }
            HStack {
                Button("Cancel", role: .cancel) { dismiss() }
                Spacer()
                Button("Save Address") { Task { await saveAddress() } }
                    .disabled(address == (gap.reported ?? ""))
                Button("Open Draft in Browser") { Task { await open() } }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 520)
        .task(id: added) {
            draft = try? await window.archive?.gapIssueDraft(gap.number, added: added)
        }
        .onAppear { address = gap.reported ?? "" }
    }

    private func open() async {
        do {
            guard let draft = try await window.archive?.gapIssueDraft(gap.number, added: added),
                let url = URL(string: draft.url)
            else { return }
            NSWorkspace.shared.open(url)
        } catch {
            problem = error.localizedDescription
        }
    }

    private func saveAddress() async {
        do {
            try await window.archive?.markGapReported(gap.number, at: address)
            dismiss()
        } catch {
            problem = error.localizedDescription
        }
    }
}

/// A missing feature the owner reports in their own words, about an item or none.
@Observable
final class MissingFeatureModel: Identifiable {
    let id = UUID()
    /// The item it's about, if any.
    let item: ItemSummary?
    var words = ""

    init(item: ItemSummary?) {
        self.item = item
    }
}

struct MissingFeatureSheet: View {
    let window: ArchiveWindowModel
    @Bindable var model: MissingFeatureModel

    @Environment(\.dismiss) private var dismiss
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(
                model.item.map { "Report a Missing Feature for “\($0.title)”" }
                    ?? "Report a Missing Feature"
            )
            .font(.headline)
            Text(
                model.item == nil
                    ? "Say what Carrel should be able to do. It’s kept on this Mac as a gap, "
                        + "to report to development from Status."
                    : "Say what Carrel should be able to do with it. Its type is kept with your "
                        + "words, never its name, as a gap to report from Status."
            )
            .foregroundStyle(.secondary)
            TextField("What Carrel should do", text: $model.words, axis: .vertical)
                .lineLimit(3...8)
                .textFieldStyle(.roundedBorder)
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
            }
            HStack {
                Button("Cancel", role: .cancel) { dismiss() }
                Spacer()
                Button("Record") { Task { await record() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.words.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 480)
    }

    private func record() async {
        do {
            try await window.archive?.reportMissingFeature(
                about: model.item?.id, words: model.words)
            dismiss()
        } catch {
            problem = error.localizedDescription
        }
    }
}
