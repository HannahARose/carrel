# Carrel provider protocol

**Version 1 (draft).** A provider is a separate program that gives Carrel a capability it can't provide portably by itself: a text model, or text extraction from PDFs and images. Carrel starts providers, asks what they offer, and sends them requests. Native tools are used through providers where they exist (Apple's Foundation Models, PDFKit and Vision through `carrel-provider-apple`, and open models through MLX with `carrel-provider-mlx`), and portable providers fill in elsewhere. Anyone can write a provider in any language by following this document.

## Finding providers

A provider is an executable named `carrel-provider-<name>` (with `.exe` on Windows). Carrel looks in these places, in order, and uses the first one found for each name:

1. the folder containing the `carrel` program
2. each folder on `PATH`

`<name>` identifies the provider in settings and in the provenance Carrel records on cards (FORMAT.md §5.6).

Carrel also has built-in providers, such as plain-text extraction, which need no program.

## Transport

- Carrel starts the provider with no arguments and talks to it over standard input and output, using [JSON-RPC 2.0](https://www.jsonrpc.org/specification).
- Each message is one line of UTF-8 JSON ending in `\n`. JSON escapes newlines inside strings, so a message never spans lines.
- Carrel sends one request at a time and waits for its response.
- Standard error is for the provider's own log. Carrel keeps the last lines and shows them when a provider fails.
- A provider exits when its standard input closes.

## Lifecycle

1. Carrel sends `initialize`. The provider must answer within 10 seconds.
2. Carrel sends requests for what the provider offered.
3. Carrel sends `shutdown`, waits for the response, and closes standard input.

### `initialize`

Request:

```json
{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocol": 1, "client": "carrel 0.2.0"}}
```

Response:

```json
{"jsonrpc": "2.0", "id": 1, "result": {
  "protocol": 1,
  "name": "Apple",
  "version": "0.2.0",
  "offers": [
    {"capability": "text-model", "id": "afm", "label": "Apple Foundation Model", "priority": 100,
     "available": true, "context_tokens": 4096, "structured_output": true},
    {"capability": "extract-text", "id": "pdfkit", "label": "PDFKit", "priority": 100,
     "available": true, "media_types": ["application/pdf"]},
    {"capability": "extract-text", "id": "image", "label": "Image reader", "priority": 100,
     "available": true, "media_types": ["image/png", "image/jpeg", "image/heic"]},
    {"capability": "extract-text", "id": "vision", "label": "Vision OCR", "priority": 100,
     "available": false, "reason": "OCR languages aren't installed",
     "media_types": ["image/png", "image/jpeg", "image/heic", "application/pdf"]},
    {"capability": "extract-text", "id": "model3d", "label": "3D model reader", "priority": 100,
     "available": true, "media_types": ["model/stl", "model/obj", "model/ply"]}
  ]
}}
```

`protocol` is the protocol version the provider speaks. Carrel doesn't use providers that speak a different version.

Each **offer** is one thing the provider can do:

| Field | Meaning |
| --- | --- |
| `capability` | `text-model` or `extract-text` |
| `id` | Identifies the offer within the provider, for example a model name |
| `label` | Optional; a name for people |
| `priority` | Which offer Carrel prefers when several can do the same thing; higher wins. Native tools use 100, portable providers 50, built-in fallbacks 10. A choice in settings overrides it. Offers of equal priority from one provider are tried in the order it lists them, so list the most exact first (a PDF's text layer before OCR). |
| `available` | Whether it can be used now. An offer that's unavailable says why in `reason`, such as "Apple Intelligence is turned off", so Carrel can tell the owner. |
| `media_types` | For `extract-text`: the media types it reads. Missing means any. |
| `context_tokens`, `structured_output` | For `text-model`: the context size, and whether it can return JSON that follows a schema |

Other fields are allowed and kept.

### `shutdown`

Request `{"jsonrpc": "2.0", "id": 9, "method": "shutdown"}`; the result is `null`. The provider then exits when standard input closes.

## Text models: `text/complete`

```json
{"jsonrpc": "2.0", "id": 2, "method": "text/complete", "params": {
  "offer": "afm",
  "system": "You write catalog cards for a personal archive.",
  "prompt": "Summarize this document in one sentence: ...",
  "schema": {"type": "object", "properties": {"summary": {"type": "string"}}, "required": ["summary"]},
  "max_tokens": 200
}}
```

`system`, `schema` and `max_tokens` are optional. With a `schema`, the provider returns JSON that follows it, as well as the text.

```json
{"jsonrpc": "2.0", "id": 2, "result": {
  "text": "{\"summary\": \"A note on coupled oscillators.\"}",
  "json": {"summary": "A note on coupled oscillators."},
  "model": "apple/afm-3-core",
  "usage": {"input_tokens": 812, "output_tokens": 14}
}}
```

`model` names the exact model and version, recorded in the card's provenance. `json` is present only when a schema was given, and `usage` is optional.

### Counting tokens: `text/count-tokens`

Optional. Carrel uses it to split long documents into pieces that fit a model's context.

```json
{"jsonrpc": "2.0", "id": 4, "method": "text/count-tokens", "params": {"offer": "afm", "text": "..."}}
```

```json
{"jsonrpc": "2.0", "id": 4, "result": {"tokens": 812}}
```

A provider that can't count answers with code 2 or `-32601`, and Carrel estimates instead, conservatively: one token for every three bytes of UTF-8.

## Text extraction: `text/extract`

```json
{"jsonrpc": "2.0", "id": 3, "method": "text/extract", "params": {
  "offer": "pdfkit",
  "path": "/Users/me/Archive/Reading/paper.pdf",
  "media_type": "application/pdf"
}}
```

`path` is absolute, and the provider reads the file itself. Providers must never change the file.

```json
{"jsonrpc": "2.0", "id": 3, "result": {"text": "...", "method": "embedded", "pages": 12}}
```

`method` is `embedded` for text stored in the file, `ocr` for recognized text, `described` when a model also described an image, `geometry` for a 3D model's size and shape, or `rendered` when a model also described drawings of it. `pages` is optional.

Two more fields are optional, for what a file records about itself:

- `created`: when it was made, such as a photo's date taken, as `YYYY-MM-DD` or a time with its offset (`2026-06-24T16:28:05+02:00`). Carrel uses it for the card's created date when the model gives none. Leave it out when the file doesn't say; for a local time with no offset, give only the date.
- `located`: `true` when the file records where it was made, such as a photo's location. Carrel rates the item at least `personal`. Never put the place itself in `text`.

## Errors

Providers report failures as JSON-RPC errors. Besides the standard codes (such as `-32601` for an unknown method), these codes tell Carrel what to do next:

| Code | Meaning | What Carrel does |
| --- | --- | --- |
| `1` | **Unavailable:** the offer can't be used right now | Tries the next provider |
| `2` | **Unsupported:** this input isn't one the offer handles | Tries the next provider |
| `3` | **Failed:** the offer tried and couldn't | Tries the next provider, and reports the error if none succeeds |
| `4` | **Declined:** the model refused this input, as a safety refusal | Tries the next provider, and shows the owner that the model declined it |
| `-32601` | **Method not found:** the provider is older than the method, or doesn't offer it | Treats it as unsupported; the provider stays in use |

`message` says what went wrong, in plain words.

## Privacy

Providers are local programs and run with the owner's permissions. A provider that sends anything off the device is a remote tier (see the wiki's Models and Privacy page) and must say so in its offer's `label`. Remote providers aren't used in v1.

## Writing a provider

`crates/carrel-core/examples/echo_provider.rs` is a complete provider in about 100 lines of Rust, used by Carrel's own tests. It answers `text/complete` by echoing the prompt and `text/extract` by reading plain text.
