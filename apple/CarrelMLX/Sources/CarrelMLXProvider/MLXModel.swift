// Open models through MLX (docs/design/mlx.md): loaded on the first request, kept for the
// batch, and unloaded after five idle minutes or when Carrel says the queue is done. Answers
// that must follow a JSON Schema are constrained token by token (MLXGuidedGeneration), so they
// always parse, as with the llama.cpp provider's grammars.

import CarrelAppleProviders
import Foundation
import MLX
import MLXGuidedGeneration
import MLXLLM
import MLXLMCommon

/// Below Apple's model (100), which the system shares at no memory cost to Carrel.
public let mlxPriority = 90

/// The context used when `CARREL_MLX_CONTEXT` doesn't set one, as for llama.cpp.
let defaultContext = 8192

/// The answer's room when a request doesn't say, as for llama.cpp.
let defaultMaxTokens = 1024

/// How long a loaded model may sit unused before it's unloaded.
let idleUnload: Duration = .seconds(300)

/// A fixed seed, so the same prompt gives the same answer.
let seed: UInt64 = 0x00ca_7e11

/// The one model a provider process has loaded, if any. Loading another unloads it first, so
/// at most one model's memory is held.
public actor Loaded {
    public static let shared = Loaded()

    private var current: (name: String, container: ModelContainer)?
    private var tokenizers: [String: any MLXLMCommon.Tokenizer] = [:]
    private var lastUse = ContinuousClock.now
    private var idleTask: Task<Void, Never>?

    /// `folder`'s model, loading it if it isn't the one loaded.
    func container(for folder: ModelFolder) async throws -> ModelContainer {
        lastUse = .now
        if let current, current.name == folder.name {
            return current.container
        }
        current = nil
        Memory.clearCache()
        let container = try await LLMModelFactory.shared.loadContainer(
            from: folder.url, using: LocalTokenizerLoader())
        current = (folder.name, container)
        scheduleIdleUnload()
        return container
    }

    /// `folder`'s tokenizer alone, for counting tokens without loading the weights.
    func tokenizer(for folder: ModelFolder) async throws -> any MLXLMCommon.Tokenizer {
        if let tokenizer = tokenizers[folder.name] {
            return tokenizer
        }
        let tokenizer = try await LocalTokenizerLoader().load(from: folder.url)
        tokenizers[folder.name] = tokenizer
        return tokenizer
    }

    /// Drops the loaded model and returns its memory to the system.
    public func unload() {
        idleTask?.cancel()
        idleTask = nil
        guard current != nil else { return }
        current = nil
        Memory.clearCache()
        FileHandle.standardError.write(Data("carrel-provider-mlx: unloaded the model\n".utf8))
    }

    /// Whether a model is loaded, for tests.
    public var isLoaded: Bool { current != nil }

    private func scheduleIdleUnload() {
        idleTask?.cancel()
        idleTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(30))
                guard let self else { return }
                if await self.idle(for: idleUnload) {
                    await self.unload()
                    return
                }
            }
        }
    }

    private func idle(for duration: Duration) -> Bool {
        current != nil && ContinuousClock.now - lastUse >= duration
    }

    fileprivate func touch() {
        lastUse = .now
    }
}

/// One MLX model folder, as a text model.
public struct MLXModel: TextModel {
    public let folder: ModelFolder
    let context: Int
    let memory: UInt64
    let loaded: Loaded

    public init(
        folder: ModelFolder,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        memory: UInt64 = ProcessInfo.processInfo.physicalMemory,
        loaded: Loaded = .shared
    ) {
        self.folder = folder
        self.context = environment["CARREL_MLX_CONTEXT"].flatMap(Int.init) ?? defaultContext
        self.memory = memory
        self.loaded = loaded
    }

