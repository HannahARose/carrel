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
| `carrel models` | Lists the models Carrel can download, with size, memory needed and license, and the models already downloaded. |
| `carrel models download NAME [--format mlx] [--yes]` | Downloads a model after showing its size and license and asking; `--yes` skips the question. Every file is checked against the checksum Carrel ships with. `--format mlx` downloads the MLX build, a folder, for the MLX provider. |
| `carrel models remove NAME [--format mlx] [--yes]` | Deletes a downloaded model, or its MLX build, after asking. |

## Settings

| Command | What it does |
| --- | --- |
| `carrel settings` | Lists this device's preferences and the archive's shared settings. |
| `carrel settings set KEY VALUE` | Changes one. `picker`, `editor` and `model` are this device's (an empty value, or `default`, clears them); `model` takes `provider/offer`, the offer alone, or a provider such as `apple`, and must be one that's available. `ingest.originals`, `signals.crowded_items` and `outside_edits` are shared by every device, and each change can be undone from History. |

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
| `8` | **Backups**: the snapshots, each with its change; `n` snapshot now, `v` verify, `r` restore an item as of the chosen snapshot, `s` set up or connect a repository |

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
