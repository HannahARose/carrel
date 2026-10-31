# CarrelApp

The Mac app, as designed in [the Mac app design](../../docs/design/mac-app.md): one window per archive, with the interactive mode's screens in the sidebar, a content list and an inspector. It calls the Rust core in-process through [CarrelKit](../CarrelKit/README.md).

- **`Carrel`:** the app. Status, Browse, Review, Questions, Active (checkout and return, the desk, pending entries) and History (with undo) work. Settings shows what's set today, read only.
- **`Carrel Agent`:** the background agent, a login item embedded in the app at `Contents/Library/LoginItems`. Settings → Agent registers it with `SMAppService`, or unregisters it. It follows the archives the app has open or opened recently (the app's `recentArchives` default), runs CarrelKit's `Agent` for them, and shows the menu bar list: what's checked out and on the desk, pending entries, what's waiting to be drafted, what it's doing now, and Pause. Choosing an item opens it in the app through a `carrel://open` link. While it runs, the CLI's background worker steps aside; paused or quit, the CLI's worker drafts again.
- **`CarrelTests`:** the view models, against the sample archive in `fixtures/`.

The project file is written by hand and committed; its folders are synchronized groups, so adding a Swift file under `Carrel/`, `CarrelAgent/` or `CarrelTests/` needs no change to it.

To try the agent without registering it, or touching your archives, run it from the build with an archive and a configuration folder of its own:

```bash
CARREL_AGENT_ARCHIVES=/tmp/try/archive CARREL_CONFIG_DIR=/tmp/try/config "Carrel.app/Contents/Library/LoginItems/Carrel Agent.app/Contents/MacOS/Carrel Agent"
```

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
