# Design: backups with Kopia (v0.2)

Status: proposed. Issue #32.

## What backups do

History (FORMAT.md §8) records an archive's metadata. The contents of items, which can be large, are left to a backup system. This design adds that system with [Kopia](https://kopia.io), which runs on macOS, Linux and Windows. Kopia stores files deduplicated, compressed and encrypted, and makes incremental snapshots quickly.

Backups give Carrel three things it promises elsewhere:

- **Undo that covers contents:** a history commit's `Carrel-Snapshot` trailer (§8.2) names the snapshot taken with that change. Undo restores metadata and item locations; contents come back from the snapshot.
- **Checkable manifests:** a past manifest can be rebuilt from a snapshot and checked against the card's `manifest.checksum` (§7.7).
- **Recovery** after deletion, a mistake in a sealed bundle, or a failed disk.

## How Carrel uses Kopia

- **A tool, not a library:** Carrel runs the `kopia` program and reads its JSON output. It looks for the program next to the `carrel` executable, then on `PATH`, as it does for providers. The Mac app (v0.5) and the release builds (#46) bundle it. Until then, `carrel backup` explains how to install it: `brew install kopia`, `winget install kopia`, or the Linux packages.
- **In the core:** a `Backups` trait (`snapshot`, `verify`, `restore`, `status`) with a Kopia implementation in `carrel_core::backup`. Like `providers::process`, it uses only `std::process`, so the core stays portable. A test double lets the tests run without Kopia.
- **Kopia's settings belong to the device:** each device keeps its own Kopia configuration in Carrel's configuration folder (`backups/<archive-id>.config`), passed with `--config-file`. Carrel never touches the owner's own Kopia setup.

## The repository

- **Where:** `carrel backup setup PATH` creates a Kopia filesystem repository at `PATH`, or connects to one already there. An external drive is recommended.
  - A folder on the same disk as the archive is allowed, but Carrel warns: it guards against mistakes, not against losing the disk.
  - A synced folder (iCloud Drive, Dropbox) is refused for the repository, since sync services can corrupt a repository being written.
- **What the archive records:** `settings.json`'s `backups` (§7.2) holds the type (`kopia`), the repository's Kopia identifier, and a description such as "Backup drive". A drive mounts at different paths on different devices, so each device connects with `carrel backup setup` and keeps its own path.
- **What gets snapshotted:** the whole archive folder. Kopia's ignore policy leaves out regenerable folders (§4.3, from `rules/regenerable.yaml`), ignored files (§2.8), `_System/cache.nosync/`, `_System/index/` and the writer lock. History (`_System/history.git`) and manifests are included, so a snapshot is a complete, consistent archive.
- **Retention:** Kopia's defaults (latest 10, plus hourly, daily, weekly, monthly and annual). Any snapshot that a history commit names is pinned, so undo always has its contents.

## The password

- Kopia encrypts the repository with a password. **Carrel never stores it in the archive**, as FORMAT.md's first principle requires.
- `carrel backup setup` asks for it once, hidden as it's typed, and hands it to Kopia only in the environment of the setup command. Kopia then keeps it in the operating system's keychain: Keychain on macOS, Credential Manager on Windows, and Secret Service on Linux where one is running. Where none is, Kopia keeps it in a file beside its configuration that only the owner can read, and Carrel says so.
- Setup says plainly that **a lost password means lost backups**, and suggests saving it in a password manager.

## When snapshots happen

- **One snapshot per approval batch:** after the batch is filed and before its history commit, so the commit carries `Carrel-Snapshot: kopia:<snapshot-id>` and the snapshot shows the archive as of that commit.
- **Before risky operations:** format migration (§12) and restructures, when they arrive, take a snapshot first, and refuse to go ahead without one.
- **On request:** `carrel backup now`.
- **If the backup can't be reached** (the drive isn't plugged in): approval still files the items, commits without a trailer, and says that no snapshot was taken. The next snapshot covers those items. `carrel status` shows how long ago the last snapshot was.
- **No backups set up:** ingest and approval work as before. `carrel status` and the approval output suggest `carrel backup setup` once per batch, not on every command.

## Verify and restore

- `carrel backup verify [--percent N]` runs Kopia's verification: it checks every snapshot's structure, and reads back `N`% of file contents (default 1%, or 100 for a full check). `carrel status` shows when the last verification ran and whether it passed.
- `carrel backup restore ITEM [--at COMMIT] [--to FOLDER]` restores one item's contents as they were at a commit (default: the latest snapshot holding it) into a new folder beside the archive. It **never overwrites** the item in place. The owner compares, then moves it themselves or asks Carrel to replace it. Replacing is a change with its own commit.
- `carrel backup list` shows snapshots with the history commit each belongs to.

## Building it

1. The `Backups` trait, the Kopia implementation over `std::process`, and a test double.
2. `carrel backup setup`, `now`, `list` and status in `carrel status`.
3. A snapshot per approval batch, and the `Carrel-Snapshot` trailer.
4. `verify` and `restore`.
5. CI: a Kopia job on macOS, Ubuntu and Windows against a temporary filesystem repository.

## Decisions to make

1. **Recommended: Kopia as an external program, found like providers, and bundled only in release builds and the Mac app.** The alternative is bundling it with every install, which `cargo install` can't do.
2. **Recommended: snapshot the whole archive, with ignore rules,** rather than only the items a batch changed. Kopia only reads what changed, so it stays fast, and every snapshot is a consistent archive.
3. **Recommended: a missing backup never blocks filing,** but is reported each batch. The alternative is a setting that makes approval refuse without a snapshot.
4. **Recommended: the password stays in the operating system's keychain** through Kopia, with Carrel asking for it only at setup.
5. **Recommended: restore never overwrites in place;** it writes beside the archive for the owner to compare.
