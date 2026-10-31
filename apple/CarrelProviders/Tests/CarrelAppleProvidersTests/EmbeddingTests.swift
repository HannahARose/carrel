import Foundation
import Testing

@testable import CarrelAppleProviders

/// An embedder that counts each text's letters, for testing `text/embed` without a model.
struct LetterCounter: Embedder {
    var offer: Offer {
        Offer(capability: .embed, id: "letters", priority: 10, contextTokens: 64, dimensions: 2)
    }

    func embed(_ texts: [String], purpose: EmbeddingPurpose) async throws(ProviderError)
        -> Embeddings
    {
        let vectors = texts.map { text in
            let letters = Float(text.filter(\.isLetter).count)
            return unitLength([letters, purpose == .query ? 1 : 0])
        }
        return Embeddings(vectors: vectors, model: offer.id, dimensions: 2)
    }
}

let embedServer = Server(name: "Test", version: "0", embedders: [LetterCounter()])

func embedRequest(_ params: [String: Any]) -> [String: Any] {
    ["jsonrpc": "2.0", "id": 7, "method": "text/embed", "params": params]
}

@Test func embedOffersSayTheirDimensions() async throws {
    let reply = try await send(embedServer, ["jsonrpc": "2.0", "id": 1, "method": "initialize"])
    let result = try #require(reply["result"] as? [String: Any])
    let offers = try #require(result["offers"] as? [[String: Any]])
    #expect(offers.count == 1)
    #expect(offers[0]["capability"] as? String == "embed")
    #expect(offers[0]["id"] as? String == "letters")
    #expect(offers[0]["dimensions"] as? Int == 2)
    #expect(offers[0]["context_tokens"] as? Int == 64)
}

@Test func textEmbedAnswersOneVectorPerText() async throws {
    let reply = try await send(
        embedServer, embedRequest(["offer": "letters", "texts": ["abc", "", "a b"]]))
    let result = try #require(reply["result"] as? [String: Any])
    #expect(result["model"] as? String == "letters")
    #expect(result["dimensions"] as? Int == 2)
    let vectors = try #require(result["vectors"] as? [[Double]])
    #expect(vectors == [[1, 0], [0, 0], [1, 0]])

    let query = try await send(
        embedServer, embedRequest(["offer": "letters", "texts": ["a"], "purpose": "query"]))
    let queried = try #require((query["result"] as? [String: Any])?["vectors"] as? [[Double]])
    #expect(abs(queried[0][1] - 0.5.squareRoot()) < 1e-6)
}

@Test func textEmbedRefusesWhatItCantDo() async throws {
    let code = { (params: [String: Any]) async throws -> Int? in
        let reply = try await send(embedServer, embedRequest(params))
        return (reply["error"] as? [String: Any])?["code"] as? Int
    }
    #expect(try await code(["offer": "letters"]) == -32602)
    #expect(try await code(["offer": "letters", "texts": ["a"], "purpose": "summary"]) == -32602)
    #expect(try await code(["offer": "words", "texts": ["a"]]) == 2)
}

@Test func vectorsAreWrittenShort() async throws {
    struct Fixed: Embedder {
        var offer: Offer { Offer(capability: .embed, id: "fixed", dimensions: 3) }
        func embed(_ texts: [String], purpose: EmbeddingPurpose) async throws(ProviderError)
            -> Embeddings
        {
            Embeddings(vectors: [[0.0213, 1e-05, -0.5]], model: "fixed", dimensions: 3)
        }
    }
    let line = String(
        data: try JSONSerialization.data(
            withJSONObject: embedRequest(["offer": "fixed", "texts": ["a"]])),
        encoding: .utf8)!
    let reply = try #require(
        await Server(name: "T", version: "0", embedders: [Fixed()]).reply(to: line))
    #expect(reply.contains("[[0.0213,0.00001,-0.5]]"), "\(reply)")
}

struct SentenceEmbeddingTests {
    static let available = SentenceEmbedding().offer.available

    @Test func itsOfferNamesTheCatalogModel() {
        let offer = SentenceEmbedding().offer
        #expect(offer.capability == .embed)
        #expect(offer.id == "apple-nl-sentence-en")
        #expect(offer.dimensions == 512)
        #expect(offer.priority == Priority.native)
    }

    @Test func longTextsAreCutAtTheContext() {
        let long = (1...300).map { "word\($0)" }.joined(separator: " ")
        let cut = SentenceEmbedding.cut(long)
        #expect(cut.hasSuffix("word256"))
        #expect(SentenceEmbedding.cut("a short text.") == "a short text.")
    }

    @Test(.enabled(if: available, "the English sentence embedding isn't on this Mac"))
    func theNearerTextScoresHigher() async throws {
        let embeddings = try await SentenceEmbedding().embed(
            [
                "Tenancy agreement for the flat on Cowley Road", "Receipt for a desk chair", "",
                "!!",
            ],
            purpose: .document)
        #expect(embeddings.model == "apple-nl-sentence-en")
        #expect(embeddings.vectors.count == 4)
        for vector in embeddings.vectors {
            #expect(vector.count == 512)
        }
        let length = embeddings.vectors[0].reduce(0) { $0 + $1 * $1 }.squareRoot()
        #expect(abs(length - 1) < 1e-4)
        #expect(embeddings.vectors[2].allSatisfy { $0 == 0 })

        let query = try await SentenceEmbedding().embed(["the lease"], purpose: .query).vectors[0]
        let score = { (v: [Float]) in zip(v, query).reduce(0) { $0 + $1.0 * $1.1 } }
        #expect(score(embeddings.vectors[0]) > score(embeddings.vectors[1]))
    }
}
