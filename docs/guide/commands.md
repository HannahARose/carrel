# Commands

Every command `carrel` has, and its options. `carrel help COMMAND` prints the same in the terminal. For what the parts mean, see the [User guide](user-guide.md).

## For every command

| Option | Effect |
| --- | --- |
| `--archive PATH` | The archive to use. Without it: the archive containing the current folder, then the one chosen with `carrel open` |
| `--json` | One JSON value on standard output instead of a report, for scripts. Errors are `{"error": "…"}` on standard error |

Exit status is `0` on success, `1` when the command failed, and `2` for a usage mistake. `carrel` with no command opens the interactive mode in a terminal, and prints the usage elsewhere.

## Archives

| Command | What it does |
| --- | --- |
| `carrel init [PATH] [--name NAME] [--allow-existing-files]` | Creates an archive in `PATH` (or the current folder) and starts its history. With `--allow-existing-files`, files already there become uncataloged items to ingest. Inside an archive that has no history yet, it starts the history. |
| `carrel open [PATH]` | Chooses the archive commands use outside one; with no path, shows the choice. |
| `carrel status` | Summarizes the archive: items by kind, cards with problems, names that won't work everywhere, **Structure** signals, the last change and backup, and who is writing. |
| `carrel show ITEM` | Shows an item's card. `ITEM` is its identifier, or the path of the item or its card. |
| `carrel log [-n COUNT]` | Lists recorded changes, newest first. |
| `carrel undo CHANGE` | Undoes a change with a new change that reverses it. `CHANGE` is its identifier from `carrel log`, or its start. The change that created the history can't be undone. |
| `carrel checkout ITEM [--to FOLDER] [--due DATE]` | Checks an item out to work on: copies it to `FOLDER` (the Desktop by default), checked against what it held, and makes the archive copy read-only until it's returned. Records it on the card, with the commit a Git repository was at, in one history change. |
| `carrel checkouts` | Lists what's checked out, where, since when, and whether the working copy has changed. |
| `carrel return ITEM [--from PATH] [--yes] [--trim-snapshots]` | Returns a checked-out item. Shows what changed (files changed, added, removed and renamed, and a Git repository's commits since checkout, with a warning about uncommitted work), the history entry it adds, and any session snapshots of the working copy, and asks. Then takes a backup snapshot, writes the working copy beside the archive copy, checks it, swaps it in, and records it all as one history change. The working copy stays where it is for you to delete. `--from` gives its new place if it moved. With session snapshots, it then snapshots the returned version and asks whether to forget them, keeping the return's; `--trim-snapshots` forgets them without asking, and they're kept otherwise, as with only `--yes`. |
| `carrel return ITEM --discard` | Cancels a checkout: the archive copy is unlocked as it was, and the working copy's changes aren't brought back. |
| `carrel desk` | Lists this device's desk items: files and folders being worked on where they are, tracked without being archived. |
| `carrel desk add PATH [--title T] [--summary S]` | Puts a file or folder on the desk: a card goes beside it (its name with `.md` added), and nothing is written inside it or moved. Not for things inside the archive. |
| `carrel desk remove PATH [--delete-card]` | Takes it off the desk; its card stays unless `--delete-card`. |
| `carrel sessions` | Finds finished work sessions on items checked out to this device and on its desk (changes closer together than `sessions.idle_minutes`, 45 by default, are one session, and Git commits count too) and adds a pending history entry for each, then lists every pending entry, numbered. A session still going isn't written until it's over. With backups set up, it snapshots the working copy of each item it found sessions on, unless the card says `snapshots: return-only`. Commands that change the archive, and the interactive mode, do the same every five minutes at most, unless another program is writing. |
| `carrel sessions approve\|combine\|drop ITEM [N...]` | Approves, combines into one, or drops an item's pending entries (a desk item by its path), by the numbers `carrel sessions` shows; all of them when none are given. Each is a history change. |
| `carrel edit ITEM` | Opens a filed item's card in your editor (`e` in Browse). It's checked as proposals are: a title, real dates, tags that apply in its collection, and no sensitivity below what a rule found. The body below the fields is kept exactly. Delete a line to clear it, such as a wrong date. One change, which `carrel undo` reverses. |
| `carrel correct ITEM [WORDS…] [--yes] [--learn]` | Corrects a filed item's card the same way, and refiles it when the collection changes. One change, which `carrel undo` reverses. |
| `carrel rename ITEM [NEW_NAME]` | Renames an item and its card together, and updates links to it. Without a new name, uses the portable name `carrel status` suggests. Can be undone. |
| `carrel collection new PATH [--description TEXT] [--tag NAME=MEANING]...` | Creates a collection with a design document to fill in. `PATH` is a name, or `Parent/Name` inside an existing collection. |

## Ingest

| Command | What it does |
| --- | --- |
| `carrel ingest PATH... [--each \| --whole]` | Adds files and folders to the intake queue, shows the plan, starts a worker in the background, and returns. Each argument is one item; `--each` makes each entry of a folder its own item, and `--whole` keeps a folder named like a pile (Downloads, Desktop, Inbox) as one. |
| `carrel ingest --dry-run PATH...` | Shows the plan at once (items, kinds, duplicates, what's left out) without queueing anything or running a model. |
| `carrel ingest --wait PATH...` | Queues the items and stays until they're drafted, with a progress bar, then reports each one: its text, card, destination and sensitivity. |
| `carrel ingest --status` | Shows the worker and this device's queue, including items that failed. |
| `carrel ingest --remove ID`, `--first ID`, `--retry ID` | Takes a waiting item off the queue, moves it to the front, or tries a failed one again. |
| `carrel worker` | Drafts this device's queue until it's empty. Adding to the queue starts one in the background, so this is rarely run by hand. Set `CARREL_WORKER=off` to stop one starting by itself. |

## Review

| Command | What it does |
| --- | --- |
| `carrel review [--all]` | Lists proposals waiting for review, by destination, with their confidence. `--all` includes those already decided. |
| `carrel review show ID` | Shows a proposal: the card it would write, where it would go and why, how sensitive it is and why, any model that declined it, and what's left to settle. A duplicate or likely duplicate is shown beside the archived item. |
| `carrel review approve ID... [--all-high]` | Files the items where proposed, as one history change. `--all-high` approves every high-confidence proposal that isn't sensitive or a duplicate. |
| `carrel review approve ID... --as-version-of [ITEM]` | Files them as new versions of the archived item `ITEM`, with a `version-of` link. `ITEM` can be left out when each is a duplicate of just one item. |
| `carrel review approve ID... --related-to [ITEM]` | Files them beside the archived item, with a `related` link. |
| `carrel review hold ID...` | Files the items in Holding, to place later. |
| `carrel review move-to ID COLLECTION` | Proposes another collection; the item is filed there when approved. |
| `carrel review edit ID` | Opens the card in your editor, checks it when you save, and reopens it with the problems listed until it checks out. |
| `carrel review correct ID [WORDS…] [--yes] [--learn]` | Corrects a proposal in plain words ("it's from 2019", "it's a picture of Annecy, not Geneva") or `field: value` lines. Shows each change before and after, and saves only when you agree (`--yes` saves without asking). A broad correction can route it again. `--learn` adds a lasting fact it states to the glossary without asking. |
| `carrel review reject ID...` | Rejects proposals; the items stay where they are. |

Identifiers can be shortened to their last eight characters, as the list shows them.

## Questions

| Command | What it does |
| --- | --- |
| `carrel questions` | Lists the questions waiting for an answer, most items first. |
| `carrel questions answer ID ANSWER...` | Answers a question: a sentence for what a term means, or a collection's path. Items waiting on a term are drafted again with the answer, which is saved to the glossary. |
| `carrel questions dismiss ID` | Dismisses a question; its items stop waiting on it. |

## Backups

Backups need [Kopia](https://kopia.io), beside `carrel` or on `PATH`.

| Command | What it does |
| --- | --- |
| `carrel backup setup PATH [--description TEXT]` | Creates a Kopia repository at `PATH` or connects to the one there. The password is asked for once and kept in this device's keychain. |
| `carrel backup now` | Takes a snapshot now. Each approved batch takes one anyway. |
| `carrel backup list` | Lists snapshots, newest first, with the change each was taken with. |
| `carrel backup verify [--percent N]` | Checks the repository, reading back `N`% of file contents (1% by default). |
| `carrel backup restore ITEM [--at CHANGE] [--to FOLDER]` | Restores an item's contents as they were, into a new folder beside the archive. It never overwrites the item. |

## Models and providers

| Command | What it does |
| --- | --- |
| `carrel providers` | Lists the providers on this device (text models, text extraction) and which Carrel uses for each. |
| `carrel formats` | Lists the file types this device reads and which provider reads each, then the types it couldn't read so far, most frequent first, with what would read them when Carrel knows. |
| `carrel models` | Lists the models Carrel can download, with size, memory needed and license, and the models already downloaded. |
| `carrel models download NAME [--format mlx] [--yes]` | Downloads a model after showing its size and license and asking; `--yes` skips the question. Every file is checked against the checksum Carrel ships with. `--format mlx` downloads the MLX build, a folder, for the MLX provider. |
| `carrel models remove NAME [--format mlx] [--yes]` | Deletes a downloaded model, or its MLX build, after asking. |

## Settings

| Command | What it does |
| --- | --- |
| `carrel settings` | Lists this device's preferences and the archive's shared settings. |
| `carrel settings set KEY VALUE` | Changes one. `picker`, `editor`, `model` and `second_model` are this device's (an empty value, or `default`, clears them); `model` takes `provider/offer`, the offer alone, or a provider such as `apple`, and must be one that's available; `second_model` also takes `off`. `ingest.originals`, `signals.crowded_items` and `outside_edits` are shared by every device, and each change can be undone from History. |

## The interactive mode

`carrel` with no command (or `carrel tui`) opens it. The screens are numbered, and `?` shows every key.

| Key | Screen |
| --- | --- |
| `1` | **Status**: what `carrel status` shows, and what's waiting for you |
| `2` | **Review**: `a` approve, `h` hold, `m` move, `e` edit, `x` reject, `Space` mark, `A` approve all high-confidence |
| `3` | **Questions**: `Enter` to answer, `d` to dismiss |
| `4` | **Browse**: collections and items; `/` to search |
| `5` | **History**: `u` to undo the chosen change |
| `6` | **Ingest**: the queue; `x` remove, `t` to the front, `r` retry |
| `7` | **Settings**: `Enter` to change the chosen setting |
| `8` | **Backups**: the snapshots, each with its change; `n` snapshot now, `v` verify, `r` restore an item as of the chosen snapshot, `s` set up or connect a repository, choosing its folder with the file picker |

`i` on any screen chooses files to ingest. `Ctrl-R` reads everything again, and `q` quits; the worker keeps going.

## Environment

| Variable | Effect |
| --- | --- |
| `CARREL_CONFIG_DIR` | Where Carrel keeps this device's files, instead of the default (see the [User guide](user-guide.md#what-carrel-keeps-on-this-device)) |
| `CARREL_MODELS_DIR` | Where local models are kept |
| `CARREL_WORKER=off` | Never start a worker in the background |
| `CARREL_OCR_LANGUAGES` | Tesseract's languages for OCR, such as `eng+deu` |
| `VISUAL`, `EDITOR` | The editor for cards, unless the `editor` setting is set |
| `NO_COLOR` | Draw the interactive mode without color |
| `KOPIA_PASSWORD` | The backup password, for scripts; Kopia then doesn't store it |
