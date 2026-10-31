// An archive's window: the screens in the sidebar, the chosen screen's list, and the chosen
// thing in the inspector column.

import CarrelKit
import SwiftUI

struct ArchiveWindow: View {
    static let windowID = "archive"

    @Environment(AppModel.self) private var app
    @Environment(\.dismissWindow) private var dismissWindow
    @Environment(\.controlActiveState) private var activeState
    @State private var model: ArchiveWindowModel

    init(url: URL) {
        _model = State(initialValue: ArchiveWindowModel(url: url))
    }

    var body: some View {
        @Bindable var model = model
        NavigationSplitView {
            Sidebar(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
        } content: {
            ScreenContent(model: model)
                .navigationSplitViewColumnWidth(min: 260, ideal: 340)
        } detail: {
            Inspector(model: model)
        }
        .navigationTitle(model.name)
        .navigationSubtitle(
            model.isReadOnly ? "\(model.screenTitle) · Read only" : model.screenTitle
        )
        .overlay {
            if let problem = model.problem {
                ContentUnavailableView {
                    Label("Can’t Open “\(model.url.lastPathComponent)”", systemImage: "archivebox")
                } description: {
                    Text(problem)
                } actions: {
                    Button("Try Again") { Task { await model.load() } }
                }
                .background(.background)
            }
        }
        .sheet(item: $model.correction) { CorrectionSheet(correction: $0) }
        .archiveErrorAlert(model.archive)
        .focusedSceneValue(model)
        .task {
            dismissWindow(id: WelcomeView.windowID)
            await model.load()
        }
        .onChange(of: activeState, initial: true) {
            if activeState == .key { app.activeArchive = model.root }
        }
    }
}

private struct Sidebar: View {
    @Bindable var model: ArchiveWindowModel

    var body: some View {
        List(selection: $model.screen) {
            Section {
                Label("Status", systemImage: "gauge.with.dots.needle.33percent").tag(Screen.status)
                Label("Review", systemImage: "tray.full").tag(Screen.review)
                    .badge(model.archive?.reviewCount ?? 0)
                Label("Questions", systemImage: "questionmark.bubble").tag(Screen.questions)
                    .badge(model.archive?.questions.count ?? 0)
                Label("Active", systemImage: "arrow.up.forward.square").tag(Screen.active)
            }
            Section("Collections") {
                OutlineGroup(model.tree, children: \.children) { node in
                    Label(node.collection.title, systemImage: "folder")
                        .badge(model.items(in: node.collection.path).count)
                        .help(node.collection.description ?? node.collection.path)
                        .tag(Screen.collection(node.collection.path))
                }
            }
            Section {
                Label("History", systemImage: "clock.arrow.circlepath").tag(Screen.history)
            }
        }
        .listStyle(.sidebar)
    }
}

/// The content column for the chosen screen.
private struct ScreenContent: View {
    @Bindable var model: ArchiveWindowModel

    var body: some View {
        switch model.screen {
        case .status, nil:
            StatusView(model: model)
        case .collection:
            List(model.listedItems, selection: $model.selectedItemID) { item in
                Label(item.title, systemImage: item.kind.symbol)
            }
            .overlay {
                if model.listedItems.isEmpty, !model.isLoading {
                    ContentUnavailableView("No Items", systemImage: "folder")
                }
            }
        case .review:
            if let review = model.review {
                ReviewView(review: review, collections: model.collections.map(\.path))
            }
        case .questions:
            if let questions = model.questions { QuestionsView(questions: questions) }
        case .active:
            ComingScreen(
                title: "Active", systemImage: "arrow.up.forward.square",
                text: "Checked-out and desk items with their pending entries, to return or cancel.")
        case .history:
            ComingScreen(
                title: "History", systemImage: "clock.arrow.circlepath",
                text: "Every change to the archive, with undo.")
        }
    }
}

/// A screen that arrives with CarrelKit's operations (#233) and the app's later steps.
private struct ComingScreen: View {
    let title: String
    let systemImage: String
    let text: String

    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: systemImage)
        } description: {
            Text(text)
            Text("Coming in a later build. Until then, run `carrel` for the interactive mode.")
        }
    }
}

/// The inspector column: the chosen item's card.
private struct Inspector: View {
    let model: ArchiveWindowModel

    var body: some View {
        if model.screen == .review, let review = model.review, let proposal = review.inspected {
            ProposalInspector(proposal: proposal, review: review, correct: model.correct)
        } else if model.screen == .questions, let questions = model.questions,
            let question = questions.selected
        {
            QuestionInspector(
                question: question, questions: questions,
                collections: model.collections.map(\.path))
        } else if let item = model.selectedItem {
            if let edit = model.editing, edit.target == .item(item.id) {
                ItemEditor(item: item, edit: edit) { model.editing = nil }
            } else {
                ItemCardView(item: item, card: model.card(for: item), root: model.root)
                    .id(item.id)
            }
        } else if case .collection = model.screen {
            ContentUnavailableView(
                "Nothing Selected", systemImage: "sidebar.right",
                description: Text("Choose an item to see its card."))
        } else {
            ContentUnavailableView("Nothing Selected", systemImage: "sidebar.right")
        }
    }
}

extension ItemKind {
    var symbol: String {
        switch self {
        case .file: "doc"
        case .folder: "folder"
        case .bundle: "shippingbox"
        case .physical: "books.vertical"
        case .url: "link"
        case .external: "externaldrive"
        case .tombstone: "archivebox"
        }
    }

    var name: String {
        switch self {
        case .file: "File"
        case .folder: "Folder"
        case .bundle: "Bundle"
        case .physical: "Physical"
        case .url: "Web page"
        case .external: "External"
        case .tombstone: "Removed"
        }
    }
}
