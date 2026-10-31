// EmbeddingGemma read in both directions, as it was trained and as llama.cpp runs it. MLXEmbedders'
// own port reads it with a causal mask, as Gemma 3 generates text, and its vectors would then
// differ from those of an index built with llama.cpp. Every layer here attends both ways: the
// global layers to the whole text, the sliding ones to the tokens within half the window on
// either side (Hugging Face's `use_bidirectional_attention`, llama.cpp's symmetric window).
// The text's vector is the mean of its tokens', through the model's two dense layers, scaled
// to unit length.

import Foundation
import MLX
import MLXEmbedders
import MLXLMCommon
import MLXNN

final class GemmaEncoderAttention: Module {
    let heads: Int
    let kvHeads: Int
    let scale: Float

    @ModuleInfo(key: "q_proj") var queryProj: Linear
    @ModuleInfo(key: "k_proj") var keyProj: Linear
    @ModuleInfo(key: "v_proj") var valueProj: Linear
    @ModuleInfo(key: "o_proj") var outputProj: Linear
    @ModuleInfo(key: "q_norm") var queryNorm: Gemma.RMSNorm
    @ModuleInfo(key: "k_norm") var keyNorm: Gemma.RMSNorm
    @ModuleInfo var rope: RoPE

    init(_ config: Gemma3Configuration, sliding: Bool) {
        heads = config.attentionHeads
        kvHeads = config.kvHeads
        scale = pow(config.queryPreAttnScalar, -0.5)
        let (dim, headDim) = (config.hiddenSize, config.headDim)
        _queryProj.wrappedValue = Linear(dim, heads * headDim, bias: false)
        _keyProj.wrappedValue = Linear(dim, kvHeads * headDim, bias: false)
        _valueProj.wrappedValue = Linear(dim, kvHeads * headDim, bias: false)
        _outputProj.wrappedValue = Linear(heads * headDim, dim, bias: false)
        _queryNorm.wrappedValue = Gemma.RMSNorm(dimensions: headDim, eps: config.rmsNormEps)
        _keyNorm.wrappedValue = Gemma.RMSNorm(dimensions: headDim, eps: config.rmsNormEps)
        rope = RoPE(
            dimensions: headDim, traditional: false,
            base: sliding ? config.ropeLocalBaseFreq : config.ropeTheta)
        super.init()
    }

    func callAsFunction(_ x: MLXArray, mask: MLXFast.ScaledDotProductAttentionMaskMode) -> MLXArray {
        let (b, l) = (x.dim(0), x.dim(1))
        let queries = rope(queryNorm(queryProj(x).reshaped(b, l, heads, -1).transposed(0, 2, 1, 3)))
        let keys = rope(keyNorm(keyProj(x).reshaped(b, l, kvHeads, -1).transposed(0, 2, 1, 3)))
        let values = valueProj(x).reshaped(b, l, kvHeads, -1).transposed(0, 2, 1, 3)
        let output = MLXFast.scaledDotProductAttention(
            queries: queries, keys: keys, values: values, scale: scale, mask: mask)
        return outputProj(output.transposed(0, 2, 1, 3).reshaped(b, l, -1))
    }
}

final class GemmaEncoderMLP: Module {
    @ModuleInfo(key: "gate_proj") var gateProj: Linear
    @ModuleInfo(key: "down_proj") var downProj: Linear
    @ModuleInfo(key: "up_proj") var upProj: Linear

    init(dimensions: Int, hiddenDimensions: Int) {
        _gateProj.wrappedValue = Linear(dimensions, hiddenDimensions, bias: false)
        _downProj.wrappedValue = Linear(hiddenDimensions, dimensions, bias: false)
        _upProj.wrappedValue = Linear(dimensions, hiddenDimensions, bias: false)
        super.init()
    }

    func callAsFunction(_ x: MLXArray) -> MLXArray {
        downProj(geluApproximate(gateProj(x)) * upProj(x))
    }
}

final class GemmaEncoderLayer: Module {
    let sliding: Bool

    @ModuleInfo(key: "self_attn") var attention: GemmaEncoderAttention
    @ModuleInfo var mlp: GemmaEncoderMLP
    @ModuleInfo(key: "input_layernorm") var inputNorm: Gemma.RMSNorm
    @ModuleInfo(key: "post_attention_layernorm") var postAttentionNorm: Gemma.RMSNorm
    @ModuleInfo(key: "pre_feedforward_layernorm") var preFeedforwardNorm: Gemma.RMSNorm
    @ModuleInfo(key: "post_feedforward_layernorm") var postFeedforwardNorm: Gemma.RMSNorm

