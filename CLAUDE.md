# Working on Carrel with Claude

Most of Carrel is built by Claude sessions, each working on one issue, with one session coordinating them. The owner follows the work on the project board and on a dashboard built from GitHub, so a session's progress has to show up on GitHub as it happens, not only at the end.

## Keep the board current

The board moves itself from pull requests (`.github/scripts/project-status.sh`): a draft PR puts it and the issues it closes in **In progress**, a PR ready for review in **In review**, and merging in **Done**. So:

1. **Open a draft PR as soon as you push your first commit,** with "Closes #N" (or "Part of #N" when the issue stays open) in the body. Push early, even a small first step; don't hold everything for one push at the end.
2. **Mark it ready for review** (`gh pr ready`) once the checks below pass locally. Then turn on auto-merge (`gh pr merge --auto --squash`) for a PR into `dev`: with dev's merge queue, that adds it to the queue once its checks pass, and the queue tests it together with dev and the pull requests ahead of it before merging.
3. **Ask the owner on the issue:** a comment starting `Question for the owner:`, then wait for the answer. The dashboard lists every open one.
4. **Say when you're stuck,** in a comment on the issue starting `Blocked:`, with what you need.
5. **When the plan changes,** for example when part of the issue moves elsewhere, say so in a comment on the issue, so the board and the issue agree.

## Keep CI fast, and dev whole

Many sessions work at once, and macOS runners are few, so:

- **Stay a draft while you work.** A PR into `dev` and its merge queue run only CI's Linux jobs, so run the checks below locally before marking it ready. A failure in the queue drops the PR out and rebuilds the PRs behind it.
- **macOS and Windows are checked after merging, until the first stable release.** An hourly run on `dev` checks them, and the coordinating session assigns what breaks. So when you change Swift, the Mac app or anything they build on (`carrel-core`, `carrel-ffi`), run their tests on your Mac before marking the PR ready (below): nothing else will before it lands.
- **One PR for issues that share files.** When you're given several issues that touch the same files, build them on one branch, with every issue in the body ("Closes #1, closes #2").
- **After merging `origin/dev` in, rerun the checks before pushing,** not just the conflict markers: two branches can each pass and still clash, as two `archive_work` functions once did.
- **Name things specifically.** A new function, type or test helper gets a name that says what it's for (`index_or_switch_work`, not `archive_work`), so it can't collide with another branch's.
- **Touch the shared files lightly:** `crates/carrel-cli/src/main.rs` (the command list), `crates/carrel-cli/src/tui/mod.rs` (the interactive mode's dispatch), `crates/carrel-core/src/settings.rs` (the setting keys), `crates/carrel-cli/tests/cli.rs`, `apple/CarrelApp/Carrel/ArchiveWindow*.swift`, `docs/dev/interfaces.md` and the user guide. Add to them in one place, and put the rest of your code in its own files.

## Branches and pull requests

- `dev` is the default branch. Pull requests squash-merge into it. `main` holds releases only; never open a PR into `main` unless you were asked to prepare a release.
- Every PR body names its issue. The required checks are "CI passed" and "Linked issue".
- Never force-push. Merge `origin/dev` into your branch instead. When two branches both add tests at the end of `crates/carrel-cli/tests/cli.rs`, keep both.
- Commit messages and PR titles are plain sentences in the imperative ("Show related items in Browse").

## Checks before marking a PR ready

```bash
cargo fmt --all
cargo clippy --workspace --exclude carrel-provider-llama --all-targets -- -D warnings
KOPIA_CHECK_FOR_UPDATES=false cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --exclude carrel-provider-llama --no-deps --document-private-items --locked
typos
npx markdownlint-cli2
```

- Clippy runs pedantic: no function over 100 lines, no struct with more than three bools.
- Swift changes: `swift test` in `apple/CarrelProviders` (and `apple/CarrelKit`), and `scripts/swift-format.sh --check`. The Mac app: `scripts/build-xcframework.sh`, then `xcodebuild -project apple/CarrelApp/CarrelApp.xcodeproj -scheme Carrel test`.
- When `FORMAT.md` changes, copy it to `fixtures/sample-archive/_System/FORMAT.md`; a test checks they match.

## Rules that protect the owner

- **No downloads without the owner's OK:** models, crates beyond `Cargo.lock`, Swift packages, Homebrew formulae, Rust targets, source text from the web. Ask with the name and size (a `Question for the owner:` comment, or the coordinating session).
- **Tests never write the real configuration** (`~/Library/Application Support/Carrel` on a Mac), and never write passwords into the keychain.
- **Don't open, copy or quote the owner's own files** outside a fixture they provided.
- On the owner's Macs, don't compile while `carrel bench` is running (`pgrep -f "carrel bench"`): it times the models.

## Designs

A large feature starts as a design in `docs/design/`, ending in five decisions the owner accepts before anything is built. Read the feature's design before starting, and follow its "Building it" steps and issue numbers.

## Every interface is fully capable

Anything Carrel can do should be available from the command line (`carrel`), the interactive mode and the app, on every platform each runs on, wherever the interface and platform make it possible (`docs/dev/architecture.md`, idea 4). Each interface does it in its own way; none is a lesser version of another.

- **A design says how each interface offers the feature,** in a short "In each interface" section, and names any deviation with its reason.
- **A PR that adds a capability to one interface** either adds it to the others too, or opens an issue for each one left, linked from the PR, in the same milestone unless the owner says otherwise.
- **Put the logic in `carrel-core`,** never in an interface, so that the others only have to present it.
- **Deviations are for real limits,** such as a terminal that can't draw images or a model that exists only on Apple devices, with the closest equivalent standing in. "It's easier in this interface" isn't one.
