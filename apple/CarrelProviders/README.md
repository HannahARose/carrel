# CarrelProviders

Carrel providers built on Apple's frameworks, following the [provider protocol](../../docs/providers.md).

- **`carrel-provider-apple`:** the program Carrel runs. Put it next to `carrel` or on `PATH`, and `carrel providers` lists it.
- **`CarrelAppleProviders`:** the library holding the providers themselves, so the Mac app can use them in-process too.

| Offer | Capability | Reads | Status |
| --- | --- | --- | --- |
| `pdfkit` | `extract-text` | PDFs with a text layer | Done |
| Vision OCR | `extract-text` | Scans and images | [#22](https://github.com/HannahARose/carrel/issues/22) |
| Foundation Models | `text-model` | | [#23](https://github.com/HannahARose/carrel/issues/23) |
| MLX | `text-model` | | [#24](https://github.com/HannahARose/carrel/issues/24) |

PDFs without a text layer are reported as unsupported, so Carrel moves on to OCR.

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
