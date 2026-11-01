# Carrel provider protocol

**Version 1 (draft).** A provider is a separate program that gives Carrel a capability it can't provide portably by itself: a text model, text extraction from PDFs and images, or an embedding model for search. Carrel starts providers, asks what they offer, and sends them requests. Native tools are used through providers where they exist (Apple's Foundation Models, PDFKit and Vision through `carrel-provider-apple`, and open models through MLX with `carrel-provider-mlx`), and portable providers fill in elsewhere. Anyone can write a provider in any language by following this document.

## Finding providers

A provider is an executable named `carrel-provider-<name>` (with `.exe` on Windows). Carrel looks in these places, in order, and uses the first one found for each name:

1. the folder containing the `carrel` program
2. in the Mac app, its bundle's `Contents/Helpers`
3. each folder on `PATH`
4. in the Mac app, which doesn't have the shell's `PATH` when it's opened from the Finder, the folders programs are usually installed in: `~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin` and `/usr/local/bin`

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
     "available": true, "describes": true, "media_types": ["image/png", "image/jpeg", "image/heic"]},
    {"capability": "extract-text", "id": "vision", "label": "Vision OCR", "priority": 100,
     "available": false, "reason": "OCR languages aren't installed",
     "media_types": ["image/png", "image/jpeg", "image/heic", "application/pdf"]},
    {"capability": "extract-text", "id": "model3d", "label": "3D model reader", "priority": 100,
     "available": true, "media_types": ["model/stl", "model/obj", "model/ply"]},
    {"capability": "embed", "id": "apple-nl-sentence-en", "label": "Apple sentence embedding (English)",
     "priority": 100, "available": true, "dimensions": 512, "context_tokens": 256}
  ]
}}
```

`protocol` is the protocol version the provider speaks. Carrel doesn't use providers that speak a different version.

Each **offer** is one thing the provider can do:

| Field | Meaning |
| --- | --- |
| `capability` | `text-model`, `extract-text` or `embed` |
| `id` | Identifies the offer within the provider, for example a model name. For `embed`, it's the model's identifier, the same on every provider that runs the model (see [Embedding models](#embedding-models-textembed)). |
| `label` | Optional; a name for people |
| `priority` | Which offer Carrel prefers when several can do the same thing; higher wins. Native tools use 100, portable providers 50, built-in fallbacks 10. A choice in settings overrides it. Offers of equal priority from one provider are tried in the order it lists them, so list the most exact first (a PDF's text layer before OCR). |
| `available` | Whether it can be used now. An offer that's unavailable says why in `reason`, such as "Apple Intelligence is turned off", so Carrel can tell the owner. |
| `media_types` | For `extract-text`: the media types it reads. Missing means any. |
| `describes` | For `extract-text`: `true` when it describes what images show, in a `Shows:` line of its text. Carrel gives such offers pictures to describe (see "Pictures to describe"). Say it only when a description can be written now. |
| `context_tokens`, `structured_output` | For `text-model`: the context size, and whether it can return JSON that follows a schema |
| `dimensions`, `context_tokens` | For `embed`: how many numbers each vector has, and the longest text it reads, in tokens |

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

`model` names the exact model and version, recorded in the card's provenance. `json` is present only when a schema was given, and `usage` is optional. The librarian (`docs/design/librarian.md`) works through this method alone: each step is one request whose schema lets the model name a tool or give its answer, so a provider needs nothing more to run it. It counts `usage` against its limit of tokens per answer when a provider gives it, and estimates otherwise.

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

### Pictures to describe

A provider that finds or draws pictures it can't describe itself, such as a 3MF's thumbnail or drawings of a CAD model, may return them in `pictures`:

```json
{"jsonrpc": "2.0", "id": 5, "result": {"text": "3D print project (3MF)\nTitle: Keychain", "method": "embedded",
  "pictures": [{"path": "/var/folders/…/T/carrel-picture-a1b2.png",
                "context": "the thumbnail a slicer drew of a 3D print project"}]}}
