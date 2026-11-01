# Interfaces

Every interface is fully capable ([Architecture](architecture.md), idea 4): anything Carrel can do is available from the command line, the interactive mode and the app, on every platform each runs on, wherever the interface and platform make it possible. This page is the record of where each capability stands, and of the deviations that are allowed. It's kept up to date with each change that adds a capability or closes a gap.

The command line and the interactive mode run on macOS, Linux and Windows. The Mac app arrives in v0.5. The iPad and iPhone apps follow in v0.9, before 1.0, and Linux and Windows apps later, in v2.0 (#270); until then, the command line and the interactive mode are the fully capable interfaces on those platforms.

## Where each capability stands

✓ is available, or being built for v0.5; a number is the issue that adds it.

| Capability | Command line | Interactive mode | Mac app |
| --- | --- | --- | --- |
| Create, open and switch archives | `init`, `open` | ✓ `Ctrl-O`, Settings | ✓ |
| Create an archive with a chosen embedding model, or start history for one that has none | `init --embedding-model`, `init` in the archive | ✓ `Ctrl-O` | ✓ New Archive…'s embedding model; Start History on Status and History |
| Status, problems and Recent | `status` | ✓ Status | ✓ Status: Recent, cards with problems, names to rename, restructure signals, desk suggestions |
| Add items to the intake queue | `ingest` | ✓ Ingest | ✓ drag, ⌘O (#236), a folder entry by entry or whole |
| The intake queue: list, remove, move to the front, try again | `ingest --status`, `--remove`, `--first`, `--retry` | ✓ Ingest | ✓ Status, with each failure's reason |
| The worker's progress | `ingest --status` | ✓ | ✓ (#236) |
| Review: approve, hold, move, reject | `review` | ✓ Review | ✓ (#234) |
| Review: edit and correct a proposal | `review edit`, `review correct` | ✓ | ✓ (#234) |
| Review: expand or keep compressed files | `review expand`, `keep-compressed` | ✓ | ✓ (#234) |
| Review: duplicates (keep both, file as a version) | `review` | ✓ | ✓ (#234) |
| Questions: answer and dismiss | `questions` | ✓ Questions | ✓ (#234) |
| Questions: ranked, with suggested answers, skip | `questions`, `questions skip` | ✓ Questions (`1`–`9`, `s`) | ✓ Questions (`s`, Show Skipped) |
| Questions about an item, a group, a sender | `questions answer`, `questions about` | ✓ Questions; `A` in Browse asks for an item's | ✓ Questions; Carrel Asks under an item's card |
| Questions while browsing and wandering, from the librarian, and a few a day on Status | `show`, `discover wander`, `ask`, `status` | ✓ Browse, Discover, Librarian, Status (`a`) | ✓ inspector, Discover, Librarian, Status |
| Browse collections and items | `show`, `status` | ✓ Browse | ✓ Browse |
| Previews of items | the item's path in `show` | ✓ Browse, in terminals that draw images (#258) | ✓ inspector and grid (#256) |
| An item's history and related items | `show` | ✓ | ✓ the inspector's History and Related |
| Search, with filters and reasons | `search` | ✓ `/` | ✓ ⌘F (#236) |
| Discovery: on this day, timeline | `discover today`, `discover timeline` | ✓ Discover (`D`) | ✓ Discover |
| Discovery: the timeline's words, filters and date | `discover timeline QUERY --by` | ✓ `/` and `b` on the timeline | ✓ Discover |
| Discovery: wander the stacks | `discover wander` | ✓ Discover, with previews where the terminal draws them | ✓ Discover, with previews |
| Discovery: wander from an item | `discover wander ITEM` | ✓ `w` in Browse | ✓ Wander from Here on an item |
| Discovery: the archive map, activity | `discover map`, `discover activity` | ✓ a sized tree and a block heatmap | ✓ a treemap and a heatmap |
| Discovery: activity over chosen days | `discover activity --from --to` | ✓ `r` on activity | ✓ From and To on Activity |
| Discovery: the tag and link graph | `discover graph [--items]` | ✓ Discover's graph (`g`), the strongest connections as text (#326) | ✓ Discover → Connections, drawn |
| Resurfacing forgotten collections | `status` | ✓ Status | ✓ Status |
| The embedding index: status, rebuild, switch models | `index` | ✓ Status, Settings | ✓ Settings → Search (#267) |
| Correct a filed item | `correct` | ✓ `c` | ✓ ⌘K (#234) |
| Edit a filed item's card | `edit` | ✓ `e` | ✓ ⌘E (#234), its Notes and body too |
| Create collections | `collection` | ✓ `n` in Browse | ✓ ⌥⌘N (#268) |
| Rename an item | `rename` | ✓ `r` in Browse | ✓ ⇧⌘R (#268) |
| Expand a filed compressed file | `expand` | ✓ `z` in Browse | ✓ ⇧⌘E (#268) |
| Check out an item | `checkout` | ✓ `o` in Browse | ✓ (#235) |
| Return or cancel a checkout | `return` | ✓ Active | ✓ (#235) |
| Return a working copy that moved, and trim its session snapshots | `return --from`, `--trim-snapshots` | ✓ `m`, and `t` when asked, in Active | ✓ Return From…, and the return sheet's snapshots toggle |
| List checkouts | `checkouts` | ✓ Active | ✓ (#235) |
| Desk items: add, remove, archive | `desk` | ✓ Active | ✓ Active: Take Off Desk… keeps or deletes the card, or archives |
| Desk items: a title and summary for the card | `desk add --title --summary` | ✓ asked when adding | ✓ asked when adding |
| Work-session entries: approve, edit, combine, drop | `sessions` | ✓ Active | ✓ (#235) |
| History and undo | `log`, `undo` | ✓ History | ✓ History, a page at a time or all, and the librarian's alone |
| Restructure tags: merge, split and retire, approved all or in part | `restructure tags` | ✓ Review's restructure proposals (`f`): before and after, approve all or part; `t` proposes | ✓ Restructure, before and after side by side |
| Restructure a section: propose, list, show, approve all or part, reject | `restructure propose`, `list`, `show`, `approve`, `reject` | ✓ Review's restructure proposals (`f`): before and after, approve all or part; `p` proposes for the archive, `R` in Browse for a collection | ✓ Restructure: Propose Changes…, how it was judged, before and after |
| How eager to rearrange: archive-wide, and per collection | `settings set restructure.eagerness`, `restructure eagerness COLLECTION` | ✓ Settings (archive-wide); `E` on a collection in Browse | ✓ Settings (archive-wide); a collection's inspector |
| The librarian: ask, converse, save to the Journal | `ask` | ✓ the Librarian screen (`L`) | ✓ the Librarian panel (⌥⌘L), Ask About This (⇧⌘L) |
| The librarian's changes: list and undo them together | `status`, `log --librarian`, `undo --librarian` | ✓ the Librarian screen, Status and History | ✓ the Librarian panel, Status and History |
| The crawler: how far it has got, a pass now, its findings, its changes and undoing them | `crawl status`, `crawl run`, `review`, `status`, `crawl undo` | ✓ Review's crawler findings (`f`): how far it has got, `r` a pass now, `u` undo this week's; its changes on Status and in History (#326) | ✓ Review (From the Crawler, `f`), Status, History, the agent and its menu bar list |
| Backups: set up, snapshot, list, verify, restore | `backup` | ✓ Backups | ✓ Backups (#266) |
| Backups: the repository's name | `backup setup --description` | ✓ asked on set up | ✓ the setup sheet |
| Backups: how much to verify, where to restore | `backup verify --percent`, `backup restore --to` | ✓ asked along the way | ✓ Verify… asks how much; Restore asks where |
| Settings, this device's and the archive's | `settings` | ✓ Settings | ✓ Settings → General (#267) |
| Models: list, download, remove | `models` | ✓ Settings | ✓ Settings → Models (#267) |
| Providers in use, and file types not read | `providers`, `formats` | ✓ Settings | ✓ Settings → Providers (#267) |
| The usage report and its decision log, on or off | `report`, `settings set report.log` | ✓ Report on Status, Settings | the calls are in CarrelKit; the Report view #327 |
| Background work: sessions as they happen, the queue, the index, scheduled snapshots | `agent`, `agent install` (#269) | ✓ Settings (#279) | ✓ agent (#237) |
| What's checked out, at a glance | `checkouts` | ✓ Active | ✓ menu bar (#237) |

## Allowed deviations

These stay in one interface, for the reason given:

- **`carrel bench`** (the model benchmark) and **`carrel models embed`** (printing vectors) are tools for developing Carrel, not for using it, so they stay on the command line.
- **`carrel worker`** is what the other interfaces start in the background; nobody needs a screen for it.
- **`carrel agent` runs in the foreground** until stopped, because it's what the operating system's service manager starts at log-in (`carrel agent install`). The interactive mode shows and installs it (#279) rather than running it while open. The Mac app has its own agent, a login item, with the same schedule; one agent looks after an archive on each device, and the other steps aside.
- **Previews in a terminal** need a terminal that can draw images (#258). Elsewhere the interactive mode shows the card. The command line's output is text, so `carrel show` gives the card and the item's path instead.
- **Grid views, dragging and the menu bar** are ways of working in a graphical app. The command line and the interactive mode reach the same capabilities through their own commands and screens, as the table shows.
- **Platform-only models and readers,** such as Apple's model or Vision's OCR, are available in every interface on the platforms that have them. Elsewhere, the open models and the extraction provider stand in.
