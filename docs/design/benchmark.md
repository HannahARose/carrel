# Design: the model benchmark (v0.2)

Status: accepted 2026-10-30, with the five recommendations below. Issue #33.

## What it's for

Ingest asks a text model for three things: a card draft, a destination, and a sensitivity rating. Which model is good enough for each is a measurement, not a guess. Earlier tries showed why: Qwen 0.5B wrote unusable cards, and Apple's model misfiled 3 of 4 items when it saw only folder names. The benchmark measures each available model on the same documents, so that:

- `carrel models` can mark which catalog models are **recommended** for each memory size (#99);
- the defaults for ingest are chosen from evidence and can be re-checked when models change;
- later, the device self-benchmark in the spec (`_System/devices/<id>/`) can run a short version of it.

## The test archive and documents

- **Its own archive:** `fixtures/bench/archive`, with about a dozen collections whose design documents are realistic and sometimes close to one another, such as Finances vs. Receipts, Health vs. Insurance, Work vs. Projects, and Reading vs. Research. The sample archive has only four collections, too few to tell models apart.
- **Its own documents:** `fixtures/bench/items`, about 60 small documents as plain text, Markdown or small PDFs. The kinds are what a Downloads folder holds: receipts and invoices, bank and insurance letters, medical notes, papers, manuals, tickets, recipes, CVs, code READMEs, and meeting notes.
  - **Synthetic, so it can be public:** written for Carrel, with fake names, addresses and account numbers. There's no licensing question and no one's real data. A few are public-domain or CC BY texts, where the license is recorded beside them.
  - **Hard cases on purpose:** documents that fit two collections, ones that belong in Holding, items whose date is only in the text, and items that are sensitive only because of one line.
- **Answers beside them:** `fixtures/bench/answers.yaml` gives each item's expected destination (or the acceptable ones), sensitivity, date, and tags that must and may appear.

## What's scored

Each run goes through the real ingest stages (`ingest::run` with the chosen model), so prompts, validation and retries are measured as they ship. Each stage is scored automatically:

| Stage | Score |
| --- | --- |
| Valid output | share of items whose model output passed validation without falling back |
| Routing | share filed to an acceptable destination; Holding counts as half for ambiguous items, and as wrong otherwise |
| Sensitivity | share rated exactly; **under-rating** is counted separately, because it's the error that matters |
| Date | share with the expected date (or none when there isn't one) |
| Tags | must-have tags found, and tags outside the must and may lists |
| Speed | seconds per item and peak memory |

Titles and summaries can't be scored well automatically, so each run writes every card beside the expected answers in a side-by-side report for the owner to read.

## Running it

- **`carrel bench`**, a command for people developing Carrel and choosing models, is hidden from `carrel --help`. It copies the test archive to a temporary folder, ingests the items with each model given (`--model NAME`, repeatable; default all available), and writes `results.json` and `report.md` to `--out FOLDER`.
- **Stopping and carrying on:** items are ingested one at a time and each result is saved to `runs/` in the output folder as soon as it's done, so a run can be stopped at any point (Ctrl-C) and continued by running it again with the same `--out`; `--fresh` starts over, as is needed after changing prompts, checks or models. Each model shows a progress bar with the time left.
- **Not in CI:** models are gigabytes and runs take minutes per model. CI checks the corpus and answers file instead: every item has an answer, and every destination exists.
- **Recorded results:** a run's summary is committed under `docs/bench/` with the device, models, provider versions and date. The `recommended` flags in the catalog cite it.

## Choosing defaults

A model is recommended for a memory size when it is the smallest that fits and meets all of these:

- valid output for at least 95% of items;
- routing at least 80%;
- **no under-rated item rated restricted** (the rule-based floor should already catch those), and under-rating at most 5% otherwise;
- under 30 seconds an item on the reference machine.

If none qualifies at a size, nothing is recommended there, and `carrel models` says so.

## Which models

- The five catalog models, and Apple's on-device model where it's available.
- One larger model for Macs with 32 GB or more, as the spec expects (a 30–35B mixture-of-experts model, about 20 GB). It joins the catalog if it meets the bar.
- The runs happen on the owner's M4 Max. Each model download is asked for first, as `carrel models download` always does.

## Building it

1. The test archive, the documents and `answers.yaml`, and a CI test that checks them.
2. `carrel_core::bench`: loading answers, scoring a run, and the results type.
3. `carrel bench`, with the side-by-side report.
4. A first run on the owner's Mac, committed under `docs/bench/`, and the `recommended` flags set from it.

## Decisions (accepted 2026-10-30)

1. **A synthetic test set of about 60 documents** in its own archive of about a dozen collections, plus a few public-domain or CC BY texts, instead of real documents. Recommended: it can be public and shared, and the hard cases can be chosen on purpose.
2. **Score through the real ingest stages** with automatic scores for routing, sensitivity, dates, tags and validity, with titles and summaries left to a side-by-side report. Recommended.
3. **A hidden `carrel bench` command**, run by hand, with results committed under `docs/bench/`, and only the corpus checked in CI. Recommended.
4. **The bar for recommending:** valid output at least 95%, routing at least 80%, no under-rated restricted items and at most 5% under-rated otherwise, and under 30 seconds an item. The smallest qualifying model at each memory size is recommended. Recommended as a starting point; the first run may show the bar needs adjusting.
5. **Add one larger model for 32 GB Macs** to the candidates, and download each model only when you approve it. Recommended.
