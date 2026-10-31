# Getting started

This walks you from nothing to your first files filed in a Carrel archive, in about twenty minutes. It uses the `carrel` command-line tool and its interactive mode; the Mac app comes in v0.5. For what each part means and everything else Carrel does, see the [User guide](user-guide.md).

## What Carrel does

Carrel keeps a **personal archive**: an ordinary folder you can open in Finder or Obsidian, organized into **collections** (folders with a written description of what belongs in them). Every file you add, an **item**, gets a **card** beside it: a small Markdown file with its title, summary, tags, dates and how sensitive it is.

You don't file things by hand. You give Carrel files, and it reads each one, drafts its card with a model running on your own computer, and proposes a collection. Nothing moves until you approve the proposal. Every change is recorded, so any of them can be undone.

## 1. Install

Until the first release with downloads (v0.2), install from the source. On a Mac you need [Homebrew](https://brew.sh), Rust (`brew install rustup`, then `rustup toolchain install stable`) and Xcode for the Apple provider. On Linux or Windows, Rust is enough.

```bash
git clone --branch dev https://github.com/HannahARose/carrel ~/Code/carrel-dev
```

```bash
cd ~/Code/carrel-dev && for p in carrel-cli carrel-fetch carrel-provider-extract carrel-provider-llama; do cargo install --locked --path crates/$p; done
```

On a Mac, also build the Apple provider, which gives Carrel Apple's on-device model, PDF text and OCR:

```bash
cd ~/Code/carrel-dev && swift build -c release --package-path apple/CarrelProviders && cp "$(swift build -c release --package-path apple/CarrelProviders --show-bin-path)/carrel-provider-apple" ~/.cargo/bin/
```

To have Carrel draw STEP and IGES models and describe them, install [OpenCascade](https://dev.opencascade.org) (`brew install opencascade`, or on Debian and Ubuntu `apt install libocct-data-exchange-dev libocct-modeling-algorithms-dev`), then build the CAD provider. Without it, Carrel still reads those files' headers and part names:

```bash
cd ~/Code/carrel-dev && cargo install --locked --path crates/carrel-provider-cad
```

On an Apple-silicon Mac you can also build the MLX provider, for open models through MLX. It needs Xcode and its Metal Toolchain (`xcodebuild -downloadComponent MetalToolchain`), and takes a few minutes:

```bash
cd ~/Code/carrel-dev/apple/CarrelMLX && xcodebuild -scheme carrel-provider-mlx -destination 'platform=macOS' -derivedDataPath .build/xcode -configuration Release -skipMacroValidation -quiet build && cp -R .build/xcode/Build/Products/Release/carrel-provider-mlx .build/xcode/Build/Products/Release/mlx-swift_Cmlx.bundle ~/.cargo/bin/
```

Make sure `~/.cargo/bin` is on your `PATH`, then check what Carrel found:

```bash
carrel providers
```

You need at least one **text model**. Download a recommended one (Carrel asks before downloading, and shows the size and license first): `gemma-4-e4b` if your computer has 16 GB of memory or more, `qwen3.5-4b` if it has 8 GB.

```bash
carrel models
```

```bash
carrel models download gemma-4-e4b
```

On a Mac with Apple Intelligence, Apple's model is already there and is the fastest, but on its own it doesn't meet the [benchmark](../bench/README.md)'s bar: it declines some documents, health records especially, and routes less well. With a recommended model installed too, Carrel passes it what Apple's model declines.

For backups, install [Kopia](https://kopia.io) too (`brew install kopia` on a Mac).

## 2. Make an archive

Pick a folder that isn't inside iCloud Drive or Dropbox for now:

```bash
carrel init ~/Archive
```

```bash
carrel open ~/Archive
```

`carrel open` makes it the archive that commands use when you're not inside its folder.

## 3. Create collections

Collections are where things go. Start with a handful that fit what you have, say six to twelve; you can add more later, and Carrel will tell you when the structure stops fitting. Each needs a sentence saying what belongs in it:

```bash
carrel collection new Receipts --description "Proof of purchase, kept for returns, warranty and expenses"
```

```bash
carrel collection new Health --description "Medical records, prescriptions, test results and appointments"
```

Then open each collection's design document, such as `~/Archive/Receipts/_collection.md`, and fill in the two sections the models read before filing anything:

- **Purpose and scope:** what belongs here, in a sentence or two.
- **Not here:** what looks as if it belongs here but doesn't, and where it goes instead. This is what keeps similar collections apart, such as Receipts and Manuals.

The better these read, the better Carrel files. Items that fit nowhere go to **Holding**, which every archive has.

## 4. Add some files

Open the interactive mode by running `carrel` with no command:

```bash
carrel
```

Press `i` to choose files. Carrel opens your terminal file manager if you have one (such as yazi or lf), or its own browser: move with the arrow keys, mark with `Space`, and press `i` to add what's marked. `/` lets you type or paste a path instead.

You'll see the **plan**: what will be added, and what's left out and why (duplicates, things already waiting). For a folder like Downloads, press `e` to treat each file in it as its own item. Press `Enter` to add them to the queue.

A **worker** now drafts each item in the background, one at a time. The bottom line shows what it's working on, and it keeps going if you close the terminal. On the **Ingest** screen (`7`) you can see what's waiting.

You can do the same from the shell, which returns at once:

```bash
carrel ingest --each ~/Downloads/to-file
```

## 5. Review

Proposals appear on the **Review** screen (`2`) as they're ready. For each one you see the card Carrel drafted and the collection it chose, with how sure it is and why.

| Key | Decision |
| --- | --- |
| `a` | Approve: file it where proposed |
| `m` | Move: choose another collection |
| `e` | Edit the card in your editor first |
| `h` | Hold: file it in Holding for later |
| `x` | Reject: leave the file where it is |
| `Space` | Mark several, then decide them together |

Anything sensitive (bank statements, medical letters, IDs) always waits for you; nothing is filed without your approval.

If several items share a term Carrel doesn't know, such as a project's code name, it asks on the **Questions** screen (`4`) instead of guessing. Answer once and it drafts those items again with what you said, and remembers it for next time.

## 6. Look around, and undo

- **Browse** (`5`) lists collections and their items; `/` searches the archive as `carrel search` does, with filters such as `tag:receipts` or `after:2025`. Below each card are the changes that touched the item and the items it links to; `Enter` moves among them, and `u` undoes the chosen change.
- **History** (`6`) lists every change. Choose one and press `u` to undo it: files it moved go back.
- **Status** (`1`) summarizes the archive, including **Structure**: signs that a collection is getting crowded or that something's missing.
- Your archive is a normal folder: open it in Finder, or as an Obsidian vault.

## 7. Back it up

History keeps the cards; Kopia keeps the files themselves. Set up a repository on an external drive:

```bash
carrel backup setup /Volumes/Backup/carrel
```

From then on, each approved batch takes a snapshot.

## Next

- The [User guide](user-guide.md) explains each part in more depth: sensitivity, questions, renaming, settings and more.
- The [Commands](commands.md) page lists every command and option.
- To contribute, start with the [Architecture](../dev/architecture.md) page.
