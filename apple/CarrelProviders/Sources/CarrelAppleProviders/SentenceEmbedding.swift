// Apple's sentence embedding from NaturalLanguage, for search. It comes with macOS, so nothing
// is downloaded, but only Apple devices have it: an archive also searched from Linux or
// Windows can't use it, so it's never the default, and an archive chooses it explicitly
// (docs/design/search.md).

import Foundation
import NaturalLanguage

/// NaturalLanguage's English sentence embedding, pinned to one revision: a later macOS may
/// bring another, whose vectors couldn't be compared with an index built with this one. That
/// one would be a model of its own, with an identifier of its own.
public struct SentenceEmbedding: Embedder {
    /// The model's identifier, as in Carrel's catalog (`crates/carrel-core/src/models.rs`).
    public static let id = "apple-nl-sentence-en"
    static let language = NLLanguage.english
    static let revision = 1
    static let dimensions = 512
    /// The longest text it reads, in words: its embedding has no limit, but it averages over
    /// the text, so it's meant for passages, and Carrel splits documents to fit this.
    static let contextWords = 256

    public init() {}

    public var offer: Offer {
        var offer = Offer(
            capability: .embed, id: Self.id, label: "Apple sentence embedding (English)",
            contextTokens: Self.contextWords, dimensions: Self.dimensions)
        if let why = Self.unavailable(Self.load()) {
            offer.available = false
            offer.reason = why
        }
        return offer
    }

    /// Why `embedding` can't be used, or nil if it can.
    static func unavailable(_ embedding: NLEmbedding?) -> String? {
        guard let embedding else {
            return "macOS's English sentence embedding (revision \(revision)) isn't on this Mac"
        }
        guard embedding.dimension == dimensions else {
            return
                "macOS's English sentence embedding has \(embedding.dimension) dimensions, not \(dimensions)"
        }
        return nil
    }

    static func load() -> NLEmbedding? {
        NLEmbedding.sentenceEmbedding(for: language, revision: revision)
    }

    public func embed(_ texts: [String], purpose: EmbeddingPurpose) async throws(ProviderError)
        -> Embeddings
    {
        // It has no prompts: queries and documents are embedded alike.
        let embedding = Self.load()
        if let why = Self.unavailable(embedding) {
            throw .unavailable(why)
        }
        guard let embedding else { throw .unavailable("no sentence embedding") }
        let vectors = texts.map { text in
            // A text with nothing to embed, such as punctuation alone, gets no vector; it's
            // given zeros, as the llama.cpp provider gives a text that embeds to zeros.
            let vector = embedding.vector(for: Self.cut(text)) ?? []
            guard vector.count == Self.dimensions else {
                return [Float](repeating: 0, count: Self.dimensions)
            }
            return unitLength(vector.map(Float.init))
        }
        return Embeddings(vectors: vectors, model: Self.id, dimensions: Self.dimensions)
    }

    /// `text` up to the end of its `contextWords`th word.
    static func cut(_ text: String) -> String {
        let tokenizer = NLTokenizer(unit: .word)
        tokenizer.string = text
        var words = 0
        var end = text.endIndex
        tokenizer.enumerateTokens(in: text.startIndex..<text.endIndex) { range, _ in
            words += 1
            guard words == contextWords else { return true }
            end = range.upperBound
            return false
        }
        return String(text[..<end])
    }
}
