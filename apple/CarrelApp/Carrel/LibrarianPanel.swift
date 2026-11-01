// The Librarian panel beside an archive's window (⌥⌘L), "Ask About This" for any item
// (⇧⌘L, the item's context menu and its card), and confirming what the librarian suggests
// through the sheets that already make those changes: a correction for refiling or a field,
// a rename, a new collection.

import CarrelKit
import SwiftUI

extension View {
    /// Adds the Librarian panel to an archive's window, with its toolbar button.
    func librarianPanel(_ model: ArchiveWindowModel) -> some View {
        modifier(LibrarianPanelModifier(model: model))
    }
}

private struct LibrarianPanelModifier: ViewModifier {
    let model: ArchiveWindowModel

    func body(content: Content) -> some View {
        content
            .inspector(
                isPresented: Binding(
                    get: { model.librarian?.isShown ?? false },
                    set: { model.librarian?.isShown = $0 })
            ) {
                if let librarian = model.librarian {
                    LibrarianPanel(librarian: librarian, window: model)
                        .inspectorColumnWidth(min: 300, ideal: 380, max: 560)
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        if model.librarian?.isShown == true {
                            model.librarian?.isShown = false
                        } else {
                            model.librarian?.show()
                        }
                    } label: {
                        Label("Librarian", systemImage: "text.bubble")
                    }
                    .help("Ask the librarian about the archive")
                    .disabled(model.librarian == nil)
                }
            }
    }
}

struct LibrarianPanel: View {
    @Bindable var librarian: LibrarianModel
    let window: ArchiveWindowModel
    @FocusState private var questionFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { scroll in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 18) {
                        if librarian.answers.isEmpty, librarian.asking == nil {
                            introduction
                        }
                        ForEach(Array(librarian.answers.enumerated()), id: \.offset) { _, answer in
                            AnswerView(answer: answer, window: window)
                        }
                        if let asking = librarian.asking {
                            AskingView(question: asking, progress: librarian.progress)
                                .id("asking")
                        }
                        if !librarian.changes.isEmpty {
                            LibrarianChangesView(librarian: librarian)
                        }
                    }
                    .padding()
                }
                .onChange(of: librarian.answers.count) {
                    withAnimation { scroll.scrollTo(librarian.answers.count - 1, anchor: .top) }
                }
            }
            Divider()
            footer
        }
        .task(id: ObjectIdentifier(librarian)) { await librarian.refresh() }
        .onChange(of: librarian.focusRequests, initial: true) { questionFocused = true }
        .toolbar {
            ToolbarItemGroup {
                Button("Save to Journal", systemImage: "book.closed") {
                    Task { await librarian.save() }
                }
                .help("Save this conversation to this month’s Journal")
                .disabled(librarian.answers.isEmpty || librarian.isAsking || window.isReadOnly)
                Button("New Conversation", systemImage: "square.and.pencil") { librarian.clear() }
                    .help("Start again; this conversation is gone unless it was saved")
                    .disabled(librarian.answers.isEmpty && librarian.asking == nil)
            }
        }
    }

    private var introduction: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Ask the Librarian").font(.title3.bold())
            Text(
                "Ask about what’s in your archive, in your own words: what you have on a subject, what a document says, how two things connect. It answers with the items it found and the passages it read, and suggests where to look next."
            )
            .foregroundStyle(.secondary)
            Text(
                "Nothing you ask is kept unless you save the conversation to the Journal."
            )
            .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let problem = librarian.problem {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red).font(.callout).textSelection(.enabled)
            }
            if let saved = librarian.savedTo {
                Label("Saved to \(saved)", systemImage: "checkmark.circle")
                    .font(.callout).foregroundStyle(.secondary)
            }
            HStack(alignment: .bottom) {
                TextField("Ask about your archive", text: $librarian.draft, axis: .vertical)
                    .lineLimit(1...6)
                    .textFieldStyle(.roundedBorder)
                    .focused($questionFocused)
                    .onSubmit { Task { await librarian.ask() } }
                Button("Ask", systemImage: "arrow.up.circle.fill") {
                    Task { await librarian.ask() }
                }
                .labelStyle(.iconOnly)
                .font(.title2)
                .buttonStyle(.borderless)
                .disabled(!librarian.canAsk)
                .keyboardShortcut(.return, modifiers: .command)
            }
            Text(modelLine).font(.caption).foregroundStyle(.secondary)
        }
        .padding()
    }

    private var modelLine: String {
        let model = librarian.modelLabel.map { "Answers with \($0)." } ?? "No model chosen yet."
        return librarian.makesChanges
            ? "\(model) It may make changes it’s sure of; undo them here or in History."
            : "\(model) It only suggests changes, for you to confirm."
    }
}

