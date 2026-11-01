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
- **both:** the two merged as `carrel search` merges them, with the model's weights (`search::Merge`).

Each is scored by **top-five recall** (the share of descriptions whose item is among the first five items that come back) and **mean reciprocal rank** (1 for first place, ½ for second, and so on, 0 when it isn't in the first 100). The report lists every description that missed, with where its item came and what came back instead, so a miss can be read: a bad description, a card that says too little, or a model that doesn't understand.

The cards are titled by file name unless `--cards RESULTS` gives a `results.json` from a filing run, whose drafted titles and summaries are used instead; the tags, dates and sensitivity are the expected ones. Item text is read by the providers on the device, as search reads it, so the scores depend on what the device can read (images and PDFs need Apple's or the extraction provider).

```bash
carrel bench --search --out bench-results
```

It writes `bench-results/search/results.json` and `report.md`. For words alone and meaning alone, `results.json` keeps each description's whole ranking (`places`, up to 100, with each score and exact-match count), so how search merges them can be replayed and tuned without the models: `crates/carrel-core/tests/replay.rs` replays a run's rankings with each merge strategy. Runs before November 2026 kept only the first five, and the replay estimates the rest. The filed archive and each model's index are kept in `bench-results/search/archive`, so a second run embeds only what changed and says it reused the index; `--fresh` starts over, and `--model NAME` measures one embedding model.

### 2026-10-31

On DeskBook, an M2 Max MacBook Pro with 32 GB (the on-device runner), with Carrel from `dev` at 860cdf0, the embedding models through llama.cpp and Apple's NaturalLanguage. 230 descriptions of 81 items. [Full report](2026-10-31/findability/report.md) · [results.json](2026-10-31/findability/results.json) · [with drafted cards](2026-10-31/findability-drafted-cards/report.md)

| Ranking | Model | Top five | Mean reciprocal rank | Missed |
| --- | --- | --- | --- | --- |
| Words alone | none | 90% | 0.84 | 22 |
| **Meaning alone** | **EmbeddingGemma 300M** | **97%** | **0.92** | **6** |
| Words and meaning | EmbeddingGemma 300M | 94% | 0.88 | 13 |
| Meaning alone | Qwen3-Embedding 0.6B | 97% | 0.88 | 7 |
| Words and meaning | Qwen3-Embedding 0.6B | 92% | 0.88 | 19 |
| Meaning alone | Apple NaturalLanguage (English) | 39% | 0.28 | 141 |
| Words and meaning | Apple NaturalLanguage (English) | 84% | 0.62 | 37 |

- **EmbeddingGemma 300M stays the default.** It finds 97% of descriptions in the top five by meaning alone, ties Qwen3-Embedding at half its size (334 MB against 639 MB), and ranks the right item higher (0.92 against 0.88).
- **Words alone miss descriptions** that share no words with the card: "my résumé" for `cv alex morgan.md`, "which jabs I've had" for a vaccination record. Meaning finds those, which is what the embedding index is for.
- **Merging the two ranks lower than meaning alone** with either open model: 94% against 97% for EmbeddingGemma. Reciprocal rank fusion weighs a weak word match as much as a strong meaning match. Replayed with the merge from #249 (exact names and numbers first, then meaning, with the words breaking near-ties), the same rankings give 97% and 0.92 for EmbeddingGemma and 97% and 0.91 for Qwen3-Embedding, at least as good as meaning alone.
- **Apple's sentence embedding is too weak for search** on its own (39%), and drags the merge below words alone. It stays available to Apple-only archives but isn't recommended.
- **With drafted cards** (only the 9 items in the image run had them; the rest kept file names) every ranking does a point or two better, EmbeddingGemma reaching 98%.
- Embedding all 81 items took under a minute per model on the M2 Max.

After #251 changed the merge (exact names and numbers first, then words weighted by how strong the embedding model is), the same run on DeskBook at e8c0c1c gives [these results](2026-10-31/findability-merged/report.md) (the full rankings, for replaying merges, are in `results.json.gz`):

| Ranking | Model | Top five | Mean reciprocal rank | Missed |
| --- | --- | --- | --- | --- |
| Words alone | none | 91% | 0.85 | 21 |
| Meaning alone | EmbeddingGemma 300M | 97% | 0.92 | 6 |
| **Words and meaning** | **EmbeddingGemma 300M** | **98%** | **0.92** | **5** |
| Meaning alone | Qwen3-Embedding 0.6B | 97% | 0.88 | 7 |
| Words and meaning | Qwen3-Embedding 0.6B | 97% | 0.90 | 7 |
| Words and meaning | Apple NaturalLanguage (English) | 91% | 0.84 | 20 |

The merge now beats meaning alone with EmbeddingGemma, and with Apple's weak embedding it no longer falls below words alone (91%, up from 84%).

## Librarian

How well the librarian answers (`docs/design/librarian.md`, "Measuring it"). [`fixtures/bench/librarian.yaml`](../../fixtures/bench/librarian.yaml) holds 42 questions about the test set, written by hand the way the owner would ask ("which receipts are from 2025?", "what does the boiler note say to check weekly?"). Each lists the items that should come back and the facts the answer must state, each fact a phrase or a few phrases any one of which counts. It also lists, for each restricted item, text that must never show, such as the IBAN and the wifi password.

`carrel bench --librarian` files the test set into a copy of its archive, as `--search` does, then asks each text model every question in a conversation of its own. Each answer is scored by:

- **recall:** the share of the items that should come back that did, over every question;
- **precision:** the share of the items that came back that should have;
- **facts:** the share of facts the answers state, ignoring case and spacing;
- **restricted text shown:** answers whose text or passages contain a restricted item's listed text, or six words in a row from its text. It must be none;
- **steps, tokens and time:** the tools used, the tokens taken and the seconds per question, and the questions it stopped or failed on before answering.

```bash
carrel bench --librarian --out bench-results
```

It writes `bench-results/librarian/results.json` and `report.md`, with every answer beside what was expected. Each answer is saved to `librarian/runs/` as it's given, so a stopped run carries on; `--fresh` starts over, and `--model NAME` measures one model.

### The bar and the default model

A model is recommended for the librarian when it shows no restricted text, finds at least 80% of the items, at least 60% of what it returns is right, it states at least 80% of the facts, and it takes under 60 seconds a question on the reference machine. This is a starting point, as the filing bar was; the first runs may show it needs adjusting.

The catalog's `librarian` flag records the recommendation, as `recommended` does for filing. Once a model has it, the librarian runs by default on the largest recommended model installed that fits the device's memory, and otherwise, as now, on the largest installed open model that fits, then Apple's model. The `librarian.model` setting still overrides both. No model has the flag yet.

### Librarian: 2026-11-01

On DeskBook, an M2 Max MacBook Pro with 32 GB, with Carrel 0.5.1, one model at a time with nothing else compiling. [Full report](2026-11-01/librarian/report.md) · [results.json](2026-11-01/librarian/results.json)

| Model | Recall | Precision | Facts | Restricted shown | Unanswered | Steps | s/question | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Apple Foundation Model | 67% | 74% | 67% | 0 | 2 | 6.8 | 67.2 | no: recall, facts, too slow |
| Granite 4.2 3B (llama.cpp) | 54% | 92% | 72% | 0 | 4 | 3.9 | 36.0 | no: recall, facts |
| Qwen3.5 4B (llama.cpp) | 48% | 86% | 87% | 0 | 7 | 1.7 | 37.0 | no: recall |

No model meets the bar yet, so none has the `librarian` flag, and the default stays as it was: the largest installed open model that fits, then Apple's model. What held them back is mostly in the librarian, not the models (#356):

- **Answers cut off:** every one of Qwen's 7 unanswered questions, and all 4 of Granite's, stopped at the 900-token output limit before the answer ended. Qwen states the most facts when it does answer (87%).
- **Repeated tool calls:** Apple's model ran the same tool call twice in a row 122 times over 41 questions, and Granite 40 times, using up steps and time. One question searched "laptop cost warranty" seven times.
- **Context:** a listing question ("which receipts are from 2025?") was too long for Apple's on-device model.
- **No restricted text was shown** by any model.

### Librarian: after #361

The same run on DeskBook with `dev` after #361, which lets the librarian finish its answers (a larger output limit), returns an earlier result for a repeated tool call and trims tool results to fit a model's context. The test set now has 116 items, as #357 added 35. [Full report](2026-11-01-after-361/librarian/report.md) · [results.json](2026-11-01-after-361/librarian/results.json)

| Model | Recall | Precision | Facts | Restricted shown | Unanswered | Steps | s/question | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Apple Foundation Model | 88% (was 67%) | 67% | 65% | 0 | 1 | 3.5 | 108.9 | no: facts, too slow |
| Granite 4.2 3B (llama.cpp) | 60% (was 54%) | 93% | 70% | 0 | 4 | 2.4 | 53.9 | no: recall, facts |
| Qwen3.5 4B (llama.cpp) | 70% (was 48%) | 90% | 89% | 0 | 4 | 1.8 | 51.3 | no: recall |

- **Qwen3.5 4B is closest:** it meets every part of the bar but recall, 70% against 80%.
- **Cut-off answers:** Qwen's and Granite's four unanswered questions are all still answers cut off, now at the 2,400-token limit. Finishing those would likely bring Qwen to the bar.
- **Apple's model** finds the most (88%) but states the fewest facts, and at 109 seconds a question it's too slow. It takes fewer steps than before (3.5, from 6.8), but more tokens (35,000 a question, from 21,000).
- **No model showed restricted text.**

No model has the `librarian` flag yet, so the default is unchanged.

## Question value

Whether the question queue asks the questions that help most (`docs/design/librarian.md`, "Measuring it"). `carrel bench --questions` ingests the test set with each text model in one batch, as ingest would, and ranks the questions the batch raises as the queue does (`questions::rank`). It then answers each question alone with the answer the test set implies, and re-routes:

- **a collection, group or sender question** is answered with the collection most of its items are expected in, and its items go there, as answering does;
- **a term question** is answered with a glossary fact saying where items named for it belong, and its items are ingested again with that fact, so the model reads it as it would after a real answer;
- **an item question** doesn't change where anything goes, so it wins nothing.

Each answer's **gain** is the routing points it wins, scored as filing's routing is. The run reports:

- **routing before and after** answering every question;
- **top k:** the routing after answering the k highest-ranked questions, beside the best any k questions give;
- **rank correlation:** Spearman's, between what the ranking said each answer is worth and what it won. Near 1 means the ranking asks the most useful questions first.

```bash
carrel bench --questions --out bench-results
```

It writes `bench-results/questions/results.json` and `report.md`, with every question, its known answer, what moved and what it won. Each model's run is saved to `questions/runs/` and reused; `--fresh` starts over.

### Question value: 2026-11-01

On DeskBook, as above. [Report](2026-11-01/questions/report.md) · [results.json](2026-11-01/questions/results.json)

The batch of 81 items raised no questions with any of the three models, so there was nothing to rank. Routing was 79% with Apple's model, 72% with Granite 4.2 3B and 88% with Qwen3.5 4B. As expected, the test set's file names don't raise questions, so the next step is the handful of items whose names share a term or a sender (#357).

So 35 items were added for this benchmark (#357), each with its known answer in `answers.yaml`:

- **names sharing an unknown term:** two or three items each for ORCA, QPCR, ISA, DnD, LIGO, EHIC, BrewLog, CNC, PhD and OSM. These raise a term question whatever the model does;
- **a sender with no collection:** Calder Building Society and Mariner Travel, three or four items each, across collections;
- **a group that would make a collection:** four Allotment items;
- **uncertain placements:** three Larkspur Road items from an old flat, each fitting more than one collection.

The senders, the group and the uncertain placements raise a group question when the model splits them or isn't sure of them. The filing, findability and librarian benchmarks see the added items too, so their totals from runs after this aren't comparable with the 2026-11-01 runs item for item.

### Question value: after #357

On DeskBook, with the 35 items above. [Report](2026-11-01-after-361/questions/report.md) · [results.json](2026-11-01-after-361/questions/results.json)

| Model | Questions | Routing before | After all | Top 1 (best) | Top 3 (best) | Top 5 (best) | Rank correlation |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Apple Foundation Model | 14 | 73% | 77% | 75% (75%) | 75% (76%) | 76% (77%) | 0.58 |
| Granite 4.2 3B | 14 | 62% | 68% | 64% (64%) | 67% (67%) | 68% (69%) | 0.87 |
| Qwen3.5 4B | 14 | 82% | 84% | 84% (84%) | 84% (85%) | 84% (85%) | 0.48 |

- **The ranking asks the right questions first:** with each model, answering the top 1, 3 or 5 questions comes within a point of the best any that many could do.
- **Answers help most where routing is weakest:** answering every question adds 6 points for Granite, 4 for Apple's model and 2 for Qwen.
- **Rank correlation** is high for Granite (0.87) and moderate for the others. That's over only 14 questions, many worth the same, so the top-k results say more.

## Crawler

How well each model finds misplaced items, as the crawler would (`docs/design/restructure.md`, "Measuring it"). [`fixtures/bench/muddle.yaml`](../../fixtures/bench/muddle.yaml) muddles a copy of the test set's archive. Every item is filed where `answers.yaml` expects it, except:

- **a collection that's missing:** Recipes isn't there, and its 4 items wait in Home. The test archive's `min_collection_items` is 4, so a restructure proposal can gather them, and `carrel bench` checks that the missing and emptied collections have that many;
- **a collection that's overfilled:** Manuals' 6 items are dumped in Home, and Manuals is left there empty;
- **items scattered:** 8 single items filed in collections they have nothing to do with, such as the desk chair receipt in Reading.

That makes 18 misplaced items among 113 that count; Holding's own items don't, since nothing fits there. What `answers.yaml` expects is the intended shape, which the restructure benchmark will score against too.

`carrel bench --crawl` files the muddled archive into `crawl/archive` in the output folder, then has each text model judge every item there as the crawler does: whether it fits its collection's design document, and where it would go instead. Each model is scored by:

- **recall:** the share of misplaced items it says don't fit, overall and for each kind of muddle;
- **precision:** the share of the items it says don't fit that are misplaced. A false alarm is a finding the owner has to reject;
- **placement:** of the misplaced items whose collection is there (14 of the 18), the share it would move to an acceptable collection. An item it missed counts as not placed;
- **unusable answers and seconds per item.**

As in the crawler, the model is asked which collection the item belongs in, its current one among the choices, rather than whether it fits: the first runs showed small models judging "fits" against whichever collection they had in mind (#428). An item counts as flagged when the model names another collection, or none, and is at least 0.5 sure; a less sure misfit counts as fitting. The muddled archive's cards are summarized from `descriptions.yaml`, as drafted cards would be, so run with `--fresh` after upgrading from a version without summaries.

```bash
carrel bench --crawl --out bench-results
```

It writes `bench-results/crawl/results.json` and `report.md`, with what each model missed, put wrong and raised false alarms on. Each judgment is saved to `crawl/runs/` as it's given, so a stopped run carries on; `--fresh` starts over, `--model NAME` measures one model and `--limit N` judges the first N items.

### The bar and the crawler's default model

A model is recommended for the crawler when it finds at least 70% of the misplaced items, at least 60% of what it flags is misplaced, it would put at least 70% of what it finds in the right place, and it takes under 30 seconds an item on the reference machine. This is a starting point, as the other bars were.

The catalog's `crawler` flag records the recommendation. Once a model has it, the crawler judges with the largest recommended model installed that fits the device's memory, and otherwise, as now, with the librarian's choice: the strongest installed model that fits, then Apple's model. No model has the flag yet.

### Crawler: 2026-11-01

On DeskBook, an M2 Max with 32 GB, with `dev` after #431, one model at a time, on the muddled copy of the test set. [Report](2026-11-01-v0.7/crawl/report.md) · [results.json](2026-11-01-v0.7/crawl/results.json)

| Model | Found | Recall | Precision | Placed right | Unusable | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Qwen3.5 4B** (llama.cpp) | 17 of 18 | 94% | 68% | 86% | 0 | 5.9 | **yes** |
| Apple Foundation Model | 18 of 18 | 100% | 41% | 79% | 1 | 3.0 | no: precision |
| Granite 4.2 3B (llama.cpp) | 7 of 18 | 39% | 100% | 36% | 0 | 4.2 | no: recall, placement |

- **Qwen3.5 4B has the catalog's `crawler` flag,** so the crawler uses it by default wherever it fits.
- **The earlier runs** asked whether an item fits, and the models answered whether it fits *some* collection. They now say where it belongs instead, and say it briefly, so nothing is cut off (#405, #428).
- **Apple's model** finds every misplaced item but flags many that are fine. Granite rarely flags anything.

## Restructure

Whether a restructure proposal recovers what was muddled (`docs/design/restructure.md`, "Measuring it"). `carrel bench --restructure` builds the same muddled archive as `--crawl`, embeds its cards with the catalog's default embedding model when a provider here runs it, then has Carrel propose a restructure of the whole archive at each eagerness (settled, balanced and eager), once with numbers alone and once with each text model naming the groups and running the findability test. Each proposal is scored on:

- **the shape:** how closely the items that end up together match the intended shape, as the F1 of the pairs of items put in the same collection, before and after the proposal. It recovers the shape when the F1 rises. Scoring by pairs means a new `Cooking` collection recovers the missing Recipes as well as a `Recipes` would;
- **fixed and broken:** the misplaced items it would put in a collection `answers.yaml` accepts, or a subcollection of one, and the well-placed items it would move out of one;
- **moves:** the items it moves, and those moves weighted by each change's invasiveness, from 1 for rewriting to 5 for moving top-level collections, with the weighted moves for each item it fixes;
- **the findability gain:** in its own findability test, the held-out cards found in the proposed tree less those found in the current one, as a share of those tried. Numbers alone can't run the test.

```bash
carrel bench --restructure --out bench-results
```

It writes `bench-results/restructure/results.json` and `report.md`. Each proposal's score is saved to `restructure/runs/` as it's made, so a stopped run carries on; `--fresh` starts over, and `--model NAME` measures one text model beside numbers alone. Proposals are never applied: where each item would end up is worked out from the changes. The report lists each group a proposal held back, and why.

### Restructure: 2026-11-01

On DeskBook, as above, after #431. [Report](2026-11-01-v0.7/restructure/report.md) · [results.json](2026-11-01-v0.7/restructure/results.json)

**Nothing is proposed, for every wording at every eagerness, so the muddled archive's shape stays at 77%.** The grouping, on the cards' embeddinggemma-300m vectors and the tag and link graph, finds two candidates:

- a new collection of 4 items in Projects (gain -0.02);
- a new top-level collection of 4 items (gain -0.40).

Neither clears the margin, so both are held back. That's safe: an earlier run, before #431's findability check, proposed changes that lowered the shape to 68%. But it isn't useful yet. The real vectors don't show the groups the stub vectors in the tests do, which is #435.

## Running it again

```bash
carrel bench --out bench-results
```

To measure images without Apple's descriptions, run it with `CARREL_IMAGE_DESCRIPTIONS=off`.

It's resumable (`--fresh` starts over), shows progress, and `--compare` puts a run beside an earlier `results.json`. The on-device runner runs it weekly ([docs/dev/on-device-runner.md](../dev/on-device-runner.md)). Results change with the prompts, so record a new dated folder here when a release changes them.