    public var offer: Offer {
        var offer = Offer(
            capability: .textModel, id: folder.name, label: "\(folder.name) (MLX)",
            priority: mlxPriority, contextTokens: context, structuredOutput: true)
        if let why = folder.tooBig(forMemory: memory) {
            offer.available = false
            offer.reason = why
        }
        return offer
    }

    public func countTokens(_ text: String) async throws(ProviderError) -> Int {
        do {
            return try await loaded.tokenizer(for: folder).encode(text: text).count
        } catch {
            throw .failed("can't read \(folder.name)'s tokenizer: \(error)")
        }
    }

    public func complete(_ request: CompletionRequest) async throws(ProviderError) -> Completion {
        let container: ModelContainer
        do {
            container = try await loaded.container(for: folder)
        } catch {
            throw .unavailable("can't load \(folder.name): \(error)")
        }
        let (name, limit) = (folder.name, context)
        let schema: String? = request.schema.flatMap { value in
            (try? JSONSerialization.data(withJSONObject: value.any, options: [.sortedKeys]))
                .flatMap { String(data: $0, encoding: .utf8) }
        }
        let text: String
        do {
            text = try await container.perform { context in
                try await Self.generate(request, schema: schema, context: context, name: name, limit: limit)
            }
        } catch let error as ProviderError {
            throw error
        } catch GuidedGenerationError.incompleteOutput {
            throw .failed("\(name)'s answer was cut off before the JSON ended")
        } catch GuidedGenerationError.prematureEOS {
            throw .failed("\(name) stopped before the JSON ended")
        } catch GrammarError.invalidJSONSchema(let why) {
            throw .unsupported("can't follow the schema: \(why)")
        } catch {
            throw .failed("\(name): \(error)")
        }
        await loaded.touch()

        var completion = Completion(text: text, model: "mlx/\(name)")
        if request.schema != nil {
            guard let json = JSONValue(parsing: text) else {
                throw .failed("\(name)'s answer isn't valid JSON")
            }
            completion.json = json
        }
        return completion
    }

    /// Runs `request` in the loaded model's context: constrained to `schema` when given.
    static func generate(
        _ request: CompletionRequest, schema: String?, context: ModelContext, name: String,
        limit: Int
    ) async throws -> String {
        var messages: [Chat.Message] = []
        if let system = request.system {
            messages.append(.system(system))
        }
        messages.append(.user(request.prompt))
        // Thinking models answer directly; their reasoning would only use up the answer's room.
        let input = try await context.processor.prepare(
            input: UserInput(chat: messages, additionalContext: ["enable_thinking": false]))
        let maxTokens = request.maxTokens ?? defaultMaxTokens
        let promptTokens = input.text.tokens.size
        if promptTokens + maxTokens > limit {
            throw ProviderError.unsupported(
                "too long for \(name): \(promptTokens) prompt tokens plus \(maxTokens) for the answer exceed its context of \(limit)"
            )
        }
        MLXRandom.seed(seed)

        guard let schema else {
            let parameters = GenerateParameters(maxTokens: maxTokens, temperature: 0.7)
            var output = ""
            for await generation in try MLXLMCommon.generate(
                input: input, parameters: parameters, context: context)
            {
                if case .chunk(let text) = generation {
                    output += text
                }
            }
            return output
        }
        let tokenizer = context.tokenizer
        let vocab = TokenizerVocabExtractor.extractForGrammar(from: tokenizer)
        let grammarTokenizer = try GrammarTokenizer(
            vocab: vocab.vocab, vocabType: vocab.vocabType,
            eosTokenId: Int32(tokenizer.eosTokenId ?? 0))
        let constraint = try GrammarConstraint(
            tokenizer: grammarTokenizer, jsonSchema: schema, fastForward: true,
            hostTokenizer: tokenizer)
        var output = ""
        try GuidedGenerationLoop.run(
            input: input, context: context, constraint: constraint, maxTokens: maxTokens,
            vocabSize: grammarTokenizer.vocabSize
        ) { delta in
            output += delta
            return true
        }
        return output
    }
}
