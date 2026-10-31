# Carrel Archive Format

**Version 1.0 (draft)** · This specification is licensed under [CC BY 4.0](LICENSES/CC-BY-4.0.txt).

A Carrel archive is an ordinary folder that a person can understand with a file browser and a Markdown reader, and that any program can read and write by following this document. The Carrel app is one such program; it is not required to read an archive.

The key words **must**, **must not**, **should** and **may** are used as in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

## Contents

1. [Principles](#1-principles)
2. [Conventions](#2-conventions)
3. [Layout](#3-layout)
4. [Items](#4-items)
5. [Cards](#5-cards)
6. [Collections and design documents](#6-collections-and-design-documents)
7. [The `_System` folder](#7-the-_system-folder)
8. [History](#8-history)
9. [Active items and the Journal](#9-active-items-and-the-journal)
10. [Removal](#10-removal)
11. [Reserved for later versions](#11-reserved-for-later-versions)
12. [Versioning and compatibility](#12-versioning-and-compatibility)
13. [Conformance checklist](#13-conformance-checklist)

## 1. Principles

These rules follow from Carrel's design principles and settle any ambiguity in this document.

- **Readable without software.** Every piece of information a person needs is in plain files: Markdown, YAML and JSON. Binary data is limited to the items themselves, the history repository and rebuildable caches.
- **Self-contained.** Everything needed to interpret an archive is inside it, except secrets (such as backup passwords) and per-device preferences.
- **Items are never altered.** A program describes items in cards beside them. It never writes inside a sealed bundle and never changes an item's contents except when the owner returns a checked-out version.
- **Owner text is never lost.** Programs own specific fields and marked sections; everything else in a card belongs to the owner and is preserved byte for byte.

## 2. Conventions

### 2.1 Text files

- All text files are **UTF-8** without a byte-order mark, with **LF** line endings.
- Markdown follows [CommonMark](https://commonmark.org) with GitHub Flavored Markdown tables and task lists.
- **YAML** is restricted to the [YAML 1.2](https://yaml.org/spec/1.2.2/) JSON-compatible subset: no anchors, aliases, tags or multi-document streams. Strings that could be read as another type (`yes`, `no`, `1.0`, dates) **must** be quoted.
- **JSON** files are [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259) objects, pretty-printed with two-space indentation.

### 2.2 Frontmatter

A Markdown file managed by Carrel starts with YAML frontmatter between two lines containing exactly `---`. The first frontmatter key is `carrel`, whose value names the document type (`card`, `collection`, `archive`).

### 2.3 Identifiers

Archives, collections, items and devices have permanent identifiers: lowercase [UUIDv7](https://www.rfc-editor.org/rfc/rfc9562) strings, for example `01928f3a-7c41-7d2e-9b6a-3f0c5e2a1b44`. An identifier never changes when the thing it names is moved or renamed, and is never reused.

### 2.4 Dates and times

Dates are ISO 8601 calendar dates (`2026-10-29`). Times are RFC 3339 timestamps with an offset (`2026-10-29T15:42:07-07:00`). Durations are ISO 8601 durations (`PT2H10M`).

### 2.5 Paths

- Paths inside the archive are relative to the archive root, use `/` as the separator, and never start with `/` or contain `.` or `..` segments.
- Names are compared case-insensitively, because the default file systems on Apple platforms are case-insensitive. Two siblings **must not** differ only by case.
- Names **must not** contain `/`, `:`, control characters, or leading or trailing spaces, and **should** stay under 200 bytes.
- **Portable names.** So an archive can move between macOS, Linux, Windows and exFAT drives, names **should** also:
  - not contain `<`, `>`, `"`, `\`, `|`, `?` or `*`;
  - not be a name Windows reserves for devices (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`), compared without case and ignoring everything from the first `.`, so `con.txt` is reserved too;
  - not end with a `.`;
  - keep paths from the archive root under 200 characters, which leaves room under Windows' 260-character limit in a typical location.

  Programs **must** give everything they create a portable name. They **should** report existing names that aren't portable, with a portable suggestion, and **must not** rename items without the owner's approval.
- In **Markdown link destinations**, paths are percent-encoded so they work in every Markdown reader: at least space as `%20`, `(` as `%28`, `)` as `%29`, `%` as `%25`, `#` as `%23`, `?` as `%3F`, `<` as `%3C` and `>` as `%3E`. For example, `[RFC 9562](RFC%209562.md)`. Paths in frontmatter and JSON (such as `links[].path` or `manifest.path`) are plain, unencoded paths.

### 2.6 Checksums

Checksums are written as `algorithm:hex`. Version 1.0 uses SHA-256 (`sha256:9f86d0…`), which can be checked with standard tools such as `shasum -a 256`. Readers **must** reject algorithms they don't know rather than ignore the checksum.

### 2.7 Names reserved by the format

At the archive root: `ARCHIVE.md`, `README.md`, `Active/`, `Holding/`, `Journal/`, `_System/`. In any collection: `_collection.md`, `CHANGELOG.md`. Later versions of the format only add new names inside `_System/` or names starting with `_carrel`, so owners can use any other name, including ones starting with `_`.

### 2.8 Ignored files

Programs ignore, and never treat as items: `.DS_Store`, files starting with `._`, `Icon\r`, and the folders other tools create, `.obsidian/` and `.trash/`. Later versions of this format may add names to this list. (Obsidian's own trash folder should be turned off in favor of the system trash, so deletions made in Obsidian are visible as outside edits.)

## 3. Layout

```text
Archive/                              archive root (also an Obsidian vault)
  ARCHIVE.md                          archive design document (§6.2)
  README.md                           how to read this archive without Carrel
  Active/                             what is out of the archive right now (§9)
  Holding/                            a collection for items ingest couldn't place
    _collection.md
  Journal/                            monthly logs of work sessions (§9.3)
    2026/2026-10.md
  Research/                           a collection
    _collection.md                    its design document (§6.3)
    CHANGELOG.md                      what moved in or out, and why (§6.4)
    Projects/                         a subcollection
      _collection.md
      example-repo/                   an item: a hard-sealed bundle
      example-repo.md                 its card
      Paper.pdf                       an item: a loose file
      Paper.pdf.md                    its card
  _System/                            data for programs (§7)
  .obsidian/                          optional Obsidian settings
```

- A **collection** is any folder that contains a `_collection.md`. The archive root is not a collection; its design document is `ARCHIVE.md`.
- Everything else inside a collection is either an **item**, a **card**, or `CHANGELOG.md`.
- Items live only in collections. A file or folder at the root that isn't a reserved name, or one inside a collection that has no card, is **uncataloged**. Uncataloged content is never an error: programs handle it according to the `outside_edits` setting (§7.2), typically by offering it for ingest.
- Top-level collections sit directly under the root. Collections may nest to any depth.
- `README.md` at the root is a plain explanation for people. A program **should** create it with the archive and **must not** overwrite owner edits to it.

## 4. Items

### 4.1 Kinds

| Kind | `kind` value | What it is |
| --- | --- | --- |
| Loose file | `file` | A single file |
| Open folder | `folder` | A folder whose contents programs may reorganize |
| Bundle | `bundle` | A folder stored and moved whole (§4.2) |
| Physical object | `physical` | Something not in the archive, such as a book or notebook; the card records where it is |
| URL | `url` | A web resource; may have a local snapshot (§11) |
| External reference | `external` | A file or folder stored outside the archive, such as on another disk or service |
| Tombstone | `tombstone` | A record of an item that was removed (§10) |

Items of kind `physical`, `url`, `external` and `tombstone` are **card-only**: the card exists without a file or folder beside it.

A symbolic link is a `file` item in its own right; programs never follow it to decide an item's kind.

The contents of a folder or bundle are part of that one item: they are recorded in its manifest (§7.7) and have no cards of their own. If the files inside a folder need cards of their own, the folder should become a collection instead. Programs reorganize an open folder's contents only through a change the owner approves.

### 4.2 Sealing

A bundle's `sealed` field says how strictly it is protected:

| `sealed` | Assigned by | Programs may |
| --- | --- | --- |
| `hard` | Detection of project markers (§7.5), or the owner | Move or rename the folder as a whole. Never create, change, rename or delete anything inside it. |
| `soft` | The owner | As `hard`, except that a program may add a `README.md` inside unless the card sets `allow_readme: false`. |

An open folder (`kind: folder`) has no `sealed` field.

### 4.3 Regenerable content

Folders listed in `_System/rules/regenerable.yaml` (for example `node_modules`, `.venv`, `target`, `__pycache__`) may exist inside items. They are part of the item, but programs **should** leave them out of backups and **must not** delete them.

## 5. Cards

Every item has exactly one card: a Markdown file whose frontmatter starts with `carrel: card`.

### 5.1 Card location and name

- A card sits in the same folder as its item.
- For a loose file, the card is named after the full file name plus `.md`: `Paper.pdf` → `Paper.pdf.md`.
- For a folder or bundle, the card is named after the folder plus `.md`: `example-repo/` → `example-repo.md`.
- If that name is already taken by a file that is not a card, the card is named with `.card.md` instead: `notes/` → `notes.card.md`.
- A card-only item's card can have any name ending in `.md`.
- The `item` field (§5.2) is authoritative; names are a convenience for people.

A Markdown file whose frontmatter does not start with `carrel: card` is not a card, even if its name ends in `.md` beside another file.

### 5.2 Frontmatter fields

Fields are owned by programs unless marked as owner fields. Optional fields are omitted rather than set to `null`.

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `carrel` | `"card"` | yes | Document type |
| `format` | string | yes | Format version the card was written with, such as `"1.0"` |
| `id` | UUIDv7 | yes | The item's permanent identifier |
| `kind` | enum | yes | §4.1 |
| `title` | string | yes | Human-readable title (owner may edit) |
| `item` | string | for `file`, `folder`, `bundle` | The item's name, relative to the card's folder |
| `sealed` | `hard` \| `soft` | for `bundle` | §4.2 |
| `allow_readme` | boolean | no | For soft-sealed bundles; default `true` |
| `summary` | string | no | Two or three sentences describing the item |
| `tags` | list of strings | no | Tags from the vocabularies in the design documents along the item's path (§6.5) |
| `links` | list | no | Typed links (§5.4) |
| `dates` | map | no | `created`, `modified`, `added`, `worked` (dates or timestamps), `document` (the item's own date, such as a publication date) |
| `sensitivity` | enum | yes | `public`, `personal`, `sensitive` or `restricted` |
| `checksum` | checksum | for `file` | The file's checksum |
| `size` | integer | for `file`, `folder`, `bundle` | Bytes |
| `manifest` | map | for `folder`, `bundle` | `checksum` of the manifest file and its `path` under `_System/manifests/` (§7.7) |
| `location` | string | for `physical`, `external` | Where the item is, in words or as a path or URI |
| `url` | string | for `url` | The resource's address |
| `checked_out` | map | no | Present while checked out (§9.1) |
| `snapshots` | `return-only` | no | Snapshot the working copy only at checkout and return, not after each work session (§9.1), as for large data (owner may set) |
| `generated` | map | no | Provenance of program-written fields (§5.6) |
| `placement` | map | no | How sure filing was of the item's collection, when it wasn't (§5.8) |
| `removed` | map | for `tombstone` | §10 |
| `expanded_from` | map | no | The compressed file the item was expanded from when it was filed or with `carrel expand`: its `name`, `size` and `checksum` (§2.6) |

Programs **must** preserve fields they don't recognize when rewriting a card.

### 5.3 Body

The body is the owner's. Programs may only write inside **managed sections**, delimited by HTML comments that render invisibly:

```markdown
<!-- carrel:history -->
## History
…
<!-- /carrel:history -->
```

- A marker counts only when it is alone on its line (surrounding spaces allowed) and outside fenced code blocks, so documentation can show markers without creating sections. Section names use lowercase letters, digits and `-`. A section appears at most once per document, and sections never nest.
- A program **must not** change anything outside managed sections, including whitespace.
- If a managed section's markers are missing or unbalanced, a program **must not** guess; it leaves the body alone and reports the problem.
- Version 1.0 defines one managed section, `history` (§5.5). New cards place it at the end of the body.
- Markdown editors such as Obsidian realign tables by padding cells with spaces. Programs **must** treat differences in table cell padding and delimiter-row dash counts as insignificant, both inside managed sections and when detecting outside edits.

### 5.4 Links

`links` is a list of maps:

| Key | Required | Meaning |
| --- | --- | --- |
| `type` | yes | `version-of`, `part-of`, `derived-from`, `cites`, `about`, `related`, or a custom type declared in `ARCHIVE.md` |
| `id` | yes | The target item's identifier |
| `path` | no | The target card's path relative to this card, kept current by programs as a convenience for readers |
| `note` | no | A short explanation |

Links are directed: "this item *is a version of* that item". Links in the body are ordinary relative Markdown links and are the owner's; programs **may** rewrite their targets when items move, and **should** do so only for links that resolve to a card.

### 5.5 History entries

The history section lists work sessions and other notable events, newest first. Each entry is a level-3 heading followed by text:

```markdown
### 2026-10-29 · 2h 10m
Added a phase-response analysis and regenerated figure 4.
Commits: "add PRC analysis", "fix damping units"
My notes: compare with the 2024 notebook.
```

An entry written by a program and not yet approved is a pending callout, so it stands out in Obsidian and in plain Markdown readers:

```markdown
> [!pending] 2026-10-29 · 2h 10m · auto-generated
> Added a phase-response analysis and regenerated figure 4.
```

Approving an entry turns the callout into a normal entry. Text the owner adds to an entry is preserved when entries are combined.

### 5.6 Provenance

`generated` records how program-written fields were produced:

```yaml
generated:
  fields: [summary, tags]
  by: "carrel 0.2.0"
  model: "apple/afm-3-core-advanced"
  device: 01928f3a-0000-7000-8000-00000000d001
  at: "2026-10-29T15:42:07-07:00"
  level: preferred        # or below-preferred, if a stronger device should re-check it
  corrections:            # optional: what the owner said to correct the card
    - said: "This is a picture of Lake Annecy, not Geneva"
      at: "2026-10-31T09:12:40-07:00"
```

A field the owner set outright is left out of `fields`, so programs never overwrite it. `corrections` keeps the owner's corrections, oldest first; a program that rewrites a field in `fields` must give them to any model it uses and must not contradict them.

### 5.7 Example

```markdown
---
carrel: card
format: "1.0"
id: 01928f3a-7c41-7d2e-9b6a-3f0c5e2a1b44
kind: file
title: "Targeted optogenetic stimulation of moving C. elegans"
item: "Liu2022.pdf"
summary: >-
  A closed-loop system that delivers targeted light stimulation to many moving worms
  at once, used to study how head and tail touch signals combine.
tags: [optogenetics, c-elegans, mechanosensation]
links:
  - type: cites
    id: 01928f3b-1111-7aaa-8bbb-0123456789ab
    path: "../Reviews/Touch circuits review.pdf.md"
dates:
  added: "2026-10-29"
  document: "2022-01-20"
sensitivity: public
checksum: "sha256:2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae"
size: 4718302
---

Useful for the closed-loop latency numbers in the methods section.

<!-- carrel:history -->
## History
<!-- /carrel:history -->
```

### 5.8 Placement

When an item is filed where routing wasn't sure it belongs, its card says so, as evidence for restructuring (`docs/design/signals.md`):

```yaml
placement:
  confidence: low          # low or medium
  by: model                # model or rule
  at: "2026-10-30T14:02:11-04:00"
```

Programs write `placement` when filing a proposal whose destination confidence (§7.9) was `low` or `medium`, and never for items filed to Holding, where being there says the same. They remove it when the owner places the item themselves, such as by moving or renaming it, since the owner's choice settles where it belongs.

## 6. Collections and design documents

### 6.1 Design documents

A design document is a Markdown file with frontmatter for programs and a body written for people and models. The body explains what belongs in the collection and how it is organized; programs read it as context before filing anything.

### 6.2 `ARCHIVE.md`

```yaml
---
carrel: archive
format: "1.0"
id: 01928f39-aaaa-7bbb-8ccc-000000000001
name: "Personal archive"
link_types:                     # custom link types, beyond the built-in ones
  - name: supersedes
    description: "A newer document that replaces an older one"
tags:                           # archive-wide tag vocabulary
  - name: draft
    description: "Work in progress"
sensitivity_default: personal
size_threshold: 10737418240     # bytes; larger items prompt a placement choice at ingest
min_collection_items: 5         # literary warrant: minimum items to propose a new collection
---
```

The body states the archive's purpose and principles, naming conventions, and anything else that helps a person or program understand it. It ends with a managed section `collections`, generated by programs, that links every top-level collection:

```markdown
<!-- carrel:collections -->
## Collections

| Collection | Items | Description |
| --- | --- | --- |
| [Projects](Projects/_collection.md) | 3 (+4 in subcollections) | Things being made: code, notes and data |
| [Holding](Holding/_collection.md) | 1 | Items waiting for a better home |
<!-- /carrel:collections -->
```

`ARCHIVE.md` is the entry point for browsing: from it, every collection, item and card can be reached by following links.

### 6.3 `_collection.md`

```yaml
---
carrel: collection
format: "1.0"
id: 01928f3a-bbbb-7ccc-8ddd-000000000002
title: "Projects"
description: "Things being made: code, notes and data"   # one line, used in listings
item_template: project          # optional: a card template name for this collection's items
compressed_files: keep          # optional: expand, keep or ask; overrides settings.json (§7.2)
tags:                           # this collection's tag vocabulary
  - name: simulation
    description: "Numerical models and simulation code"
---
```

The body has these sections, in this order. Sections a collection doesn't need may be omitted.

1. `## Purpose and scope`: what belongs here.
2. `## Not here`: what doesn't belong, and where it goes instead.
3. `## Organization`: subcollections, when a new one is warranted, naming conventions.
4. `## Examples`
5. `## Related collections`
6. A managed section `listing`, generated by programs, with a link up to the parent, the subcollections and the items:

```markdown
<!-- carrel:listing -->
## Contents

Part of [Research](../_collection.md)

### Subcollections

| Collection | Items | Description |
| --- | --- | --- |
| [Reviews](Reviews/_collection.md) | 6 | Review articles and surveys |

### Items

| Item | Kind | Added |
| --- | --- | --- |
| [Liu2022.pdf](Liu2022.pdf.md) | file | 2026-10-29 |
<!-- /carrel:listing -->
```

- The "Part of" line links to the parent collection's `_collection.md`, or to `../ARCHIVE.md` for a top-level collection.
- The Subcollections table is omitted when there are none; the Items table is omitted when the collection holds no items directly.
- The Items column counts items directly in a collection; when subcollections hold more, the count reads like `3 (+4 in subcollections)`.
- Item rows link to cards, subcollection rows to their `_collection.md`, and link text is the item's or collection's title.
- Subcollection rows are sorted by path, and item rows by the date in the Added column (`dates.added`, or for a tombstone the date it was removed), then by title, so a listing rebuilt from the same cards is always the same.

Changes to a design document itself are recorded in its collection's `CHANGELOG.md`.

### 6.4 `CHANGELOG.md`

A collection's changelog lists, newest first, items moved in or out and design-document changes, one line each:

```markdown
# Changelog

- 2026-10-29 · added [Liu2022.pdf](Liu2022.pdf.md) from ingest
- 2026-10-28 · moved [example-repo](example-repo.md) here from Research/Code (restructure)
```

It is a convenience for people; the authoritative record is the history (§8).

### 6.5 Tags

- A tag is lowercase, uses `-` between words, and contains only letters, digits and `-` (`phase-response`, `c-elegans`).
- The vocabulary available to an item is the union of the `tags` lists in `ARCHIVE.md` and every `_collection.md` from the root down to the item's collection.
- A card may carry a tag outside that vocabulary; programs **should** flag it for review rather than reject it.

### 6.6 Subcollections

A subcollection is a collection inside another collection. It follows every rule for collections, plus these:

- **Scope:** its purpose is a narrower part of its parent's. The parent's `## Organization` section names its subcollections and says when a new one is warranted.
- **Membership:** an item belongs to exactly one collection: the nearest folder above it with a `_collection.md`. A parent's listing shows its subcollections but not their items.
- **Inheritance:** a subcollection inherits its ancestors' tag vocabularies (§6.5) and card template, unless it sets its own `item_template`. Sensitivity, routing and other rules are never inherited implicitly; they are stated where they apply.
- **Links:** parent and child always link to each other through their managed `listing` sections (§6.3).
- **Changes:** creating, renaming, moving, merging or removing a subcollection is recorded in the parent's `CHANGELOG.md` and, for moves, in both collections' changelogs. A subcollection moves as a whole, with its items, cards and own subcollections.
- **Depth:** nesting has no limit, but archives **should** stay within three levels below the root; deeper structures are usually easier to navigate with tags and links.
- **Warrant:** programs propose a new subcollection only when it would hold at least `min_collection_items` items (§6.2).

## 7. The `_System` folder

`_System` holds data for programs. People can read it, but should not need to.

```text
_System/
  archive.json          identity and format version (§7.1)
  settings.json         archive-level settings (§7.2)
  FORMAT.md             a copy of this specification
  glossary.md           facts the owner has taught Carrel (§7.3)
  rules/                routing, sealing and regenerable rules (§7.4–7.5)
  manifests/            folder and bundle manifests (§7.7)
  history.git/          the history repository (§8)
  writer.lock           which device may write, on local and external disks (§8.4)
  inbox/<device-id>/    quick actions from devices that aren't the writer (§7.8)
  queue/                pending proposals, questions and tasks (§7.9)
  devices/<device-id>/  device profiles and desk-item registries (§7.10)
  index/                search index (§7.11)
  cache.nosync/         rebuildable caches, excluded from iCloud sync
```

### 7.1 `archive.json`

```json
{
  "carrel": "archive",
  "format": "1.0",
  "id": "01928f39-aaaa-7bbb-8ccc-000000000001",
  "created": "2026-10-29T15:00:00-07:00",
  "created_by": "carrel 0.1.0"
}
```

This file identifies a folder as a Carrel archive. `id` **must** match `ARCHIVE.md`.

### 7.2 `settings.json`

Archive-level settings that every device follows. Version 1.0 defines:

| Key | Values | Default |
| --- | --- | --- |
| `outside_edits` | `flag`, `reconcile`, `drift` | `flag` |
| `checkout.destination` | a path or `desktop` | `desktop` |
| `checkout.after_return` | `ask`, `trash`, `keep` | `ask` |
| `sessions.idle_minutes` | integer | `45` |
| `reminders` | list of `idle`, `weekly`, `menu-bar`, `due-date` | `["idle", "weekly", "menu-bar"]` |
| `embedding.model` | model identifier | chosen at creation |
| `backups` | map describing the backup repository: `type` (`kopia`), `repository` (the backup system's identifier for it) and `description`. Never its path, which differs between devices, and never the password | none |
| `ingest.originals` | `move` (the original is moved into the archive on approval) or `keep` (it's copied, and the original is left for the owner to delete) | `move` |
| `signals.crowded_items` | integer: how many loose items make a collection with no subcollections crowded, as a restructure signal | `30` |
| `compressed_files` | `expand` (a compressed file, such as a zip, is expanded when its proposal is approved, so what it holds can be browsed), `keep` (it's filed as it is) or `ask` (the owner decides for each one in Review). A collection's own `compressed_files` (§6.3) and a routing rule's `keep_compressed` (§7.4) take precedence; software, encrypted, split and unreadable archives are kept, and unsafe or very large expansions ask, whatever the setting | `expand` |

Programs **must** preserve unknown keys.

### 7.3 `glossary.md`

Facts the owner has taught Carrel, one level-3 heading per fact, newest first:

```markdown
### scope-ds
The scope-ds repositories are part of Cognitogram.
<!-- id: 01928f3c-…  added: 2026-10-29 -->
```

The heading is the term; the text is the fact in plain language.

### 7.4 `rules/routing.yaml`

Owner-written routing rules, applied in order before any model is asked. The first matching rule wins.

```yaml
rules:
  - name: "KiCad projects"
    match: { marker: ".kicad_pro" }
    collection: 01928f3a-cccc-7ddd-8eee-000000000003
  - name: "Course homework"
    match: { name: "^Ph[0-9]+_HW[0-9]+.*\\.pdf$" }
    collection: 01928f3a-dddd-7eee-8fff-000000000004
    auto_file: false
```

`match` keys: `extension`, `name` (a regular expression on the item's name), `marker` (a file present in a folder's top level, or at the top level of what a compressed file would expand to), `source` (the folder the item came from). All keys in one `match` must hold. `auto_file: true` files matches without review. `keep_compressed: true` keeps matching compressed files compressed rather than expanding them (§7.2).

### 7.5 `rules/markers.yaml` and `rules/regenerable.yaml`

`markers.yaml` lists files and folders whose presence at a folder's top level makes it a hard-sealed bundle:

```yaml
markers: [".git", "Cargo.toml", "package.json", "pyproject.toml", "go.mod", "Package.swift", "*.xcodeproj", "*.xcworkspace", "*.kicad_pro"]
```

`regenerable.yaml` lists folder names treated as regenerable (§4.3):

```yaml
regenerable: ["node_modules", ".venv", "venv", "target", "build", "dist", "__pycache__", ".ipynb_checkpoints", "DerivedData"]
```

Patterns use `*` for any run of characters within a name and match whole names, ignoring case (§2.5). Only entries at a folder's top level are checked for markers, a marker may be a file or a folder (a Git worktree's `.git` is a file), and ignored files (§2.8) never count. A missing rules file means the defaults shown above. When looking for regenerable folders, programs don't search inside a regenerable folder or a `.git` folder.

### 7.6 Card templates

`rules/templates/<name>.md` holds card body templates named by `item_template` (§6.3). A template is a Markdown body with optional headings the owner fills in.

### 7.7 Manifests

A manifest records the files in a folder or bundle, one per line, as tab-separated values:

```text
# carrel manifest 1.0	id=01928f3a-7c41-7d2e-9b6a-3f0c5e2a1b44
README.md	sha256:9f86d081…	512	2026-09-09T18:30:00.25-07:00
src/main.rs	sha256:e3b0c442…	2048	2026-09-14T10:02:11.137401922-07:00
```

- **Header:** the first line is `# carrel manifest 1.0` followed by tab-separated `key=value` fields, including the item's `id`. Readers ignore fields they don't know.
- **Columns:** path relative to the folder with `/` separators, checksum, size in bytes, modification time. Modification times keep the file system's full precision (fractional seconds).
- **Order:** rows are sorted by path in byte order (UTF-8), and a path appears at most once.
- **Encoding:** in paths, `%`, tab, carriage return and line feed are written as `%25`, `%09`, `%0D` and `%0A`; nothing else is encoded.
- **What's listed:** every file, including those inside `.git`, except ignored files (§2.8) and the contents of regenerable folders (§4.3), which change too often to track and can be rebuilt.
- **Symbolic links** are listed, not followed: the checksum is of the link's target path, and the size is that path's length in bytes.
- **Storage:** manifests are stored at `_System/manifests/<item-id>.tsv`. They sync with the archive but are not part of the history repository (§8.1). While an item is checked out, the manifest of what it held when it went out is kept beside them as `<item-id>.checkout.tsv`, for comparing with on return (§9.1); it's deleted when the item comes back.
- **Content checksum:** the card's `manifest.checksum` identifies the item's contents, not the manifest file. It is the checksum of the text made by writing, for every row in order, the encoded path, a tab, the checksum, a tab, the size and a line feed. Modification times and the header are left out, so touching a file without changing it never changes the card, and a manifest rebuilt from a backup can be checked exactly against history.
- **Reusing checksums:** a program may reuse an entry's checksum when a file's size and modification time both match the previous manifest, instead of reading the file again.
- **Past manifests** can be rebuilt from the backup snapshot named in a history commit's `Carrel-Snapshot` trailer, using the rules in `_System/rules/` as of that commit, and checked against the `manifest.checksum` in the card as of that commit.

### 7.8 Inboxes

Devices that are not the current writer (§8.2) record quick actions as one JSON file each in `_System/inbox/<device-id>/`, named `<timestamp>-<uuid>.json`:

```json
{
  "action": "approve-entry",
  "item": "01928f3a-7c41-7d2e-9b6a-3f0c5e2a1b44",
  "entry": "2026-10-29 · 2h 10m",
  "person": "01928f39-eeee-7fff-8000-000000000005",
  "device": "01928f3a-0000-7000-8000-00000000d002",
  "at": "2026-10-29T18:01:00-07:00"
}
```

A device only ever writes to its own inbox folder. The writer applies inbox files in timestamp order and records each as applied in the history.

### 7.9 Queues

`_System/queue/` holds pending work as one JSON file per entry, in subfolders `intake/` (items waiting to be ingested), `review/` (ingest and change proposals), `questions/` (questions for the owner), `tasks/` (work for a more capable device) and `proposals/` (restructure proposals). Entry schemas are defined alongside the features that use them and are versioned with this specification. Programs **must** preserve unknown keys in entries.

#### Review entries

`review/<id>.json` proposes filing one item. Nothing is moved or written into a collection until the owner approves it.

```json
{
  "carrel": "review",
  "format": "1.0",
  "id": "019a2c1e-0000-7000-8000-000000000001",
  "batch": "019a2c1e-0000-7000-8000-0000000000b1",
  "created": "2026-11-02T10:15:00-08:00",
  "state": "proposed",
  "source": {
    "path": "/Users/me/Downloads/parts-list.csv",
    "device": "01928f3a-0000-7000-8000-00000000d001",
    "checksum": "sha256:9f2c…",
    "size": 1294
  },
  "kind": "file",
  "duplicate_of": null,
  "card": {
    "title": "Desk lamp parts list",
    "summary": "Bill of materials for the desk lamp build.",
    "tags": ["electronics"],
    "sensitivity": "personal",
    "generated": {"fields": ["title", "summary", "tags"], "by": "carrel 0.2.0", "model": "apple/afm-3-core-advanced", "at": "2026-11-02T10:15:04-08:00", "level": "preferred"}
  },
  "proposed_tags": [],
  "destination": {
    "collection": "01a18701-0500-776a-9867-0804857e5ab6",
    "path": "Projects/Hardware",
    "confidence": "medium",
    "reasons": ["A bill of materials for a hardware build"]
  },
  "problems": [],
  "attempts": [{"stage": "card", "provider": "apple", "offer": "afm", "result": "ok"}]
}
```

| Field | Meaning |
| --- | --- |
| `id`, `batch` | The entry's identifier, and the ingest batch it came from (UUIDv7) |
| `state` | `proposed`, `needs-owner` (something couldn't be settled; see `problems`), `approved`, `held` (to go to Holding), `rejected` (to stay where it is) or `archived` |
| `source` | Where the item is now: an absolute path on the device `device`, with its checksum (§2.6) and size, or the manifest's content checksum and total size for a folder or bundle |
| `kind` | `file`, `folder` or `bundle`; a bundle also has `sealed` |
| `duplicate_of` | The identifier of an archived item with the same checksum, if any |
| `likely_duplicates` | Optional. Archived items it's likely a duplicate of, at most three: each with its `id`, its `card` path, and `why`, a list of `same-name`, `newer-version`, `older-version` and `same-title-and-date` |
| `card` | The card fields to write (§5), except those programs fill in when archiving (`id`, `item`, `checksum`, `size`, `manifest`). In `links`, a `path` is relative to the archive root until the item is filed, when it's rewritten relative to the card |
| `proposed_tags` | At most one tag outside the destination's vocabulary (§6.5); approving the entry adds it to the destination collection's `tags` |
| `destination` | The collection proposed, its path when proposed, `confidence` (`high`, `medium` or `low`), the `reasons`, and `rule` when a routing rule (§7.4) decided |
| `problems` | What validation or the owner needs to settle, as `{"stage", "message"}` objects |
| `attempts` | Every provider call made for the entry: `stage`, `provider`, `offer`, `result` (`ok`, `unsupported`, `unavailable`, `declined` (a model's safety refusal) or `failed`) and an optional `message` |
| `sensitivity_reasons` | Why `card.sensitivity` is `sensitive` or `restricted`, in words that never quote what was found; absent otherwise |
| `desk_card` | Optional. For a desk item being archived (§9.4), its desk card's absolute path: the filed card keeps that card's `id` and body |
| `compressed` | Optional, for a compressed file. `format` (`zip`, `tar`, `tar-gz`, `tar-bz2`, `tar-xz`, `tar-zst`, `gz`, `bz2`, `xz`, `zst`, `7z` or `rar`); `action`: `expand` (on approval, what it holds is filed in its place, with `expanded_from` on the card, and the compressed file goes to the system Trash), `keep` or `ask` (the entry needs the owner); `reason` for keeping or asking; `by_owner` when the owner chose the action, which lifts the size limits; and when known, what expanding makes: `files`, `size`, `kind`, `sealed` and `name` |

When an approved or held entry has been archived, programs remove the entry; the history commit (§8.2) records what was filed. A rejected entry is removed once the owner has seen the batch summary. Entries whose `source.device` is another device can't be archived there; they wait for that device.

#### Intake entries

Items added for ingest wait in `_System/queue/intake/`, one JSON file each, named for its `id`, until a program on their device has drafted their proposal:

```json
{
  "carrel": "intake",
  "format": "1.0",
  "id": "01928f3d-1111-7aaa-8bbb-000000000001",
  "path": "/Users/me/Downloads/parts-list.csv",
  "device": "01928f3a-0000-7000-8000-00000000d001",
  "added": "2026-10-30T14:02:11-04:00"
}
```

| Key | Meaning |
| --- | --- |
| `path` | The item's absolute path on its device. Only that device processes the entry |
| `device` | The device that has the item |
| `added` | When it was added |
| `order` | Its place in the queue, lower first, then by `id`; omitted when `0` |
| `attempts`, `failure` | How many times processing it has failed, and why the last time; after two, it waits for the owner to retry it |
| `desk_card` | Optional. For a desk item being archived (§9.4), its desk card's absolute path. The proposal gets the card's fields and the same key |

Programs process entries one at a time, save each item's proposal to `review/` as soon as it's ready, and then remove the entry.

#### Question entries

When several items depend on the same unknown, a program asks the owner one question instead of guessing on each. Each question is a JSON file in `_System/queue/questions/`, named for its `id`:

```json
{
  "carrel": "question",
  "format": "1.0",
  "id": "01a18701-0500-776a-9867-08f000000002",
  "batch": "01a18701-0500-776a-9867-08f0000000b1",
  "created": "2026-10-29T16:20:05-07:00",
  "kind": "term",
  "subject": "scope-ds",
  "question": "What is “scope-ds”?",
  "items": ["01a18701-0500-776a-9867-08f000000001"]
}
```

| Field | Meaning |
| --- | --- |
| `kind` | `term` (what a recurring term means; the answer becomes a glossary fact, §7.3) or `collection` (which collection the items belong in; the answer is a collection's path) |
| `subject` | The term the question is about, as it appears in the items' names |
| `question` | The question in words |
| `items` | The review entries waiting on the answer. Programs mark each with a problem whose `stage` is `question`, and set it to `needs-owner` |
| `choices` | For a `collection` question, the collections' paths to choose from |

Once a question is answered or dismissed, programs remove it and clear the problem from its items. Items waiting on a term are drafted again with the new fact.

### 7.10 Devices

`_System/devices/<device-id>/device.json` records a device's name, type and capability profile. `desk.json` lists the desk items tracked on that device with their local paths. Paths in `desk.json` are only meaningful on that device.

`desk.json` is an object whose `items` is a list, in the order they were added, of:

| Key | Meaning |
| --- | --- |
| `id` | The desk card's `id` |
| `path` | Where the item is on that device, absolute |
| `card` | Where its card is: beside it, its name with `.md` added |
| `added` | When it was put on the desk |

### 7.11 Search index

`_System/index/` holds the search index. Its files are tagged with the embedding model identifier and version from `settings.json`. The index can always be rebuilt from cards and items, so readers may ignore it.

### 7.12 `cache.nosync/`

Rebuildable, device-specific data. The `.nosync` suffix keeps iCloud Drive from syncing it. Readers **must** ignore it and programs **must** tolerate it being missing.

## 8. History

### 8.1 The history repository

The archive's metadata history is a Git repository whose Git directory is `_System/history.git` and whose work tree is the archive root. Keeping the Git directory under `_System` leaves no `.git` folder at the root, so the root never looks like a code repository and never interferes with sealed bundles that are repositories themselves.

The repository tracks **only metadata**: `ARCHIVE.md`, the root `README.md`, every collection's `_collection.md` and `CHANGELOG.md`, every card, everything under `Active/` and `Journal/`, and `_System/` files other than `history.git/`, `manifests/`, `index/`, `cache.nosync/` and the writer lock (`writer.lock` and files named `writer.lock.*`, §8.4). Nothing else is tracked: not the contents of any item or sealed bundle, not uncataloged files, and not other tools' folders such as `.obsidian/`. Item contents are versioned by the backup system, not by Git. Manifests are left out because a manifest of a large bundle can reach hundreds of megabytes and is fully determined by the item's contents; history keeps each manifest's content checksum in the card instead (§7.7).

- **Staging:** programs compute that set of files and stage exactly it for each commit, rather than relying on ignore patterns. The repository's `info/exclude` contains `*`, so Git tools never add item contents by accident.
- **Work tree:** the repository's `core.worktree` is the relative path `../..`, so it stays valid when the archive is moved or synced to another device.
- **Branch:** history is a single branch, `main`.

Anyone with Git can read the history:

```bash
git --git-dir=_System/history.git --work-tree=. log
```

### 8.2 Commits

- Each logical change (an ingest batch, a checkout, a return, a restructure, a set of approvals) is one commit.
- Only the current **writer** commits. How a device becomes the writer depends on where the archive is stored and is outside this format, except for the lock file used on local and external disks (§8.4).
- Commit messages have a one-line summary, an optional body, and these trailers:

```text
Carrel-Operation: ingest
Carrel-Person: 01928f39-eeee-7fff-8000-000000000005
Carrel-Device: 01928f3a-0000-7000-8000-00000000d001
Carrel-Snapshot: kopia:k3f9a1c0e2b4d6
Carrel-Items: 01928f3a-7c41-7d2e-9b6a-3f0c5e2a1b44
```

`Carrel-Operation` is one of `create`, `ingest`, `approve`, `edit`, `move`, `checkout`, `return`, `session`, `restructure`, `deaccession`, `purge`, `outside-edit`, `migrate`, `inbox`. `Carrel-Snapshot` names the content backup taken with the change, when there is one. `Carrel-Items` may repeat.

- **Moves:** each item a change moves is recorded, in the order it was moved, as a `Carrel-Move` trailer whose value is a one-line JSON object: `Carrel-Move: {"item":"01928f3a-…","from":"Research/Code/sim","to":"Research/Projects/sim"}`. Paths are the item's (not the card's), relative to the archive root. Git sees card moves itself; these trailers let undo move item contents, which history doesn't track.
- **Empty changes** that leave every tracked file unchanged are not committed.
- The commit author and committer are `Carrel <carrel@localhost>`; who made the change is in the trailers.

### 8.3 Undo

Undoing a change creates a new commit that restores the previous state, with `Carrel-Operation` set to the original operation and a `Carrel-Reverts:` trailer naming the reverted commit. History is never rewritten, except by purge (§10.2).

- Items the change moved are moved back first, in reverse order. If any item is no longer where the change put it, or its old location is taken, nothing is changed.
- Tracked files are then restored to their state before the change, merged with any later changes. If a later change touched the same lines, nothing is changed and the conflicting files are reported.
- The undo commit carries reversed `Carrel-Move` trailers, so it can itself be undone.
- Undo restores metadata and item locations, not item contents; restoring contents is the backup system's job.

### 8.4 The writer lock

On a local or external disk, the writer is whoever holds `_System/writer.lock`, a JSON file:

```json
{
  "token": "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b",
  "person": "01928f39-eeee-7fff-8000-000000000005",
  "device": "01928f3a-0000-7000-8000-00000000d001",
  "purpose": "ingest",
  "program": "carrel 0.1.0",
  "acquired": "2026-10-29T14:02:11.137-07:00",
  "expires": "2026-10-29T14:07:11.137-07:00"
}
```

- **Taking the lock:** a program writes the complete lock to `writer.lock.new-<token>`, hard-links it to `writer.lock` and deletes the temporary file. Linking fails if the lock exists, so of several programs trying at once exactly one succeeds, and no reader sees a half-written lock. On file systems without hard links, the program creates `writer.lock` exclusively instead and readers allow a moment for it to be written. `token` is a new UUID for each lease.
- **Leases:** the lock is a lease that ends at `expires` (RFC 3339). A writer doing a long operation renews it before then by writing a new `expires`, replacing the file atomically (write a temporary file `writer.lock.renew-<token>` beside it, then rename it over the lock). The default lease is five minutes.
- **Before each commit** the writer re-reads the lock and commits only if it still holds its own `token` and has not expired. A writer whose lease was lost stops and does not commit.
- **Expired locks** may be taken over: rename the lock to `writer.lock.expired-<token>` (only one program's rename succeeds), delete it, and take the lock as usual. A lock that is not valid JSON is never taken over automatically; a person must confirm no writer is running before it is removed.
- **Releasing:** the writer deletes the lock when it finishes, but only while it still holds its own `token`.
- **Synced folders:** a lock file is only safe while a single device uses the archive. Sync services such as iCloud Drive can let two devices each create the file before either sees the other's, so archives shared between devices use a coordinator the sync service supports instead (to be specified with iCloud support). Programs should warn when the lock is used inside a synced folder.

## 9. Active items and the Journal

### 9.1 Checkout

While an item is checked out, its card has:

```yaml
checked_out:
  to: "~/Desktop/example-repo"
  device: 01928f3a-0000-7000-8000-00000000d001
  person: 01928f39-eeee-7fff-8000-000000000005
  since: "2026-10-29T09:12:00-07:00"
  due: "2026-11-12"           # optional
  manifest: "sha256:…"        # manifest checksum at checkout
  git_head: "3f9a…"           # optional: the commit a Git repository was at
```

The archive copy **must not** be modified while `checked_out` is present, except by the return that removes it.

Where backups are set up (`backups`, §7.2), programs **may** snapshot the working copy of a checked-out or desk item (§9.4) after each work session. They **must not** when its card has `snapshots: return-only`.

### 9.2 `Active/`

`Active/` is generated by programs and contains no owner content:

- `Active/_index.md`: a dashboard of everything checked out or on a desk.
- `Active/Checked out/<title>.md`: one note per checked-out item, linking to its card and its working location. Names are made unique by adding the start of the item's `id` when two titles collide.
- `Active/On desk/<title>.md`: mirrors of desk-item cards (§9.4), named the same way and refreshed while the archive is available. Owner notes written in a mirror's body are copied back to the desk card.

A mirror's frontmatter is `carrel: desk-mirror`, the desk card's `id`, `card` (its absolute path on its device) and `mirrored` (the checksum, §2.6, of the card's notes when the mirror was written). Its body is a managed section `mirror` (§5.3) that programs write, followed by the desk card's body. The **notes** are a body without the `mirror` section and with the history section's content left out, table padding normalized. When a mirror's notes no longer match `mirrored`, the owner edited them there: programs copy them to the desk card, keeping the card's history section, or, if the card's notes changed too, add them to the card before its history section, so nothing is lost. A mirror of a desk card the device can't read, such as another device's, is left as it is.

### 9.3 Journal

`Journal/<year>/<year>-<month>.md` lists the month's approved work sessions across all items, newest first, each linking to its card. Journal files are generated from cards and may be regenerated at any time.

### 9.4 Desk items

A desk item is tracked but stored outside the archive. Its card (with `carrel: card`) lives beside it on the device's desk and follows §5. The device's `desk.json` (§7.10) lists it, and `Active/On desk/` holds a mirror of its card. When it's archived, its proposal (§7.9) names the desk card in `desk_card`; the filed card keeps the desk card's `id` and body, history included, and programs then remove the desk card.

## 10. Removal

### 10.1 Deaccession

Deaccessioning removes an item from the archive and replaces its card with a tombstone:

```yaml
kind: tombstone
removed:
  at: "2026-10-29T16:00:00-07:00"
  reason: "Duplicate of the published version"
  former_kind: file
  former_path: "Research/Literature/Liu2022-preprint.pdf"
  recoverable_from: "kopia:k3f9a1c0e2b4d6"
```

A tombstone keeps the original `id`, `title` and `links`, so links to it still resolve to an explanation.

### 10.2 Purge

Purging erases an item everywhere: its files, its card, its manifest, every mention in the history repository, and its content in backups. After a purge, the history contains one commit with `Carrel-Operation: purge` and no information about what was purged. Programs **must** require explicit confirmation for each purge.

## 11. Reserved for later versions

These fields and names are reserved so later versions can add the features without migrating archives. Version 1.0 readers **must** preserve them and **may** otherwise ignore them.

| Reserved | For |
| --- | --- |
| `encrypted` in `_collection.md` | Encrypted collections: scheme and key identifier; item contents encrypted, cards limited to chosen fields |
| `offline` in `_collection.md` | Collections stored on another disk or archive, with a cached catalog |
| `snapshot` in `url` cards | Local copy of a web page (web archive or PDF) beside the card |
| `_System/peers/` | Catalogs of other archives |
| `_System/privacy/` | Outbound log for remote model use |
| `_System/packages/` | Collection packages in transit |
| `person` values beyond one owner | Shared archives |

## 12. Versioning and compatibility

- The format version is `MAJOR.MINOR`, recorded in `archive.json`, `ARCHIVE.md` and each card's `format`.
- A **minor** version only adds optional fields, sections or files. Readers of an older minor version keep working and preserve what they don't understand.
- A **major** version may change meaning. Carrel aims never to need one. If it does, a program that understands both versions migrates an archive only after the owner approves a proposal, and only after a backup snapshot.
- A program **must** open an archive with a newer major version read-only, and **should** open one with a newer minor version normally while preserving unknown data.
- Cards may carry older minor versions than the archive; programs update a card's `format` when they next rewrite it.

## 13. Conformance checklist

A program that writes Carrel archives conforms to version 1.0 if it:

- [ ] writes UTF-8, LF, the YAML subset in §2.1 and the frontmatter rules in §2.2;
- [ ] gives every archive, collection, item and device a permanent UUIDv7;
- [ ] never writes inside hard-sealed bundles, and only a `README.md` inside soft-sealed ones;
- [ ] keeps exactly one card per item, named as in §5.1;
- [ ] preserves unknown frontmatter fields, unknown JSON keys, and all owner text outside managed sections;
- [ ] records every change as one commit with the trailers in §8.2;
- [ ] never modifies a checked-out item's archive copy;
- [ ] opens newer major versions read-only.
