# CarrelProviders

Carrel providers built on Apple's frameworks, following the [provider protocol](../../docs/providers.md). Open models through MLX are in a package of their own, [CarrelMLX](../CarrelMLX/README.md).

- **`carrel-provider-apple`:** the program Carrel runs. Put it next to `carrel` or on `PATH`, and `carrel providers` lists it.
- **`CarrelAppleProviders`:** the library holding the providers themselves, so the Mac app can use them in-process too.

| Offer | Capability | Reads | Status |
| --- | --- | --- | --- |
| `pdfkit` | `extract-text` | PDFs with a text layer | Done |
| `vision` (Vision OCR) | `extract-text` | Images, and PDFs without a text layer | Done |
| `afm` (Foundation Models) | `text-model` | Prompts, with JSON Schema answers through guided generation; counts tokens on macOS 26.4 and later | Done |

PDFs without a text layer are reported as unsupported, so Carrel moves on to OCR: `pdfkit` is listed before `vision`, and a provider's offers of equal priority are tried in the order it lists them. Vision renders each PDF page at twice its size and reads lines top to bottom; pages are separated by a form feed.

The first OCR after building or installing a new copy of the provider can take about 40 seconds while macOS prepares Vision's models for it; after that, a page takes a fraction of a second.

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
