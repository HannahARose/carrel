# Design: iCloud, iPad and iPhone (v0.8)

Status: accepted 2026-11-01, with the five recommendations below. Issue #7.

## What this adds

Today an archive lives on one Mac, and the command line warns if it's in a synced folder. v0.8 makes one archive work across the owner's devices:

- **The archive in iCloud Drive,** as the same plain files it is now, readable in Finder, Files and Obsidian.
- **A CloudKit writer lock,** so one device at a time writes its history.
- **Inboxes,** so every other device can still act, even offline.
- **iPad and iPhone apps:** full participants, each doing every operation its hardware allows.

The spec sets the shape:

- "The archive's metadata history is a git repository written by one device at a time; a CloudKit lock decides which device writes, and other devices submit quick actions through their own inbox folders."
- The lock record holds "device ID, lease expiry, history head. No file contents."
- Devices without Kopia keep replaced content "in a holding folder until a Mac snapshots it".
- Each device keeps a capability profile.
- A quality ladder lets "a stronger device claim" work a weaker one did below the preferred level.

**The gate (the spec's):** the iPad completes an ingest and a return while the Mac is offline. When the Mac comes back, everything is filed, recorded in history once, snapshotted, and nothing is lost or duplicated.

## What already exists

v0.8 builds on the core rather than replacing it:

- **The `Coordinator` trait** (`coordinator.rs`): today implemented by a disk lock with a writer lease. It already says a CloudKit coordinator comes later.
- **The inbox** (`inbox.rs`, FORMAT §7.8): devices that aren't the writer already send quick actions, files and URLs there (#437), and the writer applies them in order.
- **Synced-folder detection,** which today warns. v0.8 turns the warning into the CloudKit coordinator for iCloud Drive.

## The writer lock

- **What CloudKit holds:** one record per archive in the owner's private CloudKit database, with the writing device's ID, the lease's expiry, and the history head it last committed. Nothing else, and no file contents.
- **The rules:**
  - A device takes the lock only when the lease has expired or was released.
  - It waits until iCloud Drive has delivered the history head the record names before writing, so it never writes on top of history it hasn't seen.
  - It commits, records the new head, and releases or renews the lock.
- **Who holds it on a Mac:** the agent (the login item), as the spec says. The app and the command line ask the agent, which already coordinates them through the writer lease.
- **On iPad and iPhone:** the app takes the lock for a batch of work while it's in the foreground or in a background task, and releases it when done. A lease left by a device that went offline expires, so nobody waits forever.
- **The command line on Linux and Windows,** on an archive synced to them (iCloud for Windows, or a copy), can't reach CloudKit. It reads freely and acts through its inbox, as a non-writer, and full writes there need an archive that isn't synced. It says so plainly rather than warning vaguely.

## Inboxes

- **Every device that isn't the writer** sends its changes as inbox entries in `_System/inbox/<device-id>/`, which only it writes, so they never conflict, even offline: approvals, answers, corrections, checkouts and returns, desk changes, new items and flags.
- **The next writer applies them** in timestamp order, each recorded in history as the device's and person's.
- **When an entry no longer applies,** for example a proposal already decided elsewhere, it's recorded as skipped, with the reason, and the device is told.
- **What v0.8 adds:** the actions the inbox doesn't carry yet, so that every operation works from a non-writer.

## iCloud Drive's files

- **Download on demand:** iCloud may evict a file's contents to save space, leaving a placeholder. Carrel never treats an evicted file as missing or changed. It reads cards and the history, which stay small, and downloads an item's contents only when it needs them (opening, previewing, checking out, drafting, snapshotting), using file coordination.
- **Keep downloaded:** a per-collection setting, `sync.keep_downloaded` (off by default), keeps a collection's items on a device, for offline use or a big screen.
- **Cache and index:** `cache.nosync/` stays out of iCloud, as FORMAT already says. The search index syncs, so a phone needn't build it.
- **Conflicted copies:** iCloud's "conflicted copy" files are caught and reported for the owner to choose, never silently picked. With one writer and per-device inboxes they should be rare.

## iPad and iPhone

- **One multiplatform SwiftUI app** shares the Mac app's models and views where they fit (CarrelKit already builds for iOS), with layouts for each size.
- **iPad:**
  - everything the Mac app does that its hardware allows: review, questions, browse, search, the librarian, discovery, checkout to a Files folder, the desk, history and undo, settings;
  - ingest with Apple's model, or a small model through MLX, chosen by its self-benchmark as the spec says;
  - Apple Pencil notes on cards;
  - writing when it holds the lock.
- **iPhone:**
  - review, questions, browse, search, the librarian (Apple's model plus the synced index), checkouts, and capture: share sheet, camera and scans;
  - heavy work, such as drafting or rebuilding the index, only while charging, per the spec's battery rules.
- **Background processing:** `BGTaskScheduler` tasks drain the intake queue and apply inbox entries when the device holds the lock and has time.
- **Every interface stays fully capable:** the phone and iPad versions are full apps, not viewers. Anything they can't do (Kopia snapshots) is a stated platform limit in `docs/dev/interfaces.md`.

## Devices without Kopia

- **The holding folder:** on iPad and iPhone, any change that replaces or removes content (a return, an expanded zip, a moved original) keeps the old version in a holding folder in the archive, until a Mac with Kopia snapshots it and clears it.
- **Status says** how much is waiting for a snapshot, and warns when it's large or old, so nothing is lost in between.

## Capability profiles and the quality ladder

- **Each device records** its chip, memory, free storage, available models, tools (git, Kopia, file watching), power and thermal state, background-time allowance, and a short self-benchmark, in `_System/devices/<id>/`.
- **Results record** the device, model and level, as #452 starts doing for cards.
- **Below-preferred work** (a card drafted on a phone, a restructure proposal from a small model) goes to a re-check queue in `_System/tasks/`. A stronger device claims items through CloudKit and proposes improvements for approval. This is #452's outdated-drafts stage, across devices.

## In each interface

| | Mac app and agent | iPad and iPhone apps | Command line (Mac) | Command line (Linux, Windows) |
| --- | --- | --- | --- | --- |
| Writer lock | the agent holds it | the app, per batch | through the agent | not available; acts through its inbox |
| Inbox | sends and applies | sends, and applies as writer | sends and applies | sends |
| On-demand files | file coordination | file coordination | through the core | the platform's sync client |
| Capability profile | yes | yes | yes | yes |
| Status | lock holder, inbox backlog, holding folder, re-checks | the same | `carrel status`, `carrel sync status` | the same, without the lock |

## Testing it

- **The coordinator and inbox logic:** in the core, with a fake CloudKit coordinator. Cover lease expiry, a stale history head, entries that no longer apply, two devices taking turns, and a device offline for days.
- **On real devices:** the gate, and a checklist (`docs/v0.8-gate.md`) run on the owner's Mac, iPad and iPhone. CloudKit and iCloud Drive can't be faked convincingly, so this is checked by hand.

## What the owner needs to do

- **Enable iCloud and CloudKit** for the app's identifier in the Apple Developer account (a CloudKit container), and add the iPad and iPhone apps to it.
- **Install builds on the iPad and iPhone** through TestFlight or Xcode, and sign in to iCloud on each.
- **Choose where the archive lives in iCloud Drive,** and move it there with Carrel's help, which v0.8 adds.

## Building it

1. The CloudKit coordinator in Swift, behind the core's `Coordinator` trait, in the agent and the iOS app, with the history-head wait.
2. Inbox coverage for every operation, and skipped-entry reporting.
3. iCloud Drive's on-demand files, file coordination, `sync.keep_downloaded`, and conflicted-copy reporting.
4. Capability profiles, and the cross-device re-check queue (with #452).
5. The holding folder for devices without Kopia.
6. The iPad app.
7. The iPhone app, with capture and the battery rules.
8. Moving an archive into iCloud Drive, and `carrel sync status`.
9. The user guide, then the gate checklist.

## Decisions (accepted 2026-11-01)

1. **The archive stays plain files in iCloud Drive, and CloudKit holds only the writer lock (device, lease, history head), never file contents.** Recommended: it keeps the archive readable everywhere without Carrel, as the spec requires, and keeps CloudKit small and private.
2. **On a Mac the agent holds the lock, and the app and command line go through it. On Linux and Windows the command line acts on a synced archive only through its inbox.** Recommended: one writer per Mac, and no way for a device without CloudKit to write history behind the lock's back.
3. **The iPad and iPhone apps are one multiplatform app sharing the Mac app's code, and full participants at their hardware's level, not viewers.** Recommended: it's the spec's principle and the project's rule that every interface is fully capable, and it reuses CarrelKit.
4. **iCloud's evicted files are downloaded on demand and never treated as missing, with an optional per-collection "keep downloaded".** Recommended: it's how iCloud Drive works, and the archive can be bigger than a phone.
5. **Devices without Kopia keep replaced content in a holding folder until a Mac snapshots it, and weaker devices' results go to a re-check queue that a stronger device claims.** Recommended: both come from the spec, and they mean nothing done on a phone is lost or left at phone quality.
