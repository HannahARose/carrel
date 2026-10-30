# Design: likely duplicates (v0.2)

Status: accepted 2026-10-30, with the five recommendations below. Issue #123, the rest of #31.

## What this adds

Ingest already catches **exact** duplicates: an item with the same checksum as an archived item, or as an earlier item in the batch, is flagged "duplicate of X" (`docs/design/ingest.md` §4). It misses the common case of the same document arriving again slightly changed: a re-exported PDF, `report (1).pdf`, `thesis-v3.docx` after `thesis-v2.docx`, or a statement downloaded twice with different metadata. This design finds those **likely** duplicates, shows them beside the archived item in review, and lets the owner choose what to do with both.

## What counts as likely

The ingest design keeps v0.2 to cheap checks, with text similarity arriving with embeddings in v0.4. Three checks, each with a reason the owner sees:

| Check | When it fires | Reason shown |
| --- | --- | --- |
| **Same name** | The file names match once normalized, the kinds match, and the checksums differ | "same file name" |
| **A newer version** | The names match apart from a version marker, and the new one's marker is later | "looks like a newer version" |
| **Same title and date** | The drafted title matches an archived card's title (case and punctuation ignored) and the `created` dates are equal, or both are empty | "same title and date" |

- **Normalizing a name** lowercases it, drops the extension's case, and removes the marks downloads and copies add: a trailing `(1)`, `copy` or `copy 2`, `-1` after a download, and trailing whitespace. `Report (1).PDF` and `report.pdf` match.
- **Version markers** are `v2`, `v3`, `version 2`, `rev 4`, `r4`, `draft 2`, `final` (later than any number), and a trailing date (`2026-10-30`, `20261030`). `thesis-v3.docx` is a newer version of `thesis-v2.docx`; `thesis-v2.docx` arriving after `v3` is still a likely duplicate, reported as "looks like an older version".
- **Archived names** come from the card's `item` path, so an item the owner renamed in the archive matches by its new name. The original name isn't recorded today, so a renamed item won't match its original download.
- **Exact duplicates win:** a candidate with the same checksum is an exact duplicate, as now, and isn't also listed as likely.
- **At most three** archived items are listed per candidate, newest first.
- The name checks run before drafting, as the exact check does. The title-and-date check needs the draft, so it runs when the proposal is saved.

## Where it's recorded

A new optional field on the review entry, beside `duplicate_of`:

```json
"likely_duplicates": [
  {"id": "0192…", "card": "Finance/Statements/2026-09 statement.pdf.md", "why": ["same file name"]}
]
```

The index that finds them extends the checksum index into an **item index**: each archived item's fingerprint, normalized name and normalized title with its date, rebuilt from the cards whenever ingest runs, as the checksum index is today. It's in memory only; writing it to `_System/index/` waits until building it is slow enough to matter.

## What the owner can do

In review, a proposal with an exact or likely duplicate gets three choices besides the usual ones:

| Key | Choice | What happens |
| --- | --- | --- |
| `v` | **File as a new version** | The new item is filed as proposed, and its card links to the archived item with `version-of` (FORMAT.md §5.4). Both stay. |
| `b` | **Keep both** | Filed as proposed, linked with `related`. |
| `s` | **Skip it** | The same as reject: the new item isn't filed and stays where it is. |

- **The default:** for an exact duplicate, *skip*, as the ingest design says. For "looks like a newer version", *file as a new version*. For the others, there's no default and the proposal isn't high confidence, so `--all-high` leaves it for the owner.
- **Replacing the archived item** (keeping only the new one) isn't offered in v0.2: it needs removal and tombstones (FORMAT.md §10) and an undo that restores the old item's files. The owner can file as a version and remove the old item later. It's listed for when removal lands.
- On the command line: `carrel review approve ID --as-version-of OTHER` and `--related-to OTHER`, and `carrel review reject ID` to skip.
- The links are ordinary card links, so the history commit that files the item undoes them too.

## Side by side

- **Interactive mode, Review tab:** a proposal with a duplicate splits its detail pane in two, the proposal on the left and the archived card on the right, with the reasons above. Rows that differ are highlighted: title, date, size, file name, collection, tags and the start of the summary. With several, `[` and `]` switch between them.
- **`carrel review show ID`** prints the same comparison as a two-column table, and `--json` includes `likely_duplicates`.
- **`carrel ingest --wait`** and the dry run note "likely duplicate of X (same file name)", as they note exact duplicates now.

## Building it

1. Item index and the three checks in `carrel_core::ingest`, with tests for each normalization and version marker.
2. `likely_duplicates` on the review entry, written by the pipeline; the title check when the proposal is saved.
3. `version-of` and `related` links in `actions::decide`, and the CLI flags.
4. The side-by-side pane in the Review tab and `carrel review show`.
5. The user guide, the command reference, and FORMAT.md §7.9 for the new review entry field.

## Decisions (accepted 2026-10-30)

1. **Three cheap checks in v0.2:** same normalized name, a newer or older version by marker, and same title and date. Text similarity waits for embeddings in v0.4, as the ingest design says. Recommended.
2. **Record likely duplicates on the review entry** (`likely_duplicates`, with reasons), with the index in memory rather than in `_System/index/` for now. Recommended: nothing to keep in sync, and the archive is read once per ingest already.
3. **Three choices: file as a new version (`version-of`), keep both (`related`), or skip.** Recommended.
4. **No "replace the archived item" in v0.2,** until removal and tombstones exist. Recommended: it's the one choice that can lose data.
5. **Defaults:** skip for exact duplicates, file as a version for a newer version, and no default (owner decides) otherwise. Recommended.
