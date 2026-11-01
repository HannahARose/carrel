# Interfaces

Every interface is fully capable ([Architecture](architecture.md), idea 4): anything Carrel can do is available from the command line, the interactive mode and the app, on every platform each runs on, wherever the interface and platform make it possible. This page is the record of where each capability stands, and of the deviations that are allowed. It's kept up to date with each change that adds a capability or closes a gap.

The command line and the interactive mode run on macOS, Linux and Windows. The Mac app arrives in v0.5. The iPad and iPhone app follows in v0.8, before 1.0 (#471): the Mac app's own code, built for iOS, with the same screens laid out for each size. Linux and Windows apps come later, in v2.0 (#270); until then, the command line and the interactive mode are the fully capable interfaces on those platforms.

On iPad and iPhone, where the Mac column names a keyboard shortcut, an iPad with a keyboard has the same one, and the inspector, context menus and toolbars offer the same action by touch. What only a Mac can do is listed under [Allowed deviations](#allowed-deviations).

## Where each capability stands

✓ is available, or being built for v0.5; a number is the issue that adds it.

| Capability | Command line | Interactive mode | Mac app | iPad and iPhone app |
| --- | --- | --- | --- | --- |
| Create, open and switch archives | `init`, `open` | ✓ `Ctrl-O`, Settings | ✓ | ✓ Archives: on this device, or in another place such as iCloud Drive |
| Open the interactive mode or the app at a screen, with the same names (#450) | `tui SCREEN`, `gui SCREEN [--item]` | ✓ starts on it | ✓ `carrel://open?screen=`; Ingest is Status, the librarian its panel, Settings its window | ✓ `carrel://open?screen=`; Settings is its sheet |
| Create an archive with a chosen embedding model, or start history for one that has none | `init --embedding-model`, `init` in the archive | ✓ `Ctrl-O` | ✓ New Archive…'s embedding model; Start History on Status and History | ✓ New Archive's embedding model; Start History on Status and History |
| Status, problems and Recent | `status` | ✓ Status | ✓ Status: Recent, cards with problems, names to rename, restructure signals, desk suggestions | ✓ |
| Add items to the intake queue | `ingest` | ✓ Ingest | ✓ drag, ⌘O (#236), a folder entry by entry or whole | ✓ the + menu: Photos, Take Photo, Scan Document, Files |
| Add a web address as a `url` item, and a note for the cards | `ingest --url`, `--title`, `--note` | ✓ a web address on the Ingest line, `n` for a note | ✓ Send to Archive and Add to Archive in Shortcuts; the window: #439 | ✓ the share sheet; the window: #439 |
| Add while another device is the writer, through this device's inbox | `ingest` | ✓ Ingest | ✓ Send to Archive, the Quick Action, Shortcuts | ✓ the share sheet and the + menu |
| The intake queue: list, remove, move to the front, try again | `ingest --status`, `--remove`, `--first`, `--retry` | ✓ Ingest | ✓ Status, with each failure's reason | ✓ |
| The worker's progress | `ingest --status` | ✓ | ✓ (#236) | ✓ |
| Review: approve, hold, move, reject | `review` | ✓ Review | ✓ (#234) | ✓ |
| Review: edit and correct a proposal | `review edit`, `review correct` | ✓ | ✓ (#234) | ✓ |
| Review: expand or keep compressed files | `review expand`, `keep-compressed` | ✓ | ✓ (#234) | ✓ |
| Review: duplicates (keep both, file as a version) | `review` | ✓ | ✓ (#234) | ✓ |
| Review: draft a proposal again with the models here now, keeping what you set | `review redraft`, `--all-failed` | ✓ `d`, offered first when the card or collection failed | ✓ Draft Again (`d`, context menu, inspector) (#445) | ✓ |
| Questions: answer and dismiss | `questions` | ✓ Questions | ✓ (#234) | ✓ |
| Questions: ranked, with suggested answers, skip | `questions`, `questions skip` | ✓ Questions (`1`–`9`, `s`) | ✓ Questions (`s`, Show Skipped) | ✓ |
| Questions about an item, a group, a sender | `questions answer`, `questions about` | ✓ Questions; `A` in Browse asks for an item's | ✓ Questions; Carrel Asks under an item's card | ✓ |
| Questions show what they're about: each item's path, and its preview | `questions` (each item's path) | ✓ Questions: the preview where the terminal draws images, else the path | ✓ Questions: Quick Look, or thumbnails for a group, with Open (#446) | ✓ |
| Questions while browsing and wandering, from the librarian, and a few a day on Status | `show`, `discover wander`, `ask`, `status` | ✓ Browse, Discover, Librarian, Status (`a`) | ✓ inspector, Discover, Librarian, Status | ✓ |
| Browse collections and items | `show`, `status` | ✓ Browse | ✓ Browse | ✓ |
| Previews of items | the item's path in `show` | ✓ Browse, in terminals that draw images (#258) | ✓ inspector and grid (#256) | ✓ inspector and grid, with Quick Look |
| Card previews outside Carrel | `show` | ✓ Browse | ✓ Quick Look in Finder, with Carrel's drawings of STEP, IGES and 3MF (#410) | — Quick Look in Finder is the Mac's; Files previews the files themselves |
| An item's history and related items | `show` | ✓ | ✓ the inspector's History and Related | ✓ |
| Search, with filters and reasons | `search` | ✓ `/` | ✓ ⌘F (#236) | ✓ |
| Search from outside Carrel | `search` | ✓ `/` | ✓ Spotlight finds cards, collections and glossary facts, by `spotlight.index`; a result opens the item in Carrel (#411) | — Spotlight on iPad and iPhone: #SPOTLIGHT |
| Discovery: on this day, timeline | `discover today`, `discover timeline` | ✓ Discover (`D`) | ✓ Discover | ✓ |
| Discovery: the timeline's words, filters and date | `discover timeline QUERY --by` | ✓ `/` and `b` on the timeline | ✓ Discover | ✓ |
| Discovery: wander the stacks | `discover wander` | ✓ Discover, with previews where the terminal draws them | ✓ Discover, with previews | ✓ |
| Discovery: wander from an item | `discover wander ITEM` | ✓ `w` in Browse | ✓ Wander from Here on an item | ✓ |
| Discovery: the archive map, activity | `discover map`, `discover activity` | ✓ a sized tree and a block heatmap | ✓ a treemap and a heatmap | ✓ |
| Discovery: activity over chosen days | `discover activity --from --to` | ✓ `r` on activity | ✓ From and To on Activity | ✓ |
| Discovery: the tag and link graph | `discover graph [--items]` | ✓ Discover's graph (`g`), the strongest connections as text (#326) | ✓ Discover → Connections, drawn | ✓ |
| Resurfacing forgotten collections | `status` | ✓ Status | ✓ Status | ✓ |
| The embedding index: status, rebuild, switch models | `index` | ✓ Status, Settings | ✓ Settings → Search (#267) | ✓ Settings → Search; rebuilding on an iPhone only while charging |
| Correct a filed item | `correct` | ✓ `c` | ✓ ⌘K (#234) | ✓ |
| Edit a filed item's card, every field a person sets | `edit` | ✓ `e` | ✓ ⌘E (#234, #415) | ✓ |
| Edit a filed item's card body, its notes, Carrel's sections kept | `edit --body` | ✓ `E` in Browse | ✓ ⌘E, Notes and Body | ✓ |
| Create collections | `collection` | ✓ `n` in Browse | ✓ ⌥⌘N (#268) | ✓ |
| Rename an item | `rename` | ✓ `r` in Browse | ✓ ⇧⌘R (#268) | ✓ |
| Expand a filed compressed file | `expand` | ✓ `z` in Browse | ✓ ⇧⌘E (#268) | ✓ |
| Flag an item or a collection that needs work, and list what's flagged | `flag`, `unflag`, `flagged` | ✓ `f` and `F` in Browse, ⚑ in the lists, Status | ✓ Flag… and Unflag in context menus and the inspector, a flag in the list and grid, Flagged in the sidebar, Status (#451) | ✓ |
| Check out an item | `checkout` | ✓ `o` in Browse | ✓ (#235) | ✓ to Carrel's folder in Files, or a place chosen, such as iCloud Drive |
| Return or cancel a checkout | `return` | ✓ Active | ✓ (#235) | ✓ |
| Return a working copy that moved, and trim its session snapshots | `return --from`, `--trim-snapshots` | ✓ `m`, and `t` when asked, in Active | ✓ Return From…, and the return sheet's snapshots toggle | ✓ Return From… (the document picker), and the return sheet's snapshots toggle |
| The working copy after a return: Trash, keep or ask (`checkout.after_return`) | `return --working-copy trash\|keep` | ✓ asked once it's back, when the setting says to ask | ✓ asked once it's back, when the setting says to ask | ✓ |
| List checkouts | `checkouts` | ✓ Active | ✓ (#235) | ✓ |
| Desk items: add, remove, archive | `desk` | ✓ Active | ✓ Active: Take Off Desk… keeps or deletes the card, or archives | ✓ Active, choosing the file or folder in the document picker |
| Desk items: a title and summary for the card | `desk add --title --summary` | ✓ asked when adding | ✓ asked when adding | ✓ |
| Work-session entries: approve, edit, combine, drop | `sessions` | ✓ Active | ✓ (#235) | ✓ |
| History and undo | `log`, `undo` | ✓ History | ✓ History, a page at a time or all, and the librarian's alone | ✓ History |
| Restructure tags: merge, split and retire, approved all or in part | `restructure tags` | ✓ Review's restructure proposals (`f`): before and after, approve all or part; `t` proposes | ✓ Restructure, before and after side by side | ✓ |
| Restructure a section: propose, list, show, approve all or part, reject | `restructure propose`, `list`, `show`, `approve`, `reject` | ✓ Review's restructure proposals (`f`): before and after, approve all or part; `p` proposes for the archive, `R` in Browse for a collection | ✓ Restructure: Propose Changes…, how it was judged, before and after | ✓ |
| How eager to rearrange: archive-wide, and per collection | `settings set restructure.eagerness`, `restructure eagerness COLLECTION` | ✓ Settings (archive-wide); `E` on a collection in Browse | ✓ Settings (archive-wide); a collection's inspector | ✓ |
| The librarian: ask, converse, save to the Journal | `ask` | ✓ the Librarian screen (`L`) | ✓ the Librarian panel (⌥⌘L), Ask About This (⇧⌘L) | ✓ |
| The librarian's changes: list and undo them together | `status`, `log --librarian`, `undo --librarian` | ✓ the Librarian screen, Status and History | ✓ the Librarian panel, Status and History | ✓ |
| The crawler: how far it has got, a pass now, its findings, its changes and undoing them | `crawl status`, `crawl run`, `review`, `status`, `crawl undo` | ✓ Review's crawler findings (`f`): how far it has got, `r` a pass now, `u` undo this week's; its changes on Status and in History (#326) | ✓ Review (From the Crawler, `f`), Status, History, the agent and its menu bar list | ✓ Review (From the Crawler), Status, History |
| Backups: set up, snapshot, list, verify, restore | `backup` | ✓ Backups | ✓ Backups (#266) | — Kopia runs only on a Mac; replaced content waits in the holding folder (#470) |
| Backups: the repository's name | `backup setup --description` | ✓ asked on set up | ✓ the setup sheet | — on a Mac |
| Backups: how much to verify, where to restore | `backup verify --percent`, `backup restore --to` | ✓ asked along the way | ✓ Verify… asks how much; Restore asks where | — on a Mac |
| Settings, this device's and the archive's (each listed below) | `settings` | ✓ Settings | ✓ Settings → General (#267, #342) | ✓ Settings → General, and This Device |
| Models: list, download, remove, as GGUF or MLX | `models` | ✓ Settings (#342) | ✓ Settings → Models (#267, #342) | ✓ Settings → Models |
| Providers in use, and file types not read | `providers`, `formats` | ✓ Settings | ✓ Settings → Providers (#267) | ✓ Settings → Providers |
| The usage report and its decision log, on or off | `report`, `settings set report.log` | ✓ Report on Status, Settings | the calls are in CarrelKit; the Report view #327 | the calls are in CarrelKit; the Report view #327 |
| Gaps: what Carrel couldn't do, an issue draft for each opened in the browser, reported, and missing features in the owner's words | `report gaps` | ✓ Gaps on Status (`g`); `M` on an item in Browse | ✓ Status's Gaps; Report a Missing Feature… on an item (#455) | ✓ |
| Background work: sessions as they happen, the queue, the index, scheduled snapshots | `agent`, `agent install` (#269) | ✓ Settings (#279) | ✓ agent (#237) | ✓ background tasks draft the queue and apply inboxes; an iPhone only while charging; no snapshots (Kopia) |
| Reminders: entries waiting, checkouts due back, the week's sum, as notifications and on Status | `reminders`, `status`; `agent` notifies through `notify-send` or a PowerShell toast | ✓ Status (`Enter` goes to the item) | ✓ Status; the agent's notifications; `menu-bar` hides its menu bar list | ✓ Status; notifications: #NOTIFY |
| What's checked out, at a glance | `checkouts` | ✓ Active | ✓ menu bar (#237) | ✓ Active |

## Settings

Each setting, and where it's changed. `carrel settings set KEY VALUE` changes every one; the interactive mode's Settings screen and the app's Settings window build the archive's rows from the core's list of settings (`carrel_core::settings::Key`), so a new one reaches both.

| Setting | Interactive mode | Mac app | iPad and iPhone app |
| --- | --- | --- | --- |
| `picker`, the file picker, with the pickers Carrel knows and the one in use | Settings | Settings → General | Settings → General |
| `editor`, the card editor, with the one in use | Settings | Settings → General | Settings → General |
| `model` and `second_model`, the text models that draft cards | Settings | Settings → Models | Settings → Models |
| `librarian.model`, the librarian's model | Settings | Settings → Models | Settings → Models |
| `report.log`, `report.days`, the decision log for the usage report | Settings | #327 | #327 |
| `previews`, how Browse draws pictures | Settings, applied at once | (not used: the app draws previews itself) | (not used: the app draws previews itself) |
| `ingest.originals`, `compressed_files`, `signals.crowded_items`, `questions.skip_days`, `outside_edits` | Settings | Settings → General | Settings → General |
| `checkout.destination`, `checkout.after_return`, `sessions.idle_minutes`, `reminders` | Settings | Settings → General | Settings → General |
| `librarian.changes`, `librarian.confidence` | Settings | Settings → General | Settings → General |
| `spotlight.index`, what a Mac's Spotlight finds (#411) | Settings | Settings → General | Settings → General (it decides what a Mac shows) |
| `embedding.model`, chosen once, then switched | Settings, the index's row | Settings → Search | Settings → Search |
| `backups` | Backups | Backups, and its status in Settings → Backups | — on a Mac |

## Allowed deviations

These stay in one interface, for the reason given:

- **`carrel bench`** (the model benchmark) and **`carrel models embed`** (printing vectors) are tools for developing Carrel, not for using it, so they stay on the command line.
- **`carrel worker`** is what the other interfaces start in the background; nobody needs a screen for it.
- **`carrel agent` runs in the foreground** until stopped, because it's what the operating system's service manager starts at log-in (`carrel agent install`). The interactive mode shows and installs it (#279) rather than running it while open. The Mac app has its own agent, a login item, with the same schedule; one agent looks after an archive on each device, and the other steps aside.
- **The `previews` setting** chooses how a terminal draws pictures, so only the interactive mode has it.
- **Previews in a terminal** need a terminal that can draw images (#258). Elsewhere the interactive mode shows the card. The command line's output is text, so `carrel show` gives the card and the item's path instead.
- **Quick Look** is how macOS previews files in Finder, so only the Mac has it. `carrel show` and Browse show the same card (`docs/design/macos-integration.md`).
- **Spotlight** is how macOS searches everything at once, so only the Mac has it. `carrel search` and `/` find the same cards. `spotlight.index` is an archive setting in every interface, since it decides what the archive shows anyone at a Mac.
- **Grid views, dragging and the menu bar** are ways of working in a graphical app. The command line and the interactive mode reach the same capabilities through their own commands and screens, as the table shows.
- **Platform-only models and readers,** such as Apple's model or Vision's OCR, are available in every interface on the platforms that have them. Elsewhere, the open models and the extraction provider stand in.
- **Backups on iPad and iPhone:** Kopia runs only on a Mac, so the iPad and iPhone app can't set up, take, verify or restore snapshots. What a change there replaces or removes is kept in the archive's holding folder until a Mac snapshots it (`docs/design/sync-and-mobile.md`, decision 5), and Status says how much is waiting.
- **The agent, its menu bar list, and the Finder and Quick Look extensions** are how macOS runs things in the background and in Finder, so only the Mac has them. On iPad and iPhone, background tasks drain the intake queue and apply inboxes when the system gives the app time, and the share extension stands in for Send to Archive.
- **The iPhone's battery:** drafting and rebuilding the index wait until an iPhone is charging, and an iPad's wait in Low Power Mode; reviewing, answering, browsing and searching work at any time.
