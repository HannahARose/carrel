# Model benchmark results

How each text model does at Carrel's work: drafting a card, choosing a collection, and rating sensitivity, on the benchmark's test set of invented documents and images with known answers (`fixtures/bench`). The design, the scoring and the bar are in [docs/design/benchmark.md](../design/benchmark.md). Each run's full report, with every item's draft, is kept in a folder named by its date.

## 2026-10-30

On the reference machine, an M4 Max MacBook Pro with 36 GB, with Carrel 0.1.0 from `dev` after #146, one model at a time with nothing else compiling. [Full report](2026-10-30/report.md) · [results.json](2026-10-30/results.json)

| Model | Size | Valid | Routing | Sensitivity exact | Under-rated | Dates | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Gemma 4 E4B** (llama.cpp) | 5.2 GB | 100% | 94% | 83% | 2% | 58% | 17.9 | **yes** |
| **Qwen3.5 4B** (llama.cpp) | 2.7 GB | 97% | 87% | 95% | 2% | 63% | 21.0 | **yes** |
| Qwen3.5 9B (llama.cpp) | 5.7 GB | 100% | 96% | 72% | 2% | 69% | 33.2 | no: too slow |
| Granite 4.2 8B (llama.cpp) | 5.4 GB | 95% | 80% | 85% | 3% | 45% | 32.3 | no: too slow |
| Apple Foundation Model | shared | 89% | 75% | 83% | 3% | 77% | 6.9 | no: declined 11%, routing |
| Granite 4.2 3B (llama.cpp) | 2.2 GB | 86% | 72% | 78% | 2% | 62% | 18.3 | no: valid, routing |

The bar: valid output for at least 95% of items, routing at least 80%, no restricted item under-rated and at most 5% under-rated, under 30 seconds an item. No model under-rated a restricted item.

### Recommended

