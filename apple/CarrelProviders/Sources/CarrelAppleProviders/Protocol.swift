// The provider protocol (docs/providers.md): JSON-RPC 2.0, one message per line.

import Foundation

/// The protocol version these providers speak.
public let protocolVersion = 1

/// Priorities by convention (docs/providers.md).
public enum Priority {
    /// A platform's own tools.
    public static let native = 100
}

/// What an offer does.
public enum Capability: String, Codable, Sendable {
    case textModel = "text-model"
    case extractText = "extract-text"
    case embed
}

/// One thing a provider offers, as reported by `initialize`.
public struct Offer: Codable, Equatable, Sendable {
    public var capability: Capability
    public var id: String
    public var label: String?
    public var priority: Int
    public var available: Bool
    public var reason: String?
    public var mediaTypes: [String]?
    /// For text models: the context size, in tokens.
    public var contextTokens: Int?
    /// For text models: whether answers can follow a JSON Schema.
    public var structuredOutput: Bool?
    /// For extractors: whether it describes what images show, so Carrel gives it pictures to
    /// describe, such as a 3MF's thumbnail.
    public var describes: Bool?
    /// For embedding models: how many numbers each vector has.
    public var dimensions: Int?

    public init(
        capability: Capability, id: String, label: String? = nil, priority: Int = Priority.native,
        available: Bool = true, reason: String? = nil, mediaTypes: [String]? = nil,
        contextTokens: Int? = nil, structuredOutput: Bool? = nil, describes: Bool? = nil,
        dimensions: Int? = nil
    ) {
        self.capability = capability
        self.id = id
        self.label = label
        self.priority = priority
        self.available = available
        self.reason = reason
        self.mediaTypes = mediaTypes
        self.contextTokens = contextTokens
        self.structuredOutput = structuredOutput
        self.describes = describes
        self.dimensions = dimensions
    }

    enum CodingKeys: String, CodingKey {
        case capability, id, label, priority, available, reason, describes, dimensions
        case mediaTypes = "media_types"
        case contextTokens = "context_tokens"
        case structuredOutput = "structured_output"
    }
}

/// Why a request failed, with the protocol's error codes.
public enum ProviderError: Error, Equatable {
    /// The offer can't be used right now (code 1).
    case unavailable(String)
    /// The input isn't one the offer handles (code 2).
    case unsupported(String)
    /// The offer tried and couldn't (code 3).
    case failed(String)
    /// The model declined the input, as a safety refusal (code 4).
    case declined(String)
    /// A malformed request (JSON-RPC's -32602).
    case invalidParams(String)
    /// An unknown method (JSON-RPC's -32601).
    case unknownMethod(String)

    var code: Int {
        switch self {
        case .unavailable: 1
        case .unsupported: 2
        case .failed: 3
        case .declined: 4
        case .invalidParams: -32602
        case .unknownMethod: -32601
        }
    }

    var message: String {
        switch self {
        case .unavailable(let m), .unsupported(let m), .failed(let m), .declined(let m),
            .invalidParams(let m), .unknownMethod(let m):
            m
        }
    }
}

/// The text read from a file.
public struct Extraction: Equatable, Sendable {
    public var text: String
    /// `embedded` for text stored in the file, `ocr` for recognized text, `described` when a
    /// model also described an image.
    public var method: String
    public var pages: Int?
    /// When the file was made, if it says: a date, or a time with its offset.
    public var created: String?
    /// Whether it records where it was made, such as a photo's location.
    public var located: Bool?

    public init(
        text: String, method: String, pages: Int? = nil, created: String? = nil,
        located: Bool? = nil
    ) {
        self.text = text
        self.method = method
        self.pages = pages
        self.created = created
        self.located = located
    }
}

/// Something that reads the text of files.
public protocol TextExtractor: Sendable {
    /// What it offers. Checked when the provider starts, so availability is current.
    var offer: Offer { get }

    /// Reads the text of the file at `url`, of media type `mediaType`. Never changes the file.
    func extract(_ url: URL, mediaType: String) async throws(ProviderError) -> Extraction

    /// The same, for a picture Carrel was given to describe: `context` says what it is, since
    /// its file's name says nothing. Extractors that don't describe images ignore it.
    func extract(_ url: URL, mediaType: String, context: String?) async throws(ProviderError)
        -> Extraction
}

extension TextExtractor {
    public func extract(_ url: URL, mediaType: String, context: String?) async throws(ProviderError)
        -> Extraction
    {
        try await extract(url, mediaType: mediaType)
    }
}

