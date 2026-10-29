# CarrelProviders

Carrel providers built on Apple's frameworks, following the [provider protocol](../../docs/providers.md).

- **`carrel-provider-apple`:** the program Carrel runs. Put it next to `carrel` or on `PATH`, and `carrel providers` lists it.
- **`CarrelAppleProviders`:** the library holding the providers themselves, so the Mac app can use them in-process too.

| Offer | Capability | Reads | Status |
| --- | --- | --- | --- |
| `pdfkit` | `extract-text` | PDFs with a text layer | Done |
| Vision OCR | `extract-text` | Scans and images | [#22](https://github.com/HannahARose/carrel/issues/22) |
| `afm` (Foundation Models) | `text-model` | Prompts, with JSON Schema answers through guided generation | Done |
| MLX | `text-model` | | [#24](https://github.com/HannahARose/carrel/issues/24) |

PDFs without a text layer are reported as unsupported, so Carrel moves on to OCR.

`afm` is Apple's on-device model, never Private Cloud Compute. It needs macOS 26 or later with Apple Intelligence turned on; otherwise its offer is unavailable and says why. Prompts that are too long for its context, blocked by its guardrails, or in an unsupported language, and schemas guided generation can't express, are reported as unsupported, so Carrel falls back to another model. The schema subset it understands:

- objects with `properties`, `required` and `additionalProperties: false`
- arrays with `items`, `minItems` and `maxItems`
- strings, string `enum`s, integers, numbers and booleans
- a `description` on any of them

The live model test runs only where the model is available.

From this folder, run the tests:

```bash
swift test
```

Or build and install the provider:

```bash
swift build -c release --product carrel-provider-apple
```

```bash
cp .build/release/carrel-provider-apple ~/.cargo/bin/
```

It needs no Rust build: the provider speaks to Carrel only through the protocol.
