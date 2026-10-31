// Drafting with a real MLX model. Runs only when `CARREL_TEST_MLX_MODELS` names a models folder
// with an MLX model in `mlx/`, as the on-device runner's does; elsewhere it checks nothing.

import CarrelAppleProviders
import Foundation
import Testing

@testable import CarrelMLXProvider

struct ModelTests {
    static let folder: ModelFolder? = ProcessInfo.processInfo.environment["CARREL_TEST_MLX_MODELS"]
        .flatMap { ModelFolder.discover(in: URL(filePath: $0, directoryHint: .isDirectory)).first }

    @Test(.enabled(if: folder != nil, "CARREL_TEST_MLX_MODELS isn't set"))
    func answersFollowTheSchemaAndCountTokens() async throws {
        let model = MLXModel(folder: try #require(Self.folder))
        #expect(model.offer.available, "\(model.offer.reason ?? "")")
        let schema = JSONValue(parsing: """
            {"type": "object", "properties": {"title": {"type": "string"},
             "year": {"type": "integer"}}, "required": ["title", "year"]}
            """)
        let answer = try await model.complete(
            CompletionRequest(
                system: "You file documents.",
                prompt: "Give a title and year for: 'Lease agreement, 1 March 2024, 12 Elm St.'",
                schema: schema, maxTokens: 128))
        guard case .object(let fields) = try #require(answer.json) else {
            Issue.record("not an object: \(answer.text)")
            return
        }
        #expect(fields["title"] != nil && fields["year"] != nil, "\(answer.text)")
        #expect(answer.model == "mlx/\(model.folder.name)")

        let tokens = try await model.countTokens("The quick brown fox jumps over the lazy dog.")
        #expect(tokens > 5 && tokens < 30)

        await Loaded.shared.unload()
        #expect(await Loaded.shared.isLoaded == false)
    }
}
