# CarrelMLX

Carrel's provider for open models through Apple's [MLX](https://github.com/ml-explore/mlx-swift), on Apple-silicon Macs, following the [provider protocol](../../docs/providers.md). The design is [docs/design/mlx.md](../../docs/design/mlx.md).

- **`carrel-provider-mlx`:** the program Carrel runs. Put it next to `carrel` or on `PATH`, with the `mlx-swift_Cmlx.bundle` built beside it, and `carrel providers` lists it.
- **`CarrelMLXProvider`:** the library holding the provider.

It offers each model in the `mlx` folder inside Carrel's models folder, one folder per model, as `carrel models download NAME --format mlx` leaves them. Each offer is a `text-model` at priority 90, below Apple's on-device model, with:

- **JSON answers that always parse:** a schema is enforced token by token through `MLXGuidedGeneration` (XGrammar), as the llama.cpp provider does with grammars.
- **Memory:** a model is offered only on a Mac with twice its size in memory. It's loaded on the first request, one at a time, and unloaded after five idle minutes.
- **Context:** 8,192 tokens, or `CARREL_MLX_CONTEXT`. A prompt that doesn't fit with room for the answer is reported as unsupported, so Carrel tries another model.
- **No network:** tokenizers are read from the model's folder. Models are downloaded by `carrel-fetch`, never by this provider.

A folder named after one of the catalog's embedding models, `embeddinggemma-300m` or `qwen3-embedding-0.6b`, is offered as `embed` instead, for search, with the catalog's prompts and pooling, so its vectors can be compared with the llama.cpp provider's. EmbeddingGemma runs through this provider's own encoder, which reads in both directions as the model was trained; MLXEmbedders' port reads one way, as Gemma 3 generates text.

MLX's Metal shaders can only be compiled by Xcode, so the package is built with `xcodebuild`, which needs Xcode's Metal Toolchain (`xcodebuild -downloadComponent MetalToolchain`). From this folder, run the tests:

```bash
xcodebuild -scheme CarrelMLX-Package -destination 'platform=macOS' -derivedDataPath .build/xcode -skipMacroValidation test
```

The tests that draft and embed with real models run only when `CARREL_TEST_MLX_MODELS` names a models folder with MLX models in it; the others use a small model with random weights. `xcodebuild` passes it to the tests as `TEST_RUNNER_CARREL_TEST_MLX_MODELS`.

Or build the provider:

```bash
xcodebuild -scheme carrel-provider-mlx -destination 'platform=macOS' -derivedDataPath .build/xcode -configuration Release -skipMacroValidation build
```

The program and its bundle are in `.build/xcode/Build/Products/Release/`.
