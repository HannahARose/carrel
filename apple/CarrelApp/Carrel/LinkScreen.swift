// The screen a `carrel://open?screen=` link asks for, from `carrel gui SCREEN`, shown in the
// archive's window by the names the interactive mode uses.

import CarrelKit

extension ArchiveWindowModel {
    /// Shows the screen a link asked for. The window has no Ingest or Librarian screen:
    /// Ingest is Status, where the intake queue is, and the librarian is its panel. Settings
    /// is its own window, which the archive's window opens.
    func show(linked screen: AgentLink.Screen) {
        switch screen {
        case .status, .ingest: self.screen = .status
        case .review: self.screen = .review
        case .active: self.screen = .active
        case .questions: self.screen = .questions
        case .history: self.screen = .history
        case .backups: self.screen = .backups
        case .browse:
            if let first = tree.first { self.screen = .collection(first.collection.path) }
        case .discover(let view):
            self.screen = .discover(view.flatMap { DiscoverPage(rawValue: $0.rawValue) } ?? .today)
        case .librarian: librarian?.show()
        case .settings: break
        }
    }
}
