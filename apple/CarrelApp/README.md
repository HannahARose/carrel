# CarrelApp

The Mac app, as designed in [the Mac app design](../../docs/design/mac-app.md): one window per archive, with the interactive mode's screens in the sidebar, a content list and an inspector. It calls the Rust core in-process through [CarrelKit](../CarrelKit/README.md).

- **`Carrel`:** the app. Status, Browse, Active (checkout and return, the desk, pending entries) and History (with undo) work; Review and Questions say what's coming. Settings shows what's set today, read only.
- **`CarrelTests`:** the view models, against the sample archive in `fixtures/`.

The agent (#237) will be a second target in the same project. The project file is written by hand and committed; its folders are synchronized groups, so adding a Swift file under `Carrel/` or `CarrelTests/` needs no change to it.

macOS 26 and later, Apple silicon only.

## Building

Build CarrelKit's framework first, from the repository root, and again after changing `carrel-ffi`:

```bash
PROFILE=dev scripts/build-xcframework.sh
```

Then open `CarrelApp.xcodeproj` in Xcode and run the `Carrel` scheme. It signs to run locally; choose your own team under Signing & Capabilities to sign with it instead.

From the command line:

```bash
xcodebuild -project apple/CarrelApp/CarrelApp.xcodeproj -scheme Carrel test
```

CI builds the app on every push and pull request and keeps it, zipped and signed ad hoc, as the "Carrel" artifact for 30 days. It isn't notarized, so macOS won't open it at first: try once, then choose Open Anyway in System Settings → Privacy & Security.
