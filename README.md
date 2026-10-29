# Carrel

A private, on-device archivist for your files.

Carrel is a native app for macOS, iPadOS and iOS that ingests, catalogs, organizes and versions a personal archive. It works the way a librarian does: each collection has a written policy for what belongs in it, every item gets a catalog card, items are checked out for work and returned, and the structure changes only when the benefit outweighs the cost of moving things around.

Every document is analyzed on your own devices. Nothing is sent anywhere unless you explicitly opt in.

> **Status:** early development (v0.1): the archive format, Rust core and first `carrel` commands. There is no app yet.

## Principles

1. **Readable without the app.** An archive is an ordinary folder of files and Markdown cards. Anyone with a file browser and a Markdown reader can understand it.
2. **Self-contained.** Everything Carrel needs is stored in the archive. Move the folder to a new computer, install Carrel, and carry on.
3. **Respects existing structure.** Git repositories and other structured folders are stored whole and never altered inside.
4. **Private by default.** On-device models do the reading and filing. Remote models are opt-in, never see file contents, and only get minimized information.
5. **Organization comes from the material.** Collections are proposed from what is actually in the archive and reorganized only when that clearly helps.
6. **You are the expert.** Carrel asks good questions and remembers the answers.
7. **Native to Apple platforms.** It should feel like part of the operating system.
8. **Scales with the device.** Every device can do everything; stronger devices do it better.

## What it does

- **Ingest:** drop in files or folders. Carrel extracts text, detects projects, drafts a card (summary, tags, links), proposes a collection, and waits for your approval.
- **Collections with design documents:** each collection has a `_collection.md` describing what belongs there and how it is organized. Models read these before filing anything.
- **Check out and return:** take an item out to work on it. Carrel locks the archive copy, logs each work session as a short entry, and shows what changed when you return it.
- **History and backups:** every change is recorded in a Git history of the archive's metadata and can be undone. File contents are versioned with [Kopia](https://kopia.io).
- **Obsidian-compatible:** the archive root opens as an Obsidian vault.
- **Command-line tool:** `carrel` does everything the app does, with JSON output for scripts. See the [wiki's Command-Line Tool page](https://github.com/HannahARose/carrel/wiki/Command-Line-Tool).

## Architecture (planned)

- **Core:** Rust, shared by the apps and the CLI.
- **Apps:** SwiftUI for macOS, iPadOS and iOS, using the core through generated Swift bindings (UniFFI).
- **Models:** Apple's Foundation Models on-device, plus larger local open models via MLX on Macs that can run them.
- **Sync:** iCloud Drive, with a CloudKit lock so only one device writes history at a time.

## Roadmap

| Version | Contents |
| --- | --- |
| 0.1 | Archive format, Rust core and the `carrel` CLI |
| 0.2 | Ingest on Mac |
| 0.3 | Check out, desk items and edit cards |
| 0.4 | Explorer and search |
| **1.0** | **First working release:** one Mac plus the CLI, fully on-device |
| 1.1 – 1.3 | Librarian and discovery, restructure proposals, macOS system integration |
| **2.0** | **iCloud, iPad and iPhone** |
| 2.1 | Optional remote models through a privacy filter |
| **3.0** | **Offline collections and sharing between archives** |
| 3.1 | Encryption, web snapshots, Linux CLI |

Details are on the [wiki's Roadmap page](https://github.com/HannahARose/carrel/wiki/Roadmap).

## Documentation

- **[Wiki](https://github.com/HannahARose/carrel/wiki):** design, archive format overview, roadmap, development setup and workflow.
- **[Code documentation](https://hrose.me/carrel/):** every module, type and function, built from the `dev` branch on each change.
- **[FORMAT.md](FORMAT.md):** the archive format specification.

## License

- **Code:** [Mozilla Public License 2.0](LICENSE). You can use Carrel's code in other projects, including closed-source ones; changes to Carrel's own files must be shared under the same license.
- **Archive format specification** (`FORMAT.md` and anything under `docs/format/`): [Creative Commons Attribution 4.0](LICENSES/CC-BY-4.0.txt), so other tools are free to read and write Carrel archives.

"Carrel" is a working name and is not covered by either license.
