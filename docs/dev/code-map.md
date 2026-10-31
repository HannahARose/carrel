# Code map

Where to look for each part of Carrel. Each row names the type or function to read first; its page in the [code documentation](https://hrose.me/carrel/) explains how to use it, with examples, and links on to the rest. The [Architecture](architecture.md) page explains how the parts fit together.

The code documentation is built from `dev` on every merge, private items included, so it always matches the code. Build it locally with `cargo doc --workspace --exclude carrel-provider-llama --no-deps --document-private-items --open`.

## The archive and its format

| To work on | Read | Where |
| --- | --- | --- |
| Creating and opening archives | [`Archive`](https://hrose.me/carrel/carrel_core/archive/struct.Archive.html): `init`, `open`, `collections`, `cards_in`, `items`, `settings` | `crates/carrel-core/src/archive.rs` |
| A card's fields | [`Card`](https://hrose.me/carrel/carrel_core/card/struct.Card.html), with `Kind`, `Sensitivity`, `Provenance`, `PlacementRecord` | `card.rs` |
| Reading and writing card files, keeping the owner's text | [`CardFile`](https://hrose.me/carrel/carrel_core/card_file/struct.CardFile.html) | `card_file.rs` |
| `ARCHIVE.md`, `_collection.md`, `settings.json` | [`ArchiveDoc`](https://hrose.me/carrel/carrel_core/documents/struct.ArchiveDoc.html), [`CollectionDoc`](https://hrose.me/carrel/carrel_core/documents/struct.CollectionDoc.html), [`Settings`](https://hrose.me/carrel/carrel_core/documents/struct.Settings.html) | `documents.rs` |
| The parts of a Markdown body programs may write | [`sections`](https://hrose.me/carrel/carrel_core/sections/) | `sections.rs` |
| What kind of item something is | [`detect`](https://hrose.me/carrel/carrel_core/detect/), with the `Rules` from `_System/rules/` | `detect.rs` |
| Checksums and folder manifests | [`Manifest`](https://hrose.me/carrel/carrel_core/manifest/struct.Manifest.html) | `manifest.rs` |
| Names that work on every system | [`names::check`](https://hrose.me/carrel/carrel_core/names/fn.check.html), [`names::portable`](https://hrose.me/carrel/carrel_core/names/fn.portable.html) | `names.rs` |
| The generated contents lists and changelogs | [`listing::refresh`](https://hrose.me/carrel/carrel_core/listing/fn.refresh.html) | `listing.rs` |
| The format itself | [FORMAT.md](../../FORMAT.md), and the JSON Schema in `schemas/card.schema.json` | |

## Changing the archive

| To work on | Read | Where |
| --- | --- | --- |
| One writer at a time | [`DiskLock`](https://hrose.me/carrel/carrel_core/coordinator/struct.DiskLock.html), [`WriterLease`](https://hrose.me/carrel/carrel_core/coordinator/struct.WriterLease.html), the [`Coordinator`](https://hrose.me/carrel/carrel_core/coordinator/trait.Coordinator.html) trait | `coordinator.rs` |
| History, commits and undo | [`History`](https://hrose.me/carrel/carrel_core/history/struct.History.html): `start`, `commit`, `log`, `undo`; [`Change`](https://hrose.me/carrel/carrel_core/history/struct.Change.html), [`Operation`](https://hrose.me/carrel/carrel_core/history/enum.Operation.html) | `history.rs` |
| Renaming an item and its card | [`rename::rename`](https://hrose.me/carrel/carrel_core/rename/fn.rename.html) | `rename.rs` |
| Creating collections | [`collections::create`](https://hrose.me/carrel/carrel_core/collections/fn.create.html) | `collections.rs` |
| Changing shared settings | [`settings::change`](https://hrose.me/carrel/carrel_core/settings/fn.change.html), [`settings::Key`](https://hrose.me/carrel/carrel_core/settings/enum.Key.html) | `settings.rs` |

## Ingest, review and questions

| To work on | Read | Where |
| --- | --- | --- |
| Planning paths: intake, kinds, duplicates | [`ingest::plan`](https://hrose.me/carrel/carrel_core/ingest/fn.plan.html), [`Candidate`](https://hrose.me/carrel/carrel_core/ingest/struct.Candidate.html), [`IntakeOptions`](https://hrose.me/carrel/carrel_core/ingest/struct.IntakeOptions.html) | `ingest.rs` |
| The intake queue | [`intake::add`](https://hrose.me/carrel/carrel_core/intake/fn.add.html), [`intake::process`](https://hrose.me/carrel/carrel_core/intake/fn.process.html), [`IntakeEntry`](https://hrose.me/carrel/carrel_core/intake/struct.IntakeEntry.html) | `intake.rs` |
| One item through every stage, with progress | [`run_with_progress`](https://hrose.me/carrel/carrel_core/ingest/run/fn.run_with_progress.html), [`Run`](https://hrose.me/carrel/carrel_core/ingest/run/struct.Run.html) | `ingest/run.rs` |
| Reading text, and fitting it to a model | [`ingest::text::read`](https://hrose.me/carrel/carrel_core/ingest/text/fn.read.html), [`fit`](https://hrose.me/carrel/carrel_core/ingest/text/fn.fit.html) | `ingest/text.rs` |
| Drafting and checking cards | [`ingest::card::draft`](https://hrose.me/carrel/carrel_core/ingest/card/fn.draft.html), [`Draft`](https://hrose.me/carrel/carrel_core/ingest/card/struct.Draft.html), [`machine_made`](https://hrose.me/carrel/carrel_core/ingest/card/fn.machine_made.html) | `ingest/card.rs` |
| Choosing a collection | [`Router`](https://hrose.me/carrel/carrel_core/ingest/route/struct.Router.html), [`RoutingRules`](https://hrose.me/carrel/carrel_core/ingest/route/struct.RoutingRules.html) | `ingest/route.rs` |
| Sensitivity | [`assess`](https://hrose.me/carrel/carrel_core/ingest/sensitivity/fn.assess.html), [`floor`](https://hrose.me/carrel/carrel_core/ingest/sensitivity/fn.floor.html) | `ingest/sensitivity.rs` |
| Questions from a batch | [`ingest::questions::ask`](https://hrose.me/carrel/carrel_core/ingest/questions/fn.ask.html), [`intake::ask_with_waiting`](https://hrose.me/carrel/carrel_core/intake/fn.ask_with_waiting.html) | `ingest/questions.rs`, `intake.rs` |
| Filing approved items | [`filing::file_batch`](https://hrose.me/carrel/carrel_core/ingest/filing/fn.file_batch.html) | `ingest/filing.rs` |
| The review queue and proposals | [`ReviewEntry`](https://hrose.me/carrel/carrel_core/review/struct.ReviewEntry.html), and [`Queue`](https://hrose.me/carrel/carrel_core/review/struct.Queue.html), the file-per-entry queue every queue uses | `review.rs` |
| The question queue and glossary | [`QuestionEntry`](https://hrose.me/carrel/carrel_core/questions/struct.QuestionEntry.html), [`questions::add_fact`](https://hrose.me/carrel/carrel_core/questions/fn.add_fact.html) | `questions.rs` |
| The owner's decisions, for every interface | [`actions`](https://hrose.me/carrel/carrel_core/actions/): `decide`, `move_to`, `reject`, `teach`, `apply_draft` | `actions.rs` |
| Restructure signals | [`signals::signals`](https://hrose.me/carrel/carrel_core/signals/fn.signals.html), [`Signal`](https://hrose.me/carrel/carrel_core/signals/enum.Signal.html) | `signals.rs` |

## Providers, models and backups

| To work on | Read | Where |
| --- | --- | --- |
| Choosing a provider for a capability | [`Registry`](https://hrose.me/carrel/carrel_core/providers/struct.Registry.html), [`Provider`](https://hrose.me/carrel/carrel_core/providers/trait.Provider.html), [`Offer`](https://hrose.me/carrel/carrel_core/providers/struct.Offer.html), [`ProviderError`](https://hrose.me/carrel/carrel_core/providers/enum.ProviderError.html) | `providers.rs` |
| Running provider programs | [`ProcessProvider`](https://hrose.me/carrel/carrel_core/providers/process/struct.ProcessProvider.html), [`discover`](https://hrose.me/carrel/carrel_core/providers/process/fn.discover.html) | `providers/process.rs` |
| Writing a provider in Rust | [`providers::serve`](https://hrose.me/carrel/carrel_core/providers/serve/fn.serve.html), and `crates/carrel-core/examples/echo_provider.rs` | `providers/serve.rs` |
| The protocol | [docs/providers.md](../providers.md) | |
| The llama.cpp provider | [`crates/carrel-provider-llama`](https://github.com/HannahARose/carrel/tree/dev/crates/carrel-provider-llama) (left out of the published docs, since building llama.cpp takes minutes) | `crates/carrel-provider-llama` |
| The extraction provider, and 3MF, G-code, STEP and IGES details | [`carrel_provider_extract`](https://hrose.me/carrel/carrel_provider_extract/), [`models`](https://hrose.me/carrel/carrel_provider_extract/models/) | `crates/carrel-provider-extract` |
| Drawing STEP and IGES models | `cad.rs` (the offer), `occt.rs` and `shim/carrel_cad.cpp` (OpenCascade), `render.rs` (the software renderer) | `crates/carrel-provider-cad` |
| The Apple provider | `FoundationModel.swift`, `PDFText.swift`, `VisionOCR.swift`, `ImageReader.swift`, `ModelReader.swift`, `Protocol.swift` | `apple/CarrelProviders` |
| The model catalog and checked downloads | [`models::CATALOG`](https://hrose.me/carrel/carrel_core/models/constant.CATALOG.html), [`models::download`](https://hrose.me/carrel/carrel_core/models/fn.download.html), [`Fetcher`](https://hrose.me/carrel/carrel_core/models/trait.Fetcher.html) | `models.rs` |
| The network helper | [`carrel_fetch`](https://hrose.me/carrel/carrel_fetch/) | `crates/carrel-fetch` |
| Backups | [`Backups`](https://hrose.me/carrel/carrel_core/backup/trait.Backups.html), [`Kopia`](https://hrose.me/carrel/carrel_core/backup/struct.Kopia.html) | `backup.rs` |
| The model benchmark | [`bench::score`](https://hrose.me/carrel/carrel_core/bench/fn.score.html), [`bench::check`](https://hrose.me/carrel/carrel_core/bench/fn.check.html), and `fixtures/bench` | `bench.rs` |

## The command-line tool

| To work on | Read | Where |
| --- | --- | --- |
| How a command reports | [`Report`](https://hrose.me/carrel/carrel/struct.Report.html), [`Failure`](https://hrose.me/carrel/carrel/struct.Failure.html), and the `Command` enum in `main.rs` | `crates/carrel-cli/src/main.rs` |
| The archive commands | [`commands`](https://hrose.me/carrel/carrel/commands/) | `commands.rs` |
| Ingest and the queue | [`ingest`](https://hrose.me/carrel/carrel/ingest/) | `ingest.rs` |
| The worker | [`worker::work`](https://hrose.me/carrel/carrel/worker/fn.work.html), [`worker::start`](https://hrose.me/carrel/carrel/worker/fn.start.html), [`Status`](https://hrose.me/carrel/carrel/worker/struct.Status.html) | `worker.rs` |
| Review, questions, backups, models, settings | [`review`](https://hrose.me/carrel/carrel/review/), [`questions`](https://hrose.me/carrel/carrel/questions/), [`backup`](https://hrose.me/carrel/carrel/backup/), [`models`](https://hrose.me/carrel/carrel/models/), [`settings`](https://hrose.me/carrel/carrel/settings/) | one file each |
| File pickers | [`picker::KNOWN`](https://hrose.me/carrel/carrel/picker/constant.KNOWN.html), [`picker::run`](https://hrose.me/carrel/carrel/picker/fn.run.html) | `picker.rs` |
| Progress bars | [`progress::Display`](https://hrose.me/carrel/carrel/progress/struct.Display.html) | `progress.rs` |
| What's kept on this device | [`config`](https://hrose.me/carrel/carrel/config/), and [`carrel_core::local`](https://hrose.me/carrel/carrel_core/local/) | `config.rs` |

## The interactive mode

| To work on | Read | Where |
| --- | --- | --- |
| State, keys and effects | [`App`](https://hrose.me/carrel/carrel/tui/app/struct.App.html), [`Tab`](https://hrose.me/carrel/carrel/tui/app/enum.Tab.html), [`Effect`](https://hrose.me/carrel/carrel/tui/app/enum.Effect.html) | `tui/app.rs` |
| The event loop, background work, suspending for the editor or picker | [`tui`](https://hrose.me/carrel/carrel/tui/) | `tui/mod.rs` |
| Drawing | [`tui::ui`](https://hrose.me/carrel/carrel/tui/ui/) | `tui/ui.rs` |
| Each screen | `review.rs`, `screens.rs` (Questions and Browse), `intake.rs` (Ingest), `settings.rs`, `browser.rs` (the built-in file browser) | `tui/` |

`App` never touches the terminal, so screens are tested by sending it keys and drawing it to ratatui's `TestBackend`; see the tests in `tui/mod.rs`.

## The Swift side

| To work on | Read | Where |
| --- | --- | --- |
| What Swift can call | [`carrel_ffi`](https://hrose.me/carrel/carrel_ffi/) | `crates/carrel-ffi/src/lib.rs` |
| Building the XCFramework and bindings | `scripts/build-xcframework.sh` | |
| The Swift package the apps use | `apple/CarrelKit` | |
