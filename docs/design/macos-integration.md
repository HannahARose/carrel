# Design: macOS system integration (v0.8)

Status: accepted 2026-11-01, with the five recommendations below. Issue #35.

## What this adds

Carrel works inside its own windows and the terminal today. v0.8 brings it into the rest of the Mac:

- **Finder** badges on the archive's items and on working copies, with context actions (Add to Archive, Check Out, Return);
- a **Share** extension, "Send to Archive", for anything another app can share;
- **Quick Look** previews of cards;
- **Spotlight** results for what's in the archive;
- **App Intents**, so Shortcuts, Siri and Apple Intelligence can file, find, check out and ask.

The spec lists them under the Mac app: "Finder badges (sealed, checked out, tracked) and context actions: Ingest, Check Out, Return"; "Share-sheet 'Send to Archive', Quick Look previews of cards"; "Spotlight indexing of the archive, built on-device"; "App Intents for Shortcuts, Siri and Apple Intelligence". Its architecture puts them in Swift connectors the core calls, with the agent owning file watching and the writer lock.

**The gate:** for a week, the owner files, finds, checks out and returns items from Finder, the share sheet, Spotlight and Shortcuts, without opening Carrel's window for any of them. Nothing done that way is lost or filed twice, and every change it makes is in History, undoable.

## One rule for every extension

Finder Sync, Share and Quick Look extensions must be sandboxed, though the app and the agent aren't. So:

- **Extensions never write the archive.** They send requests to the agent, which holds the writer lease and does the work through the core, exactly as the app's own commands do. A request is "add these files", "check out this item", "return this working copy", "open this item in Carrel".
- **They talk over XPC.** The agent, already a login item, offers a Mach service in the app group. Each request is answered with what happened, or why not. A request the agent can't take, because it's paused, not running or another writer holds the lease, is refused with the reason, never queued silently.
- **They read what they need from a small state file** that the agent keeps in the app group container. For badges, it lists:
  - each archive's root;
  - its sealed bundles;
  - the checked-out items and their working copies;
  - the desk items.

  It's written when those change, so badges stay current without each extension opening the archive.
- **Quick Look reads the card itself.** A card is a Markdown file beside its item, which the extension is given access to when the person previews it.

## Finder

- **Badges**, through a Finder Sync extension over each open archive and every working copy's and desk item's folder:
  - **sealed:** a bundle Carrel never enters;
  - **checked out:** the archive copy, which is read-only until it's returned;
  - **working copy:** the copy being worked on, wherever it is;
  - **tracked:** a desk item.

  The existing "Checked Out" Finder tag stays, since tags work without the extension and show in other apps.
- **Context actions** on the archive's items and working copies:
  - **Check Out…:** to the Desktop, or Choose…, with an optional due date;
  - **Return…:** opens Carrel's return preview, since a return shows what changed and asks;
  - **Cancel Checkout…**;
  - **Show in Carrel**;
  - **Ask About This**.

  Actions that need a decision open the app's sheet for it rather than deciding in Finder.
- **Add to Archive, anywhere:** a Finder Sync extension sees only the folders it watches, so adding any file or folder from anywhere is a Quick Action (an Action extension). It's in Finder's context menu, the Touch Bar and the Services menu. It adds to the intake queue, as dropping onto the window does, and offers "entry by entry" or "as one item" for a folder.

## Share: Send to Archive

- **What it takes:** files, URLs, text and images from any app's share sheet. Files and images join the intake queue. A URL becomes a `url` item, and text a note item, drafted like anything else.
- **The sheet:** says which archive it goes to, offers an optional note that becomes part of the card's context, and shows "Added to the intake queue". It doesn't wait for the drafting.
- **Another Mac as writer:** if another device holds the writer lease, the request goes into this device's inbox (`_System/inbox/<device-id>/`, FORMAT §7.8), and the writer applies it. That's the same path iPad and iPhone use in v0.9, so the share extension is ready for them.

## Quick Look

