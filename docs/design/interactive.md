# Design: interactive mode (v0.2)

Status: proposed. Issue #88.

## What it's for

Reviewing a batch one command at a time is slow:

- `carrel review` to list
- `show` to read one
- `move-to` when the destination is wrong
- `approve` with an identifier copied from the list

The interactive mode puts that loop on one screen: the queue on one side, the proposal on the other, and a key for each decision. It also browses the archive, answers questions, and shows status and history.

It's a terminal interface built with [ratatui](https://ratatui.rs) and crossterm. It works in any terminal and over SSH on macOS, Linux and Windows, which is why it comes before the Mac app (v0.5).

## Opening it

- **Recommended: `carrel` with no command opens it when it's run in a terminal.** When output goes to a pipe or a file, as in scripts, it prints help instead, as it does now. `carrel tui` always opens it.
- It opens on the archive commands would use: `--archive`, the one containing the current folder, or the one chosen with `carrel open`. With none, it offers the archives it knows and `carrel init`.

## Screens

Tabs along the top, switched with `1` to `5` or `Tab`:

1. **Status:** what `carrel status` shows, including:
   - counts, problems, and names that won't work everywhere
   - who is writing, and the last snapshot
   - what's waiting: proposals and questions, which `r` and `q` jump to
2. **Review:** the queue grouped by destination, as `carrel review` shows it, beside the selected proposal in full:
   - the card, the destination and why, sensitivity and why, problems, and the provider calls
   - keys: `a` approve, `h` hold, `m` move (a collection picker), `x` reject, `e` edit, `Space` mark
   - marks: decisions apply to every marked proposal, as one batch with one commit and one snapshot
   - `A` approves every high-confidence proposal that isn't sensitive (`--all-high`)
3. **Questions:** ranked as `carrel questions` ranks them, with the items each affects. `Enter` answers: a line of text for a term, or a collection picker. `d` dismisses.
4. **Browse:** the collection tree beside the selected collection's items, and the selected item's card, read only. `/` searches titles, tags and summaries in what's loaded.
5. **History:** `carrel log`, with the selected change's items and moves. `u` undoes it after asking.

A help line along the bottom always shows the keys that work on the current screen; `?` shows them all. Mouse clicks and scrolling work, but nothing needs them.

## How it works

- **One implementation:** everything the interactive mode does is also a command with `--json` (#88). Today the review and question logic lives in the CLI's command functions: deciding a batch, redirecting a proposal, teaching the glossary and redrafting. **Recommended:** that logic moves into the core, as `carrel_core::review` and `carrel_core::questions` operations that return structured results. The commands, the interactive mode and later the Mac app then share it, and the commands only format results.
- **State and drawing apart:** an `App` holds the state and turns key presses into actions. Drawing reads the state, so screens are tested with ratatui's test backend and actions without a terminal.
- **Long work in the background:** approval batches (with their snapshots) and redrafts after an answer run on a worker thread. The screen stays responsive and shows progress, and the writer lease is held only while the work runs. If another device holds the lease, the screen says so and the decision waits.
- **Refreshing:** the queues and status are reread after each action, and on `Ctrl-R`. The interactive mode doesn't watch the disk in v0.2.
- **Terminals:**
  - it needs at least 80×24 and says so below that
  - it honors `NO_COLOR`, and color never carries meaning on its own: confidence and sensitivity also have words
  - it restores the terminal on exit, on errors and on `Ctrl-C`

## Editing a proposal's card

- **Recommended: `e` opens the draft card in the owner's editor** (`$VISUAL`, then `$EDITOR`, then `notepad` on Windows or `vi` elsewhere) as YAML: title, summary, tags, dates and sensitivity.
  - Saving checks the draft: tags are checked against the destination's vocabulary, and a sensitivity can only be raised above what was found.
  - A draft that doesn't check out reopens with the problems listed as comments at the top.
  - This is also the missing `carrel review edit ID` command.
- The alternative is a form inside the interface, which is more work and less pleasant for editing a summary.

## Not in v0.2

- **Running ingest from inside the interactive mode:** ingest stays a command, which can take minutes with a model. Afterwards `carrel` opens on the new proposals. Showing a running ingest's progress can come later.
- **Editing archived cards, checkout and sessions:** these come with Phase 3 (#4).
- **Watching the archive for changes made elsewhere.**

## Building it

1. Move the review and question operations into the core, with the commands rewritten on top. Their output stays the same, checked by the existing CLI tests.
2. `carrel review edit` with the editor flow.
3. The interactive mode's shell: opening, tabs, help, the terminal lifecycle and Status.
4. Review, with marks, batches and the collection picker.
5. Questions, Browse and History.

## Decisions to make

1. **Recommended: `carrel` alone opens the interactive mode in a terminal,** and prints help otherwise. The alternative is only `carrel tui`.
2. **Recommended: v0.2 covers status, review, questions, browsing and history;** ingest stays a command.
3. **Recommended: cards are edited in the owner's editor as YAML,** checked on save. This also gives `carrel review edit`.
4. **Recommended: review and question operations move into the core** first, so the commands, the interactive mode and the Mac app share one implementation.
5. **Recommended: undo from History asks first** and shows what the change did. Every other decision is made with one key, and each can be undone from History.
