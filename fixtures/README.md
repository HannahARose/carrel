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

### Not included, and why

- **`_System/history.git`** and a **`.git` marker** inside a bundle: Git can't store a repository's internal files inside another repository cleanly. Tests create the history repository at runtime, and `oscillator-sim` is sealed by `Cargo.toml` instead.
- **Desk items:** they live outside the archive by definition.
- **Queue entries:** their schemas are defined with the features that use them.

### Changing the fixture

Checksums, sizes and manifests must match the files exactly. After editing an item, update its card's `checksum` and `size` (or its manifest file and the card's `manifest.checksum`). Until the core can do this ([#12](https://github.com/HannahARose/carrel/issues/12), [#15](https://github.com/HannahARose/carrel/issues/15)), `shasum -a 256 <file>` gives the checksum. CI validation of fixtures is [#43](https://github.com/HannahARose/carrel/issues/43).
