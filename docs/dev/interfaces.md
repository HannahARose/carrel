# Interfaces

Every interface is fully capable ([Architecture](architecture.md), idea 4): anything Carrel can do is available from the command line, the interactive mode and the app, on every platform each runs on, wherever the interface and platform make it possible. This page is the record of where each capability stands, and of the deviations that are allowed. It's kept up to date with each change that adds a capability or closes a gap.

The command line and the interactive mode run on macOS, Linux and Windows. The Mac app arrives in v0.5. The iPad and iPhone apps follow in v2.0, and Linux and Windows apps later still (#270); until then, the command line and the interactive mode are the fully capable interfaces on those platforms.

## Where each capability stands

✓ is available, or being built for v0.5; a number is the issue that adds it.

| Capability | Command line | Interactive mode | Mac app |
| --- | --- | --- | --- |
| Create, open and switch archives | `init`, `open` | #265 | ✓ |
| Status, problems and Recent | `status` | ✓ Status | ✓ Status |
| Add items to the intake queue | `ingest` | ✓ Ingest | ✓ drag, ⌘O (#236) |
| The worker's progress | `ingest --status` | ✓ | ✓ (#236) |
| Review: approve, hold, move, reject | `review` | ✓ Review | ✓ (#234) |
| Review: edit and correct a proposal | `review edit`, `review correct` | ✓ | ✓ (#234) |
| Review: expand or keep compressed files | `review expand`, `keep-compressed` | ✓ | ✓ (#234) |
| Review: duplicates (keep both, file as a version) | `review` | ✓ | ✓ (#234) |
| Questions: answer and dismiss | `questions` | ✓ Questions | ✓ (#234) |
| Browse collections and items | `show`, `status` | ✓ Browse | ✓ Browse |
| Previews of items | the item's path in `show` | #258 | #256 |
| An item's history and related items | `show` | ✓ | ✓ |
| Search, with filters and reasons | `search` | ✓ `/` | ✓ ⌘F (#236) |
| The embedding index: status, rebuild, switch models | `index` | #263 | #267 |
| Correct a filed item | `correct` | ✓ `c` | ✓ ⌘K (#234) |
| Edit a filed item's card | `edit` | ✓ `e` | ✓ ⌘E (#234) |
| Create collections | `collection` | ✓ `n` in Browse | ✓ ⌥⌘N (#268) |
| Rename an item | `rename` | ✓ `r` in Browse | ✓ ⇧⌘R (#268) |
| Expand a filed compressed file | `expand` | ✓ `z` in Browse | ✓ ⇧⌘E (#268) |
| Check out an item | `checkout` | ✓ `o` in Browse | ✓ (#235) |
| Return or cancel a checkout | `return` | ✓ Active | ✓ (#235) |
| List checkouts | `checkouts` | ✓ Active | ✓ (#235) |
| Desk items: add, remove, archive | `desk` | ✓ Active | ✓ (#235) |
| Work-session entries: approve, edit, combine, drop | `sessions` | ✓ Active | ✓ (#235) |
| History and undo | `log`, `undo` | ✓ History | ✓ (#235) |
| Backups: set up, snapshot, list, verify, restore | `backup` | ✓ Backups | #266 |
| Settings, this device's and the archive's | `settings` | ✓ Settings | #267 |
| Models: list, download, remove | `models` | ✓ Settings | #267 |
| Providers in use, and file types not read | `providers`, `formats` | #265 | #267 |
| Background work: sessions as they happen, the queue, the index | #269 | #269 | ✓ agent (#237) |
| What's checked out, at a glance | `checkouts` | ✓ Active | ✓ menu bar (#237) |

## Allowed deviations

These stay in one interface, for the reason given:

- **`carrel bench`** (the model benchmark) and **`carrel models embed`** (printing vectors) are tools for developing Carrel, not for using it, so they stay on the command line.
- **`carrel worker`** is what the other interfaces start in the background; nobody needs a screen for it.
- **Previews in a terminal** need a terminal that can draw images (#258). Elsewhere the interactive mode shows the card. The command line's output is text, so `carrel show` gives the card and the item's path instead.
- **Grid views, dragging and the menu bar** are ways of working in a graphical app. The command line and the interactive mode reach the same capabilities through their own commands and screens, as the table shows.
- **Platform-only models and readers,** such as Apple's model or Vision's OCR, are available in every interface on the platforms that have them. Elsewhere, the open models and the extraction provider stand in.