/// The question being answered, and how far the librarian has got.
private struct AskingView: View {
    let question: String
    let progress: LibrarianProgress?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            QuestionBubble(text: question)
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(status).foregroundStyle(.secondary).font(.callout)
            }
        }
    }

    private var status: String {
        guard let progress, progress.step > 0 else { return "Thinking…" }
        let step = "Step \(progress.step) of up to \(progress.steps)"
        return progress.did.map { "\(step) · \($0)" } ?? step
    }
}

private struct QuestionBubble: View {
    let text: String

    var body: some View {
        Text(text)
            .textSelection(.enabled)
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(.tint.opacity(0.15), in: .rect(cornerRadius: 8))
            .frame(maxWidth: .infinity, alignment: .trailing)
    }
}

/// One answer: the text, the items it found, the passages it read, its suggestions and the
/// model that gave it.
private struct AnswerView: View {
    let answer: LibrarianAnswer
    let window: ArchiveWindowModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            QuestionBubble(text: answer.question)
            Text(answer.text).textSelection(.enabled)
            if !answer.items.isEmpty {
                Section {
                    ForEach(answer.items, id: \.id) { item in
                        Button {
                            window.open(item: item.id)
                        } label: {
                            Label(item.title, systemImage: "doc.text")
                                .lineLimit(1)
                        }
                        .buttonStyle(.link)
                        .help(item.card)
                    }
                } header: {
                    heading("Items")
                }
            }
            if !answer.passages.isEmpty {
                Section {
                    ForEach(Array(answer.passages.enumerated()), id: \.offset) { _, passage in
                        VStack(alignment: .leading, spacing: 2) {
                            Text("“\(passage.text)”").italic().textSelection(.enabled)
                            Button(passage.item.title) { window.open(item: passage.item.id) }
                                .buttonStyle(.link).font(.caption)
                        }
                        .padding(.leading, 8)
                        .overlay(alignment: .leading) {
                            Rectangle().fill(.quaternary).frame(width: 2)
                        }
                    }
                } header: {
                    heading("Passages")
                }
            }
            if !answer.suggestions.isEmpty {
                Section {
                    ForEach(Array(answer.suggestions.enumerated()), id: \.offset) { _, suggestion in
                        SuggestionRow(suggestion: suggestion, window: window)
                    }
                } header: {
                    heading("Suggestions")
                }
            }
            // Now and then, Carrel's question about an item the answer found.
            if let asks = answer.asks {
                VStack(alignment: .leading, spacing: 4) {
                    heading("While we’re here…")
                    AskedQuestionView(question: asks, window: window)
                }
                .padding(8)
                .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 6))
            }
            Text(footnote).font(.caption).foregroundStyle(.tertiary)
                .help(
                    answer.steps.map { "\($0.tool) \($0.input): \($0.did)" }.joined(separator: "\n")
                )
        }
    }

    private func heading(_ text: String) -> some View {
        Text(text).font(.caption.bold()).foregroundStyle(.secondary)
    }

    private var footnote: String {
        var parts = ["\(answer.model)"]
        if !answer.steps.isEmpty {
            parts.append(answer.steps.count == 1 ? "1 step" : "\(answer.steps.count) steps")
        }
        switch answer.stopped {
        case "steps": parts.append("out of steps")
        case "tokens": parts.append("out of room")
        default: break
        }
        return parts.joined(separator: " · ")
    }
}

/// A suggestion, and what became of it: made (with undo), or to confirm.
private struct SuggestionRow: View {
    let suggestion: LibrarianSuggestion
    let window: ArchiveWindowModel

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(suggestion.description.prefix(1).uppercased() + suggestion.description.dropFirst())
                .font(.callout.bold())
            if !suggestion.why.isEmpty {
                Text(suggestion.why).font(.callout).foregroundStyle(.secondary)
            }
            switch suggestion.outcome {
            case .made(let commit):
                HStack {
                    Label("Done by the librarian", systemImage: "checkmark.circle")
                        .font(.caption).foregroundStyle(.secondary)
                    if let commit {
                        Button("Undo") {
                            Task { await window.librarian?.undo(commitID: commit) }
                        }
                        .controlSize(.small)
                    }
                }
            case .proposed:
                Button("Review This Change…") { Task { await window.confirm(suggestion) } }
                    .controlSize(.small)
                    .disabled(!window.canChange)
            case .refused(let reason), .failed(let reason):
                Label(reason, systemImage: "xmark.circle")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(8)
        .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 6))
    }
}

