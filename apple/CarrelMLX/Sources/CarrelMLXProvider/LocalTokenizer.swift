// Tokenizers read from a model's own folder. MLXLMCommon leaves tokenizers to other packages;
// this adapts swift-transformers' to it, as mlx-swift-lm's `#huggingFaceTokenizerLoader()` macro
// does, without the Hugging Face downloader: Carrel's models are downloaded by `carrel-fetch`
// and this provider never reaches the network.

import Foundation
import MLXLMCommon
import Tokenizers

/// Loads the tokenizer in a model folder (`tokenizer.json` and `tokenizer_config.json`).
public struct LocalTokenizerLoader: MLXLMCommon.TokenizerLoader {
    public init() {}

    public func load(from directory: URL) async throws -> any MLXLMCommon.Tokenizer {
        Bridge(try await Tokenizers.AutoTokenizer.from(modelFolder: directory))
    }
}

/// A swift-transformers tokenizer as MLXLMCommon expects one.
struct Bridge: MLXLMCommon.Tokenizer {
    let upstream: any Tokenizers.Tokenizer

    init(_ upstream: any Tokenizers.Tokenizer) {
        self.upstream = upstream
    }

    func encode(text: String, addSpecialTokens: Bool) -> [Int] {
        upstream.encode(text: text, addSpecialTokens: addSpecialTokens)
    }

    func decode(tokenIds: [Int], skipSpecialTokens: Bool) -> String {
        upstream.decode(tokens: tokenIds, skipSpecialTokens: skipSpecialTokens)
    }

    func convertTokenToId(_ token: String) -> Int? {
        upstream.convertTokenToId(token)
    }

    func convertIdToToken(_ id: Int) -> String? {
        upstream.convertIdToToken(id)
    }

    var bosToken: String? { upstream.bosToken }
    var eosToken: String? { upstream.eosToken }
    var unknownToken: String? { upstream.unknownToken }

    func applyChatTemplate(
        messages: [[String: any Sendable]],
        tools: [[String: any Sendable]]?,
        additionalContext: [String: any Sendable]?
    ) throws -> [Int] {
        do {
            return try upstream.applyChatTemplate(
                messages: messages, tools: tools, additionalContext: additionalContext)
        } catch Tokenizers.TokenizerError.missingChatTemplate {
            throw MLXLMCommon.TokenizerError.missingChatTemplate
        }
    }
}
