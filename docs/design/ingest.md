# Design: ingest (v0.2)

**Status: accepted (2026-10-29).** Covers #26, the pipeline. It also fixes the shape of #25 (routing rules), #27 (review queue), #28 (Holding), #29 (questions), #30 (sensitivity) and #31 (duplicates), which the pipeline calls. The owner accepted all five decisions listed at the end.

## What ingest does

Ingest puts new files and folders into the archive's existing structure, with a card for each, and nothing moves until the owner approves. It never invents structure: a collection is never created by ingest, and anything that fits poorly goes to Holding. It works from the CLI first (`carrel ingest`, `carrel review`, `carrel questions` and the interactive mode), and the Mac app reuses the same core in v0.5.

Three rules carry most of the weight:

1. **Proposals first, moves later.** Ingest only writes proposals. Approving one moves the item and writes its card, in a single history commit per approval batch.
2. **Rules before models.** The owner's routing rules decide what they can. Models only choose among existing collections, with each collection's design document as context.
3. **Every model output is checked.** Schemas guarantee the shape (guided generation, or grammars in llama.cpp), and a validator checks the meaning: the collection exists, the tags are known, the fields fit their limits. Anything that fails is retried once with the error, then tried on the next model, then left for the owner.

## The pipeline

Each item passes through these stages. Each stage's result is saved in the item's proposal, so a batch can stop and resume, and slow model work is kept apart from fast approval.

| Stage | Done by | Output |
| --- | --- | --- |
| 1. Intake | core | The item's path, source folder and batch |
| 2. Kind | core (`detect`, already built) | file, open folder, or sealed bundle; regenerable folders; size |
| 3. Fingerprint | core (`manifest`, already built) | Checksum, or the manifest's content checksum for folders |
| 4. Duplicates | core | Exact duplicates in the archive or in the batch |
| 5. Text | extraction providers (built) | Text for the model, trimmed to a budget |
| 6. Sensitivity | core rules, then a model | `public`, `personal`, `sensitive` or `restricted`, with reasons |
| 7. Card | a model | title, summary, tags, dates |
| 8. Route | routing rules, then a model | A collection, or Holding, with reasons and a confidence |
| 9. Validate | core | Problems found in steps 6–8, with a retry or the next model |
| 10. Questions | core | Questions for the owner, when an answer would settle several items |
| 11. Propose | core | A review entry in `_System/queue/review/` |

Approval then does step 12, **archive**:

- take the writer lease
- move the item and verify its checksum at the new place
- write the card, and update the listing and the changelog
- commit one history change for the batch (operation `ingest`, with the items and moves as trailers)

A Kopia snapshot per batch comes with #32.

### 1. Intake

`carrel ingest PATH...` creates a batch. Each argument is one item.

- **A plain folder argument** (decided): a folder that isn't sealed is one item, an open folder, unless `--each` is given, in which case each entry inside is its own item. A folder named `Downloads`, `Desktop` or `Inbox` prompts the choice instead of guessing.
- Items inside the archive (in collections) aren't ingested. Uncataloged files already sitting in the archive are offered as an ingest batch by `carrel status` later.

### 4. Duplicates

- **Exact:** the same checksum as an archived item or another item in the batch. The proposal says "duplicate of X" and defaults to *don't ingest*. The owner can still keep it, with a `version-of` or `related` link.
- **Likely:** the same name and size with a different checksum, or text that mostly matches. These are shown side by side in review. Only the cheap checks are in v0.2; text similarity comes with embeddings in v0.4.
- This needs an index from checksums to cards, kept in `_System/index/` and rebuilt when missing.

### 5. Text and the context budget

