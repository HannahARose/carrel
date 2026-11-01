// Adding from the window itself (#439): the add sheet for files and folders dropped on the
// window or chosen with File → Add to Archive…, with the owner's note and, for folders, how they
// go in; File → Add Web Address… and Add Note…; and pasting any of them onto the window. Each
// goes through the core as the share sheet's do, and into this Mac's inbox while another device
// is the writer.

import AppKit
import CarrelKit
import SwiftUI

/// The add, web address and note sheets, and pasting onto the window.
struct AddFromWindowSheets: ViewModifier {
    let model: ArchiveWindowModel

    func body(content: Content) -> some View {
        content
            .sheet(
                item: Binding(
                    get: { model.intake?.adding },
                    set: { model.intake?.adding = $0 })
            ) { adding in
                if let intake = model.intake { AddFilesSheet(intake: intake, adding: adding) }
            }
            .sheet(
                item: Binding(
                    get: { model.intake?.writing },
                    set: { model.intake?.writing = $0 })
            ) { writing in
                if let intake = model.intake { WriteToAddSheet(intake: intake, writing: writing) }
            }
            .onPasteCommand(of: [.fileURL, .url, .plainText]) { _ in
                guard let intake = model.intake, !model.isReadOnly else { return }
                Task { await intake.paste(from: .general) }
            }
    }
}

/// What's being added, with a note for their cards and, for folders, how they go in.
struct AddFilesSheet: View {
    let intake: IntakeModel
    let adding: PendingAdd
    @State private var note = ""
    @State private var how = FolderIntake.decide
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        Form {
            Section {
                ForEach(adding.urls, id: \.self) { url in
                    Label(url.lastPathComponent, systemImage: Self.isFolder(url) ? "folder" : "doc")
                }
            } header: {
                Text(
                    adding.urls.count == 1
                        ? "Add to the Intake Queue"
                        : "Add \(adding.urls.count) Items to the Intake Queue"
                )
                .font(.headline)
            }
            if adding.hasFolders {
                Picker("Folders", selection: $how) {
                    Text("Let Carrel Decide").tag(FolderIntake.decide)
                    Text("Each Entry as Its Own Item").tag(FolderIntake.each)
                    Text("Each Folder as One Item").tag(FolderIntake.whole)
                }
                .help(
                    "Carrel takes a folder as one item unless its name suggests a pile, such as Downloads, whose entries go in one by one."
                )
            }
            TextField(
                "Note", text: $note,
                prompt: Text("Optional; given to the drafter, kept in the card"),
                axis: .vertical
            )
            .lineLimit(2...5)
        }
        .formStyle(.grouped)
        .frame(minWidth: 440)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("Add") {
                    let urls = adding.urls
                    let how = how
                    let note = note
                    Task { await intake.add(urls, as: how, note: note) }
                }
                .keyboardShortcut(.defaultAction)
            }
        }
    }

    private static func isFolder(_ url: URL) -> Bool {
        (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true
    }
}

/// A web address, or a note, written in the window to add to the intake queue.
struct WriteToAddSheet: View {
    let intake: IntakeModel
    let writing: Writing
    @State private var text = ""
    @State private var title = ""
    @State private var note = ""
    @Environment(\.dismiss) private var dismiss

    private var isWebAddress: Bool { writing.kind == .webAddress }

    private var canAdd: Bool {
        isWebAddress
            ? IntakeModel.isWebAddress(text.trimmingCharacters(in: .whitespacesAndNewlines))
            : !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        Form {
            Section {
                if isWebAddress {
                    TextField("Address", text: $text, prompt: Text("https://"))
                } else {
                    TextField("Note", text: $text, prompt: Text("What to keep"), axis: .vertical)
                        .lineLimit(4...12)
                }
                TextField(
                    "Title", text: $title,
                    prompt: Text(isWebAddress ? "Optional, such as the page’s" : "Optional"))
            } header: {
                Text(isWebAddress ? "Add a Web Address" : "Add a Note").font(.headline)
            } footer: {
                Text(
                    isWebAddress
                        ? "Filed as a web address, drafted from the address and title alone; nothing is fetched."
                        : "Filed as an item of its own, a Markdown note named by its title or first words."
                )
                .foregroundStyle(.secondary)
            }
            if isWebAddress {
                TextField(
                    "Your note", text: $note,
                    prompt: Text("Optional; given to the drafter, kept in the card"),
                    axis: .vertical
                )
                .lineLimit(2...5)
            }
        }
        .formStyle(.grouped)
        .frame(minWidth: 460)
        .onAppear { text = writing.text }
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("Add") {
                    let (text, title, note) = (text, title, note)
                    Task {
                        if isWebAddress {
                            await intake.addWebAddress(text, title: title, note: note)
                        } else {
                            await intake.addNote(text, title: title, note: note)
                        }
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!canAdd)
            }
        }
    }
}

/// File → Add Web Address…, Add Note… and Add from Clipboard, for the archive window in front.
struct AddFromWindowCommands: View {
    let window: ArchiveWindowModel?

    private var disabled: Bool { window?.intake == nil || window?.isReadOnly != false }

    var body: some View {
        Button("Add Web Address…") { window?.intake?.writing = Writing(kind: .webAddress) }
            .keyboardShortcut("u", modifiers: [.command, .shift])
            .disabled(disabled)
        Button("Add Note…") { window?.intake?.writing = Writing(kind: .note) }
            .keyboardShortcut("n", modifiers: [.command, .control])
            .disabled(disabled)
        Button("Add from Clipboard") {
            guard let intake = window?.intake else { return }
            Task { await intake.paste(from: .general) }
        }
        .keyboardShortcut("v", modifiers: [.command, .shift])
        .disabled(disabled)
    }
}
