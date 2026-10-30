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
    private var biases: [String: Biases] = [:]
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

    /// The token biases guided generation uses for `name`'s tokenizer, worked out once: they
    /// scan the whole vocabulary.
    func biases(for name: String, tokenizer: any MLXLMCommon.Tokenizer) -> Biases {
        if let cached = biases[name] {
            return cached
        }
        let (whitespace, whitespaceTokenIDs) = WhitespaceTokenBias.compute(tokenizer: tokenizer)
        let made = Biases(
            closing: ClosingTokenBias.compute(tokenizer: tokenizer, eosTokenId: tokenizer.eosTokenId),
            whitespace: whitespace, whitespaceTokenIDs: whitespaceTokenIDs)
        biases[name] = made
        return made
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

/// Nudges for guided generation, as `mlx-swift-lm`'s own Foundation Models adapter uses them:
/// against long runs of whitespace, which a JSON grammar allows and small models fall into,
/// and toward closing the JSON as the answer nears its token limit, so it ends rather than
/// being cut off.
final class Biases: @unchecked Sendable {
    let closing: MLXArray
    let whitespace: MLXArray
    let whitespaceTokenIDs: Set<Int>

    init(closing: MLXArray, whitespace: MLXArray, whitespaceTokenIDs: Set<Int>) {
        self.closing = closing
        self.whitespace = whitespace
        self.whitespaceTokenIDs = whitespaceTokenIDs
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
        let (name, limit, loaded) = (folder.name, context, loaded)
        let schema = request.schema.map(SchemaText.write)
        let text: String
        do {
            text = try await container.perform { context in
                try await Self.generate(
                    request, schema: schema, context: context, name: name, limit: limit,
                    loaded: loaded)
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
        limit: Int, loaded: Loaded
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
        let biases = await loaded.biases(for: name, tokenizer: tokenizer)
        // The reserves near the limit where the closing nudge starts, then where closing is
        // forced: worked out from the schema, as the library's adapter does.
        let structural = CompletionReserve.estimate(schemaJSON: schema, tokenizer: tokenizer)
        var output = ""
        try GuidedGenerationLoop.run(
            input: input, context: context, constraint: constraint, maxTokens: maxTokens,
            vocabSize: grammarTokenizer.vocabSize,
            completionReserve: max(structural * 3, maxTokens / 4),
            hardReserve: structural * 8,
            closingBias: biases.closing, whitespaceBias: biases.whitespace,
            whitespaceTokenIDs: biases.whitespaceTokenIDs
        ) { delta in
            output += delta
            return true
        }
        return output
    }
}

/// A JSON Schema as text, with each object's properties in the order the model should write
/// them. Guided generation writes fields in the order the schema lists them, and the order
/// matters: a model that writes the summary before the title, say, tends to fall back to the
/// file name for the title. The request's order is lost when it's parsed, but `required`, a
/// list, keeps it, so properties follow `required`, then any others by name.
enum SchemaText {
    static func write(_ value: CarrelAppleProviders.JSONValue) -> String {
        var out = ""
        append(value, to: &out)
        return out
    }

    static func append(_ value: CarrelAppleProviders.JSONValue, to out: inout String) {
        switch value {
        case .null: out += "null"
        case .bool(let b): out += b ? "true" : "false"
        case .number(let n):
            out += n.rounded() == n && abs(n) < 1e15 ? String(Int(n)) : String(n)
        case .string(let s): out += quoted(s)
        case .array(let items):
            out += "["
            for (i, item) in items.enumerated() {
                if i > 0 { out += "," }
                append(item, to: &out)
            }
            out += "]"
        case .object(let members):
            let order = keys(of: members)
            out += "{"
            for (i, key) in order.enumerated() {
                if i > 0 { out += "," }
                out += quoted(key) + ":"
                if key == "properties", case .object(let properties)? = members[key] {
                    appendProperties(properties, order: requiredOrder(members), to: &out)
                } else if let member = members[key] {
                    append(member, to: &out)
                }
            }
            out += "}"
        }
    }

    /// An object's keys: `type` first, so the schema reads naturally, then by name.
    static func keys(of members: [String: CarrelAppleProviders.JSONValue]) -> [String] {
        members.keys.sorted { a, b in
            a == "type" ? b != "type" : b == "type" ? false : a < b
        }
    }

    static func requiredOrder(_ members: [String: CarrelAppleProviders.JSONValue]) -> [String] {
        guard case .array(let required)? = members["required"] else { return [] }
        return required.compactMap { if case .string(let s) = $0 { s } else { nil } }
    }

    static func appendProperties(
        _ properties: [String: CarrelAppleProviders.JSONValue], order: [String], to out: inout String
    ) {
        let listed = order.filter { properties[$0] != nil }
        let rest = properties.keys.filter { !listed.contains($0) }.sorted()
        out += "{"
        for (i, key) in (listed + rest).enumerated() {
            if i > 0 { out += "," }
            out += quoted(key) + ":"
            if let property = properties[key] {
                append(property, to: &out)
            }
        }
        out += "}"
    }

    static func quoted(_ s: String) -> String {
        let data = (try? JSONSerialization.data(withJSONObject: [s], options: [.fragmentsAllowed])) ?? Data()
        let array = String(data: data, encoding: .utf8) ?? "[\"\"]"
        return String(array.dropFirst().dropLast())
    }
}


