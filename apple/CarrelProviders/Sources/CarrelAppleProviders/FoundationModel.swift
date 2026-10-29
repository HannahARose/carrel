// Apple's on-device model through Foundation Models. On-device only: Private Cloud Compute is a
// remote tier and isn't used in v1 (see the wiki's Models and Privacy page).

import Foundation

#if canImport(FoundationModels)
import FoundationModels
#endif

/// Apple's on-device language model (`SystemLanguageModel`).
///
/// Answers that must follow a JSON Schema use guided generation. Schemas, prompts or content the
/// model can't handle (too long for its context, blocked by its guardrails, an unsupported
/// language or schema feature) are reported as unsupported, so Carrel falls back to another
/// model instead of failing.
public struct FoundationModel: TextModel {
    public init() {}

    public var offer: Offer {
        var offer = Offer(
            capability: .textModel, id: "afm", label: "Apple Foundation Model",
            structuredOutput: true)
        #if canImport(FoundationModels)
        if #available(macOS 26.0, *) {
            let model = SystemLanguageModel.default
            offer.contextTokens = model.contextSize
            if case .unavailable(let why) = model.availability {
                offer.available = false
                offer.reason = Self.describe(why)
            }
            return offer
        }
        #endif
        offer.available = false
        offer.reason = "Apple's on-device model needs macOS 26 or later"
        return offer
    }

    public func complete(_ request: CompletionRequest) async throws(ProviderError) -> Completion {
        #if canImport(FoundationModels)
        if #available(macOS 26.0, *) {
            return try await Self.run(request)
        }
        #endif
        throw .unavailable("Apple's on-device model needs macOS 26 or later")
    }

    public func countTokens(_ text: String) async throws(ProviderError) -> Int {
        #if canImport(FoundationModels)
        if #available(macOS 26.4, *) {
            do {
                return try await SystemLanguageModel.default.tokenCount(for: text)
            } catch {
                throw Self.map(any: error)
            }
        }
        #endif
        throw .unsupported("counting tokens needs macOS 26.4 or later")
    }

    #if canImport(FoundationModels)
    @available(macOS 26.0, *)
    static func describe(_ reason: SystemLanguageModel.Availability.UnavailableReason) -> String {
        switch reason {
        case .deviceNotEligible: "this Mac can't run Apple Intelligence"
        case .appleIntelligenceNotEnabled: "Apple Intelligence is turned off in System Settings"
        case .modelNotReady: "Apple's model is still downloading"
        @unknown default: "Apple's model isn't available"
        }
    }

    /// The model's name for provenance, such as `apple/afm-3-core-advanced`.
    @available(macOS 26.0, *)
    static var modelName: String {
        // `variant` is in the macOS 27 SDK, which comes with Swift 6.4; older SDKs can't name it.
        #if compiler(>=6.4)
        if #available(macOS 27.0, *) {
            let slug = SystemLanguageModel.default.variant.displayName
                .lowercased()
                .split(whereSeparator: { !$0.isLetter && !$0.isNumber })
                .joined(separator: "-")
            if !slug.isEmpty { return "apple/\(slug)" }
        }
        #endif
        return "apple/afm"
    }

    @available(macOS 26.0, *)
    static func run(_ request: CompletionRequest) async throws(ProviderError) -> Completion {
        let session = LanguageModelSession(model: .default, instructions: request.system)
        let options = GenerationOptions(maximumResponseTokens: request.maxTokens)
        do {
            if let schema = request.schema {
                let guided = try GenerationSchema(
                    root: try JSONSchemaConverter().convert(schema, name: "Answer"),
                    dependencies: [])
                let response = try await session.respond(
                    to: request.prompt, schema: guided, options: options)
                let text = response.content.jsonString
                return Completion(text: text, json: JSONValue(parsing: text), model: modelName)
            }
            let response = try await session.respond(to: request.prompt, options: options)
            return Completion(text: response.content, model: modelName)
        } catch {
            throw map(any: error)
        }
    }

    /// Sorts the model's errors into the protocol's codes. macOS 27 reports them as
    /// `LanguageModelError` and `SystemLanguageModel.Error`; macOS 26 as `GenerationError`.
    @available(macOS 26.0, *)
    static func map(any error: any Error) -> ProviderError {
        if let error = error as? ProviderError {
            return error
        }
        #if compiler(>=6.4)
        if #available(macOS 27.0, *) {
            if let error = error as? LanguageModelError {
                return map(error)
            }
            if let error = error as? SystemLanguageModel.Error {
                switch error {
                case .assetsUnavailable: return .unavailable(error.localizedDescription)
                @unknown default: return .failed(error.localizedDescription)
                }
            }
        }
        #endif
        if let error = error as? LanguageModelSession.GenerationError {
            return map(error)
        }
        return .failed(error.localizedDescription)
    }

    #if compiler(>=6.4)
    @available(macOS 27.0, *)
    static func map(_ error: LanguageModelError) -> ProviderError {
        let why = error.localizedDescription
        switch error {
        case .contextSizeExceeded:
            return .unsupported("too long for Apple's on-device model: \(why)")
        case .guardrailViolation, .refusal:
            return .unsupported("Apple's on-device model declined: \(why)")
        case .unsupportedCapability, .unsupportedTranscriptContent, .unsupportedGenerationGuide,
            .unsupportedLanguageOrLocale:
            return .unsupported(why)
        case .rateLimited, .timeout:
            return .failed(why)
        @unknown default:
            return .failed(why)
        }
    }
    #endif

    @available(macOS 26.0, *)
    static func map(_ error: LanguageModelSession.GenerationError) -> ProviderError {
        let why = error.localizedDescription
        switch error {
        case .exceededContextWindowSize:
            return .unsupported("too long for Apple's on-device model: \(why)")
        case .guardrailViolation, .refusal:
            return .unsupported("Apple's on-device model declined: \(why)")
        case .unsupportedLanguageOrLocale, .unsupportedGuide:
            return .unsupported(why)
        case .assetsUnavailable:
            return .unavailable(why)
        default:
            return .failed(why)
        }
    }
    #endif
}