- **A preview extension for cards:** the card's title, summary, fields, tags, the item's collection and its history, laid out as the app's inspector shows them, instead of raw front matter.
- **Item previews:** an item's own preview stays the system's, as now. Previews Carrel draws itself (STEP, IGES, 3MF, #256) become Quick Look previews too, so Finder shows them.
- **Restricted items:** previewing a restricted item's card shows its title only, with "Restricted: open in Carrel to see it", as the app's inspector does.

## Spotlight

- **What it indexes:** cards, not files. Spotlight already indexes the files themselves where it can. The agent adds each item's card to Core Spotlight: title, summary, tags, collection, dates and kind. A result opens the item in Carrel.
- **Updates:** the index is built on the device and kept current as the agent files, edits and moves items. It's removed for an archive that's closed or forgotten.
- **By sensitivity, as a setting** (`spotlight.index`), since anyone using the Mac sees Spotlight:
  - public and personal items in full;
  - sensitive items by title only;
  - restricted items not at all.

  The setting can also be "everything but restricted" or "off".
- **Collections and glossary facts** are indexed too, as search finds them.

## App Intents

- **Intents:**
  - Add to Archive: files or URLs, with an optional note;
  - Search the Archive: words and filters, returning items;
  - Ask the Librarian: a question, returning the answer and its items;
  - Check Out and Return an item;
  - What's Checked Out;
  - Take a Snapshot;
  - Open in Carrel.
- **Entities:** items and collections are App Entities, indexed for Spotlight and Apple Intelligence, so "the warranty for my laptop" can name an item.
- **Where they run:** in the agent, through the same XPC requests, so they work with the app closed. Anything that needs a decision, such as approving a return, opens Carrel.
- **The command line stays scriptable** for Shortcuts too (`--json`), as the spec says, so a Shortcut can use either.

## In each interface

These are integrations with macOS itself, so they're allowed deviations from "every interface fully capable" (`docs/dev/interfaces.md`). Each capability they reach is already in the CLI and the interactive mode:

- adding is `carrel ingest`;
- checking out and returning are `carrel checkout` and `carrel return`;
- searching is `carrel search`;
- asking is `carrel ask`;
- badges have `carrel checkouts` and `carrel desk` as their equivalents.

The Linux and Windows file managers' context menus (Nautilus scripts, Explorer's shell extension) come with those platforms' apps in v2.0 (#270).

| | macOS | Command line and interactive mode |
| --- | --- | --- |
| Add from anywhere | Quick Action, Share | `carrel ingest`, the interactive mode's `i` |
| Badges | Finder | `carrel checkouts`, `carrel desk`, Active |
| Check out, return | Finder, Intents | `carrel checkout`, `carrel return`, Active |
| Search | Spotlight, Intents | `carrel search`, `/` |
| Ask | Intents | `carrel ask`, the Librarian screen |
| Card previews | Quick Look | `carrel show`, Browse |

## Testing it

- **Requests and the state file:** in CarrelKit, with unit tests:
  - each XPC request does what the matching CLI command does;
  - a refused request says why;
  - the badge state file follows checkouts, returns and desk changes.
- **Spotlight's index:** tested against the sensitivity setting: a restricted card never reaches it.
- **The extensions:** checked by hand, in a gate checklist, since Finder, Share and Spotlight can't be driven from a test. The checklist covers every action from each, with the agent running, paused and absent, and with another device as writer.

## Building it

1. The agent's XPC service and the badge state file, in CarrelKit and the agent.
2. The Quick Action (Add to Archive) and the Share extension.
3. The Finder Sync extension: badges and context actions.
4. Quick Look: the card preview, and Carrel's own drawings as previews.
5. Spotlight: indexing cards in the agent, and `spotlight.index` in every interface's settings.
6. App Intents and entities.
7. The user guide, then the gate checklist.

## Decisions (accepted 2026-11-01)

1. **Extensions never write the archive. They ask the agent over XPC, and read a state file it keeps.** Recommended: the extensions must be sandboxed and the app and agent aren't. It also keeps one writer, with every change going through the core and into History.
2. **Finder Sync for badges and the archive's actions, plus a Quick Action for adding from anywhere.** Recommended: Finder Sync only sees folders it watches, and File Provider would change how the archive folder lives on disk, at the cost of its plain-folder and Obsidian compatibility.
3. **Spotlight indexes cards, not contents, with sensitive items by title only and restricted items left out by default (`spotlight.index`).** Recommended: anyone at the Mac sees Spotlight, and the files themselves are already indexed where macOS can read them.
4. **Shares and intents from a device that isn't the writer go through its inbox (FORMAT §7.8).** Recommended: it's the format's existing path, and the one iPad and iPhone need in v0.9.
5. **These are macOS-only, as allowed deviations; Linux and Windows file manager actions come with v2.0's apps.** Recommended: everything they reach is already in the CLI and the interactive mode.
