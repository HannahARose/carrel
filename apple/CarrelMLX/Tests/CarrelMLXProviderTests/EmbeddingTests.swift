// Embedding through MLX. Most of these run a tiny EmbeddingGemma with random weights, so they
// need no download; the last runs a real one when `CARREL_TEST_MLX_MODELS` names a models
// folder with `mlx/embeddinggemma-300m` in it, as the on-device runner's may.

import CarrelAppleProviders
import Foundation
import MLX
import MLXEmbedders
import MLXLMCommon
import Testing

@testable import CarrelMLXProvider

struct EmbeddingTests {
    /// A Gemma 3 small enough to run with random weights: two sliding layers, then a global one.
    static func tinyGemma(slidingWindow: Int = 4) throws -> EmbeddingGemma {
        let json = """
            {"model_type": "gemma3_text", "hidden_size": 32, "num_hidden_layers": 3,
             "intermediate_size": 64, "num_attention_heads": 2, "head_dim": 16,
             "num_key_value_heads": 1, "vocab_size": 50, "query_pre_attn_scalar": 16,
             "sliding_window": \(slidingWindow), "sliding_window_pattern": 3}
            """
        let config = try JSONDecoder().decode(Gemma3Configuration.self, from: Data(json.utf8))
        let model = EmbeddingGemma(config)
        eval(model)
        return model
    }

    static func run(_ model: EmbeddingGemma, _ tokens: [Int32]) -> EmbeddingModelOutput {
        model(MLXArray(tokens).reshaped(1, -1), positionIds: nil, tokenTypeIds: nil, attentionMask: nil)
    }

    @Test func slidingLayersSeeHalfTheWindowEitherSide() throws {
        guard case .none = EmbeddingGemma.slidingMask(length: 3, window: 4) else {
            Issue.record("a text within the window needs no mask")
            return
        }
        guard case .array(let mask) = EmbeddingGemma.slidingMask(length: 6, window: 4) else {
            Issue.record("a text longer than the window needs a mask")
            return
        }
        let seen = mask.asArray(Bool.self)
        for i in 0..<6 {
            for j in 0..<6 {
                #expect(seen[i * 6 + j] == (abs(i - j) <= 2), "\(i) sees \(j)")
            }
        }
    }

    @Test func everyTokenSeesTheTokensAfterIt() throws {
        let model = try Self.tinyGemma()
        let a = Self.run(model, [2, 7, 9, 11, 3]).hiddenStates!
        let b = Self.run(model, [2, 7, 9, 11, 4]).hiddenStates!
        // Read in one direction, as Gemma generates text, the first token's state couldn't
        // change with the last token.
        #expect(!allClose(a[0..., 0, 0...], b[0..., 0, 0...]).item(Bool.self))
    }

    @Test func theTextsVectorIsOneOfUnitLength() throws {
        let model = try Self.tinyGemma()
        let pooled = try #require(Self.run(model, [2, 7, 9, 11, 3, 5, 8]).pooledOutput)
        #expect(pooled.shape == [1, 32])
        let length = (pooled * pooled).sum().sqrt().item(Float.self)
        #expect(abs(length - 1) < 1e-4)
    }

    @Test func catalogEmbeddingFoldersAreOfferedForEmbeddingOnly() {
        let root = URL(filePath: "/nonexistent")
        let folders = ["embeddinggemma-300m", "qwen3.5-4b", "qwen3-embedding-0.6b"].map {
            ModelFolder(name: $0, url: root.appending(path: $0), size: 1)
        }
        let offered = offerFolders(folders)
        #expect(offered.models.map(\.folder.name) == ["qwen3.5-4b"])
        #expect(offered.embedders.map(\.offer.id) == ["embeddinggemma-300m", "qwen3-embedding-0.6b"])
        let gemma = offered.embedders[0].offer
        #expect(gemma.capability == .embed)
        #expect(gemma.dimensions == 768)
        #expect(gemma.contextTokens == 2048)
        #expect(gemma.priority == mlxPriority)
    }

    @Test func promptsSayWhatTheTextIsFor() throws {
        let gemma = try #require(EmbeddingCatalog.find("embeddinggemma-300m"))
        #expect(gemma.prompt("the lease", purpose: .query) == "task: search result | query: the lease")
        #expect(gemma.prompt("A lease.", purpose: .document) == "title: none | text: A lease.")
        let qwen = try #require(EmbeddingCatalog.find("qwen3-embedding-0.6b"))
        #expect(qwen.prompt("A lease.", purpose: .document) == "A lease.")
        #expect(qwen.pooling == .last)
    }

    static let gemmaFolder: ModelFolder? = ProcessInfo.processInfo.environment["CARREL_TEST_MLX_MODELS"]
        .flatMap {
            offerFolders(ModelFolder.discover(in: URL(filePath: $0, directoryHint: .isDirectory)))
                .embedders.first?.folder
        }

    @Test(.enabled(if: gemmaFolder != nil, "no MLX embedding model in CARREL_TEST_MLX_MODELS"))
    func anEmbeddingModelFindsTheNearerText() async throws {
        let folder = try #require(Self.gemmaFolder)
        let embedder = MLXEmbedder(folder: folder, spec: try #require(EmbeddingCatalog.find(folder.name)))
        let documents = try await embedder.embed(
            ["Tenancy agreement for the flat on Cowley Road", "Receipt for a desk chair"],
            purpose: .document)
        let query = try await embedder.embed(["the lease"], purpose: .query).vectors[0]
        #expect(documents.vectors.allSatisfy { $0.count == embedder.spec.dimensions })
        let score = { (v: [Float]) in zip(v, query).reduce(0) { $0 + $1.0 * $1.1 } }
        #expect(score(documents.vectors[0]) > score(documents.vectors[1]))
        await Loaded.shared.unload()
    }
}
