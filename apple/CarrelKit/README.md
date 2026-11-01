# CarrelKit

`carrel-core` for Swift. The Rust core is built into `CarrelFFI.xcframework` by [UniFFI](https://mozilla.github.io/uniffi-rs/), from `crates/carrel-ffi`, and the generated Swift bindings are compiled into the `CarrelKit` library.

- **`CarrelKit`:** the core's operations as the Mac app uses them (`docs/design/mac-app.md`): review and questions, corrections, editing cards, checkout and return, the desk and pending entries, search, an item's history and related items, history and undo, Status, settings and models, the embedding index, and the intake queue. `ArchiveModel` is the `@MainActor` observable archive the screens show: it runs each call off the main thread, publishes what each screen shows, and reports long operations' progress. Errors are `CarrelError`s carrying the core's messages; `CarrelError.Busy` says who holds the writer lease. Swift supplies models through `ModelProvider` (and `TextModel`) and progress through `ProgressListener`, which Rust calls back.
- **The agent** (`Sources/CarrelKit/Agent`): what the Mac app's background agent runs. `Agent` follows archives, claims each so the CLI's worker steps aside, watches them and their working copies with FSEvents (`FileWatcher`), and runs the jobs `AgentPlan` says are due: the worker loop, recording work sessions once they've gone idle, and daily snapshots once backups are set up. `AgentLink` is the `carrel://` link its menu opens items with. It keeps what Spotlight finds current through a `SpotlightIndexing` (`CoreSpotlightIndex` on the device): the core's `spotlightEntries()` for each archive, by `spotlight.index`, each indexed under the link that opens it, and an archive's entries removed when it's no longer followed.
- **`CarrelPoC`:** a minimal SwiftUI app over `CarrelKit`, the proof of concept for #20.
- **Tests:** `Tests/CarrelKitTests`, run against the sample archive in `fixtures/`.

The framework isn't committed. Build it from the repository root first, and again after changing `carrel-ffi`:

```bash
PROFILE=dev scripts/build-xcframework.sh
```

The generated Swift, in `Sources/CarrelKit/Generated`, is committed: the build regenerates it, and so does `scripts/generate-bindings.sh` on its own. Commit it with the change to `carrel-ffi`; CI fails while it's out of date. Format the Swift with `scripts/swift-format.sh`.

Then, from this folder:

```bash
swift test
```

```bash
swift run CarrelPoC ../../fixtures/sample-archive
```

Setup, including the Rust targets for iOS, is on the wiki's [Development Setup](https://github.com/Horizon-Labs/carrel/wiki/Development-Setup) page.

The framework has Apple-silicon slices only: macOS, iOS and the iOS Simulator.