/// The changes the librarian made itself this week, with undo for each and for all.
private struct LibrarianChangesView: View {
    let librarian: LibrarianModel
    @State private var confirmingAll = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("The Librarian’s Changes This Week").font(.caption.bold())
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Undo All…") { confirmingAll = true }.controlSize(.small)
            }
            ForEach(librarian.changes, id: \.id) { change in
                HStack {
                    VStack(alignment: .leading) {
                        Text(change.summary).lineLimit(2)
                        Text(change.when).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Undo") { Task { await librarian.undo(change) } }
                        .controlSize(.small)
                }
            }
        }
        .confirmationDialog(
            "Undo the librarian’s \(librarian.changes.count) changes this week?",
            isPresented: $confirmingAll
        ) {
            Button("Undo All") { Task { await librarian.undoAll() } }
        } message: {
            Text(
                "Each is undone as a new change, newest first. Any that can’t be are left and listed."
            )
        }
    }
}

extension LibrarianModel {
    /// Undoes the change with `commitID`, as the answer that made it names it.
    func undo(commitID: String) async {
        if changes.isEmpty { await refreshChanges() }
        guard let change = changes.first(where: { $0.id.hasPrefix(commitID) }) else {
            problem = "That change isn’t among the librarian’s this week; undo it from History."
            return
        }
        await undo(change)
    }
}

/// Opens the librarian with the item in the question.
struct AskAboutButton: View {
    let model: ArchiveWindowModel
    let item: ItemSummary

    var body: some View {
        Button("Ask About This", systemImage: "text.bubble") { model.librarian?.ask(about: item) }
            .disabled(model.librarian == nil)
    }
}

/// The Librarian's commands in the View menu.
struct LibrarianCommands: Commands {
    @FocusedValue(ArchiveWindowModel.self) private var window

    var body: some Commands {
        CommandGroup(before: .sidebar) {
            Button("Show Librarian") { window?.librarian?.show() }
                .keyboardShortcut("l", modifiers: [.command, .option])
                .disabled(window?.librarian == nil)
            Button("Ask About This") {
                if let item = window?.selectedItem { window?.librarian?.ask(about: item) }
            }
            .keyboardShortcut("l", modifiers: [.command, .shift])
            .disabled(window?.librarian == nil || window?.selectedItem == nil)
            Divider()
        }
    }
}

extension ArchiveWindowModel {
    /// Shows the item `id` in Browse, in its collection, with its card in the inspector.
    func open(item id: String) {
        guard let item = items.first(where: { $0.id == id }) else { return }
        if let path = collectionPath(of: item) { screen = .collection(path) }
        selectedItemID = id
    }

    /// Opens the sheet that makes `suggestion`, for the owner to see and confirm: a
    /// correction already worked out for a refile or a field, a rename, or a new collection.
    func confirm(_ suggestion: LibrarianSuggestion) async {
        switch suggestion.change {
        case .refile(let item, let to):
            await correct(item, words: "collection: \(to)")
        case .correct(let item, let field, let value):
            await correct(item, words: "\(field): \(value)")
        case .rename(let item, let name):
            guard let browse, let summary = items.first(where: { $0.id == item.id }) else { return }
            await browse.startRename(of: summary)
            browse.renaming?.name = name
        case .collection(let path, let description):
            browse?.creating = CollectionDraft(path: path, description: description)
        }
    }

    /// Opens the correction sheet for `item` with `words`, worked out already: they're
    /// `field: value` lines, which need no model.
    private func correct(_ item: LibrarianItem, words: String) async {
        guard let archive else { return }
        let correction = CorrectionModel(
            archive: archive, target: .item(id: item.id, title: item.title))
        correction.words = words
        correction.saved = { [weak self] in await self?.refreshItems() }
        self.correction = correction
        await correction.prepare()
    }
}
