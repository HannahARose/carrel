// Moving an archive into iCloud Drive (docs/design/sync-and-mobile.md, "What the owner needs
// to do"), as `carrel sync move` does: File → Move to iCloud Drive… shows where it goes (a
// folder of its name in iCloud Drive unless another is chosen), how many files and how much it
// moves, and the space free there; then copies it holding the writer lease, checks the copy,
// and reopens the window on the new copy. The old copy stays until the owner removes it.

import AppKit
import CarrelKit
import SwiftUI

/// The move's sheet: where to, the plan, and the move itself.
@Observable
final class ICloudMoveModel: Identifiable {
    let id = UUID()
    let archive: ArchiveModel
    /// The archive's folder now.
    let from: URL
    /// Where it goes: a folder that doesn't exist yet.
    private(set) var destination: URL?
    /// What the move would do, once checked.
    private(set) var plan: ArchiveMove?
    /// Why it can't move, or why the move failed.
    private(set) var problem: String?
    /// Whether the move is under way.
    private(set) var isMoving = false
    /// The new root, once moved.
    private(set) var moved: URL?

    init(archive: ArchiveModel, from: URL) {
        self.archive = archive
        self.from = from
        destination = archive.defaultICloudDestination
        if destination == nil {
            problem = "iCloud Drive isn’t turned on for this Mac. Choose another folder."
        }
    }

    /// How far the copy has got.
    var progress: OperationProgress? { isMoving ? archive.progress : nil }

    /// Whether the move can start.
    var canMove: Bool { plan != nil && !isMoving && moved == nil }

    /// Moves it into the folder `parent` chosen in the folder panel, under its own name.
    func choose(parent: URL) async {
        destination = parent.appending(path: from.lastPathComponent, directoryHint: .isDirectory)
        await check()
    }

    /// Checks the destination: nothing is there, and there's room.
    func check() async {
        plan = nil
        guard let destination else { return }
        do {
            plan = try await archive.planICloudMove(to: destination)
            problem = nil
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
    }

    /// Copies the archive to the destination and makes the copy this Mac's archive.
    func move() async {
        guard canMove, let destination else { return }
        isMoving = true
        defer { isMoving = false }
        do {
            moved = try await archive.moveIntoICloud(to: destination)
            problem = nil
        } catch {
            archive.lastError = nil
            problem = message(for: error)
        }
    }
}

extension ArchiveWindowModel {
    /// Opens Move to iCloud Drive…'s sheet and checks the default destination.
    func startICloudMove() {
        guard let archive, !isReadOnly else { return }
        let move = ICloudMoveModel(archive: archive, from: root)
        icloudMove = move
        Task { await move.check() }
    }
}

/// File → Move to iCloud Drive…, for the key archive window.
struct ICloudMoveCommands: Commands {
    @FocusedValue(ArchiveWindowModel.self) private var window

    var body: some Commands {
        CommandGroup(after: .importExport) {
            Button("Move to iCloud Drive…") { window?.startICloudMove() }
                .disabled(window?.archive == nil || window?.isReadOnly != false)
        }
    }
}

extension View {
    /// Move to iCloud Drive…'s sheet, for `window`.
    func icloudMoveSheet(_ window: ArchiveWindowModel) -> some View {
        sheet(
            item: Binding(get: { window.icloudMove }, set: { window.icloudMove = $0 })
        ) { move in
            ICloudMoveSheet(window: window, move: move)
        }
    }
}

struct ICloudMoveSheet: View {
    let window: ArchiveWindowModel
    let move: ICloudMoveModel

    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Move to iCloud Drive").font(.headline)
            Text(
                "Carrel copies the archive, checks the copy, then uses the copy from now on. The old copy stays where it is until you remove it."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            Form {
                LabeledContent("From") { path(move.from) }
                LabeledContent("To") {
                    HStack {
                        if let destination = move.destination { path(destination) }
                        Button("Choose…") { choose() }
                            .disabled(move.isMoving || move.moved != nil)
                    }
                }
                if let plan = move.plan {
                    LabeledContent("Files", value: plan.files, format: .number)
                    LabeledContent(
                        "Size", value: Int64(clamping: plan.bytes), format: .byteCount(style: .file)
                    )
                    if let free = plan.free {
                        LabeledContent(
                            "Free There", value: Int64(clamping: free),
                            format: .byteCount(style: .file))
                    }
                    if !plan.synced {
                        Label(
                            "This folder isn’t in iCloud Drive or another synced folder.",
                            systemImage: "exclamationmark.triangle"
                        )
                        .foregroundStyle(.orange)
                    }
                }
            }
            .formStyle(.columns)
            if let progress = move.progress {
                ProgressView(
                    value: progress.fraction ?? 0,
                    label: { Text(progress.title) },
                    currentValueLabel: {
                        Text(
                            "\(Int64(clamping: progress.done).formatted(.byteCount(style: .file))) of \(Int64(clamping: progress.total).formatted(.byteCount(style: .file)))"
                        )
                    })
            } else if move.isMoving {
                ProgressView("Copying the archive").progressViewStyle(.linear)
            }
            if let moved = move.moved {
                Label(
                    "Moved to \(moved.path(percentEncoded: false)). The old copy is kept at \(move.from.path(percentEncoded: false)); remove it once you’re sure.",
                    systemImage: "checkmark.circle"
                )
                .fixedSize(horizontal: false, vertical: true)
            }
            if let problem = move.problem {
                Label(problem, systemImage: "exclamationmark.triangle").foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                if let moved = move.moved {
                    Button("Open the Moved Archive") { reopen(at: moved) }
                        .keyboardShortcut(.defaultAction)
                } else {
                    Button("Cancel", role: .cancel) { dismiss() }
                        .disabled(move.isMoving)
                    Button("Move") { Task { await move.move() } }
                        .keyboardShortcut(.defaultAction)
                        .disabled(!move.canMove)
                }
            }
        }
        .padding(20)
        .frame(width: 520)
        .interactiveDismissDisabled(move.isMoving)
    }

    private func path(_ url: URL) -> some View {
        Text(url.path(percentEncoded: false))
            .lineLimit(1).truncationMode(.middle)
            .textSelection(.enabled)
            .help(url.path(percentEncoded: false))
    }

    /// Chooses the folder to move the archive into.
    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.directoryURL = move.destination?.deletingLastPathComponent()
        panel.message = "Choose the folder to move “\(move.from.lastPathComponent)” into."
        panel.prompt = "Choose"
        guard panel.runModal() == .OK, let parent = panel.url else { return }
        Task { await move.choose(parent: parent) }
    }

    /// Opens the moved archive in a window of its own, in place of this one.
    private func reopen(at root: URL) {
        app.recents.remove(move.from)
        let opened = (try? app.resolve(root)) ?? root
        dismiss()
        openWindow(id: ArchiveWindow.windowID, value: opened)
        dismissWindow(id: ArchiveWindow.windowID, value: window.url)
    }
}
