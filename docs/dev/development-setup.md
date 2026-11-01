# Development Setup

How to set up a Mac to build and work on Carrel. Steps marked **later** are only needed once the issue named next to them lands; this page is updated as they do.

## Requirements

| Requirement | Why |
| --- | --- |
| Mac with Apple silicon, macOS 27 or later | Foundation Models (on-device model) and MLX |
| Apple Intelligence turned on | The on-device model is unavailable otherwise |
| Xcode 27 | macOS 27 SDK, Swift 6.4, Foundation Models |
| [Homebrew](https://brew.sh) | Installs the other tools |
| A GitHub account | Issues, pull requests, the project board |

## 1. Xcode

1. Install Xcode 27 from the App Store or [Apple Developer](https://developer.apple.com/xcode/), open it once, and let it install its components.
2. Point the command-line tools at it and accept the license:

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
```

```bash
sudo xcodebuild -license accept
```

To check: `xcodebuild -version` prints Xcode 27, and `swift --version` prints Swift 6.4 or later.

## 2. GitHub CLI and the repository

```bash
brew install gh
```

```bash
gh auth login
```

```bash
gh auth setup-git
```

`gh auth setup-git` lets plain `git push` over HTTPS use your GitHub CLI login. To work with the project board from the CLI, and to push changes to GitHub Actions workflows, also run `gh auth refresh -s project,workflow`.

Clone the repository. The default branch is `dev`; see the wiki's [Development Workflow](https://github.com/HannahARose/carrel/wiki/Development-Workflow) page for branches.

```bash
gh repo clone HannahARose/carrel
```

## 3. Rust

Carrel uses `rustup` from Homebrew. Homebrew installs it "keg-only", so its directory has to be added to your `PATH`.

```bash
brew install rustup
```

Add this line to `~/.zshrc`, then open a new terminal window:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
```

Install the stable toolchain (includes `rustfmt` and `clippy`) and the Apple targets used for the universal CLI and the iOS framework:

```bash
rustup toolchain install stable --profile default
```

```bash
rustup default stable
```

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-darwin
```

The repository's `rust-toolchain.toml` pins the exact Rust version (currently 1.99.1); the first `cargo` command inside the repository installs it automatically. Targets belong to a toolchain, so run the `rustup target add` command again from inside the repository to add them to the pinned one. Updating: this `rustup` can't update itself, so use `brew upgrade rustup` for rustup and `rustup update` for toolchains.

## 4. Build and test

From the repository root, these are the same checks CI runs on every pull request that changes Rust:

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace --all-targets
```

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

```bash
cargo run --bin carrel -- --version
```

`cargo fmt --all` fixes formatting. To try the CLI without touching your own identity and settings, set `CARREL_CONFIG_DIR` to a scratch folder (see [Commands](../guide/commands.md)). The workspace has three crates under `crates/`: `carrel-core` (the library), `carrel-cli` (the `carrel` binary) and `carrel-ffi` (Swift bindings).

### Which CI jobs run when

CI (`.github/workflows/ci.yml`) runs only what a change affects. Its first job, "Changes" (`.github/scripts/ci-changes.sh`), lists the files a pull request or push changes and says which areas they touch: Rust, the Swift package, the Mac app, the MLX provider, Swift formatting, each portable provider, Kopia backups, the editor plugins, dependencies and the workflows. Each job runs only for its own area, so a change to the docs runs Format and Docs hygiene, which always run, and nothing else. Everything runs when a workflow changes, for the weekly schedule and a manual run, and for a pull request into `main`, which also restores no caches so that a release is checked from a clean build. A push to `dev`, which is a merged pull request, runs the jobs for what that merge changed, to catch two pull requests that pass alone but not together, and it's the only run that saves caches. Once the merge queue is on ([#298](https://github.com/HannahARose/carrel/issues/298)), a queue group runs the jobs for what it changes against its base, and "CI passed" reports on it. "CI passed" is the required check: it passes when every job passed or was skipped because its area didn't change. While a pull request is a draft, only the Linux jobs run; its macOS and Windows jobs wait until it's marked ready, so pushes made while it's being worked on don't queue for the few macOS runners, and a draft can't merge. Coverage, a Linux job, also waits until a pull request is ready. To see what a change would run, `EVENT=push BEFORE=origin/dev SHA=HEAD .github/scripts/ci-changes.sh`.

### Coverage and performance

**Coverage.** CI's "Coverage" job measures which lines of the Rust workspace its tests run, with [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov), on ready pull requests that touch Rust and on pushes to `dev`. Its job summary gives the total, each crate's share, and how many of the lines the pull request added or changed are covered, listing those no test runs. It only reports: no number fails CI. To measure it locally, install `cargo-llvm-cov` (`brew install cargo-llvm-cov`, which also needs `rustup component add llvm-tools-preview`), then run it as CI does:

```bash
cargo llvm-cov --workspace --exclude carrel-provider-llama --ignore-filename-regex '/(tests|benches|examples)/' --html --open
```

For the same summary as CI, write LCOV and summarize it against `dev`:

```bash
cargo llvm-cov --workspace --exclude carrel-provider-llama --ignore-filename-regex '/(tests|benches|examples)/' --lcov --output-path target/lcov.info
```

```bash
.github/scripts/coverage-summary.py target/lcov.info origin/dev HEAD
```

**Performance.** `crates/carrel-core/benches/scale` times the work that grows with an archive, for the scale target of a million files: card parsing (`card_parse`), manifest parsing and diffing (`manifest_parse`, `manifest_diff`), building and searching the word index (`word_index_build`, `word_search`), searching and opening the embedding index of 384-number vectors (`embedding_search`, `embedding_index_open`), and reading history (`history_log`). Each runs over generated fixtures at three sizes: up to a million items for the ones that are fast per item, and up to 100,000 for card parsing and history, whose time per item shows how they grow. It's a timing harness of its own, with no benchmarking crate: each measurement is the median of at least three runs. `cargo test` runs each once, at its smallest size, to check it still works. To run them all, which takes about ten minutes and 3 GB of memory:

```bash
cargo bench -p carrel-core --bench scale
```

Results go to `target/scale-bench/results.json`, and a table to standard output. A name after `--` runs only the benchmarks it's part of, and settings in the environment change the rest:

| Setting | What it does |
| --- | --- |
| `cargo bench -p carrel-core --bench scale -- manifest` | Runs only benchmarks whose name contains `manifest` |
| `CARREL_SCALE_MAX=100000` | Leaves out sizes above 100,000, for a quick run |
| `CARREL_SCALE_SIZES=5000,50000` | Runs at these sizes instead |
| `CARREL_SCALE_SECONDS=1` | Samples each measurement for about this long (3 by default) |
| `CARREL_SCALE_BASELINE=old.json` | Compares with an earlier results file, flagging anything slower by more than the threshold |
| `CARREL_SCALE_THRESHOLD=10` | The threshold, in percent (25 by default) |
| `CARREL_SCALE_OUT`, `CARREL_SCALE_REPORT` | Where to write the results and to append the Markdown report |

To check a change, copy `target/scale-bench/results.json` aside on `dev`, then run with `CARREL_SCALE_BASELINE` set to the copy on your branch. CI's "Performance" workflow (`.github/workflows/perf.yml`) runs them on each push to `dev` that changes Rust, weekly and by hand, keeps each run's results as an artifact for 90 days, and compares them with the last run's in its job summary, flagging what got slower by more than 25%, with a warning but no failure. Shared runners vary by 10–20% from run to run, so rerun before chasing a single flag. On the owner's Macs, don't run them while `carrel bench` is running.

### The Swift package

`apple/CarrelKit` is a Swift package over the core: `CarrelFFI.xcframework` (built from `crates/carrel-ffi` with UniFFI), the generated Swift bindings, Swift tests, and `CarrelPoC`, a minimal SwiftUI app. The framework is built, not committed, so build it first and again after changing `carrel-ffi`:

```bash
PROFILE=dev scripts/build-xcframework.sh
```

It builds `carrel-ffi` for macOS, iOS and the iOS Simulator, Apple silicon only, and stops with the `rustup target add` command if a target is missing. Leave out `PROFILE=dev` for a smaller, optimized release build. Then, from `apple/CarrelKit`:

```bash
swift test
```

```bash
swift run CarrelPoC ../../fixtures/sample-archive
```

```bash
xcodebuild -scheme CarrelKit -destination 'generic/platform=iOS Simulator' -derivedDataPath .build/xcode build
```

The generated Swift bindings, in `apple/CarrelKit/Sources/CarrelKit/Generated`, are committed. `scripts/build-xcframework.sh` regenerates them. To regenerate only the bindings, which needs no Apple targets, run:

```bash
scripts/generate-bindings.sh
```

Commit the result with the change to `carrel-ffi`. CI regenerates them and fails if they differ from what's committed.

The Swift under `apple/` is formatted with swift-format, which comes with Xcode, using the settings in `.swift-format` at the repository root. The generated bindings are left as they are. To format, and to check as CI does:

```bash
scripts/swift-format.sh
```

```bash
scripts/swift-format.sh --check
```

`apple/CarrelApp` is the Mac app, an Xcode project over CarrelKit; its [README](../../apple/CarrelApp/README.md) says how to build it. After building the framework, its view-model tests run with:

```bash
xcodebuild -project apple/CarrelApp/CarrelApp.xcodeproj -scheme Carrel test
```

`apple/CarrelProviders` is the Apple provider (`carrel-provider-apple`), a separate Swift package that needs no Rust build: run `swift test` there. `CARREL_TEST_APPLE_PROVIDER=<path to the built program> cargo test -p carrel-core --test apple_provider` tests it through the provider protocol, as CI does.

The app's Describe button asks Apple's on-device model (Foundation Models, macOS 26 and later with Apple Intelligence turned on) to describe the archive through Rust. Without it, a stand-in that only counts is used.

## 5. Check the setup

```bash
rustc --version && cargo clippy --version && rustfmt --version
```

```bash
xcodebuild -version && swift --version
```

```bash
gh auth status
```

## 6. Tools added later

| Tool | Install | Needed from |
| --- | --- | --- |
| `cargo-deny` (dependency checks: `cargo deny check`) | `brew install cargo-deny` | [#40](https://github.com/HannahARose/carrel/issues/40) |
| `cargo-llvm-cov` (coverage: see [Coverage and performance](#coverage-and-performance)) | `brew install cargo-llvm-cov` and `rustup component add llvm-tools-preview` | [#48](https://github.com/HannahARose/carrel/issues/48) |
| `zizmor` and `actionlint` (workflow checks: `zizmor .` and `actionlint`) | `brew install zizmor actionlint` | [#40](https://github.com/HannahARose/carrel/issues/40) |
| `cmake` (builds llama.cpp for `carrel-provider-llama`; its tests take a model folder in `CARREL_TEST_LLAMA_MODELS`) | `brew install cmake` | [#87](https://github.com/HannahARose/carrel/issues/87) |
| `typos`, `markdownlint-cli2` and `lychee` (docs checks: `typos`, `markdownlint-cli2`, `lychee --offline README.md FORMAT.md`) | `brew install typos-cli markdownlint-cli2 lychee` | [#41](https://github.com/HannahARose/carrel/issues/41) |
| `uniffi-bindgen` (Swift bindings) | built from the workspace by `scripts/generate-bindings.sh` | [#20](https://github.com/HannahARose/carrel/issues/20) |
| swift-format (Swift style: `scripts/swift-format.sh`) | comes with Xcode | [#44](https://github.com/HannahARose/carrel/issues/44) |
| Kopia (backups) | `brew install kopia` | [#32](https://github.com/HannahARose/carrel/issues/32), v0.2 |
| A local MLX model (about 15–20 GB) | downloaded by Carrel's model settings | [#24](https://github.com/HannahARose/carrel/issues/24), v0.2 |

## Privacy while developing

Use the public fixtures in the repository for tests. Never commit, attach or paste private documents; describe them in issues instead.