- Text comes from the registry: plain text, PDFKit, then Vision OCR on a Mac, and portable providers elsewhere. Bundles contribute their README, file tree and project files (such as `Cargo.toml` and `package.json`), not every source file.
- **Budget:** each model call gets its context size minus the prompt and the answer's room, measured with the model's own token count (#96). Long text is cut at sentence boundaries, keeping the start and the headings, and for papers the abstract and conclusion. Reference and acknowledgment sections are dropped (from #22).
- Media types come from the extension, checked against the file's first bytes.

### 6. Sensitivity

Sensitivity decides what waits for the owner, so it's built to err high.

- **Floor rules first**, in the core and without a model:
  - patterns such as account numbers, IBANs, passport and social security numbers, tax forms and medical terms
  - names like `passport`, `tax`, `medical` or `bank`
  - the destination collection's `sensitivity_default`

  The result is never lower than what these say.
- **Then a model** picks one of the four levels, given the text and the reasons the rules found. It can only raise the level, never lower it.
- `sensitive` and `restricted` items always wait for the owner, whatever the rules say. For `restricted` items, the card's summary is kept to what the owner would want visible: a model prompt says so, and the validator flags summaries that repeat matched patterns.

### 7. Card

One model call with a schema:

- **title:** under 120 characters
- **summary:** one to three sentences
- **tags:** up to five, from the vocabulary that applies at the destination
- **dates:** when the item was created, from metadata or the text

The prompt includes the glossary (§7.3), so facts the owner has taught are used.

- **New tags** (decided): the vocabulary is fixed per collection (§6.5), but the model may *propose* up to one new tag per item, marked as a proposal in review. Approving it adds it to the collection's vocabulary in the same commit.
- **Provenance** goes in `generated` (§5.6). `level` is `below-preferred` when the model came from a lower-priority provider than the device's best, so a stronger device can redraft it later.

### 8. Route

1. **Rules:** `rules/routing.yaml` (§7.4), in order, with the first match winning (#25). A match has high confidence. With `auto_file: true` it can skip review, but sensitive items still wait.
2. **Model, one level at a time:** the model sees the top-level collections' titles, descriptions and design-document sections ("Purpose and scope", "Not here"), plus the card draft. It picks the collection that fits best and says how well it fits: well, partly or poorly. Holding isn't offered as a choice: offered as one, models take it whenever unsure. If the chosen collection has subcollections, the same question repeats with those, plus the collection itself, which always comes last. Each step sees only siblings, so the context stays small even for a large archive. That matters for Apple's 8,192 tokens, and it scales to the 100,000-card target. The answer carries a short reason for review.
3. **Confidence**, from asking each level twice with the choices in opposite orders:
   - **high:** a rule matched
   - **medium:** both passes agreed and called it a good fit at every level
   - **low:** a partial fit (the collection is kept); passes that disagreed or a poor fit at the top (Holding); or disagreement or a poor fit below the top (the parent collection)

   The second pass costs one more call per level, and is skipped when a rule matched.
4. **Low confidence proposes Holding** (#28), and the item is recorded as a restructure signal (`_System/index/signals`), as are items the owner moves out of Holding later.

The early benchmark (wiki: Models and Privacy) found Apple's model weak at routing from names alone. Design documents as context and the second pass are the answer to that, and the benchmark harness (#33) measures it.

### 9. Validate

Checks after each model call, beyond what the schema guarantees:

- the collection ID exists and is one of the choices offered
- tags are in the vocabulary, or are the one allowed proposal
- each field is within its length limit
- the title isn't just a machine-made file name repeated (`IMG_2041`, `scan0003`, `document (3)`); a name a person chose is often already the right title
- dates are real dates
- the sensitivity isn't below the floor

On failure the stage retries once with the problem stated, then moves to the next model in the registry. If both fail, the proposal is marked **needs owner** with the problems listed. Every attempt is logged in the proposal, so the owner can see what happened.

### 10. Questions

When several items in a batch depend on the same unknown (a project name, an acronym, a person), the pipeline asks one question instead of guessing on each (#29):

- the question names the items it affects, and questions are ranked by how many that is
- answers are saved to the glossary
- items waiting on a question are redrafted after it's answered

v0.2 asks only two kinds of question: what a recurring unknown term is, and which collection a cluster of low-confidence items belongs in.

- **Terms** come from item names, without a model, so a batch always asks the same questions. A term is a hyphenated run (`scope-ds` in `scope-ds-plan.pdf`) or a word that looks like an identifier: an acronym, letters mixed with digits, or `CamelCase`. It's asked about when it appears in at least two names and the glossary, the tag vocabularies, the collections' names and the archive's name don't already explain it.
- **Where items go** is asked only about a term the glossary already explains, when at least two of its items still have low confidence. A new term is asked about first, since its answer often settles the placement.
- `carrel questions` lists the questions, `answer ID ...` answers one, and `dismiss ID` drops it. Answering a term adds the fact to the glossary as one history commit and drafts its items again; answering a collection points its items there.

### 11. Proposals and review

Each item's proposal is a JSON file, `_System/queue/review/<proposal-id>.json`, specified in FORMAT.md §7.9:

```json
{
  "carrel": "review",
  "format": "1.0",
  "id": "019a…",
  "batch": "019a…",
  "created": "2026-11-02T10:15:00-08:00",
  "state": "proposed",
  "source": {"path": "/Users/me/Downloads/parts-list.csv", "checksum": "sha256:…", "size": 1294},
  "kind": "file",
  "duplicate_of": null,
  "card": { "title": "…", "summary": "…", "tags": ["electronics"], "sensitivity": "personal", "generated": { "…": "…" } },
  "proposed_tags": ["led-strips"],
  "destination": {"collection": "019a…", "path": "Projects/Hardware", "confidence": "medium", "reasons": ["…"]},
  "problems": [],
  "attempts": [{"stage": "route", "provider": "apple", "offer": "afm", "result": "ok"}]
}
```

- **States:** `proposed` → `approved` → `archived`, or `held` (to Holding), `rejected` (left where it is), or `needs-owner`.
- **`carrel review`** lists proposals grouped by destination, with confidence and sensitivity, and supports:
  - `show`
  - `approve [IDS | --all-high]`
  - `edit` (opens the card draft)
  - `move-to COLLECTION`
  - `hold`
  - `reject`

  The interactive mode (#88) offers the same actions. `edit` comes with it, since editing a draft wants a screen rather than a command.
- Approving many at once makes one commit per batch. `--all-high` leaves out sensitive items, duplicates and anything that needs the owner.
- An item on another device stays approved until that device files it. An item that's gone or changed since it was proposed goes back to `needs-owner`, saying why.

### 12. Archive

- **Moving the original** (decided): on approval, the item is **moved** into the archive. A move within one volume is an atomic rename. Across volumes it's copy, verify the checksum, then remove the original, and the approval counts as the owner's consent. The setting `ingest.originals: move | keep` (§7.2) defaults to `move`; `keep` leaves the original and lists it in the batch summary for the owner to delete.
- Items go into the collection's folder under their own name. On a clash, Carrel adds a number, as in `notes (2).md`,, and the card notes the original name.

## Where it runs

- **Core:** a new `carrel_core::ingest` module.
  - stages as plain functions over a `Proposal`
  - a `Pipeline` that runs a batch with a `Registry`
  - prompt templates kept with the code and versioned, so a template change can be benchmarked
- **CLI:**
  - `carrel ingest PATH... [--each] [--json]`, which reports the batch and its proposals
  - `carrel review ...` and `carrel questions ...`
- **The writer lease** is taken only by approval, never by drafting. Several devices can draft into their own inboxes (§7.8), and only the writer archives.
- **Privacy:** only on-device providers are used. Nothing leaves the device, as v1 requires.

## Speed

Targets for a Mac with Apple's model, to check with the benchmark (#33):

- under 5 seconds per item for text, card and route
- a 500-item Downloads folder proposed in under an hour, without attention
- approval of a reviewed batch in seconds

Batches run items one at a time per model, since the providers answer one request at a time.

## Building it

In order, each a PR:

1. Proposals, queue storage and `carrel review` with hand-written proposals.
2. Intake, kind, fingerprint, duplicates and the checksum index (#31).
3. Text and the budget (finishing #22's trimming).
4. Card drafting and validation.
5. Routing: rules (#25), then the per-level model routing with the second pass.
6. Sensitivity (#30).
7. Archive on approval, with Holding (#28).
8. Questions and the glossary (#29).
9. Kopia snapshots (#32), separately.

The gate for v0.2 stays as it is: a sample of a real Downloads backlog is filed end to end and approved.

## Decisions (accepted 2026-10-29)

1. **A plain folder argument:** one item, with `--each` to split it.
2. **New tags:** the model may propose one per item, approved in review.
3. **Originals:** moved on approval, with a `keep` setting.
4. **Auto-filing:** only through rules with `auto_file: true`. Never on model confidence alone in v0.2, and never for sensitive items. Model-only auto-filing could come later, once results earn trust (Foundational Rules: autonomy limit).
5. **Commits:** one history commit per approval batch, not per item.
