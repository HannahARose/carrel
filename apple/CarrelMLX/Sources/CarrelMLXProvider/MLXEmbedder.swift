// The catalog's embedding models through MLX, for search (`text/embed` in docs/providers.md).
// Each is used as the catalog says, with its prompts and pooling, so a query embedded here can
// be compared with an index built with llama.cpp on another device.

import CarrelAppleProviders
import Foundation
import MLX
import MLXEmbedders
import MLXLMCommon

/// Loads embedding models: MLXEmbedders' own for Qwen3, and EmbeddingGemma read in both
/// directions (`EmbeddingGemma.swift`) in place of its causal port.
let embedderFactory = EmbedderModelFactory(
    typeRegistry: ModelTypeRegistry<any EmbeddingModel>(creators: [
        "qwen3": { Qwen3Model(try JSONDecoder().decode(Qwen3Configuration.self, from: $0)) },
        "gemma3_text": { EmbeddingGemma(try JSONDecoder().decode(Gemma3Configuration.self, from: $0)) },
        "gemma3": { EmbeddingGemma(try JSONDecoder().decode(Gemma3Configuration.self, from: $0)) },
    ]),
    modelRegistry: EmbedderRegistry.shared)

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
        let container: EmbedderModelContainer
        do {
            container = try await loaded.embedder(for: folder, using: embedderFactory)
        } catch {
            throw .unavailable("can't load \(folder.name): \(error)")
        }
        let model = spec
        let prompts = texts.map { model.prompt($0, purpose: purpose) }
        let vectors = await container.perform { context in
            prompts.map { Self.vector(of: $0, spec: model, context: context) }
        }
        await loaded.touch()
        guard vectors.allSatisfy({ $0.count == spec.dimensions }) else {
            throw .failed("\(folder.name) gave vectors of the wrong size")
        }
        return Embeddings(vectors: vectors, model: spec.id, dimensions: spec.dimensions)
    }

    /// One text's vector: its tokens, cut to the context, through the model on their own, with
    /// no padding; then pooled as the catalog says and scaled to unit length.
    static func vector(of prompt: String, spec: EmbeddingSpec, context: EmbedderModelContext)
        -> [Float]
    {
        let tokens = Array(
            context.tokenizer.encode(text: prompt, addSpecialTokens: true).prefix(spec.contextTokens))
        guard !tokens.isEmpty else { return [Float](repeating: 0, count: spec.dimensions) }
        let output = context.model(
            MLXArray(tokens.map(Int32.init)).reshaped(1, -1), positionIds: nil, tokenTypeIds: nil,
            attentionMask: nil)
        let pooled: MLXArray
        switch spec.pooling {
        case .mean:
            // EmbeddingGemma pools itself, through its dense layers.
            pooled = output.pooledOutput ?? output.hiddenStates!.mean(axis: 1)
        case .last:
            let hidden = output.hiddenStates!
            pooled = hidden[0..., hidden.dim(1) - 1, 0...]
        }
        let vector = pooled.asType(.float32).reshaped(-1)
        eval(vector)
        return unitLength(vector.asArray(Float.self))
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
