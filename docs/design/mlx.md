# Design: an MLX model provider (v0.2)

Status: accepted 2026-10-30, with the five recommendations below. Issue #24.

## What this adds

On a Mac, Carrel drafts with Apple's on-device model (Foundation Models) or with an open model through llama.cpp. MLX is Apple's own machine-learning framework for Apple silicon, and running open models through it is usually faster than llama.cpp on the same Mac, with better use of unified memory. This design adds a provider that runs the catalog's open models through MLX, for Macs with enough memory, so the owner can use a stronger model than Apple's without leaving native tools.

On Linux, Windows and Intel Macs nothing changes: the llama.cpp provider fills this tier.

## Where it lives

- **A program of its own, `carrel-provider-mlx`,** in a new Swift package `apple/CarrelMLX`, rather than inside `carrel-provider-apple` as the issue first said.
  - MLX's Metal kernels have to be compiled into a Metal library, which `swift build` doesn't do: the package is built with `xcodebuild`. Keeping it apart means `carrel-provider-apple` still builds with plain `swift build` and keeps its 35 ms start-up and small memory.
  - A model's gigabytes live in their own process and go back to the system when it exits, as with llama.cpp (`docs/dev/overhead.md`).
  - The protocol code (`Protocol.swift`) is shared from `CarrelAppleProviders` as a package dependency, so the two don't drift.
- **Library:** `mlx-swift-lm` (Apple's `ml-explore` project, MIT), pinned to an exact version in `Package.resolved`. Adding it downloads the package and its dependencies, so that step waits for the owner's go-ahead.
- **Found like other providers:** next to `carrel`, then on `PATH`. The release build for Apple-silicon Macs ships it; others don't.

## Models

- **The same catalog, a second format:** each catalog entry can list an MLX build beside its GGUF file, the model converted to MLX at 4 bits, pinned to a fixed Hugging Face commit with every file's size and SHA-256, as GGUF files are. `carrel models download qwen3.5-4b --format mlx` fetches it through `carrel-fetch` into `models/mlx/<name>/`.
- **Only conversions it can vouch for:** from the `mlx-community` organization or converted by Carrel's own script, under the original model's license, and checked to load with the pinned library.
- **The benchmark compares like with like:** the same model in both formats, so the result says whether MLX is worth its extra download.

## Answers

- **Structured JSON:** llama.cpp constrains output with a grammar made from the JSON Schema, so every answer parses. MLX has no grammar sampler, but it lets a provider filter the next-token choices. The provider constrains output with a JSON-Schema token filter covering what Carrel's schemas use (objects, required keys, strings, enums, arrays and integers), so answers are always valid, as they are with llama.cpp.
- **Fallback:** a schema it can't constrain is answered unconstrained and validated; one that doesn't parse is error 3 (failed), and the pipeline retries as it does now.
- **Chat templates** come from the model's `tokenizer_config.json`, which `mlx-swift-lm` applies.
- **Token counts** use the model's own tokenizer, for the context budget (#96).

## Memory

- **Loaded on the first request,** as llama.cpp does, and kept for the rest of the batch.
- **Unloaded after five idle minutes.** The worker exits when its queue is empty, which stops its providers and returns the memory, so no unload message is needed; the idle unload covers a provider kept running longer, as the Mac app's agent will (v0.5). (The accepted design also had a `models/unload` notification; building it showed the worker's exit already does its job.)
- **Offered only with room:** the provider reports a model as available only when the Mac's memory is at least twice the model's size, so a 5 GB model needs a 16 GB Mac. Otherwise it's listed as unavailable with the reason, and llama.cpp or Apple's model is used.

## Choosing it

- **Priority 90:** native, but below Apple's model (100), which is shared and costs no memory. With both available, Apple's model is used unless the owner chooses otherwise.
- **The choice in settings:** a new device setting, `model` in `cli.json` (`apple`, or a catalog name with its format, such as `mlx/qwen3.5-4b`), shown in the Settings tab with each installed model's memory and benchmark score. It's tried first (`Registry::prefer`); if it isn't available, Carrel falls back to the next best rather than drafting nothing. (Accepted as "pins"; changed while building it, since a removed model file would otherwise stop all drafting.) The benchmark's recommendation is the default.

## Building it

1. `apple/CarrelMLX` with the provider, loading a model from a folder and answering `text/generate` and `text/count-tokens`.
2. Constrained JSON through `MLXGuidedGeneration`, checked against Carrel's card, route and rating schemas.
3. MLX entries in the catalog, `--format mlx` downloads, and the memory check.
4. Idle unloading.
5. The `model` setting, in the CLI and the Settings tab.
6. A benchmark run of the MLX builds, added to `docs/bench/`, and the release build.

## Decisions (accepted 2026-10-30)

1. **A separate program, `carrel-provider-mlx`, in its own Swift package built with `xcodebuild`,** rather than inside `carrel-provider-apple`. Recommended: MLX can't be built with plain `swift build`, and a separate process keeps the model's memory apart.
2. **`mlx-swift-lm`, pinned,** added only after you approve the download. Recommended.
3. **MLX builds as a second format of the existing catalog models,** pinned and checksummed like GGUF files. Recommended: the benchmark can compare formats directly.
4. **Constrained JSON through a token filter,** with validation and a retry as the fallback. Recommended: without it, open models produced invalid answers often enough to fail the bar.
5. **Priority 90, a `model` setting to choose, unloading after five idle minutes, and offered only with twice the model's size in memory.** Recommended.
