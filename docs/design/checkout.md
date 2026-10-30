# Design: checkout, desk items and work sessions (v0.3)

Status: accepted 2026-10-30, with the five recommendations below. Issue #4.

## What this adds

So far, Carrel files items and leaves them alone. v0.3 handles items being worked on: a project checked out of the archive and returned, a folder on the desk that isn't archived yet, and the work done on either, recorded in the card's history and a monthly journal. The design spec describes the flow (Checkout and return; Desk items, work sessions and edit cards) and FORMAT.md fixes the files (§5.5 history entries, §6.2 settings, §7.10 devices, §9 Active and Journal). This design covers how the command line does it, since the Mac app's background agent, which watches files all the time, doesn't arrive until v0.5.

**The gate:** one real project goes through a full checkout and return.

## Checkout

`carrel checkout ITEM [--to FOLDER] [--due DATE]`, or `o` on an item in Browse:

1. **Checks** that the item can come out: a file, a folder or a sealed bundle (card-only items can't), not already checked out, and on this device's archive. A file can come out of an open folder on its own; a sealed bundle comes out whole.
2. **Saves a manifest** of the item as it is (§7.7), and for a bundle that's a Git repository, the commit it's at.
3. **Copies it** to the destination: `--to`, else `checkout.destination` (§6.2), default the Desktop. A name already taken there gets a number added, as filing does. The copy is checked against the manifest before anything else changes.
4. **Records it,** in one history commit (`Carrel-Operation: checkout`): `checked_out` on the card (§9.1, with the manifest checksum and a new optional `git_head`), the note in `Active/Checked out/`, and `Active/_index.md`.
5. **Locks the archive copy:** it's made read-only (write permission removed from its files), so an editor opened on it by mistake can't save, and every Carrel command that would change it refuses while `checked_out` is present. On macOS the working copy gets a "Checked Out" Finder tag.

`carrel checkouts` (and the Active screen) lists what's out, where, since when, and whether it has changed.

## Return

`carrel return ITEM`, or `r` on the Active screen, prepares a **return proposal** in the review queue. Nothing in the archive changes until it's approved.

1. **Compares** the working copy with the saved manifest: files added, removed, changed, and renamed (matched by checksum). Regenerable folders and clutter are left out by the archive's rules, as at ingest.
2. **For a Git repository,** reads the commits since `git_head` and the branch, and warns about uncommitted changes and unpushed commits.
3. **Drafts the history entry:** what changed, from the commits or, for documents, from comparing the text before and after with a model; and, if the item changed a lot, a revised summary and tags. The draft is a pending entry (§5.5), editable like any proposal.
4. **On approval,** in one commit (`Carrel-Operation: return`):
   - a backup snapshot, when backups are set up;
   - the archive copy replaced by the working copy, safely: the new copy is written beside the old one, checked, then swapped, so a failure leaves the old copy intact;
   - the lock removed, `checked_out` removed, the manifest and checksum updated, the history entry approved, `Active/` and the month's journal regenerated;
   - the working copy then trashed, kept, or asked about, by `checkout.after_return`. Trashing uses the system's Trash, never a permanent delete.
5. **Cancelling a checkout** (`carrel return ITEM --discard`) unlocks the archive copy and drops the working copy's changes from Carrel's view, after confirming; the working copy itself is left where it is.

**Edge cases:** if the archive copy changed while locked (someone removed the read-only flag), both are shown and the owner chooses. New files in the working copy that aren't part of the item are offered for ingest. A working copy that's gone (moved or deleted) is reported, and the checkout can be cancelled or its new place given with `--from PATH`. Editing very large items in place, and returning while the archive's drive is unplugged, wait for later versions.

## Desk items

`carrel desk add PATH` tracks a file or folder that isn't archived, where it is:

- **A card beside it** (`NAME.md` next to the item, as in the archive), drafted by the same card step as ingest. Nothing is written inside the item.
- **Registered** in this device's `desk.json` (§7.10), and mirrored in `Active/On desk/`; notes written in a mirror's body are copied back to the desk card when Carrel next runs.
- `carrel desk` lists them; `carrel desk remove PATH` stops tracking (the card stays unless asked).
- **Archiving it later** (`carrel desk archive PATH`) queues it for ingest with its card and history already written.
- **Suggestions,** in `carrel status` and the Status screen: folders on the Desktop or in `~/Code` that look like active work (changed in the last two weeks, or a Git repository) as candidates to track, and desk items unchanged for 60 days as candidates to archive. Nothing is tracked or moved without the owner.

## Work sessions without a watcher

The design spec has a background agent watch tracked items and close a session after 45 idle minutes. Until that agent exists (v0.5), the command line **works sessions out afterwards**, from what's on disk:

- **When:** whenever Carrel runs (any command, the interactive mode, and the worker), at most once every few minutes, it looks at each checked-out and desk item on this device.
- **How:** each file's modification time since the last look; times closer together than `sessions.idle_minutes` (default 45) belong to one session, whose length runs from its first change to its last. For a Git repository, commit times count too, and commit messages describe the session.
- **What's written:** one pending entry per session in the card's history section (§5.5), with the files changed, the commits, and the time spent. A session still going (changes in the last `idle_minutes`) isn't written until it's over.
- **Approving:** `carrel sessions` lists pending entries; approve, edit, combine several into one, or drop them, from the command line or the Active screen. Changing `[!pending]` to a normal entry in Obsidian approves one too, and Carrel notices. Each approval is a history commit; the journal is regenerated from the cards.
- **What it can't see:** a file changed and then changed back, or the order of edits within a session. The agent will fill those in; the entries' format doesn't change.

## Session snapshots

- **At the end of each session** found, when backups are set up, Carrel takes a Kopia snapshot of the working copy, so work in progress is backed up even before it's returned. Without a watcher, that's the state when Carrel noticed the session, not the moment it ended; for most work that's the same.
- **Large data:** a new optional card field, `snapshots: return-only`, skips session snapshots for items like a 60 GB dataset, keeping only checkout and return.
- **Trimming on return:** the return proposal lists the item's session snapshots, with their total new size, and offers to keep only the checkout and return snapshots, or all of them. The default is to keep all (deleting backups is the owner's call).

