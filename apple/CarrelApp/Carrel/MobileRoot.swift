// The iPad and iPhone app's root (docs/design/sync-and-mobile.md, "iPad and iPhone"): the
// archive that's open, in the same window the Mac shows, or the archives to choose from. An
// archive outside the app's own folder, such as one in iCloud Drive, is reached through the
// document picker, and kept with a bookmark so it opens again next time.

#if os(iOS)
import CarrelKit
import Observation
import SwiftUI

/// The archives this device knows, and the one that's open.
@Observable
final class MobileLibrary {
    static let bookmarksKey = "archiveBookmarks"
    /// The archive that's open, which background work and the next launch use.
    static let openKey = "openArchive"

    let app: AppModel
    private let defaults: UserDefaults
    /// The root of the archive that's open.
    private(set) var open: URL?
    /// What went wrong opening or creating an archive, until it's dismissed.
    var problem: String?
    /// Whether Settings is showing, as a sheet over the archive.
    var showingSettings = false

    init(app: AppModel, defaults: UserDefaults = .app) {
        self.app = app
        self.defaults = defaults
    }

    /// The folder in the app's container that Files shows as "On My iPad" or "On My iPhone":
    /// new archives and checked-out working copies go here unless another place is chosen.
    static var documents: URL { URL.documentsDirectory }

    /// Opens the archive this device had open last, if it's still there.
    func reopen() {
        guard open == nil, let path = defaults.string(forKey: Self.openKey) else { return }
        let url = URL(filePath: path, directoryHint: .isDirectory)
        _ = access(url)
        if let root = try? app.resolve(url) { show(root) }
    }

    /// Opens the archive containing `url`, chosen in the document picker or from recents.
    func open(_ url: URL) {
        _ = access(url)
        do {
            let root = try app.resolve(url)
            remember(root)
            show(root)
        } catch {
            if !FileManager.default.fileExists(atPath: url.path(percentEncoded: false)) {
                app.recents.remove(url)
            }
            problem = message(for: error)
        }
    }

    /// Creates an archive called `name` in `parent`, the app's own folder unless another place
    /// such as iCloud Drive is chosen, and opens it.
    func create(named name: String, in parent: URL = documents, embeddingModel: String? = nil) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        do {
            let root = try app.create(
                at: parent.appending(path: name, directoryHint: .isDirectory),
                embeddingModel: embeddingModel)
            remember(root)
            show(root)
        } catch {
            problem = message(for: error)
        }
    }

    /// Closes the archive, back to the list.
    func close() {
        open = nil
        defaults.removeObject(forKey: Self.openKey)
    }

    private func show(_ root: URL) {
        open = root
        defaults.set(root.path(percentEncoded: false), forKey: Self.openKey)
    }

    // MARK: Bookmarks

    /// Keeps a bookmark to `root` when it's outside the app's container, so it can be reached
    /// again after the app restarts.
    private func remember(_ root: URL) {
        guard !root.path(percentEncoded: false).hasPrefix(Self.documents.path(percentEncoded: false)),
            let data = try? root.bookmarkData()
        else { return }
        var bookmarks = defaults.dictionary(forKey: Self.bookmarksKey) ?? [:]
        bookmarks[root.path(percentEncoded: false)] = data
        defaults.set(bookmarks, forKey: Self.bookmarksKey)
    }

    /// Opens `url` for access, from its bookmark if it has one. Returns whether it could.
    @discardableResult
    func access(_ url: URL) -> Bool {
        Self.access(url, defaults: defaults)
    }

    /// Opens `url` for access, from its bookmark in `defaults` if it has one.
    @discardableResult
    nonisolated static func access(_ url: URL, defaults: UserDefaults) -> Bool {
        if url.startAccessingSecurityScopedResource() { return true }
        let bookmarks = defaults.dictionary(forKey: Self.bookmarksKey) ?? [:]
        guard let data = bookmarks[url.path(percentEncoded: false)] as? Data else { return false }
        var stale = false
        guard
            let resolved = try? URL(
                resolvingBookmarkData: data, bookmarkDataIsStale: &stale)
        else { return false }
        return resolved.startAccessingSecurityScopedResource()
    }
}

/// The app's one window on iPad and iPhone.
struct MobileRoot: View {
    @Environment(AppModel.self) private var app
    @Environment(\.scenePhase) private var scenePhase
    @State private var library: MobileLibrary?

