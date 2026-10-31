# CarrelKit

`carrel-core` for Swift. The Rust core is built into `CarrelFFI.xcframework` by [UniFFI](https://mozilla.github.io/uniffi-rs/), from `crates/carrel-ffi`, and the generated Swift bindings are compiled into the `CarrelKit` library.

- **`CarrelKit`:** open archives, list collections and items, and give Rust a Swift-implemented `TextModel`. Model and text-extraction connectors will use the same callback pattern.
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

Setup, including the Rust targets for iOS, is on the wiki's [Development Setup](https://github.com/HannahARose/carrel/wiki/Development-Setup) page.

The framework has Apple-silicon slices only: macOS, iOS and the iOS Simulator.