/// A JSON value, so requests and answers can cross concurrency boundaries.
public enum JSONValue: Sendable, Equatable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    /// Converts what `JSONSerialization` produces.
    public init?(_ value: Any) {
        switch value {
        case is NSNull:
            self = .null
        case let number as NSNumber where CFGetTypeID(number) == CFBooleanGetTypeID():
            self = .bool(number.boolValue)
        case let number as NSNumber:
            self = .number(number.doubleValue)
        case let string as String:
            self = .string(string)
        case let array as [Any]:
            var items: [JSONValue] = []
            for item in array {
                guard let value = JSONValue(item) else { return nil }
                items.append(value)
            }
            self = .array(items)
        case let object as [String: Any]:
            var members: [String: JSONValue] = [:]
            for (key, item) in object {
                guard let value = JSONValue(item) else { return nil }
                members[key] = value
            }
            self = .object(members)
        default:
            return nil
        }
    }

    /// Parses JSON text.
    public init?(parsing text: String) {
        guard
            let object = try? JSONSerialization.jsonObject(
                with: Data(text.utf8), options: [.fragmentsAllowed])
        else { return nil }
        self.init(object)
    }

    /// The value as `JSONSerialization` expects it.
    public var any: Any {
        switch self {
        case .null: NSNull()
        case .bool(let b): b
        case .number(let n): n.rounded() == n && abs(n) < 1e15 ? Int(n) as Any : n as Any
        case .string(let s): s
        case .array(let a): a.map(\.any)
        case .object(let o): o.mapValues(\.any)
        }
    }

    subscript(key: String) -> JSONValue? {
        if case .object(let o) = self { o[key] } else { nil }
    }
}

/// A prompt for a text model.
public struct CompletionRequest: Sendable, Equatable {
    public var system: String?
    public var prompt: String
    /// A JSON Schema the answer must follow.
    public var schema: JSONValue?
    public var maxTokens: Int?

    public init(
        system: String? = nil, prompt: String, schema: JSONValue? = nil, maxTokens: Int? = nil
    ) {
        self.system = system
        self.prompt = prompt
        self.schema = schema
        self.maxTokens = maxTokens
    }
}

/// A text model's answer.
public struct Completion: Sendable, Equatable {
    public var text: String
    /// The answer as JSON, when a schema was given.
    public var json: JSONValue?
    /// The exact model, for provenance, such as `apple/afm-3-core-advanced`.
    public var model: String

    public init(text: String, json: JSONValue? = nil, model: String) {
        self.text = text
        self.json = json
        self.model = model
    }
}

/// A language model.
public protocol TextModel: Sendable {
    /// What it offers. Checked when the provider starts, so availability is current.
    var offer: Offer { get }

    /// Answers `request`.
    func complete(_ request: CompletionRequest) async throws(ProviderError) -> Completion

    /// The number of tokens `text` takes, for fitting text into the context. Optional: models
    /// that can't count report unsupported, and Carrel estimates instead.
    func countTokens(_ text: String) async throws(ProviderError) -> Int
}

extension TextModel {
    public func countTokens(_ text: String) async throws(ProviderError) -> Int {
        throw .unsupported("\(offer.id) doesn't count tokens")
    }
}

/// What a text is embedded for: to be found, or to search with. Some models embed the two
/// differently, each with its own prompt.
public enum EmbeddingPurpose: String, Sendable {
    case document
    case query
}

/// An embedding model's answer: one vector for each text.
public struct Embeddings: Sendable, Equatable {
    /// One vector for each text, in order, each of unit length.
    public var vectors: [[Float]]
    /// The model's identifier, the same as its offer's.
    public var model: String
    public var dimensions: Int

    public init(vectors: [[Float]], model: String, dimensions: Int) {
        self.vectors = vectors
        self.model = model
        self.dimensions = dimensions
    }
}

/// An embedding model, for search (`text/embed` in docs/providers.md). Its offer's `id` is the
/// model's identifier, the same on every provider that runs it, so vectors can be compared.
public protocol Embedder: Sendable {
    /// What it offers. Checked when the provider starts, so availability is current.
    var offer: Offer { get }

    /// One vector for each of `texts`, each embedded on its own. A text longer than the
    /// model's context is cut.
    func embed(_ texts: [String], purpose: EmbeddingPurpose) async throws(ProviderError)
        -> Embeddings
}

/// `vector` scaled to unit length, or as it is if it's all zeros.
public func unitLength(_ vector: [Float]) -> [Float] {
    let length = vector.reduce(0) { $0 + $1 * $1 }.squareRoot()
    return length > 0 ? vector.map { $0 / length } : vector
}

/// Answers provider protocol messages for a set of models, extractors and embedders.
public struct Server: Sendable {
    public let name: String
    public let version: String
    public let models: [any TextModel]
    public let extractors: [any TextExtractor]
    public let embedders: [any Embedder]

    public init(
        name: String, version: String, models: [any TextModel] = [],
        extractors: [any TextExtractor] = [], embedders: [any Embedder] = []
    ) {
        self.name = name
        self.version = version
        self.models = models
        self.extractors = extractors
        self.embedders = embedders
    }

