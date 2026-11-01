// What the librarian's questions are about (#460): the chip in the Librarian panel
// ("About: Receipts 2023 ✕"), which follows the window's selection while the panel is open,
// the selected item, else the collection or Discover page shown, and is removed with ✕.
// Ask About This sets it, for an item or a collection, as `carrel ask --about` and the
// interactive mode's L do.

import CarrelKit
import SwiftUI

/// What the librarian's questions are about, with its name for the chip.
struct LibrarianSubject: Equatable {
    let about: LibrarianAbout
    let title: String

    init(item: ItemSummary) {
        about = .item(id: item.id)
        title = item.title
    }

    init(collection: CollectionSummary) {
        about = .collection(path: collection.path)
        title = collection.title
    }

    init(page: DiscoverPage) {
        about = .discover(view: page.title.lowercased())
        title = "Discover: \(page.title)"
    }
}

extension ArchiveWindowModel {
    /// What's in view, for the librarian: the selected item, else the collection Browse shows
    /// or the Discover page.
    var librarianSubject: LibrarianSubject? {
        if let item = selectedItem { return LibrarianSubject(item: item) }
        switch screen {
        case .collection(let path):
            return collections.first { $0.path == path }.map(LibrarianSubject.init(collection:))
        case .discover(let page):
            return LibrarianSubject(page: page)
        default:
            return nil
        }
    }
}

extension LibrarianModel {
    /// Follows the window's selection: what's newly in view becomes what the questions are
    /// about. Nothing in view leaves it as it was, so Ask About This from a list without a
    /// selection keeps its subject.
    func follow(_ subject: LibrarianSubject?) {
        if let subject { about = subject }
    }

    /// Opens the panel about `item`, for the owner to ask.
    func ask(about item: ItemSummary) {
        about = LibrarianSubject(item: item)
        show()
    }

    /// Opens the panel about `collection`, for the owner to ask.
    func ask(about collection: CollectionSummary) {
        about = LibrarianSubject(collection: collection)
        show()
    }
}

/// The chip saying what the questions are about, with ✕ to ask about the whole archive.
struct LibrarianAboutChip: View {
    @Bindable var librarian: LibrarianModel

    var body: some View {
        if let about = librarian.about {
            HStack(spacing: 4) {
                Text("About: \(about.title)").lineLimit(1).truncationMode(.middle)
                Button("Ask About the Whole Archive", systemImage: "xmark.circle.fill") {
                    librarian.about = nil
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .help("Ask about the whole archive instead")
            }
            .font(.callout)
            .padding(.horizontal, 8).padding(.vertical, 3)
            .background(.tint.opacity(0.15), in: .capsule)
            .help("“This” and “here” in your questions mean \(about.title)")
        }
    }
}

/// Opens the librarian about a collection.
struct AskAboutCollectionButton: View {
    let model: ArchiveWindowModel
    let collection: CollectionSummary

    var body: some View {
        Button("Ask About This", systemImage: "text.bubble") {
            model.librarian?.ask(about: collection)
        }
        .disabled(model.librarian == nil)
    }
}
