// An archive's window: the screens in the sidebar, the chosen screen's list, and the chosen
// thing in the inspector column.

import CarrelKit
import SwiftUI

struct ArchiveWindow: View {
    static let windowID = "archive"

    @Environment(AppModel.self) private var app
    @Environment(\.dismissWindow) private var dismissWindow
    @Environment(\.openWindow) private var openWindow
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
        .activeAndHistorySheets(model)
        .modifier(ArchiveWindowActions(model: model))
        .browseSheets(model.browse)
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
            choosePendingItem()
        }
        .onChange(of: app.pendingLink) { choosePendingItem() }
        .handlesExternalEvents(preferring: [AgentLink.scheme], allowing: [AgentLink.scheme])
        .onOpenURL { url in
            app.open(link: url) { openWindow(id: Self.windowID, value: $0) }
        }
        .onChange(of: activeState, initial: true) {
            if activeState == .key { app.activeArchive = model.root }
        }
    }
}

extension ArchiveWindow {
    /// Chooses the item a link from the agent's menu asked for, on Active.
    fileprivate func choosePendingItem() {
        guard model.handle != nil, let item = app.takePendingItem(for: model.root) else { return }
        model.screen = .active
        model.selectedItemID = item
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
                Label("Active", systemImage: "arrow.up.forward.square")
                    .badge(model.activeScreen?.pendingCount ?? 0)
                    .tag(Screen.active)
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
        if let search = model.search, search.isActive {
            SearchResultsView(model: model, search: search)
        } else {
            screen
        }
    }

    @ViewBuilder private var screen: some View {
        switch model.screen {
        case .status, nil:
            StatusView(model: model)
        case .collection:
            Group {
                if model.browseLayout == .grid {
                    BrowseGrid(model: model)
                } else {
                    List(model.listedItems, selection: $model.selectedItemID) { item in
                        Label(item.title, systemImage: item.kind.symbol)
                    }
                    .contextMenu(forSelectionType: ItemSummary.ID.self) { ids in
                        BrowseActions(model: model, ids: ids)
                    }
                }
            }
            .overlay {
                if model.listedItems.isEmpty, !model.isLoading {
                    ContentUnavailableView("No Items", systemImage: "folder")
                }
            }
            .toolbar {
                Picker("View", selection: $model.browseLayout) {
                    Label("List", systemImage: "list.bullet").tag(BrowseLayout.list)
                    Label("Grid", systemImage: "square.grid.2x2").tag(BrowseLayout.grid)
                }
                .pickerStyle(.segmented)
                .help("Show the items as a list or a grid")
            }
        case .review:
            if let review = model.review {
                ReviewView(review: review, collections: model.collections.map(\.path))
            }
        case .questions:
            if let questions = model.questions { QuestionsView(questions: questions) }
        case .active:
            if let active = model.activeScreen { ActiveView(model: active) }
        case .history:
            if let history = model.historyScreen { HistoryView(model: history) }
        }
    }
}

/// The inspector column: the chosen item's preview and card.
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
        } else if model.screen == .active, let active = model.activeScreen {
            ActiveInspector(model: active)
        } else if model.screen == .history, let history = model.historyScreen {
            ChangeInspector(model: history) { id in model.items.first { $0.id == id }?.title }
        } else if let item = model.selectedItem {
            if let edit = model.editing, edit.target == .item(item.id) {
                ItemEditor(item: item, edit: edit) { model.editing = nil }
            } else {
                VStack(spacing: 0) {
                    if let previews = model.previews {
                        ItemPreviewView(item: item, store: previews)
                    }
                    ItemCardView(item: item, card: model.card(for: item), root: model.root) {
                        ItemActions(model: model, item: item)
                    }
                }
                .id(item.id)
            }
        } else if case .collection = model.screen {
            ContentUnavailableView(
                "Nothing Selected", systemImage: "sidebar.right",
                description: Text("Choose an item to see its preview and card."))
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