    /// The reply to one line of input, or nil when no reply is due (a notification, or a line
    /// that isn't a request at all).
    public func reply(to line: String) async -> String? {
        guard let data = line.data(using: .utf8),
            let message = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let method = message["method"] as? String
        else {
            return nil
        }
        guard let id = message["id"], !(id is NSNull) else {
            return nil
        }
        let params = message["params"] as? [String: Any] ?? [:]
        var reply: [String: Any] = ["jsonrpc": "2.0", "id": id]
        do {
            reply["result"] = try await answer(method, params)
        } catch {
            reply["error"] = ["code": error.code, "message": error.message]
        }
        guard let out = try? JSONSerialization.data(withJSONObject: reply, options: [.sortedKeys]),
            let text = String(data: out, encoding: .utf8)
        else {
            return nil
        }
        return text
    }

    func answer(_ method: String, _ params: [String: Any]) async throws(ProviderError) -> Any {
        switch method {
        case "initialize":
            return [
                "protocol": protocolVersion,
                "name": name,
                "version": version,
                "offers": (models.map(\.offer) + extractors.map(\.offer) + embedders.map(\.offer))
                    .map(Self.json),
            ]
        case "shutdown":
            return NSNull()
        case "text/extract":
            guard let offerID = params["offer"] as? String,
                let path = params["path"] as? String,
                let mediaType = params["media_type"] as? String
            else {
                throw .invalidParams("text/extract needs offer, path and media_type")
            }
            guard let extractor = extractors.first(where: { $0.offer.id == offerID }) else {
                throw .unsupported("\(name) has no extractor \(offerID)")
            }
            guard extractor.offer.available else {
                throw .unavailable(extractor.offer.reason ?? "\(offerID) is unavailable")
            }
            let result = try await extractor.extract(
                URL(filePath: path), mediaType: mediaType, context: params["context"] as? String)
            var json: [String: Any] = ["text": result.text, "method": result.method]
            if let pages = result.pages {
                json["pages"] = pages
            }
            if let created = result.created {
                json["created"] = created
            }
            if result.located == true {
                json["located"] = true
            }
            return json
        case "text/count-tokens":
            guard let offerID = params["offer"] as? String, let text = params["text"] as? String
            else {
                throw .invalidParams("text/count-tokens needs offer and text")
            }
            guard let model = models.first(where: { $0.offer.id == offerID }) else {
                throw .unsupported("\(name) has no text model \(offerID)")
            }
            guard model.offer.available else {
                throw .unavailable(model.offer.reason ?? "\(offerID) is unavailable")
            }
            return ["tokens": try await model.countTokens(text)]
        case "text/complete":
            guard let offerID = params["offer"] as? String,
                let prompt = params["prompt"] as? String
            else {
                throw .invalidParams("text/complete needs offer and prompt")
            }
            guard let model = models.first(where: { $0.offer.id == offerID }) else {
                throw .unsupported("\(name) has no text model \(offerID)")
            }
            guard model.offer.available else {
                throw .unavailable(model.offer.reason ?? "\(offerID) is unavailable")
            }
            let request = CompletionRequest(
                system: params["system"] as? String, prompt: prompt,
                schema: params["schema"].flatMap { $0 is NSNull ? nil : JSONValue($0) },
                maxTokens: params["max_tokens"] as? Int)
            let result = try await model.complete(request)
            var json: [String: Any] = ["text": result.text, "model": result.model]
            if let value = result.json {
                json["json"] = value.any
            }
            return json
        case "text/embed":
            return try await embed(params)
        default:
            throw .unknownMethod("unknown method \(method)")
        }
    }

    func embed(_ params: [String: Any]) async throws(ProviderError) -> Any {
        guard let offerID = params["offer"] as? String, let texts = params["texts"] as? [String]
        else {
            throw .invalidParams("text/embed needs offer and texts")
        }
        let purpose: EmbeddingPurpose
        switch params["purpose"] {
        case nil, is NSNull:
            purpose = .document
        case let name as String:
            guard let named = EmbeddingPurpose(rawValue: name) else {
                throw .invalidParams("purpose is document or query, not \(name)")
            }
            purpose = named
        default:
            throw .invalidParams("purpose is document or query")
        }
        guard let embedder = embedders.first(where: { $0.offer.id == offerID }) else {
            throw .unsupported("\(name) has no embedding model \(offerID)")
        }
        guard embedder.offer.available else {
            throw .unavailable(embedder.offer.reason ?? "\(offerID) is unavailable")
        }
        let result = try await embedder.embed(texts, purpose: purpose)
        return [
            "vectors": result.vectors.map { $0.map(Self.number) },
            "model": result.model,
            "dimensions": result.dimensions,
        ] as [String: Any]
    }

    /// `value` as JSON writes it shortest, so 0.0213 is written as that and not as the Double
    /// nearest the Float, 0.021299999207258224: a vector's text is half as long.
    static func number(_ value: Float) -> NSNumber {
        value.isFinite ? NSDecimalNumber(string: "\(value)") : NSNumber(value: 0)
    }

    static func json(_ offer: Offer) -> Any {
        let data = (try? JSONEncoder().encode(offer)) ?? Data("{}".utf8)
        return (try? JSONSerialization.jsonObject(with: data)) ?? [:]
    }
}