## Active and the Journal

All generated, never owner content (§9.2, §9.3), and regenerated with every checkout, return and approved entry:

- `Active/_index.md`: what's checked out and on the desk, with how long and the last session.
- `Active/Checked out/<title>.md`, `Active/On desk/<title>.md`: as FORMAT.md describes.
- `Journal/<year>/<year>-<month>.md`: the month's approved sessions across all items, newest first, linking to the cards.
- A new **Active** screen in the interactive mode (between Review and Questions) shows the same, with keys to return, open the working copy, and approve entries.

## Editing cards

Proposals can be edited in the owner's editor already. v0.3 adds `carrel edit ITEM` for any archived card: the same editor, the same checks (vocabulary tags, real dates, a title), and the owner's body text kept exactly. The change is one history commit (`Carrel-Operation: edit`) and can be undone.

## Building it

1. The manifest diff and the read-only lock, in `carrel_core::checkout`.
2. `carrel checkout`, `checkouts`, and `return` with the return proposal and its approval; `Active/`.
3. History entries: writing, approving and combining pending entries inside the managed section, and the Journal.
4. Session detection from modification times and commits, and session snapshots.
5. Desk items: `desk add`, the card beside it, `desk.json`, mirrors, suggestions, `desk archive`.
6. `carrel edit ITEM`.
7. The Active screen; the user guide and command reference.
8. The gate: a real project out and back.

## Decisions (accepted 2026-10-30)

1. **Sessions worked out afterwards** from modification times and commits whenever Carrel runs, until the background agent (v0.5) watches them live; same entries, same format. Recommended: no daemon to install, and nothing is lost between runs except edits that undo themselves.
2. **The archive copy is locked by making it read-only,** besides Carrel refusing to change it, and restored on return or cancel. Recommended: it stops an editor saving into the archive by mistake, and it's undone by any tool if something goes wrong.
3. **Return is a proposal in review,** approved like an ingest: the archive copy is replaced safely (written beside, checked, swapped), and the working copy is then trashed (to the system Trash), kept or asked about, per `checkout.after_return`. Recommended.
4. **Session snapshots are kept unless the owner trims them** at return, with a per-item `return-only` setting for large data. Recommended: the spec says the owner is asked; deleting backups by default could lose work.
5. **Left for later:** editing very large items in place, returns while the archive's drive is unplugged, reminders and notifications (with the Mac app). Recommended: they need the agent or the app, and the gate doesn't.