    var body: some View {
        Group {
            if let library {
                if let root = library.open {
                    ArchiveWindow(url: root)
                        .id(root)
                } else {
                    MobileWelcomeView(library: library)
                }
            }
        }
        .environment(library)
        .fileChoosing()
        .task {
            let library = MobileLibrary(app: app)
            self.library = library
            library.reopen()
        }
        .onOpenURL { url in
            // A link from a notification or another app: open its archive here.
            app.open(link: url) { root in library?.open(root) }
        }
        .onChange(of: scenePhase) {
            if scenePhase == .background { BackgroundWork.schedule() }
        }
        .alert(
            "Carrel Can’t Open the Archive",
            isPresented: Binding(
                get: { app.openingProblem != nil }, set: { if !$0 { app.openingProblem = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(app.openingProblem ?? "")
        }
    }
}

/// The archives to choose from, when none is open.
struct MobileWelcomeView: View {
    @Bindable var library: MobileLibrary
    @State private var naming = false
    @State private var newParent: URL?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    VStack(spacing: 8) {
                        Image(systemName: "archivebox")
                            .font(.system(size: 56))
                            .foregroundStyle(.tint)
                        Text("Carrel").font(.largeTitle.bold())
                        Text("Your archive, filed and remembered.")
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical)
                }
                .listRowBackground(Color.clear)
                if !library.app.recents.urls.isEmpty {
                    Section("Recent") {
                        ForEach(library.app.recents.urls, id: \.self) { url in
                            Button {
                                library.open(url)
                            } label: {
                                Label {
                                    VStack(alignment: .leading) {
                                        Text(url.lastPathComponent)
                                        Text(place(of: url))
                                            .font(.caption).foregroundStyle(.secondary)
                                    }
                                } icon: {
                                    Image(systemName: "archivebox")
                                }
                            }
                        }
                        .onDelete { offsets in
                            for index in offsets { library.app.recents.remove(library.app.recents.urls[index]) }
                        }
                    }
                }
                Section {
                    Button("Open an Archive…", systemImage: "folder") {
                        pickFolder(
                            prompt: "Open",
                            message: "Choose an archive’s folder, or any folder inside it."
                        ) { library.open($0) }
                    }
                    Button("New Archive on This Device…", systemImage: "plus") {
                        newParent = nil
                        naming = true
                    }
                    Button("New Archive in Another Place…", systemImage: "folder.badge.plus") {
                        pickFolder(
                            prompt: "Create Here",
                            message: "Choose where the new archive’s folder goes, such as iCloud Drive."
                        ) { parent in
                            library.access(parent)
                            newParent = parent
                            naming = true
                        }
                    }
                } footer: {
                    Text(
                        "An archive is a folder of plain files. Keep it in iCloud Drive to use it on your Mac, iPad and iPhone."
                    )
                }
            }
            .navigationTitle("Archives")
            .navigationBarTitleDisplayMode(.inline)
            .sheet(isPresented: $naming) {
                NewArchiveSheet { name, model in
                    library.create(
                        named: name, in: newParent ?? MobileLibrary.documents, embeddingModel: model)
                }
            }
            .alert(
                "Carrel Can’t Open the Archive",
                isPresented: Binding(
                    get: { library.problem != nil }, set: { if !$0 { library.problem = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(library.problem ?? "")
            }
        }
    }

    /// Where an archive is, in words: on this device, or the folder it's in.
    private func place(of url: URL) -> String {
        let path = url.path(percentEncoded: false)
        if path.hasPrefix(MobileLibrary.documents.path(percentEncoded: false)) {
            return "On this device"
        }
        if path.contains("Mobile Documents") { return "iCloud Drive" }
        return url.deletingLastPathComponent().lastPathComponent
    }
}
/// New Archive: its name and embedding model, as `carrel init --embedding-model` takes it.
private struct NewArchiveSheet: View {
    let create: (String, String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name = "Archive"
    @State private var model = defaultEmbeddingModel()

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name)
                } footer: {
                    Text("The archive is a new folder with this name.")
                }
                Section {
                    Picker("Embedding model", selection: $model) {
                        ForEach(embeddingModelChoices(), id: \.self) { Text($0).tag($0) }
                    }
                } footer: {
                    Text("The model that finds items by meaning. It can be changed later.")
                }
            }
            .navigationTitle("New Archive")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", role: .cancel) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") {
                        dismiss()
                        create(name, model)
                    }
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
        .presentationDetents([.medium])
    }
}

/// The archive's toolbar on iPad and iPhone, in the sidebar: back to the archives, Settings,
/// and capture. It also adds what the share sheet left, whenever the app comes to the front.
private struct MobileArchiveToolbar: ViewModifier {
    let model: ArchiveWindowModel
    @Environment(MobileLibrary.self) private var library
    @Environment(AppModel.self) private var app
    @Environment(\.scenePhase) private var scenePhase
    @State private var capture: CaptureModel?

    func body(content: Content) -> some View {
        @Bindable var library = library
        content
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Archives", systemImage: "archivebox") { library.close() }
                }
                ToolbarItemGroup(placement: .topBarTrailing) {
                    if let capture, !model.isReadOnly { CaptureMenu(capture: capture) }
                    Button("Settings", systemImage: "gearshape") { library.showingSettings = true }
                }
            }
            .sheet(isPresented: $library.showingSettings) {
                SettingsView().environment(app)
            }
            .onChange(of: model.archive != nil, initial: true) {
                if let archive = model.archive, capture == nil {
                    capture = CaptureModel(archive: archive)
                }
            }
            .task(id: TaskID(phase: scenePhase, ready: capture != nil)) {
                if scenePhase == .active { await capture?.addShared() }
            }
    }

    private struct TaskID: Equatable {
        var phase: ScenePhase
        var ready: Bool
    }
}

extension View {
    func mobileArchiveToolbar(_ model: ArchiveWindowModel) -> some View {
        modifier(MobileArchiveToolbar(model: model))
    }
}
#endif
