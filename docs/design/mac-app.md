# Design: the Mac app (v0.5)

Status: accepted 2026-10-31, with the five recommendations below. Issue #84; built in #232, #233, #91 (with its CI, #44), #234, #235, #236, #256, #237 and #238. Item previews and the grid view (#256) were added on 2026-10-31 at the owner's request, from the design spec's browsing.

## What this adds

Everything Carrel does so far happens in a terminal: `carrel` and its interactive mode. v0.5 is the native Mac app over the same Rust core: filing, the review and question queues, checkout and return, the desk, browsing and search, corrections and history, in a window that looks and behaves like a Mac app. It also adds the background agent that the spec puts at the centre of the Mac version: a login item that notices work sessions, drafts queued items and keeps the search index current while the app is closed, with a menu bar list of what's checked out.

**The gate:** the owner can do everything from the app that they do with the CLI, and uses the app for a week instead of the interactive mode.

The spec settles the look (SwiftUI, Apple's design guidelines and Liquid Glass: sidebar, toolbar, inspector, keyboard shortcuts) and the agent (a login item that owns the models, the writer lock, the backup schedule and file watching; the app and the CLI are its clients). This design fills in how v0.5 gets there, and what waits.

## The window

One window per archive, a three-column `NavigationSplitView`:

- **Sidebar:** the screens the interactive mode has, in the same order, with their counts as badges: **Status**, **Review**, **Questions**, **Active**, then the collection tree (**Browse**), then **History**. A search field sits at the top of the sidebar's toolbar.
- **Content:** the list for the chosen screen: proposals grouped by destination, questions by rank, checked-out and desk items, a collection's items, search results, changes.
- **Inspector:** the chosen thing in full: a proposal's or item's card with its fields, summary, sensitivity and reasons, history and related items; a question with the items it affects; a change with what it did.

Each screen does what its interactive-mode counterpart does, through the same core functions (below), so the two never disagree:

| Screen | In the app |
| --- | --- |
| Status | What `carrel status` shows, with the worker's progress, Recent, and suggestions (folders to track, desk items to archive). |
| Review | Approve, hold, move and reject, one at a time or a selection; batches approve together. The inspector edits the card in place. |
| Questions | Answer a term in a text field or pick a collection; dismiss. |
| Active | Checked-out and desk items with their pending entries: return, cancel, open the working copy, approve, combine, edit or drop entries. |
| Browse | The collection tree in the sidebar, items in the content column as a list or a grid of thumbnails, and the chosen item's preview above its card in the inspector. Quick actions: check out, correct, edit, show in Finder. |
| History | Every change, with undo after a confirmation sheet that says what undoing does and doesn't restore. |

- **Search** is the toolbar field (⌘F): the same `carrel search` with its filters, results as a list with the matched passage highlighted. Typing filters as you go over words; meaning search runs when you pause, so typing never waits on the model.
- **Adding items:** drag files or folders onto the window, or File → Add to Archive… (⌘O). They join the intake queue, as `carrel ingest` adds them, and the agent drafts them.
- **Corrections:** ⌘K on a proposal or an item opens a sheet: type the correction, see the changes the model proposes, confirm. The same flow as `c` in the interactive mode.
- **Editing:** the inspector's fields are editable in place for a proposal, and for a filed item after Edit (⌘E). Saving runs the checks `carrel edit` runs; problems show beside the field.
- **Settings** (⌘,): models and providers (what's installed, downloads with their sizes, which model drafts and which checks), the embedding model, backups, routing rules, and the agent (on or off, when it runs).
- **Keyboard:** each interactive-mode key has a menu command, and the single letters work in lists where they don't collide with typing (`a` approve, `h` hold, `r` return). Every command is in the menus, so it's findable and accessible.

## How the app reaches the core

- **CarrelKit, extended.** The app links the Rust core through CarrelKit's UniFFI bindings (`crates/carrel-ffi`), as the proof of concept (#20) does today. v0.5 grows `carrel-ffi` from opening archives and listing items to the operations the interactive mode uses: `actions` (review and questions), `correct`, `checkout`, `desk`, `sessions`, `search`, `history` and undo, `settings`, `models`, and the intake queue.
- **The core owns the logic; the FFI layer only translates.** Anything the CLI does in `carrel-cli` that the app also needs (choosing what to show on Status, preparing a correction, the worker loop) moves into `carrel-core` first, as review and questions did for v0.2, with the CLI rewritten on top and its tests unchanged.
- **Swift sees an observable archive.** A `@MainActor` `ArchiveModel` in CarrelKit wraps the archive handle, calls the core on a background executor, and publishes what each screen shows. Long operations report progress through a callback, so the app shows the same progress bars the CLI does.
- **One writer at a time, as today.** The app takes the archive's writer lease (`coordinator.rs`, FORMAT.md §8.4) for each change, exactly as the CLI and the interactive mode do, so the app, the CLI and the agent can all be open on one archive. A change that can't get the lease says who holds it.

## Models in the app

- **The same providers, bundled.** The app runs `carrel-provider-apple`, `carrel-provider-mlx`, `carrel-provider-llama` and `carrel-provider-extract` as the CLI does, over the same provider protocol (`docs/providers.md`), from inside the app bundle's `Contents/Helpers`. The app doesn't call Foundation Models, Vision or MLX in-process.
- **Why not in-process:** one path for the CLI and the app means one set of tests and the same results; a provider that crashes or runs out of memory takes down a helper, not the window; and the models folder, catalog and downloads stay shared, so a model downloaded from the CLI is there for the app.
- **Downloads** go through the same catalog and network helper as `carrel models download`, with the size shown and a confirmation, never on their own.

## The background agent

- **`Carrel Agent`, a login item** (`SMAppService`), bundled inside the app. It runs what `carrel worker` runs today, and more:
  - drafts the intake queue as items arrive;
  - notices work sessions on checked-out and desk items as they happen, with file-system events instead of polling every five minutes;
  - keeps the embedding index current after every change;
  - runs scheduled backups once backups are set up.
- **The menu bar list** (a `MenuBarExtra`): what's checked out and on the desk, pending entries, what the agent is doing now, and Pause. Choosing an item opens it in the app.
- **No new protocol between them in v0.5.** The agent, the app and the CLI coordinate the way the CLI and its worker already do: through the archive's files, the writer lease and the worker's status file. The app reads the status file to show progress; the CLI's own detached worker steps aside when the agent is running, through the same per-device lock. An XPC service can come later if the app needs to talk to the agent directly; nothing here prevents it.
- **Off switch:** the agent is optional. With it off, the app starts a worker on demand as the CLI does, and work sessions are noticed when the app or the CLI runs.
- **One worker loop.** The worker loop moves from `carrel-cli/src/worker.rs` into the core, so the CLI's worker and the agent run the same code.

## Building and shipping it

- **An Xcode project** at `apple/CarrelApp/` with two targets, the app and the agent, both depending on CarrelKit and embedding the provider helpers. The project file is committed; nothing generates it.
- **macOS 26 and later**, Apple silicon only, like the XCFramework: Foundation Models and Liquid Glass need it, and every Mac that runs Apple's model has it.
- **Development builds from CI:** each push to dev builds the app on a hosted macOS runner, signed ad hoc, as a zipped `.app` artifact kept for 30 days. Developer ID signing, notarization and TestFlight stay with v1.0 (#47), so there are no signing secrets in CI yet.
- **Locally,** Xcode builds and runs it with the owner's personal team.
- **Tests:** CarrelKit's Swift tests against the sample archive; the FFI layer's Rust tests; and UI tests for the review, correction and checkout flows against a temporary archive, run on the self-hosted runner because they need a logged-in session.

## Previews

- **The inspector shows the chosen item** above its card: Quick Look's own preview (`QLPreviewView`) for anything it can show, such as PDFs, images, documents, video, USDZ and STL, and a folder's contents for folders and bundles. A restricted item shows its card only, with its preview one click away.
- **Browse has a grid view** of thumbnails beside the list, made by `QLThumbnailGenerator` in the background.
- **Formats Quick Look can't show,** such as STEP, IGES or a 3MF, use the drawings the CAD provider and the 3MF reader already make (`docs/design/models-3d.md`).
- **Thumbnails and drawings are cached on each device,** in Carrel's `cache.nosync` folder, so each is made once. They're never written into the archive, so the archive stays plain files, and history and backups leave them out.
- **Previews never hold up the window:** until one is ready, the item shows its kind's icon.

The cache is `_System/cache.nosync/previews/`, made and read through `carrel_core::previews` so the interactive mode's previews (#258) share it:

- **`thumbnails/<key>.png`:** a thumbnail at most 512 pixels on its longer side. The Mac app makes them with `QLThumbnailGenerator`.
- **`drawings/<key>.png`:** Carrel's own drawing of a STEP, IGES or 3MF file: the first picture a text extractor for its type returns, such as the CAD provider's four views or a 3MF's embedded thumbnail.
- **`<key>`** is the SHA-256, in hex, of the item's path relative to the archive root, its size and its modification time, so a changed or moved file gets a new preview and an old one is never shown. Previews left behind only take up space, and the whole folder is safe to delete.
- **Nothing is kept for a restricted item:** its thumbnail isn't made, and its drawing, when its preview is asked for, is a temporary file that's deleted once shown.

## Not in v0.5

- **Finder integration** (badges, context actions), the share sheet, Quick Look previews of cards, Spotlight and App Intents: v1.3, macOS system integration (#35).
- **iPad and iPhone,** and iCloud with its `CloudKit` coordinator: v2.0 (#7).
- **Signed and notarized builds,** TestFlight: v1.0 (#47).
- **Kopia bundled inside the app:** backups use the installed Kopia, as the CLI does.

## Building it

1. Move the worker loop, Status's summary, correction preparation and the intake queue's operations from `carrel-cli` into `carrel-core`, with the CLI on top and its tests unchanged.
2. Grow `carrel-ffi` and CarrelKit to cover the core operations above, with `ArchiveModel` and progress callbacks, tested in Swift against the sample archive.
3. The app shell (#91): the Xcode project, opening and creating archives, the window, sidebar and inspector, Status and Browse, Settings, and the CI development build (#44).
4. Review, Questions, corrections and editing.
5. Active, checkout and return, the desk, and History with undo.
6. Search in the toolbar, and adding items by dragging and from the menu.
7. Item previews and the grid view in Browse (#256), below.
8. The agent: the login item, file-system watching for work sessions, the menu bar list, and the CLI's worker stepping aside.
9. The user guide's Mac app pages, then the gate.

## Decisions (accepted 2026-10-31)

1. **One window per archive, a sidebar of the interactive mode's screens with a content list and an inspector,** each screen doing what its interactive-mode counterpart does through the same core functions. Recommended: the owner already knows the screens, the two can't drift apart, and it's the layout the spec and Apple's guidelines describe. The alternative is a document-style app that opens collections as windows, which suits browsing but not the queues.
2. **The app calls the core in-process through CarrelKit's UniFFI bindings, and logic the app needs moves into `carrel-core` first.** Recommended: the proof of concept already works this way, it's fast, and the CLI and the app share one implementation. The alternative, running `carrel --json` commands, needs no bindings but parses text and can't stream progress or hold a lease.
3. **The app runs the same provider helpers the CLI does, bundled, rather than calling Apple's models in-process.** Recommended: the same results from the app and the CLI, crash isolation, and one models folder. The cost is a helper process per provider, which the CLI already pays.
4. **A login-item agent runs the worker, notices work sessions as they happen and shows the menu bar list, coordinating through the archive's files and writer lease, with no new protocol in v0.5.** Recommended: it's what the spec describes, it reuses coordination that already works between the CLI and its worker, and an XPC channel can be added later. The alternative is no agent until v1.3, with work sessions noticed only while something runs.
5. **macOS 26 and Apple silicon only; ad hoc–signed development builds from CI, with Developer ID signing and notarization left for v1.0.** Recommended: Apple's model needs macOS 26, the XCFramework is Apple silicon only, and keeping signing secrets out of CI until v1.0 keeps the repository simple. The owner runs Xcode builds locally with their personal team in the meantime.
