// The catalog's embedding models, as the MLX provider runs them. Every provider that runs one
// must use it as Carrel's catalog says (`crates/carrel-core/src/models.rs`), so its vectors can
// be compared with an index another device built with llama.cpp. A test in carrel-core checks
// this table against the catalog.

import CarrelAppleProviders

/// How an embedding model's tokens' vectors become one vector.
public enum EmbeddingPooling: Sendable {
    /// The mean of every token's vector, then the model's own dense layers.
    case mean
    /// The last token's vector.
    case last
}

/// One of the catalog's embedding models.
public struct EmbeddingSpec: Sendable, Equatable {
    /// Its identifier, also the name of its folder in `mlx/`.
    public var id: String
    public var label: String
    public var dimensions: Int
    /// The longest text it reads, in tokens; longer texts are cut.
    public var contextTokens: Int
    public var pooling: EmbeddingPooling
    /// What goes before a search query, and before a text to be found, as it was trained.
    public var queryPrefix: String
    public var documentPrefix: String

    /// `text` with the prefix for `purpose`.
    func prompt(_ text: String, purpose: EmbeddingPurpose) -> String {
        (purpose == .query ? queryPrefix : documentPrefix) + text
    }
}

public enum EmbeddingCatalog {
    public static let models: [EmbeddingSpec] = [
        EmbeddingSpec(
            id: "embeddinggemma-300m", label: "EmbeddingGemma 300M (MLX)", dimensions: 768,
            contextTokens: 2048, pooling: .mean,
            queryPrefix: "task: search result | query: ", documentPrefix: "title: none | text: "),
        EmbeddingSpec(
            id: "qwen3-embedding-0.6b", label: "Qwen3-Embedding 0.6B (MLX)", dimensions: 1024,
            contextTokens: 8192, pooling: .last,
            queryPrefix:
                "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:",
            documentPrefix: ""),
    ]

    /// The embedding model whose folder is called `name`, if it's one.
    public static func find(_ name: String) -> EmbeddingSpec? {
        models.first { $0.id == name }
    }
}
