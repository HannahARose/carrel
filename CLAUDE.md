# Working on Carrel with Claude

Most of Carrel is built by Claude sessions, each working on one issue, with one session coordinating them. The owner follows the work on the project board and on a dashboard built from GitHub, so a session's progress has to show up on GitHub as it happens, not only at the end.

## Keep the board current

The board moves itself from pull requests (`.github/scripts/project-status.sh`): a draft PR puts it and the issues it closes in **In progress**, a PR ready for review in **In review**, and merging in **Done**. So:

1. **Open a draft PR as soon as you push your first commit,** with "Closes #N" (or "Part of #N" when the issue stays open) in the body. Push early, even a small first step; don't hold everything for one push at the end.
2. **Mark it ready for review** (`gh pr ready`) once the checks below pass locally. Then turn on auto-merge (`gh pr merge --auto --squash`) for a PR into `dev`.
3. **Ask the owner on the issue:** a comment starting `Question for the owner:`, then wait for the answer. The dashboard lists every open one.
4. **Say when you're stuck,** in a comment on the issue starting `Blocked:`, with what you need.
5. **When the plan changes,** for example when part of the issue moves elsewhere, say so in a comment on the issue, so the board and the issue agree.

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
- Swift changes: `swift test` in `apple/CarrelProviders` (and `apple/CarrelKit`), and `scripts/swift-format.sh --check`.
- When `FORMAT.md` changes, copy it to `fixtures/sample-archive/_System/FORMAT.md`; a test checks they match.

## Rules that protect the owner

- **No downloads without the owner's OK:** models, crates beyond `Cargo.lock`, Swift packages, Homebrew formulae, Rust targets, source text from the web. Ask with the name and size (a `Question for the owner:` comment, or the coordinating session).
- **Tests never write the real configuration** (`~/Library/Application Support/Carrel` on a Mac), and never write passwords into the keychain.
- **Don't open, copy or quote the owner's own files** outside a fixture they provided.
- On the owner's Macs, don't compile while `carrel bench` is running (`pgrep -f "carrel bench"`): it times the models.

## Designs

A large feature starts as a design in `docs/design/`, ending in five decisions the owner accepts before anything is built. Read the feature's design before starting, and follow its "Building it" steps and issue numbers.
