# Design: the ingest queue, ingesting from the interactive mode, and settings (v0.2)

Status: accepted 2026-10-30, with the five recommendations below. Revisits the interactive mode's decision to keep ingest a command (`docs/design/interactive.md`), now that ingest can report progress item by item (#127).

## What this changes

Today `carrel ingest PATH` runs a batch start to finish: every item through one stage before the next, saved only at the end, one batch at a time. A quit loses the whole batch, and nothing can be added while it runs.

With this design, ingest becomes a queue you feed whenever you like:

- adding files only queues them, and returns at once;
- a worker drafts, routes and rates them **one item at a time**, saving each proposal as soon as it's ready;
- it keeps going after the interactive mode or the terminal closes, and picks up where it left off after a quit or crash;
- and you can keep adding, reviewing, answering and browsing meanwhile.

Nothing about review changes: every proposal still waits for the owner's approval before anything is filed.

## The intake queue

- **Where:** `_System/queue/intake/`, beside the review and question queues (FORMAT.md §7.9), one JSON file per item.
- **What an entry holds:** the source path, the device it's on, whether a folder is one item or each entry (`each`/`whole`), when it was added, its place in the order, and, once it has been tried, how far it got.
- **Device-bound:** an entry is processed only on its own device, since the path exists only there, the same rule review entries follow. Other devices see it as waiting for that device.
- **Duplicates while queued:** adding something already queued, or already proposed, is skipped, with a note in the plan.

## Item by item

- The worker takes the next entry for this device and runs it through text, card, route and sensitivity, then saves its proposal to the review queue and removes the entry. The work is the same as today; only the order changes, from stage by stage to item by item.
- **Interruptions** lose only the item in progress, which starts again on resume.
- **Questions** are recomputed after each item across all open proposals, so a question that would settle several items still ranks first, and items waiting on it are marked as they arrive.
- **Rules that auto-file** (`auto_file: true`) file their item as soon as its proposal is saved, as `carrel ingest` does today, one history commit per item.

## The worker

- **`carrel worker`** processes this device's intake queue for an archive until it's empty, then exits.
- **Started on demand:** adding to the queue starts a worker in the background, detached so it outlives the terminal, unless one is already running. A lock file in Carrel's configuration folder keeps it to one worker per archive per device, and a stale lock (its process gone) is taken over.
- **No system service in v0.2:** launchd, systemd or a Windows task would need installing and would run when idle. An on-demand worker covers "keeps going after I quit" without either. `carrel worker install`, for always-on processing such as watching a folder, can come later.
- **Logs:** the worker writes a log in Carrel's configuration folder, which the interactive mode and `carrel ingest --status` read for progress and errors.

## Commands

- **`carrel ingest PATH...`** adds to the queue, shows the plan, starts the worker, and returns.
- **`carrel ingest --wait PATH...`** also stays until the queue is done, with the progress bar, for scripts and CI.
- **`carrel ingest --dry-run PATH...`** shows the plan only, at once, and queues nothing: what would be added, the kinds, duplicates, what's skipped and why, and how each folder splits. It runs no models. Today's dry run, which drafts and routes everything and then throws it away, isn't needed: proposals already wait for approval, so the normal path is the preview.
- **`carrel ingest --status`** shows the queue: what's running, what's waiting, and anything that failed. With `--json` too.
- **`carrel ingest --remove ITEM`** and **`--first ITEM`** take a waiting item off the queue or move it to the front.

## In the interactive mode

- **`i` on any screen** opens **Ingest**: choose files, see the plan (as `--dry-run` shows it, with `space` toggling a folder between one item and each entry), and `Enter` to add them to the queue.
- **The status line** shows the worker's progress on every screen, with the item, its stage and the count left.
- **The Ingest screen** lists the queue: running, waiting and failed, with `x` to remove a waiting item, `t` to move it to the front, and `r` to retry a failed one.
- **Review and Questions** refresh as proposals and questions arrive.
- **Quitting** never stops the worker; the status line says it's still going.

## Choosing files

Three ways, each always available:

1. **Your own file picker.** The interactive mode suspends, as it does for the editor, runs the picker, and reads back the paths chosen. Most terminal file managers have a mode for this:

   | Picker | Command Carrel runs |
   | --- | --- |
   | yazi | `yazi --chooser-file={out}` |
   | lf | `lf -selection-path={out}` |
   | nnn | `nnn -p {out}` |
   | ranger | `ranger --choosefiles={out}` |
   | vifm | `vifm --choose-files {out}` |
   | fzf | `fzf --multi --walker=file,dir` (paths on standard output) |

   Carrel uses the one chosen in Settings, or else the first of these on `PATH`. Any command works: `{out}` is replaced by a file to write paths to, one per line, and a command without `{out}` writes them to standard output. That's also how an editor can be the picker (#136).
2. **A built-in browser,** for when no picker is installed, as on a fresh Windows machine: `Enter` opens a folder, `space` marks, `i` adds what's marked.
3. **Typing or pasting a path,** with `/` from either of the above, which is handy for dragging a file from Finder into the terminal.

## Settings

A **Settings** screen in the interactive mode, in two clearly labeled parts.

**This device**, kept in `cli.json` in Carrel's configuration folder:

- **File picker:** the one in use, chosen from those installed, or a custom command. Pickers that aren't installed are listed with a line on what each is good at and the install command for this system (`brew install yazi`, `apt install lf`, `winget install sxyazi.yazi`). Carrel never installs anything itself.
- **Editor:** `$VISUAL` or `$EDITOR`, with an override for Carrel.
- **Models:** what `carrel models` shows, with a download started from here, after the same question about size and license.
- **The worker:** whether it's running, and its log.

**This archive**, kept in `_System/settings.json` and shared by every device that opens the archive, marked as such:

- what happens to originals on approval (`ingest.originals`: move or keep);
- the crowded-collection threshold (`signals.crowded_items`);
- how changes made outside Carrel are handled (`outside_edits`);
- backups: where they go, and when the last snapshot and check were.

Each change is saved when it's made. A change to archive settings is a history commit, so it can be undone.

## Building it

1. The intake queue in the core and FORMAT.md §7.9, item-by-item ingest that saves each proposal, and incremental questions.
2. `carrel worker`, its lock and log, and the new `carrel ingest`: queueing, `--wait`, the plan-only `--dry-run`, `--status`, `--remove` and `--first`.
3. The interactive mode's Ingest screen and status line, with typed paths.
4. External pickers, then the built-in browser.
5. The Settings screen.

## Decisions (accepted 2026-10-30)

1. **A persistent intake queue in the archive** (`_System/queue/intake/`, device-bound like review entries), processed **item by item** with each proposal saved as it's ready. Recommended.
2. **An on-demand, detached `carrel worker`**, one per archive per device, that resumes unfinished work; no system service in v0.2. Recommended.
3. **`--dry-run` becomes a plan only** (no models, nothing queued); the drafted preview goes, since review already is one. Recommended.
4. **Ingest from the interactive mode** with `i`, a plan before queueing, the queue on its own screen, and your own file picker, with a built-in browser and typed paths as fallbacks. Recommended.
5. **A Settings screen** for this device (picker, with suggestions and install commands but no installing; editor; models; worker) and this archive (shared settings, each change a history commit). Recommended.
