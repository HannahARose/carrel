import CarrelAppleProviders
import Foundation
import Testing

@testable import CarrelMLXProvider

struct ModelFolderTests {
    func folder(files: [String: Int]) throws -> URL {
        let root = FileManager.default.temporaryDirectory.appending(path: "carrel-mlx-\(UUID())")
        for (path, size) in files {
            let url = root.appending(path: path)
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data(count: size).write(to: url)
        }
        return root
    }

    @Test func modelsAreFoldersWithAConfigInMLX() throws {
        let root = try folder(files: [
            "mlx/qwen3.5-4b/config.json": 10,
            "mlx/qwen3.5-4b/model.safetensors": 990,
            "mlx/partial/model.safetensors": 5,
            "granite.gguf": 100,
        ])
        let found = ModelFolder.discover(in: root)
        #expect(found.map(\.name) == ["qwen3.5-4b"])
        #expect(found.first?.size == 1000)
        #expect(ModelFolder.discover(in: root.appending(path: "missing")).isEmpty)
    }

    @Test func theModelsFolderFollowsCarrel() {
        let set = ModelFolder.modelsRoot(environment: ["CARREL_MODELS_DIR": "/tmp/models"])
        #expect(set?.path(percentEncoded: false) == "/tmp/models/")
        let unset = ModelFolder.modelsRoot(environment: [:])
        #expect(unset?.path(percentEncoded: false).hasSuffix("Application Support/Carrel/models/") == true)
    }

    @Test func aModelNeedsTwiceItsSizeInMemory() {
        let model = ModelFolder(name: "big", url: URL(filePath: "/x"), size: 5_000_000_000)
        #expect(model.tooBig(forMemory: 16_000_000_000) == nil)
        #expect(model.tooBig(forMemory: 8_000_000_000) == "big needs about 10 GB of memory; this Mac has 8 GB")

        let offer = MLXModel(folder: model, environment: [:], memory: 8_000_000_000).offer
        #expect(offer.available == false)
        #expect(offer.priority == mlxPriority)
        #expect(offer.contextTokens == defaultContext)
        let roomy = MLXModel(
            folder: model, environment: ["CARREL_MLX_CONTEXT": "4096"], memory: 64_000_000_000
        ).offer
        #expect(roomy.available && roomy.contextTokens == 4096 && roomy.structuredOutput == true)
    }

    @Test func unloadingWithNothingLoadedIsHarmless() async {
        let loaded = Loaded()
        await loaded.unload()
        #expect(await loaded.isLoaded == false)
    }
}
