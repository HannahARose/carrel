// An archive's window: the screens in the sidebar, the chosen screen's list, and the chosen
// thing in the inspector column.

import CarrelKit
import SwiftUI

struct ArchiveWindow: View {
    static let windowID = "archive"

    @Environment(AppModel.self) private var app
    @Environment(\.dismissWindow) private var dismissWindow
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings
    @Environment(\.controlActiveState) private var activeState
    @State private var model: ArchiveWindowModel

    init(url: URL) {
        _model = State(initialValue: ArchiveWindowModel(url: url))
    }

    var body: some View {
        @Bindable var model = model
        NavigationSplitView {
            ArchiveSidebar(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
        } content: {
            ArchiveScreenContent(model: model)
                .navigationSplitViewColumnWidth(min: 260, ideal: 340)
        } detail: {
            Inspector(model: model)
        }
        .activeAndHistorySheets(model)
        .modifier(ArchiveWindowActions(model: model))
        .browseSheets(model.browse)
        .librarianPanel(model)
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
        .sheet(item: $model.missingFeature) { MissingFeatureSheet(window: model, model: $0) }
        .archiveErrorAlert(model.archive)
        .focusedSceneValue(model)
        .task {
            dismissWindow(id: WelcomeView.windowID)
            await model.load()
            choosePendingItem()
        }
        .onChange(of: app.pendingLink) { choosePendingItem() }
        .onChange(of: app.pendingScreen, initial: true) {
            if let screen = app.takePendingScreen(for: model.root) { model.screen = screen }
        }
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
    /// Chooses the item a link asked for: on Active when it's checked out or on the desk, as
    /// in the agent's menu, or when a link from Finder asks to do something with it, which
    /// starts; else in Browse, as from Spotlight.
    /// A screen a link asks for is shown first, so an item it names is chosen where it is.
    fileprivate func choosePendingItem() {
        guard model.handle != nil, let link = app.takePendingLink(for: model.root) else { return }
        if let screen = link.screen {
            model.show(linked: screen)
            if screen == .settings { openSettings() }
        }
        guard let item = link.item else { return }
        if link.action == nil, model.archive?.active.contains(where: { $0.id == item }) == false {
            model.open(item: item)
            return
        }
        model.screen = .active
        model.selectedItemID = item
        if let action = link.action { Task { await model.start(action, on: item) } }
    }
}

struct ArchiveSidebar: View {
    @Bindable var model: ArchiveWindowModel

    var body: some View {
        // A row's tag goes last, after its badge: a tag with a modifier after it isn't the
        // row's, so the row can't be chosen (#334).
        List(selection: $model.screen) {
            Section {
                Label("Status", systemImage: "gauge.with.dots.needle.33percent").tag(Screen.status)
                Label("Review", systemImage: "tray.full")
                    .badge(
                        (model.archive?.reviewCount ?? 0)
                            + (model.archive?.crawlerFindings.count ?? 0)
                    )
                    .tag(Screen.review)
                Label("Questions", systemImage: "questionmark.bubble")
                    .badge(model.archive?.questions.count ?? 0)
                    .tag(Screen.questions)
                Label("Restructure", systemImage: "rectangle.3.group")
                    .badge(model.archive?.restructureProposals.count ?? 0)
                    .tag(Screen.restructure)
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
            Section("Discover") {
                ForEach(DiscoverPage.allCases, id: \.self) { page in
                    Label(page.title, systemImage: page.symbol).tag(Screen.discover(page))
                }
            }
            Section {
                Label("History", systemImage: "clock.arrow.circlepath").tag(Screen.history)
                Label("Backups", systemImage: "externaldrive.badge.timemachine").tag(Screen.backups)
            }
        }
        .listStyle(.sidebar)
    }
}

/// The content column for the chosen screen.
struct ArchiveScreenContent: View {
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
            VStack(spacing: 0) {
                if let crawler = model.crawler {
                    ReviewSourcePicker(
                        crawler: crawler, proposals: model.review?.proposals.count ?? 0
                    )
                    .padding(8)
                }
                if let crawler = model.crawler, crawler.isShown {
                    CrawlerFindingsView(crawler: crawler)
                } else if let review = model.review {
                    ReviewView(review: review, collections: model.collections.map(\.path))
                }
            }
        case .questions:
            if let questions = model.questions { QuestionsView(questions: questions) }
        case .restructure:
            if let restructure = model.restructure {
                RestructureView(
                    restructure: restructure, collections: model.collections.map(\.path))
            }
        case .active:
            if let active = model.activeScreen { ActiveView(model: active) }
        case .history:
            if let history = model.historyScreen { HistoryView(model: history) }
        case .backups:
            if let backups = model.backups { BackupsView(backups: backups) }
        case .discover(let page):
            if let discover = model.discover {
                DiscoverView(page: page, discover: discover, window: model)
            }
        }
    }
}

/// The inspector column: the chosen item's preview and card.
private struct Inspector: View {
    let model: ArchiveWindowModel

    var body: some View {
        if model.screen == .review, let crawler = model.crawler, crawler.isShown {
            if let finding = crawler.inspected {
                CrawlerFindingInspector(finding: finding, crawler: crawler, window: model)
            } else {
                ContentUnavailableView("Nothing Selected", systemImage: "binoculars")
            }
        } else if model.screen == .review, let review = model.review,
            let proposal = review.inspected
        {
            ProposalInspector(proposal: proposal, review: review, correct: model.correct)
        } else if model.screen == .questions, let questions = model.questions,
            let question = questions.selected
        {
            QuestionInspector(
                question: question, questions: questions,
                collections: model.collections.map(\.path), previews: model.previews
            ) { id in model.items.first { $0.id == id } }
        } else if model.screen == .restructure, let restructure = model.restructure,
            let proposal = restructure.selected
        {
            RestructureInspector(proposal: proposal, restructure: restructure)
        } else if model.screen == .active, let active = model.activeScreen {
            ActiveInspector(model: active)
        } else if model.screen == .history, let history = model.historyScreen {
            ChangeInspector(model: history) { id in model.items.first { $0.id == id }?.title }
        } else if model.screen == .backups, let backups = model.backups,
            let change = backups.selected
        {
            BackupInspector(change: change, backups: backups, items: model.items)
        } else if let item = model.selectedItem {
            if let edit = model.editing, edit.target == .item(item.id) {
                ItemEditor(item: item, edit: edit, items: model.items) { model.editing = nil }
            } else {
                VStack(spacing: 0) {
                    if let previews = model.previews {
                        ItemPreviewView(item: item, store: previews)
                    }
                    ItemCardView(
                        item: item, card: model.card(for: item), root: model.root,
                        archive: model.archive, open: model.open(item:)
                    ) {
                        ItemActions(model: model, item: item)
                    } below: {
                        ItemQuestionsSection(item: item, window: model)
                    }
                }
                .id(item.id)
            }
        } else if case .collection(let path) = model.screen,
            let collection = model.collections.first(where: { $0.path == path })
        {
            CollectionInspector(collection: collection, window: model)
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
