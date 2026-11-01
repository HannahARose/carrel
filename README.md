# Carrel

A private, on-device archivist for your files.

Carrel is a personal archivist with native apps for macOS, iPadOS and iOS and a command-line tool for macOS, Linux and Windows. It ingests, catalogs, organizes and versions a personal archive. It works the way a librarian does: each collection has a written policy for what belongs in it, every item gets a catalog card, items are checked out for work and returned, and the structure changes only when the benefit outweighs the cost of moving things around.

Every document is analyzed on your own devices. Nothing is sent anywhere unless you explicitly opt in.

> **Status:** [v0.6.0](https://github.com/Horizon-Labs/carrel/releases/tag/v0.6.0) is the latest release: the librarian, which answers questions about your archive in your own words, and discovery, over the Mac app, the `carrel` command line and its interactive mode, each fully capable on macOS, Linux and Windows. Filing, checking out and returning work, corrections and edits, search in your own words, previews, history, backups and settings, all on your computer. New here? Start with [Getting started](docs/guide/getting-started.md), or [the Mac app](docs/guide/mac-app.md).

## Principles

1. **Readable without the app.** An archive is an ordinary folder of files and Markdown cards. Anyone with a file browser and a Markdown reader can understand it.
2. **Self-contained.** Everything Carrel needs is stored in the archive. Move the folder to a new computer, install Carrel, and carry on.
3. **Respects existing structure.** Git repositories and other structured folders are stored whole and never altered inside.
4. **Private by default.** On-device models do the reading and filing. Remote models are opt-in, never see file contents, and only get minimized information.
5. **Organization comes from the material.** Collections are proposed from what is actually in the archive and reorganized only when that clearly helps.
6. **You are the expert.** Carrel asks good questions and remembers the answers.
7. **Portable core, native where it runs.** The core and CLI run on macOS, Linux and Windows; each platform's own tools are used where they exist, and portable ones fill in.
8. **Scales with the device.** Every device can do everything; stronger devices do it better.

## What it does

- **Ingest:** drop in files or folders. Carrel extracts text, detects projects, drafts a card (summary, tags, links), proposes a collection, and waits for your approval.
- **Collections with design documents:** each collection has a `_collection.md` describing what belongs there and how it is organized. Models read these before filing anything.
- **Check out and return:** take an item out to work on it. Carrel locks the archive copy, logs each work session as a short entry, and shows what changed when you return it.
- **History and backups:** every change is recorded in a Git history of the archive's metadata and can be undone. File contents are versioned with [Kopia](https://kopia.io).
- **Obsidian-compatible:** the archive root opens as an Obsidian vault.
- **Command-line tool:** `carrel` does everything the app does, with JSON output for scripts. See the [wiki's Command-Line Tool page](https://github.com/Horizon-Labs/carrel/wiki/Command-Line-Tool).

## Install

From v0.2, each [release](https://github.com/Horizon-Labs/carrel/releases) has an archive for macOS (Apple silicon and Intel), Linux (x86_64 and arm64) and Windows (x86_64), with `SHA256SUMS` and build provenance attestations. An archive holds `carrel` and everything it runs: the network helper, the llama.cpp provider, the extraction provider (PDF text, and OCR when [Tesseract](https://tesseract-ocr.github.io) is installed), Kopia for backups, and on macOS the Apple provider and, for Apple silicon, the MLX provider. Put them together in a folder on your `PATH`.

To check a download came from this repository's release workflow:

```bash
gh attestation verify carrel-0.6.0-linux-x86_64.tar.gz --repo Horizon-Labs/carrel
```

That works from v0.6, the first release since the repository moved to Horizon-Labs. GitHub's lookup doesn't find the attestations made before the move, so check v0.5.1 and earlier against their `SHA256SUMS` instead.

The binaries aren't signed yet (that comes with the Mac app), so on macOS a browser download needs `xattr -d com.apple.quarantine` on each program before it runs; downloads with `curl` or `gh release download` don't. Then download a model for drafting cards with `carrel models`, unless your Mac has Apple's on-device model.

Until then, build from source as [Getting started](docs/guide/getting-started.md#1-install) describes.

## Architecture

- **Core:** Rust, shared by the apps and the CLI; builds on macOS, Linux and Windows.
- **Apps:** SwiftUI for macOS, iPadOS and iOS, using the core through generated Swift bindings (UniFFI).
- **Providers:** models and text extraction come from separate provider programs: Apple's Foundation Models, MLX, PDFKit and Vision on Apple devices, and portable ones (llama.cpp, a Rust PDF reader, Tesseract) elsewhere.
- **Sync (planned, v0.9):** iCloud Drive, with a CloudKit lock so only one device writes history at a time.

[docs/dev/architecture.md](docs/dev/architecture.md) explains how it's built today, with diagrams.

## Roadmap

| Version | Contents |
| --- | --- |
| 0.1 | Archive format, Rust core and the `carrel` CLI |
| 0.2 | Ingest from the CLI on macOS, Linux and Windows, with an interactive terminal mode |
| 0.3 | Check out, desk items and edit cards, in the CLI |
| 0.4 | Explorer and search, in the CLI |
| 0.5 | The Mac app |
| **1.0** | **First working release:** the Mac app plus the CLI, fully on-device |
| 1.1 – 1.3 | Librarian and discovery, restructure proposals, macOS system integration |
| **2.0** | **iCloud, iPad and iPhone** |
| 2.1 | Optional remote models through a privacy filter |
| **3.0** | **Offline collections and sharing between archives** |
| 3.1 | Encryption, web snapshots |

Details are on the [wiki's Roadmap page](https://github.com/Horizon-Labs/carrel/wiki/Roadmap).

## Documentation

**Using Carrel:**

- **[Getting started](docs/guide/getting-started.md):** from installing to your first filed items.
- **[User guide](docs/guide/user-guide.md):** each part of Carrel, and how to use it.
- **[Commands](docs/guide/commands.md):** every command, option and key.

**Working on Carrel:**

- **[Architecture](docs/dev/architecture.md):** the parts, how they communicate, what they keep on disk, and the main flows.
- **[Code map](docs/dev/code-map.md):** where each part is in the code, linked to the code documentation.
- **[What separating the parts costs](docs/dev/overhead.md):** measured time, memory and disk.
- **[Wiki](https://github.com/Horizon-Labs/carrel/wiki):** design, archive format overview, roadmap, development setup and workflow.
- **[Code documentation](https://horizon-labs.github.io/carrel/):** every module, type and function, built from the `dev` branch on each change.
- **[FORMAT.md](FORMAT.md):** the archive format specification.
- **[docs/providers.md](docs/providers.md):** the protocol for providers, the programs that give Carrel its text models and text extraction.

## License

- **Code:** [Mozilla Public License 2.0](LICENSE). You can use Carrel's code in other projects, including closed-source ones; changes to Carrel's own files must be shared under the same license.
- **Archive format and provider protocol specifications** (`FORMAT.md`, `docs/providers.md` and anything under `docs/format/`): [Creative Commons Attribution 4.0](LICENSES/CC-BY-4.0.txt), so other tools are free to read and write Carrel archives and to provide models and text extraction to Carrel.

"Carrel" is a working name and is not covered by either license.
