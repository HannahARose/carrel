import Foundation
import FoundationModels
import Testing

@testable import CarrelAppleProviders

/// A model that answers with what it was asked, for testing the server.
struct FakeModel: TextModel {
    var available = true

    var offer: Offer {
        Offer(
            capability: .textModel, id: "fake", available: available,
            reason: available ? nil : "switched off", structuredOutput: true)
    }

    func complete(_ request: CompletionRequest) async throws(ProviderError) -> Completion {
        Completion(
            text: "\(request.system ?? "-")|\(request.prompt)|\(request.maxTokens ?? 0)",
            json: request.schema == nil ? nil : .object(["ok": .bool(true)]), model: "fake/1")
    }
}

func json(_ text: String) -> JSONValue { JSONValue(parsing: text)! }

@Test func completionRequestsReachTheModel() async throws {
    let server = Server(name: "Test", version: "1", models: [FakeModel()])
    let plain = try await send(
        server,
        [
            "jsonrpc": "2.0", "id": 1, "method": "text/complete",
            "params": [
                "offer": "fake", "system": "Be brief.", "prompt": "Hello", "max_tokens": 20,
            ],
        ])
    let result = try #require(plain["result"] as? [String: Any])
    #expect(result["text"] as? String == "Be brief.|Hello|20")
    #expect(result["model"] as? String == "fake/1")
    #expect(result["json"] == nil)

    let structured = try await send(
        server,
        [
            "jsonrpc": "2.0", "id": 2, "method": "text/complete",
            "params": [
                "offer": "fake", "prompt": "Hi", "schema": ["type": "object"],
            ],
        ])
    let answer = try #require(structured["result"] as? [String: Any])
    #expect((answer["json"] as? [String: Any])?["ok"] as? Bool == true)
}

@Test func completionErrorsUseTheProtocolsCodes() async throws {
    let server = Server(name: "Test", version: "1", models: [FakeModel(available: false)])
    func code(_ params: [String: Any]) async throws -> Int? {
        let reply = try await send(
            server, ["jsonrpc": "2.0", "id": 1, "method": "text/complete", "params": params])
        return (reply["error"] as? [String: Any])?["code"] as? Int
    }
    #expect(try await code(["offer": "fake", "prompt": "x"]) == 1, "unavailable")
    #expect(try await code(["offer": "other", "prompt": "x"]) == 2, "unknown offer")
    #expect(try await code(["offer": "fake"]) == -32602, "no prompt")
}

@Test func theAppleModelIsOfferedWithItsAvailability() async throws {
    let reply = try await send(
        Server(name: "Apple", version: "1", models: [FoundationModel()]),
        ["jsonrpc": "2.0", "id": 1, "method": "initialize", "params": ["protocol": 1]])
    let offers = try #require((reply["result"] as? [String: Any])?["offers"] as? [[String: Any]])
    let afm = try #require(offers.first)
    #expect(afm["capability"] as? String == "text-model")
    #expect(afm["id"] as? String == "afm")
    #expect(afm["priority"] as? Int == 100)
    #expect(afm["structured_output"] as? Bool == true)
    if afm["available"] as? Bool == true {
        #expect((afm["context_tokens"] as? Int ?? 0) > 0)
    } else {
        #expect(afm["reason"] is String, "an unavailable offer says why")
    }
}

@available(macOS 26.0, *)
@Test func schemasGuidedGenerationCanExpressConvert() throws {
    let converter = JSONSchemaConverter()
    let card = json(
        """
        {"type": "object", "additionalProperties": false, "required": ["title", "tags"],
         "properties": {
           "title": {"type": "string", "description": "A short title"},
           "tags": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 5},
           "sensitivity": {"enum": ["public", "personal", "sensitive"]},
           "pages": {"type": "integer"}, "score": {"type": "number"}, "draft": {"type": "boolean"}}}
        """)
    let root = try converter.convert(card, name: "Card")
    _ = try GenerationSchema(root: root, dependencies: [])
}

@available(macOS 26.0, *)
@Test func schemaFeaturesItCantExpressAreUnsupported() {
    let converter = JSONSchemaConverter()
    for schema in [
        #"{"type": "string", "pattern": "^a"}"#,
        #"{"anyOf": [{"type": "string"}, {"type": "integer"}]}"#,
        #"{"type": ["string", "null"]}"#,
        #"{"enum": [1, 2]}"#,
        #"{"type": "object", "additionalProperties": true}"#,
        #"{"type": "array"}"#,
        #"[1, 2]"#,
    ] {
        #expect(throws: ProviderError.self, "\(schema)") {
            try converter.convert(json(schema), name: "X")
        }
    }
}

/// Whether Apple's model can run here; CI's virtual Macs can't.
let appleModelAvailable: Bool = {
    if #available(macOS 26.0, *) { SystemLanguageModel.default.isAvailable } else { false }
}()

@Test(.enabled(if: appleModelAvailable, "Apple's on-device model isn't available here"))
func appleModelAnswersPrompts() async throws {
    let model = FoundationModel()
    let plain = try await model.complete(
        CompletionRequest(
            system: "Answer with one word.", prompt: "What color is a clear daytime sky?",
            maxTokens: 20))
    #expect(!plain.text.isEmpty)
    #expect(plain.model.hasPrefix("apple/"))

    let structured = try await model.complete(
        CompletionRequest(
            prompt:
                "Describe this file for an archive card: 'parts-list.csv', a bill of materials for a desk lamp.",
            schema: json(
                """
                {"type": "object", "required": ["title", "tags"], "properties": {
                  "title": {"type": "string", "description": "A short title"},
                  "tags": {"type": "array", "items": {"type": "string"}, "maxItems": 3}}}
                """)))
    guard case .object(let card) = structured.json, case .string(let title) = card["title"],
        case .array(let tags) = card["tags"]
    else {
        Issue.record("not a card: \(structured.text)")
        return
    }
    #expect(!title.isEmpty)
    #expect(tags.count <= 3)
}

@Test(.enabled(if: appleModelAvailable, "Apple's on-device model isn't available here"))
func promptsTooLongForTheModelAreUnsupported() async {
    // Far beyond an 8,192-token context, so another model with a larger one gets it.
    let long = String(repeating: "The oscillators drift in and out of phase. ", count: 1500)
    do {
        _ = try await FoundationModel().complete(CompletionRequest(prompt: "Summarize: " + long))
        Issue.record("a prompt this long should be refused")
    } catch {
        guard case .unsupported(let why) = error else {
            Issue.record("expected unsupported, got \(error)")
            return
        }
        #expect(why.contains("too long"), "\(why)")
    }
}

@Test func tokenCountingIsOptional() async throws {
    let server = Server(name: "Test", version: "1", models: [FakeModel()])
    let reply = try await send(
        server,
        [
            "jsonrpc": "2.0", "id": 1, "method": "text/count-tokens",
            "params": ["offer": "fake", "text": "hello"],
        ])
    #expect((reply["error"] as? [String: Any])?["code"] as? Int == 2, "the fake model can't count")
}

@Test(.enabled(if: appleModelAvailable, "Apple's on-device model isn't available here"))
func appleModelCountsTokens() async throws {
    let reply = try await send(
        Server(name: "Apple", version: "1", models: [FoundationModel()]),
        [
            "jsonrpc": "2.0", "id": 1, "method": "text/count-tokens",
            "params": [
                "offer": "afm", "text": String(repeating: "The oscillators drift. ", count: 50),
            ],
        ])
    let tokens = try #require((reply["result"] as? [String: Any])?["tokens"] as? Int, "\(reply)")
    #expect(tokens > 50 && tokens < 1000, "\(tokens)")
}