    init(_ config: Gemma3Configuration, index: Int) {
        // Every `slidingWindowPattern`th layer is global; the others slide.
        sliding = (index + 1) % config.slidingWindowPattern != 0
        _attention.wrappedValue = GemmaEncoderAttention(config, sliding: sliding)
        mlp = GemmaEncoderMLP(dimensions: config.hiddenSize, hiddenDimensions: config.intermediateSize)
        let norm = { Gemma.RMSNorm(dimensions: config.hiddenSize, eps: config.rmsNormEps) }
        _inputNorm.wrappedValue = norm()
        _postAttentionNorm.wrappedValue = norm()
        _preFeedforwardNorm.wrappedValue = norm()
        _postFeedforwardNorm.wrappedValue = norm()
        super.init()
    }

    func callAsFunction(_ x: MLXArray, mask: MLXFast.ScaledDotProductAttentionMaskMode) -> MLXArray {
        let h = Gemma.clipResidual(x, postAttentionNorm(attention(inputNorm(x), mask: mask)))
        return Gemma.clipResidual(h, postFeedforwardNorm(mlp(preFeedforwardNorm(h))))
    }
}

final class GemmaEncoderBackbone: Module {
    let config: Gemma3Configuration

    @ModuleInfo(key: "embed_tokens") var embedTokens: Embedding
    @ModuleInfo var layers: [GemmaEncoderLayer]
    @ModuleInfo var norm: Gemma.RMSNorm

    init(_ config: Gemma3Configuration) {
        self.config = config
        _embedTokens.wrappedValue = Embedding(
            embeddingCount: config.vocabularySize, dimensions: config.hiddenSize)
        layers = (0..<config.hiddenLayers).map { GemmaEncoderLayer(config, index: $0) }
        norm = Gemma.RMSNorm(dimensions: config.hiddenSize, eps: config.rmsNormEps)
        super.init()
    }

    func callAsFunction(_ inputs: MLXArray) -> MLXArray {
        var h = embedTokens(inputs)
        h = h * MLXArray(Float(config.hiddenSize).squareRoot(), dtype: .bfloat16).asType(h.dtype)
        let length = inputs.dim(1)
        let window = EmbeddingGemma.slidingMask(length: length, window: config.slidingWindow)
        for layer in layers {
            h = layer(h, mask: layer.sliding ? window : .none)
        }
        return norm(h)
    }
}

/// EmbeddingGemma, in place of MLXEmbedders' causal port.
final class EmbeddingGemma: Module, TextEncoder {
    let config: Gemma3Configuration

    @ModuleInfo(key: "model") var backbone: GemmaEncoderBackbone
    /// The two dense layers after pooling (768 to 3072 and back), sized from the weights.
    @ModuleInfo var dense: [Linear]

    init(_ config: Gemma3Configuration) {
        self.config = config
        _backbone.wrappedValue = GemmaEncoderBackbone(config)
        _dense.wrappedValue = []
        super.init()
    }

    /// The mask for a sliding layer over `length` tokens: each attends to those within half of
    /// `window` on either side, or to all of them when the text is that short.
    static func slidingMask(length: Int, window: Int) -> MLXFast.ScaledDotProductAttentionMaskMode {
        let reach = window / 2
        guard length > reach + 1 else { return .none }
        let positions = MLXArray(Int32(0)..<Int32(length))
        let distance = MLX.abs(positions[0..., .newAxis] - positions[.newAxis])
        return .array(distance .<= MLXArray(Int32(reach)))
    }

    /// The text's vector: the mean of its tokens', through the dense layers.
    func encode(_ tokens: MLXArray) -> MLXArray {
        var pooled = backbone(tokens).mean(axis: 1)
        for layer in dense {
            pooled = layer(pooled)
        }
        return pooled.reshaped(-1)
    }

    func sanitize(weights: [String: MLXArray]) -> [String: MLXArray] {
        if let first = weights["dense.0.weight"] {
            let hidden = first.dim(0)
            update(modules: .unflattened([
                ("dense.0", Linear(config.hiddenSize, hidden, bias: false)),
                ("dense.1", Linear(hidden, config.hiddenSize, bias: false)),
            ]))
        }
        var weights = weights
        if let table = weights["model.embed_tokens.weight"], table.dim(0) > config.vocabularySize {
            weights["model.embed_tokens.weight"] = table[0..<config.vocabularySize]
        }
        return weights.filter { key, _ in
            !key.contains("self_attn.rotary_emb.inv_freq") && !key.hasPrefix("lm_head")
        }
    }
}
