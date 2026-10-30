# Fixtures

Test data for Carrel. Everything here is invented or public; never add private documents.

## `sample-archive/`

A small, hand-built archive that follows [FORMAT.md](../FORMAT.md) v1.0 (draft). It is used by tests and as the v0.1 gate: it should read cleanly in Finder and in Obsidian (open the folder as a vault).

| Format feature | Where |
| --- | --- |
| Loose file with checksum and generated fields | `Reading/Coupled oscillators note.pdf` |
| Physical object | `Reading/On the Origin of Species.md` |
| URL | `Reading/RFC 9562 UUIDs.md` |
| Tombstone (deaccessioned item) with a `version-of` link | `Reading/Coupled oscillators note (preprint).pdf.md` |
| Hard-sealed bundle (detected by `Cargo.toml`), checked out, with pending and approved history entries | `Projects/oscillator-sim/` |
| Soft-sealed bundle with `allow_readme: false` | `Projects/field-notes-2025/` |
| External reference | `Projects/Sensor dataset 2026.md` |
| Nested collection | `Projects/Hardware/` |
| Open folder | `Projects/Hardware/lamp-build/` |
| `part-of` and `derived-from` links | `parts-list.csv.md`, `oscillator-sim.md` |
| Card name clash (`notes/` beside an owner file `notes.md`, so the folder's card is `notes.card.md`) | `Projects/Hardware/` |
| Holding collection | `Holding/` |
| Archive-wide and per-collection tag vocabularies | `ARCHIVE.md`, each `_collection.md` |
| Active section and Journal | `Active/`, `Journal/2026/2026-10.md` |
| `_System`: identity, settings, glossary, rules, manifests, device profile, an inbox entry, a copy of FORMAT.md | `_System/` |
| A review entry: a proposal to file a print file into Projects/Hardware, with a proposed new tag | `_System/queue/review/` |

### Not included, and why

- **`_System/history.git`** and a **`.git` marker** inside a bundle: Git can't store a repository's internal files inside another repository cleanly. Tests create the history repository at runtime, and `oscillator-sim` is sealed by `Cargo.toml` instead.
- **Desk items:** they live outside the archive by definition.
- **Questions, tasks and restructure proposals** in `_System/queue/`: their schemas are defined with the features that use them.

### Changing the fixture

Checksums, sizes and manifests must match the files exactly. After editing an item, update its card's `checksum` and `size` (or its manifest file and the card's `manifest.checksum`). `shasum -a 256 <file>` gives a file's checksum. After editing `FORMAT.md`, copy it over `sample-archive/_System/FORMAT.md`.

### How it's checked

`crates/carrel-core/tests/fixtures.rs` checks the sample archive on every test run, locally and in CI:

```bash
cargo test -p carrel-core --test fixtures
```

It reports every problem it finds, one per line:

- every card's frontmatter against `schemas/card.schema.json`, and that the core reads and writes it back byte for byte
- unique identifiers, and checksums, sizes and manifests matching the items
- tags from a vocabulary that applies, and links that resolve
- the `ARCHIVE.md` and collection listings, and that every card can be reached from `ARCHIVE.md`
- nothing left uncataloged
- valid JSON and YAML under `_System`, and an exact copy of `FORMAT.md`

## `bench/`

The test set for the model benchmark ([docs/design/benchmark.md](../docs/design/benchmark.md)): what `carrel bench` ingests with each model, and what it scores them against.

- **`archive/`:** an archive with thirteen collections and Holding, whose design documents sometimes overlap on purpose (Receipts and Manuals, Health and Insurance, Reading and Research). It has no routing rules, so models make every choice.
- **`items/`:** 65 short documents of the kinds a Downloads folder holds. Every person, address and number in them is invented; the card number, IBAN and social security number are well-known example values. Some are hard on purpose: they fit two collections, belong in Holding, or are sensitive because of one line.
- **`answers.yaml`:** each item's acceptable destinations, expected sensitivity, date and tags.

`crates/carrel-core/tests/bench.rs` checks on every test run that the three agree: every item has an answer, every destination and tag exists, and no expected level is below what the sensitivity rules alone give. To run the benchmark itself (it needs a text model, and takes minutes per model):

```bash
cargo run -p carrel-cli -- bench --out bench-results
```
