// Swift conveniences over the generated bindings in Generated/carrel_ffi.swift.

import Foundation

extension ArchiveHandle {
    /// Opens the archive containing `url`.
    public static func open(_ url: URL) throws -> ArchiveHandle {
        try open(path: url.path(percentEncoded: false))
    }
}

extension ItemSummary: Identifiable {}

extension CollectionSummary: Identifiable {
    public var id: String { path }
}