```

- Write each picture as a PNG or JPEG in the system's temporary folder (`TMPDIR`). Carrel ignores pictures anywhere else, and never deletes them.
- `context` says what the picture is. It's all the image reader is told: Carrel doesn't pass the original file's name, since a reader given the name tends to repeat it instead of looking.
- Carrel sends each picture to the best offer that `describes`, with the context in `text/extract`'s `context` parameter, adds the `Shows:` line of its answer to the text, and deletes the picture. With no such offer, the pictures are deleted unread.

`context` is optional in any `text/extract` request; offers that don't describe images ignore it.

## Embedding models: `text/embed`

Search compares vectors, and vectors from different models can't be compared, so an archive has **one embedding model**, shared by every device and recorded in its settings (`embedding.model`, FORMAT.md §7.2). Carrel only ever embeds with offers whose `id` is that model's identifier, such as `embeddinggemma-300m` or `qwen3-embedding-0.6b` from Carrel's model catalog; it never falls back to another model. A provider that runs a catalog model must offer it under the catalog's name, and use it the way the catalog says (the prompts and pooling in `crates/carrel-core/src/models.rs`), so its vectors match every other provider's.

```json
{"jsonrpc": "2.0", "id": 5, "method": "text/embed", "params": {
  "offer": "embeddinggemma-300m",
  "texts": ["Tenancy agreement for the flat on Cowley Road", "Receipt for a desk chair"],
  "purpose": "document"
}}
```

`purpose` is `document` for text to be found (the default) or `query` for a search query, which some models embed differently: the provider adds the model's own prompt for each. Each text is embedded on its own; a text longer than the model's context is cut.

```json
{"jsonrpc": "2.0", "id": 5, "result": {
  "vectors": [[0.0213, -0.0871, ...], [0.0442, 0.0105, ...]],
  "model": "embeddinggemma-300m",
  "dimensions": 768
}}
```

`vectors` has one vector for each text, in order, each of unit length and `dimensions` numbers long. `model` is the model's identifier, as in the offer. Carrel checks all three, and treats an answer that doesn't match as failed (code 3).

### Carrel's embedding models

`carrel models` lists them with the text models. An archive chooses one when it's created (`carrel init --embedding-model`), or on first use.

| Model | Dimensions | Context | Provided by |
| --- | --- | --- | --- |
| `embeddinggemma-300m` (the default) | 768 | 2,048 tokens | llama.cpp (priority 50) and MLX (90), once downloaded |
| `qwen3-embedding-0.6b` | 1,024 | 8,192 tokens | llama.cpp and MLX, once downloaded |
| `apple-nl-sentence-en` | 512 | 256 words | `carrel-provider-apple` (100), built into macOS |

- **The same model gives the same vectors on every provider.** A query embedded through MLX on a Mac can be compared with an index built through llama.cpp on Linux. Each provider uses the catalog's prompts and pooling, and runs EmbeddingGemma reading in both directions, as it was trained: the MLX provider has its own EmbeddingGemma for this, since MLXEmbedders' reads one way only, as Gemma 3 generates text.
- **The MLX provider** offers a folder in `mlx/` named after a catalog embedding model (`carrel models download embeddinggemma-300m --format mlx`) as `embed`, never as a text model.
- **Apple's sentence embedding** needs no download, but only Apple devices have it, and only for English. An archive also searched from Linux or Windows couldn't use it, so it's never the default: an archive chooses it. It's pinned to revision 1 of macOS's English sentence embedding; a later revision would be another model, since its vectors couldn't be compared. It reads up to 256 words of each text.

### How search uses it

- **Documents:** for each item, Carrel sends one request with the card as the first text (its title, summary, tags, collection, dates and kind, a line each), then the item's text in passages of about 500 tokens, up to 20, all with `purpose: document`. A restricted item's request has its card only. The vectors are kept in `_System/index/<model>/` as 16-bit floats.
- **Queries:** what's typed into search, without its filters, is sent alone with `purpose: query`, and compared with every stored vector by cosine similarity.
- **When it fails:** an error, or an answer that doesn't check out, leaves the item behind in the index, to be tried again by the worker; a query that can't be embedded is searched by words only, and search says why.

### Measuring a model

`carrel bench --search` looks up the benchmark's hand-written descriptions (`fixtures/bench/descriptions.yaml`) with each embedding model on offer, and reports how often each item comes back in the top five ([docs/bench](bench/README.md#findability)). A provider offering a new model can be measured the same way before it's proposed for the catalog.

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

`crates/carrel-core/examples/echo_provider.rs` is a complete provider in about 100 lines of Rust, used by Carrel's own tests. It answers `text/complete` by echoing the prompt, `text/extract` by reading plain text, and `text/embed` with small vectors counting each text's letters.
