// Swift conveniences over the generated bindings in Generated/carrel_ffi.swift.

import Foundation

extension ArchiveHandle {
    /// Opens the archive containing `url`.
    public static func open(_ url: URL) throws -> ArchiveHandle {
        try open(path: url.path(percentEncoded: false))
    }

    /// Opens the archive containing `url` as `options` say.
    public static func open(_ url: URL, options: OpenOptions) throws -> ArchiveHandle {
        try openWith(path: url.path(percentEncoded: false), options: options)
    }
}

extension OpenOptions {
    /// Keeps this device's identity and preferences in `config` rather than the device's own
    /// folder, and takes `home` (`home` in `config` unless given) for the person's home, so
    /// Status looks for active work there and checkouts go to its Desktop. Starts provider
    /// programs only when `startProviders` says so. Tests use it so they never write the real
    /// configuration or look in the owner's folders.
    public static func isolated(config: URL, home: URL? = nil, startProviders: Bool = false)
        -> OpenOptions
    {
        OpenOptions(
            configDir: config.path(percentEncoded: false),
            providerFolders: [],
            startProviders: startProviders,
            home: (home ?? config.appending(path: "home")).path(percentEncoded: false)
        )
    }

    /// The app's: provider programs from its bundle's Helpers folder first.
    public static func app(helpers: URL?) -> OpenOptions {
        OpenOptions(
            configDir: nil,
            providerFolders: helpers.map { [$0.path(percentEncoded: false)] } ?? [],
            startProviders: true,
            home: nil
        )
    }
}

extension CarrelError {
    /// The core's message, for people.
    public var message: String {
        switch self {
        case .NotAnArchive(let message), .Failed(let message), .Callback(let message),
            .Busy(let message), .Invalid(let message):
            message
        }
    }
}

extension ItemSummary: Identifiable {}

extension CollectionSummary: Identifiable {
    public var id: String { path }
}

extension Proposal: Identifiable {}
extension ReviewBatch: Identifiable {}
extension Question: Identifiable {}
extension ActiveItem: Identifiable {}
extension Change: Identifiable {}
extension Queued: Identifiable {}

extension Setting: Identifiable {
    public var id: String { key }
}

extension CatalogModel: Identifiable {
    public var id: String { name }
}

extension ProviderInfo: Identifiable {
    public var id: String { name }
}

extension SearchHit: Identifiable {
    /// An item's identifier, a collection's folder, or a glossary term.
    public var id: String { itemId ?? "\(found):\(path):\(title)" }
}
