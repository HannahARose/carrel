# carrel-provider-llama

Carrel's portable text-model provider ([protocol](../../docs/providers.md)): open models run locally through [llama.cpp](https://github.com/ggml-org/llama.cpp), which is built in. It uses Metal on Macs; on Linux and Windows it runs on the CPU, or on the GPU with the `vulkan` or `cuda` feature.

## Models

Every `.gguf` file in Carrel's models folder is offered as a text model:

- `~/Library/Application Support/Carrel/models` on macOS, or `~/.config/carrel/models` elsewhere
- `CARREL_MODELS_DIR` overrides it

With no model there, `carrel providers` says where to put one. A command to download recommended models is [#99](https://github.com/Horizon-Labs/carrel/issues/99). A small instruct model such as Qwen2.5 0.5B Instruct (Q4_K_M, about 470 MB) is enough to try it.

- **Context:** 8,192 tokens unless `CARREL_LLAMA_CONTEXT` says otherwise. Prompts that don't fit, with room for the answer, are reported as unsupported, so Carrel can try another model.
- **Structured output:** answers that must follow a JSON Schema are constrained by a grammar, so they're always valid JSON.
- **Provenance:** answers are recorded as `llama.cpp/<file name>`.
- **Token counts** use the model's own tokenizer.

## Building

Building compiles llama.cpp, which needs `cmake` and a C++ compiler. On a Mac, install cmake:

```bash
brew install cmake
```

Then install the provider:

```bash
cargo install --path crates/carrel-provider-llama --locked
```

The tests need a model. They use whatever is in `CARREL_TEST_LLAMA_MODELS`; CI uses [stories260K.gguf](https://huggingface.co/ggml-org/models/blob/main/tinyllamas/stories260K.gguf), at 1 MB.
