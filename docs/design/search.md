# Design: explorer and search (v0.4)

Status: accepted 2026-10-31, with the five recommendations below. Issue #5; built in #209, #210, #211, #212, #213 and #214, then #136.

## What this adds

So far an item is found by knowing where it was filed. v0.4 adds the other way: describing it. "The lease for the flat on Cowley Road", "that paper about light-sheet microscopes", "the receipt for the desk chair", in your own words, and Carrel finds it, whether or not those words are in its title. Alongside search, browsing gets the views that make an archive pleasant to explore: an item's history, what's related to it, and what changed recently.

**The gate:** the owner can find any filed item by browsing or searching.

The spec settles the shape: **one embedding model per archive**, shared by every device and chosen when the archive is created, with an index in `_System/index/` tagged with the model (FORMAT.md §7.11, the `embedding.model` setting). Changing the model later re-embeds the archive in the background while the old index keeps serving. This design fills in the rest.

## Embeddings come from providers

- **A new provider capability, `embed`** (`docs/providers.md`): given texts, it returns one vector each, with the model's identifier and dimensions. Like text models, Carrel picks an available offer, but here it must be **the archive's model**: a query embedded with a different model can't be compared with the index.
- **The default model is EmbeddingGemma 300M** for new archives: small (about 200 MB quantized), multilingual, and it runs everywhere through llama.cpp (`carrel-provider-llama`) and on Apple silicon through MLX. Qwen3-Embedding 0.6B is the larger alternative. `carrel models` lists them like text models and downloads one only when asked.
- **Apple's NaturalLanguage embeddings aren't the default,** though they need no download: they exist only on Apple devices, and a Linux or Windows device couldn't search the same archive. An archive used only on Apple devices can choose them.
- **Existing archives** have no model yet; the first `carrel index` asks which to use and records it in `settings.json`.

## What's embedded

- **Each item's card:** title, summary, tags, collection, dates and kind, as one text. Cards are short and say what the item is, so they find most things.
- **The item's own text, in chunks:** the text drafting read (PDF text, OCR, an image's description, a folder's listing), cut into passages of about 500 tokens, up to a limit per item (about 20 passages), so a long document is found by what's inside it, not just its summary.
- **Restricted items: the card only.** Their text isn't embedded, so card numbers or passwords can never be found, or reconstructed, from the index. Sensitive items are embedded like any other; the index stays on the owner's devices.
- **What isn't embedded:** history, glossary and collection documents. They're searched by words (below), not meaning.

## The index

- **Plain files in `_System/index/`,** one folder per model and version: the vectors (16-bit floats), what each vector belongs to (item, card or passage, and where), and each item's card checksum when it was embedded, so stale entries are found by comparing.
- **Searched by brute force.** A 10,000-item archive with 20 passages each is 200,000 vectors, about 300 MB at 768 dimensions, and comparing a query against all of them takes well under a second on a laptop. No vector database to install or keep in sync. An approximate index (HNSW) can come later, behind the same interface, if archives outgrow this.
- **Regenerable, so left out** of history (FORMAT.md §8.1 already excludes `index/`) and of backups, like `cache.nosync/`.

## Search

- **Words and meaning together.** A word index (BM25 over cards, item text, collection documents and the glossary) catches exact names, numbers and codes that embeddings blur ("invoice 2057-4038", "AS1115"); the embedding index catches descriptions. Their rankings are merged (reciprocal rank fusion), so either can surface an item.
- **Filters** narrow it: `collection:Finances`, `tag:receipts`, `kind:folder`, `after:2025-01`, `before:2026`, `sensitivity:sensitive`, `checked-out`.
- **Each result says why:** the title and collection, the passage that matched with the words highlighted, and the date.
- **Where:** `carrel search QUERY [--json]` on the command line, and `/` in Browse, which today only matches words in titles and summaries. Both use the same search.
- **Sensitive and restricted items** appear in results like any other, marked, since the owner is the only one searching. A restricted item is found by its card, never by its text.

## Keeping it current

- **The worker does the embedding,** in the background, after anything that changes an item: filing a batch, a correction, an edit, a refile, a return. The change itself never waits for the model. Until the worker catches up, search uses the card's words, and the result says the meaning index is behind.
- **`carrel index status`** says how many items are embedded and how many are behind; **`carrel index rebuild`** re-embeds everything, resumably, with a progress bar; **`carrel index model NAME`** switches models, building the new index while the old one keeps serving.

## Browsing

Search finds an item; browsing shows its surroundings. In the interactive mode's Browse and in `carrel show`:

- **History:** every change that touched the item, from filing to its latest correction or return, with undo from there.
- **Related:** the items nearest to it in the embedding index, and its links (`version-of`, `related`), so a paper leads to its notes and an invoice to its receipt.
- **Recent:** what was filed, changed or worked on lately, archive-wide, on the Status screen.

## Measuring it

- **A findability test** in the benchmark: for each item in the test archive, a few ways a person might describe it, written by hand (not from its title), scored by whether the item comes back in the top five. It compares words alone, meaning alone and both, and the candidate models.
- **The gate** uses the owner's real archive: they look for twenty items they remember, in their own words, and note which were found, and where in the results.

## Building it

1. The `embed` capability: protocol, the llama.cpp provider (EmbeddingGemma and Qwen3-Embedding GGUFs in the model catalog), and the archive's model setting.
2. The index: chunking, files, stale detection, `carrel index status|rebuild`, and the worker embedding after changes.
3. The word index and hybrid search: `carrel search` with filters and reasons.
4. Browse: search on `/`, the item's history and related items, and Recent on Status.
5. The MLX and Apple embedding offers, and `carrel index model` to switch.
6. The findability benchmark, then the user guide and `docs/providers.md`.
7. Neovim and Emacs (#136), driving `carrel search --json`, after the rest.

## Decisions (accepted 2026-10-31)

1. **EmbeddingGemma 300M is the default model for new archives, through a new `embed` provider capability,** with Qwen3-Embedding 0.6B as the alternative and Apple's embeddings available to Apple-only archives. Recommended: it's small, runs on every platform the CLI does, and keeps one archive searchable from every device; the findability benchmark confirms the choice before it's final.
2. **Embed each card and the first ~20 passages of each item's text; restricted items get their card only.** Recommended: cards find most things, passages find what's inside long documents, and nothing restricted can be recovered from the index.
3. **The index is plain files in `_System/index/`, searched by brute force,** regenerable and kept out of history and backups. Recommended: no database to install or corrupt, fast enough for archives of tens of thousands of items, and replaceable behind the same interface if they grow larger.
4. **Search combines a word index and the embedding index, with filters and a reason for each result,** in `carrel search` and on `/` in Browse. Recommended: exact names and numbers need words, descriptions need meaning, and the owner should see why something came back.
5. **The worker embeds in the background after every change, and `carrel index` reports, rebuilds and switches models.** Recommended: filing and correcting never wait for a model, the index catches up within moments, and switching models never leaves the archive unsearchable.
