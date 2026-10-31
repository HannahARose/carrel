// The catalog's embedding models through MLX, for search (`text/embed` in docs/providers.md).
// Each is used as the catalog says, with its prompts and pooling, so a query embedded here can
// be compared with an index built with llama.cpp on another device.

import CarrelAppleProviders
import Foundation
import MLX
import MLXEmbedders
import MLXLMCommon

/// A model that turns one text's tokens, `[1, length]`, into its vector, before it's scaled to
/// unit length.
protocol TextEncoder: BaseLanguageModel {
    func encode(_ tokens: MLXArray) -> MLXArray
}

/// Qwen3-Embedding, as MLXEmbedders has it: reading one way, pooled at the last token.
extension Qwen3Model: TextEncoder {
    func encode(_ tokens: MLXArray) -> MLXArray {
        let hidden = self(tokens, positionIds: nil, tokenTypeIds: nil, attentionMask: nil).hiddenStates!
        return hidden[0, hidden.dim(1) - 1, 0...]
    }
}

/// An embedding model and its tokenizer, loaded from its folder.
final class LoadedEncoder {
    let model: any TextEncoder
    let tokenizer: any MLXLMCommon.Tokenizer

    init(model: any TextEncoder, tokenizer: any MLXLMCommon.Tokenizer) {
        self.model = model
        self.tokenizer = tokenizer
    }

    /// The model in `folder`: EmbeddingGemma through `EmbeddingGemma.swift`, read in both
    /// directions, and Qwen3 through MLXEmbedders.
    static func load(_ folder: ModelFolder) async throws -> sending LoadedEncoder {
        let data = try Data(contentsOf: folder.url.appending(path: "config.json"))
        let base = try JSONDecoder().decode(BaseConfiguration.self, from: data)
        let model: any TextEncoder
        switch base.modelType {
        case "gemma3_text", "gemma3":
            model = EmbeddingGemma(try JSONDecoder().decode(Gemma3Configuration.self, from: data))
        case "qwen3":
            model = Qwen3Model(try JSONDecoder().decode(Qwen3Configuration.self, from: data))
        default:
            throw ProviderError.unsupported("\(folder.name) is a \(base.modelType) model, not one Carrel embeds with")
        }
        try await loadWeights(
            modelDirectory: folder.url, model: model, perLayerQuantization: base.perLayerQuantization)
        let tokenizer = try await LocalTokenizerLoader().load(from: folder.url)
        return LoadedEncoder(model: model, tokenizer: tokenizer)
    }

    /// One text's vector: its tokens, cut to the context, through the model on their own, with
    /// no padding, and scaled to unit length.
    func vector(of prompt: String, spec: EmbeddingSpec) -> [Float] {
        let tokens = Array(
            tokenizer.encode(text: prompt, addSpecialTokens: true).prefix(spec.contextTokens))
        guard !tokens.isEmpty else { return [Float](repeating: 0, count: spec.dimensions) }
        let vector = model.encode(MLXArray(tokens.map(Int32.init)).reshaped(1, -1)).asType(.float32)
        eval(vector)
        return unitLength(vector.asArray(Float.self))
    }
}

/// One MLX folder holding a catalog embedding model.
public struct MLXEmbedder: Embedder {
    public let folder: ModelFolder
    public let spec: EmbeddingSpec
    let memory: UInt64
    let loaded: Loaded

    public init(
        folder: ModelFolder, spec: EmbeddingSpec,
        memory: UInt64 = ProcessInfo.processInfo.physicalMemory, loaded: Loaded = .shared
    ) {
        self.folder = folder
        self.spec = spec
        self.memory = memory
        self.loaded = loaded
    }

    public var offer: Offer {
        var offer = Offer(
            capability: .embed, id: spec.id, label: spec.label, priority: mlxPriority,
            contextTokens: spec.contextTokens, dimensions: spec.dimensions)
        if let why = folder.tooBig(forMemory: memory) {
            offer.available = false
            offer.reason = why
        }
        return offer
    }

    public func embed(_ texts: [String], purpose: EmbeddingPurpose) async throws(ProviderError)
        -> Embeddings
    {
        let model = spec
        let prompts = texts.map { model.prompt($0, purpose: purpose) }
        let vectors: [[Float]]
        do {
            vectors = try await loaded.vectors(of: prompts, spec: model, folder: folder)
        } catch let error as ProviderError {
            throw error
        } catch {
            throw .unavailable("can't load \(folder.name): \(error)")
        }
        guard vectors.allSatisfy({ $0.count == model.dimensions }) else {
            throw .failed("\(folder.name) gave vectors of the wrong size")
        }
        return Embeddings(vectors: vectors, model: model.id, dimensions: model.dimensions)
    }
}

/// The MLX folders as the provider offers them: catalog embedding models as embedders, and
/// every other model as a text model.
public func offerFolders(_ folders: [ModelFolder]) -> (models: [MLXModel], embedders: [MLXEmbedder]) {
    var models: [MLXModel] = []
    var embedders: [MLXEmbedder] = []
    for folder in folders {
        if let spec = EmbeddingCatalog.find(folder.name) {
            embedders.append(MLXEmbedder(folder: folder, spec: spec))
        } else {
            models.append(MLXModel(folder: folder))
        }
    }
    return (models, embedders)
}