| Memory | Model | Why |
| --- | --- | --- |
| 8 GB | `qwen3.5-4b` | The smallest that meets the bar; the most exact sensitivity ratings (95%) |
| 16 GB or more | `gemma-4-e4b` | Every answer valid on the first try, the best routing that's fast enough (94%), and the fastest open model |
| A Mac with Apple Intelligence and 16 GB | Apple's model, with `gemma-4-e4b` installed | Apple's model drafts first and Gemma gives a second opinion (#168): it meets the bar at 9.7 s an item |

`carrel models` marks them, and `carrel models download NAME` fetches one.

### What the numbers say

- **Apple's on-device model** is the fastest by far and needs no download, but it declined 7 of the 65 items, 6 of them health documents its guardrails refuse. Left out of the count, its answers were all valid, but its routing (77%) is still below the bar. In use, Carrel passes a declined item to the next model, so on a Mac with Apple Intelligence it works best with a recommended open model installed beside it, as the second opinion below shows.
- **Larger isn't better at the bar's speed:** Qwen3.5 9B routes best (96%), but at 33 seconds an item a batch of 200 takes almost two hours, and its sensitivity ratings were the least exact (72%, mostly rating up, which is the safe direction).
- **Dates are the weak spot** for every model (45–77% right), since many documents give several dates; the owner sees the date in review.
- **Extra tags** (tags proposed outside the vocabulary) were highest for Apple's model and Granite 3B, which is why their tag scores look fine while their proposals are noisier.

### Apple's model with a second opinion

Apple's model drafts, routes and rates every item, and Gemma 4 E4B is asked again for what it would send to Holding or place with low confidence, and for anything it declines (#168). [Report](2026-10-30/apple-then-gemma/report.md)

| | Valid | Routing | Sensitivity exact | Dates | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- |
| Apple's model alone | 89% | 75% | 83% | 77% | 6.9 | no |
| Gemma 4 E4B alone | 100% | 94% | 83% | 58% | 17.9 | yes |
| **Apple, then Gemma** | **100%** | **89%** | **89%** | **74%** | **9.7** | **yes** |

Nearly Gemma's accuracy at about half its time, and Apple's model's better dates. An earlier run of the same pair routed 92%, so a few points either way is normal.

### MLX

The same Granite 4.2 3B through `carrel-provider-mlx` (IBM's 4-bit MLX build), after the fixes in #164. [Report](2026-10-30/mlx/report.md)

| Granite 4.2 3B | Valid | Routing | Dates | s/item |
| --- | --- | --- | --- | --- |
| llama.cpp (Q4_K_M) | 86% | 72% | 62% | 18.3 |
| MLX (4-bit) | 100% | 36% | 54% | 10.8 |

MLX is about 40% faster and every answer was valid, but it routes much worse: its second routing pass, with the collections in the other order, rates good choices as poor fits, so most items go to Holding. The likeliest cause is the quantization (plain 4-bit against llama.cpp's mixed Q4_K_M); the 8-bit build would tell. Until then no MLX build is recommended.

### Not measured yet

- **A larger model for 32 GB Macs,** as the design planned, needs a download of about 20 GB, which hasn't been asked for.

## 2026-10-31: images, with and without descriptions

Does Apple's description of an image (#179) help file it? Nine invented images were added to the test set, drawn by [`fixtures/bench/make-images.swift`](../../fixtures/bench/make-images.swift) with the metadata a phone, the screenshot tool or a scanner writes: a till receipt, a whiteboard, a cat on a sofa, a departure board with a location, a handwritten recipe card, a mail screenshot, a banking-app screenshot, a scanned letter and a wiring diagram. Each image was read by the Apple provider's `image` offer with Apple's description and again without it (`CARREL_IMAGE_DESCRIPTIONS=off`), so the only difference is the `Shows:` line. Metadata, Vision's labels and OCR were there both times.

On the reference machine (M4 Max, 36 GB, macOS 27.2), with Carrel 0.2.0 from `dev` after #198, the nine images alone, each setup run three times with nothing else compiling. The figures are the averages of the three runs. Reports: [Apple, then Gemma](2026-10-31/apple-then-gemma/report.md) ([without](2026-10-31/apple-then-gemma-no-descriptions/report.md)) · [Apple's model](2026-10-31/apple/report.md) ([without](2026-10-31/apple-no-descriptions/report.md)) · [Gemma 4 E4B](2026-10-31/gemma/report.md) ([without](2026-10-31/gemma-no-descriptions/report.md))

| Setup | Descriptions | Routing | Sensitivity exact | Dates | Tags found | Extra tags | s/item |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Apple, then Gemma** (recommended) | with | 100% | 44% | 100% | 71% | 12.0 | 15.0 |
| | without | 100% | 30% | 100% | 67% | 12.7 | 12.4 |
| Apple's model alone | with | **85%** | 52% | 100% | 79% | 11.3 | 9.6 |
| | without | 63% | 33% | 100% | 62% | 12.7 | 7.1 |
| Gemma 4 E4B alone | with | 100% | 100% | 93% | 83% | 1.7 | 21.5 |
| | without | 100% | 100% | 100% | 75% | 3.0 | 19.8 |

Every answer was valid, nothing was declined, and nothing was under-rated.

- **Descriptions help the model that's weakest at routing.** Apple's model alone routed 85% of the images with descriptions and 63% without. Without them it sent the whiteboard, the rent-review screenshot and the diagram to Holding every time (or the diagram to Manuals), and the letter once; with them, the screenshot and the letter were filed every time and the diagram two times in three. The whiteboard went to Holding every time either way. With the recommended pair, routing was already 100% either way: what Apple's model sends to Holding, Gemma places from the OCR text.
- **Text-heavy images file by their text.** The receipt, the departure board, the recipe card and the bank screenshot were filed correctly by every setup either way. The description matters most for pictures with little or no text, and for titles: the cat photo was "Photo on iPhone" or "Photo of location" without a description and "Illustration of a cat" with one (it is a drawing).
- **Dates come from the metadata**, or, for the scanned letter, from its text, and were right in almost every run. The one miss: Gemma left the letter's date empty in two of its three runs with descriptions.
- **Sensitivity was over-rated by Apple's model**, which rated most images `sensitive` (a receipt "contains payment details", a whiteboard has "security and operational implications"). The pair keeps Apple's ratings, so it shares this. It's the safe direction, and descriptions made it better (30% to 44% exact with the pair). Gemma rated every image exactly. The located photos were given "has the place it was taken" every time, and stayed `personal`.
- **Descriptions cost about 2.5 seconds an image**, the time Apple's model takes to look at it.
- **Gemma's first card for a camera-named image** usually failed the check that a title isn't just the file name (`IMG_4127.jpg`), and needed a retry, hence its low first-try rate in these reports.

So descriptions are worth keeping on: they cost little, they help when Apple's model files alone or Gemma isn't installed, and they give photos titles and summaries that say what's in them.

## Findability

How well search finds things (`docs/design/search.md`, "Measuring it"). Each item in the test set has two to four descriptions in [`fixtures/bench/descriptions.yaml`](../../fixtures/bench/descriptions.yaml), written by hand the way a person would search for it months later: what it is ("the lease for my flat"), what it's for ("proof I bought my new computer, for the warranty"), or a detail inside it ("booking reference K7Q2PX"), never its file name. `carrel bench --search` files every item in its expected collection, then looks up each description three ways:

- **words alone:** the word index, over the cards and the item's text;
- **meaning alone:** the embedding index, with each embedding model on offer;
- **both:** the two merged by reciprocal rank fusion, as `carrel search` does.

Each is scored by **top-five recall** (the share of descriptions whose item is among the first five items that come back) and **mean reciprocal rank** (1 for first place, ½ for second, and so on, 0 when it isn't in the first 100). The report lists every description that missed, with where its item came and what came back instead, so a miss can be read: a bad description, a card that says too little, or a model that doesn't understand.

The cards are titled by file name unless `--cards RESULTS` gives a `results.json` from a filing run, whose drafted titles and summaries are used instead; the tags, dates and sensitivity are the expected ones. Item text is read by the providers on the device, as search reads it, so the scores depend on what the device can read (images and PDFs need Apple's or the extraction provider).

```bash
carrel bench --search --out bench-results
```

It writes `bench-results/search/results.json` and `report.md`. The filed archive and each model's index are kept in `bench-results/search/archive`, so a second run embeds only what changed and says it reused the index; `--fresh` starts over, and `--model NAME` measures one embedding model. Not run yet: it needs the embedding models downloaded on the reference machine. The first run confirms, or not, EmbeddingGemma 300M as the default.

## Running it again

```bash
carrel bench --out bench-results
```

To measure images without Apple's descriptions, run it with `CARREL_IMAGE_DESCRIPTIONS=off`.

It's resumable (`--fresh` starts over), shows progress, and `--compare` puts a run beside an earlier `results.json`. The on-device runner runs it weekly ([docs/dev/on-device-runner.md](../dev/on-device-runner.md)). Results change with the prompts, so record a new dated folder here when a release changes them.
