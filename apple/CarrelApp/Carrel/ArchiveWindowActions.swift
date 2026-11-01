// What an archive's window does beyond its screens: search in the toolbar (⌘F), adding items
// by dropping them on the window or with File → Add to Archive… (⌘O), and watching the
// worker draft them.

import AppKit
import CarrelKit
import SwiftUI

/// Search in the toolbar, adding items by dropping them, and the window's commands.
struct ArchiveWindowActions: ViewModifier {
    let model: ArchiveWindowModel
    @State private var isTargeted = false
    @FocusState private var searchFocused: Bool

    func body(content: Content) -> some View {
        content
            .searchable(
                text: Binding(
                    get: { model.search?.text ?? "" },
                    set: { model.search?.text = $0 }),
                placement: .toolbar,
                prompt: "Search, or tag:receipts"
            )
            .searchFocused($searchFocused)
            .onChange(of: model.search?.focusRequests) { searchFocused = true }
            .onChange(of: model.screen) { model.search?.clear() }
            .dropDestination(for: URL.self) { urls, _ in
                guard let intake = model.intake, !model.isReadOnly else { return false }
                Task { await intake.add(urls) }
                return true
            } isTargeted: {
                isTargeted = $0
            }
            .overlay {
                if isTargeted, !model.isReadOnly {
                    RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(.tint, lineWidth: 3)
                        .overlay {
                            Label("Add to the Intake Queue", systemImage: "tray.and.arrow.down")
                                .font(.title2)
                                .padding()
                                .background(.regularMaterial, in: .rect(cornerRadius: 8))
                        }
                        .allowsHitTesting(false)
                }
            }
            .alert(
                model.intake?.report?.title ?? "",
                isPresented: Binding(
                    get: { model.intake?.report != nil },
                    set: { if !$0 { model.intake?.report = nil } }),
                presenting: model.intake?.report
            ) { _ in
            } message: { report in
                if let details = report.details { Text(details) }
            }
            .confirmationDialog(
                "How should the folders go in?",
                isPresented: Binding(
                    get: { model.intake?.choosingHow != nil },
                    set: { if !$0 { model.intake?.choosingHow = nil } }),
                presenting: model.intake?.choosingHow
            ) { urls in
                Button("Let Carrel Decide") { Task { await model.intake?.add(urls, as: .decide) } }
                Button("Each Entry as Its Own Item") {
                    Task { await model.intake?.add(urls, as: .each) }
                }
                Button("Each Folder as One Item") {
                    Task { await model.intake?.add(urls, as: .whole) }
                }
            } message: { _ in
                Text(
                    "Carrel takes a folder as one item unless its name suggests a pile, such as Downloads, whose entries go in one by one."
                )
            }
            .task(id: model.intake.map(ObjectIdentifier.init)) {
                await model.intake?.watch()
            }
    }
}

/// The commands for the archive window in front: Add to Archive… and Search.
struct ArchiveWindowCommands: Commands {
    @FocusedValue(ArchiveWindowModel.self) private var window

    var body: some Commands {
        CommandGroup(after: .newItem) {
            Divider()
            Button("Add to Archive…") {
                guard let intake = window?.intake else { return }
                let panel = NSOpenPanel()
                panel.canChooseFiles = true
                panel.canChooseDirectories = true
                panel.allowsMultipleSelection = true
                panel.message = "Choose files and folders to add to the intake queue."
                panel.prompt = "Add"
                guard panel.runModal() == .OK else { return }
                let urls = panel.urls
                Task { await intake.add(urls) }
            }
            .keyboardShortcut("o")
            .disabled(window?.intake == nil || window?.isReadOnly != false)
        }
        CommandGroup(before: .textEditing) {
            Button("Search Archive") { window?.search?.focus() }
                .keyboardShortcut("f")
                .disabled(window?.search == nil)
        }
    }
}