#if canImport(FoundationModels)
/// Translates the parts of JSON Schema that guided generation can express: objects with
/// properties (and `required`), arrays (with `minItems` and `maxItems`), strings, string
/// `enum`s, integers, numbers and booleans, each with a `description`. Anything else is
/// unsupported, so another model gets the request.
@available(macOS 26.0, *)
struct JSONSchemaConverter {
    static let understood: Set<String> = [
        "type", "properties", "required", "items", "enum", "description", "title",
        "minItems", "maxItems", "additionalProperties", "$schema",
    ]

    func convert(_ schema: JSONValue, name: String) throws(ProviderError) -> DynamicGenerationSchema {
        guard case .object(let fields) = schema else {
            throw .unsupported("a schema must be a JSON object")
        }
        if let unknown = fields.keys.sorted().first(where: { !Self.understood.contains($0) }) {
            throw .unsupported("guided generation can't express `\(unknown)` in a schema")
        }
        if let extra = fields["additionalProperties"], extra != .bool(false) {
            throw .unsupported("guided generation needs `additionalProperties: false`")
        }
        let description = fields["description"].flatMap { if case .string(let s) = $0 { s } else { nil } }

        if case .array(let choices) = fields["enum"] {
            let strings = choices.compactMap { if case .string(let s) = $0 { s } else { nil } }
            guard strings.count == choices.count, !strings.isEmpty else {
                throw .unsupported("only enums of strings are supported")
            }
            return DynamicGenerationSchema(name: name, description: description, anyOf: strings)
        }
        guard case .string(let type) = fields["type"] else {
            throw .unsupported("every schema needs a single `type`")
        }
        switch type {
        case "string": return DynamicGenerationSchema(type: String.self)
        case "integer": return DynamicGenerationSchema(type: Int.self)
        case "number": return DynamicGenerationSchema(type: Double.self)
        case "boolean": return DynamicGenerationSchema(type: Bool.self)
        case "array":
            guard let items = fields["items"] else {
                throw .unsupported("arrays need `items`")
            }
            return DynamicGenerationSchema(
                arrayOf: try convert(items, name: name + "Item"),
                minimumElements: Self.count(fields["minItems"]),
                maximumElements: Self.count(fields["maxItems"]))
        case "object":
            guard case .object(let properties) = fields["properties"] ?? .object([:]) else {
                throw .unsupported("`properties` must be an object")
            }
            let required: Set<String> = {
                guard case .array(let names) = fields["required"] else { return [] }
                return Set(names.compactMap { if case .string(let s) = $0 { s } else { nil } })
            }()
            var converted: [DynamicGenerationSchema.Property] = []
            for key in properties.keys.sorted() {
                let property = properties[key]!
                let propertyDescription = property["description"].flatMap {
                    if case .string(let s) = $0 { s } else { nil }
                }
                converted.append(
                    DynamicGenerationSchema.Property(
                        name: key, description: propertyDescription,
                        schema: try convert(property, name: name + key.capitalized),
                        isOptional: !required.contains(key)))
            }
            return DynamicGenerationSchema(name: name, description: description, properties: converted)
        default:
            throw .unsupported("guided generation doesn't support type `\(type)`")
        }
    }

    static func count(_ value: JSONValue?) -> Int? {
        if case .number(let n) = value { Int(n) } else { nil }
    }
}
#endif
