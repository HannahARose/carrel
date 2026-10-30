# Design: ingesting from the interactive mode (v0.2)

Status: draft for the owner. Revisits the interactive mode's decision to keep ingest a command (`docs/design/interactive.md`), now that ingest reports progress item by item (#127).

## What this adds

Today a batch starts in the shell (`carrel ingest PATH`) and is reviewed in the interactive mode. This puts both in one place: pick files with the file manager you already use, see the plan, start it, and keep reviewing, answering questions or browsing while it runs.

## Starting an ingest

- **`i` on any screen** opens **Ingest**: choose what to ingest, see the plan, start.
- **The plan before anything runs:** the same as `carrel ingest --dry-run`. It shows how many items, their kinds, duplicates, what's skipped and why, and for each folder whether it's one item or each entry (`--each`), toggled with `space`. `Enter` starts the batch, `Esc` goes back.
- **While it runs:** the status line shows the stage, the item, done/total and the time left, as `carrel ingest` does on a terminal. Every screen stays usable. Review shows the new proposals as soon as the batch is saved.
- **One batch at a time.** Quitting while one runs asks first, and says that nothing from the batch is kept: ingest saves a batch's proposals when the batch finishes.

## Choosing files

Three ways, each always available:

1. **Your own file picker.** The interactive mode suspends, as it does for the editor, runs the picker, and reads back the paths chosen. Most terminal file managers have a mode for exactly this, writing the paths you select to a file:

   | Picker | Command Carrel runs |
   | --- | --- |
   | yazi | `yazi --chooser-file={out}` |
   | lf | `lf -selection-path={out}` |
   | nnn | `nnn -p {out}` |
   | ranger | `ranger --choosefiles={out}` |
   | vifm | `vifm --choose-files {out}` |
   | fzf | `fzf --multi --walker=file,dir` (paths on standard output) |

   Carrel uses the first of these it finds on `PATH`, starting in the last folder ingested from, or your home folder. To choose another, or give any command, set `picker` in `cli.json`, Carrel's per-device CLI preferences. `{out}` is replaced by a file to write paths to, one per line; a command without `{out}` writes them to standard output. Editors can be pickers too, which is where the Vim and Emacs work below comes in.
2. **A built-in browser,** for when no picker is installed, as on a fresh Windows machine. It shows folders and files with `Enter` to open a folder, `space` to mark, and `i` to ingest what's marked.
3. **Typing or pasting a path,** with `/` from either of the above. This is handy for dragging a file from Finder into the terminal.

## Vim and Emacs

Using Neovim or Emacs as the picker, and Carrel from inside them more generally, is worth doing but is its own piece of work (issue to follow). It means plugins that drive `carrel --json`: ingest the current file or marked files, show an item's card, and review in a buffer. This design only makes sure a picker command can be an editor.

## Building it

1. The Ingest screen with typed paths, the plan, and a batch run in the background with progress, reusing `run_with_progress` and the existing event loop.
2. External pickers: detection, `picker` in `cli.json`, suspending the terminal, and reading the chosen paths.
3. The built-in browser.

## Decisions for the owner

1. **Ingest from the interactive mode** with `i`, a plan first, and the batch running in the background with progress while every screen stays usable. Recommended: it reverses the earlier decision only because progress now exists.
2. **Use the owner's own file picker** through a `picker` command in `cli.json`, with yazi, lf, nnn, ranger, vifm and fzf found automatically. Recommended: there's no picker to rebuild, and it fits how the editor already works.
3. **A built-in browser and typed paths as fallbacks,** so it works without installing anything. Recommended.
4. **One batch at a time, kept only when it finishes;** quitting mid-batch asks first. Recommended for v0.2. Saving item by item, so a stopped batch keeps what it did, can follow.
5. **Vim and Emacs integration as a separate, scoped issue,** not part of this. Recommended.
