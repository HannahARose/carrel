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
| `carrel init [PATH] [--name NAME] [--allow-existing-files] [--embedding-model NAME]` | Creates an archive in `PATH` (or the current folder) and starts its history. With `--allow-existing-files`, files already there become uncataloged items to ingest. `--embedding-model` chooses the model every device uses to search the archive by meaning: `embeddinggemma-300m` (the default), `qwen3-embedding-0.6b`, or `apple-nl-sentence-en`, which needs no download but works only on Apple devices. Inside an archive that has no history yet, it starts the history. |
| `carrel open [PATH]` | Chooses the archive commands use outside one; with no path, shows the choice and the archives opened or created on this device lately. |
| `carrel status` | Summarizes the archive: items by kind, cards with problems, names that won't work everywhere, **Structure** signals, the last change and backup, who is writing, and **Recent**: the latest changes and work sessions, archive-wide. Also suggests folders on the Desktop or in `~/Code` that look like active work (changed in the last two weeks, or a Git repository) to put on the desk, and desk items unchanged for 60 days to archive; nothing is tracked or moved unless you ask. Now and then it names a collection not opened in a long time, with an item or two from it ([Discovering the archive](user-guide.md#discovering-the-archive)). |
| `carrel show ITEM` | Shows an item's card; under Related, its links and the five items nearest it in the meaning index (by its card, which is already embedded, so it needs no model), or why only the links are listed; and every change that touched it, newest first (undo one with `carrel undo CHANGE`). `ITEM` is its identifier, or the path of the item or its card. See [Related items](user-guide.md#related-items). |
| `carrel search QUERY... [-n COUNT]` | Finds items by the words in their cards and text, and collection documents and glossary facts, best first, with the passage that matched and why. Filters narrow it: `collection:NAME`, `tag:TAG`, `kind:KIND`, `after:DATE`, `before:DATE` (a year, month or day), `sensitivity:LEVEL` and `checked-out`. Restricted items are found by their card only. Reads the text of items not read yet first. `-n`/`--limit` shows at most `COUNT` results (20). See [Searching](user-guide.md#searching). |
| `carrel index [status]` | Shows the archive's embedding model, how many items are embedded in the meaning index and how many are behind, and whether the worker is catching up. |
| `carrel index model [NAME]` | Switches the archive, on every device, to the embedding model `NAME`, which a provider on this device must offer (if none does, it says which can). The new model's index is built in a folder of its own, with a progress bar, while search and the worker keep using the current one; once it's complete, `embedding.model` changes, everything uses the new index, and the old one is removed. A device without the new model searches by words and says why. Interrupted, running it again carries on; `NAME` set to the archive's current model stops a switch part way and removes its index. Without `NAME`, shows the archive's model, any switch in progress, and the models this device can embed with. |
| `carrel index rebuild` | Embeds every item again with the archive's embedding model (asked for first if the archive has none), with a progress bar: each card, and the text of items that aren't restricted in passages of about 500 tokens, up to 20 an item. Reads the text of items not read yet first. Only the archive's model is used, never another; if no provider here runs it, nothing changes. Interrupted, running it again carries on where it stopped. |
| `carrel log [-n COUNT] [--librarian]` | Lists recorded changes, newest first, marking those the librarian made on its own; `--librarian` lists only those. |
| `carrel undo CHANGE` | Undoes a change with a new change that reverses it. `CHANGE` is its identifier from `carrel log`, or its start. The change that created the history can't be undone. |
| `carrel undo --librarian SINCE` | Undoes every change the librarian made on its own since `SINCE` (a span such as `24h` or `7d`, `today`, or a date such as `2026-11-01`) that's still there, newest first. One that can't be undone, because you changed the same card since, is left and reported. |
| `carrel ask ["QUESTION"] [--save]` | Asks the librarian (see [Asking the librarian](user-guide.md#asking-the-librarian)). With a question, one answer; without, a conversation, a question per line until an empty line. Each answer lists the items it's about, the passages it used, its suggestions and what became of them, and the model that gave it. `--save` writes the conversation to this month's Journal. With `--json`, one question prints the answer, `{"question", "text", "items", "passages", "suggestions", "steps", "model", "tokens", "stopped", "saved", "changes"}`, and a conversation prints `{"answers", "saved", "changes"}`. Each item has `id`, `title`, `card`, `collection` and `sensitivity`; each suggestion has `change` (`refile`, `correct`, `rename` or `collection`) with its fields, `why`, `confidence`, `outcome` (`made` with its `commit`, `proposed`, `refused` or `failed`, with a `reason`) and `command`, the command that makes it. |
| `carrel checkout ITEM [--to FOLDER] [--due DATE]` | Checks an item out to work on: copies it to `FOLDER` (the Desktop by default), checked against what it held, and makes the archive copy read-only until it's returned. On macOS the working copy gets a “Checked Out” Finder tag, removed again on return or cancel. Records it on the card, with the commit a Git repository was at, in one history change. |
| `carrel checkouts` | Lists what's checked out, where, since when, and whether the working copy has changed. |
| `carrel return ITEM [--from PATH] [--yes] [--trim-snapshots]` | Returns a checked-out item. Shows what changed (files changed, added, removed and renamed, and a Git repository's commits since checkout, with a warning about uncommitted work), the history entry it adds, and any session snapshots of the working copy, and asks. Then takes a backup snapshot, writes the working copy beside the archive copy, checks it, swaps it in, and records it all as one history change. The working copy stays where it is for you to delete. `--from` gives its new place if it moved. With session snapshots, it then snapshots the returned version and asks whether to forget them, keeping the return's; `--trim-snapshots` forgets them without asking, and they're kept otherwise, as with only `--yes`. |
| `carrel return ITEM --discard` | Cancels a checkout: the archive copy is unlocked as it was, and the working copy's changes aren't brought back. |
| `carrel desk` | Lists this device's desk items: files and folders being worked on where they are, tracked without being archived. First rebuilds `Active/`, copying notes you wrote in a mirror in `Active/On desk/` to its card. |
| `carrel desk add PATH [--title T] [--summary S]` | Puts a file or folder on the desk: a card goes beside it (its name with `.md` added), drafted by your text model as ingest drafts cards (title, summary, tags, date), and nothing is written inside it or moved. `--title` and `--summary` are kept as you give them; with no model, the title is the item's name. Not for things inside the archive. |
| `carrel desk remove PATH [--delete-card]` | Takes it off the desk; its card stays unless `--delete-card`. |
| `carrel desk archive PATH` | Archives a desk item: queues it for ingest and takes it off the desk. Its proposal keeps the desk card's title, summary, tags and dates, and filing it keeps the card's identifier and history, moving the card into the archive with the item. |
| `carrel sessions` | Finds finished work sessions on items checked out to this device and on its desk (changes closer together than `sessions.idle_minutes`, 45 by default, are one session, and Git commits count too) and adds a pending history entry for each, then lists every pending entry, numbered. A session still going isn't written until it's over. With backups set up, it snapshots the working copy of each item it found sessions on, unless the card says `snapshots: return-only`. It also notices entries you approved by hand in Obsidian (a callout turned into a `###` heading, or into another callout type), records the approval as a history change without touching the entry, and brings `Active/` and the Journal up to date. Commands that change the archive, and the interactive mode, do the same every five minutes at most, unless another program is writing. |
| `carrel sessions approve\|combine\|drop ITEM [N...]` | Approves, combines into one, or drops an item's pending entries (a desk item by its path), by the numbers `carrel sessions` shows; all of them when none are given. Each is a history change. |
| `carrel sessions edit ITEM N` | Opens pending entry `N`'s text in your editor (`$VISUAL` or `$EDITOR`) and saves what you write as its text, still pending, in one history change. The card is read again when the editor closes, so changes made meanwhile are kept. Text that would read as another entry or break the history section isn't saved; the message says where your text is. |
| `carrel edit ITEM` | Opens a filed item's card in your editor (`e` in Browse). It's checked as proposals are: a title, real dates, tags that apply in its collection, and no sensitivity below what a rule found. The body below the fields is kept exactly. Delete a line to clear it, such as a wrong date. One change, which `carrel undo` reverses. |
| `carrel correct ITEM [WORDS…] [--yes] [--learn]` | Corrects a filed item's card the same way, and refiles it when the collection changes. One change, which `carrel undo` reverses. |
| `carrel expand ITEM [--anyway]` | Unpacks a filed zip or other compressed file in place: what it held becomes the item, unwrapped as at filing, its card follows (with `expanded_from`), and the compressed file goes to the Trash. Software and very large archives need `--anyway`; encrypted, split and unsafe ones are refused. One change, which `carrel undo` reverses; then put the compressed file back from the Trash. |
| `carrel rename ITEM [NEW_NAME]` | Renames an item and its card together, and updates links to it. Without a new name, uses the portable name `carrel status` suggests. Can be undone. |
| `carrel collection new PATH [--description TEXT] [--tag NAME=MEANING]...` | Creates a collection with a design document to fill in. `PATH` is a name, or `Parent/Name` inside an existing collection. |

With `--json`, `carrel search` prints `{"root", "query", "total", "meaning", "results"}`, where `root` is the archive's folder as an absolute path and every path in the results is relative to it. Each result has `found` (an `item` with its `id`, `title`, `card`, `item`, `collection`, `kind`, `sensitivity`, `date`, `tags` and `checked_out`; a `collection` with its `path` and `title`; or a `glossary` fact's `term`), `score`, `why` (the fields that matched, strongest first), and `passage` (`text`, and `highlights` as byte ranges `{start, end}` into it) with `passage_from`. `total` counts every match before the limit. `meaning` says whether search by meaning was used: `{"state": "current"}`, `{"state": "behind", "items": N}` when `N` items aren't embedded yet (they're found by their words), or `{"state": "off"}`, when `words_only` says why: no embedding model, no index yet, or no provider here that runs the model. With `--json`, `carrel index status` prints `{"model", "switch", "exists", "items", "embedded", "behind", "gone", "vectors", "dimensions", "rebuilding", "worker"}`, where `switch` is `null` or `{"from", "to", "items", "embedded", "behind"}` for a switch of model in progress, and `carrel index rebuild` prints `{"model", "embedded", "items", "vectors", "dimensions", "resumed"}`. With `--json`, `carrel show` prints `{"root", "card", "item", "collection", "fields", "problems", "history"}`: `card` and `item` are relative to `root`, `fields` is the card's fields as in its front matter, and `history` lists the changes that touched it, or is `null` when the archive has no history. The [editor plugins](editors.md) are built on these. `carrel index model NAME` prints `{"model", "from", "switched", "embedded", "items", "vectors", "dimensions", "resumed", "replaced"}`, and without `NAME`, `{"model", "switch", "available", "elsewhere"}`: `available` lists `{"model", "provider", "dimensions"}` for each model this device can embed with, and `elsewhere` lists `{"model", "how"}` for the catalog's others.

## Ingest

| Command | What it does |
| --- | --- |
| `carrel ingest PATH... [--each \| --whole]` | Adds files and folders to the intake queue, shows the plan, starts a worker in the background, and returns. Each argument is one item; `--each` makes each entry of a folder its own item, and `--whole` keeps a folder named like a pile (Downloads, Desktop, Inbox) as one. |
| `carrel ingest --dry-run PATH...` | Shows the plan at once (items, kinds, duplicates, what's left out) without queueing anything or running a model. |
| `carrel ingest --wait PATH...` | Queues the items and stays until they're drafted, with a progress bar, then reports each one: its text, card, destination and sensitivity. |
| `carrel ingest --status` | Shows the worker and this device's queue, including items that failed. |
| `carrel ingest --remove ID`, `--first ID`, `--retry ID` | Takes a waiting item off the queue, moves it to the front, or tries a failed one again. |
| `carrel worker` | Drafts this device's queue until it's empty, then embeds the items behind in the meaning index, if a provider here runs the archive's embedding model. Adding to the queue starts one in the background, and so does every command that changes filed items (approving, correcting, editing, renaming, returning and undoing), so this is rarely run by hand. Set `CARREL_WORKER=off` to stop one starting by itself. |
| `carrel agent [ARCHIVE...] [--poll]` | Runs the background agent until stopped: it drafts the queue as items arrive, keeps the meaning index current, records work sessions as they finish and takes a daily snapshot once backups are set up. It hears changes through file events, or with `--poll` looks every few seconds. It steps aside while the Mac app's agent looks after an archive. With `--json`, it prints one JSON value per event (`claimed`, `stepped_aside`, `started`, `finished`, `postponed`, `failed`, `cannot_open`). |
| `carrel agent install [ARCHIVE...] [--poll]` | Starts the agent at log-in, and now: a launchd user agent on macOS, a systemd user service on Linux, a Task Scheduler task on Windows. It says the file it wrote and the commands it ran. |
| `carrel agent status` | Whether the agent is installed, for which archives, and whether an agent looks after each of them (and the current archive) now. |
| `carrel agent uninstall` | Stops the agent and removes exactly what `install` wrote. |

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
| `carrel review expand ID...` | Unpacks compressed files (zip, tar, gzip and the like) when they're approved, even if they were going to be kept or asked about. A large archive is unpacked without the size limits; an encrypted, split or unreadable one can't be. `z` in Review switches between this and keeping it compressed. |
| `carrel review keep-compressed ID...` | Files compressed files as they are when they're approved, rather than unpacking them. |
| `carrel review edit ID` | Opens the card in your editor, checks it when you save, and reopens it with the problems listed until it checks out. |
| `carrel review correct ID [WORDS…] [--yes] [--learn]` | Corrects a proposal in plain words ("it's from 2019", "it's a picture of Annecy, not Geneva") or `field: value` lines. Shows each change before and after, and saves only when you agree (`--yes` saves without asking). A broad correction can route it again. `--learn` adds a lasting fact it states to the glossary without asking. |
| `carrel review reject ID...` | Rejects proposals; the items stay where they are. |

Identifiers can be shortened to their last eight characters, as the list shows them.

## Questions

| Command | What it does |
| --- | --- |
| `carrel questions [list] [--all]` | Lists the questions waiting for an answer, the most worth answering first, each with the answers it suggests, numbered. Questions the glossary or a rule already answers are dropped, and questions that ask the same are merged. `--all` lists the skipped ones resting too. |
| `carrel questions answer ID [ANSWER...] [--yes] [--learn] [--create]` | Answers a question with a suggestion's number or your own words; without an answer, it's asked on the terminal. A term's answer is saved to the glossary and its items are drafted again. A collection's, group's or sender's answer is a collection, where its items go, filed ones as undoable moves; a picked new collection is created, and `--create` creates one you name. A sender's yes then asks whether to add a routing rule for future items (`--learn` adds it). An item's answer is a correction of its card, shown before and after and saved on your yes (`--yes` saves it without asking, `--learn` adds a fact it states to the glossary). |
| `carrel questions skip ID` | Skips a question: it rests for `questions.skip_days` (14 unless changed), then comes back. |
| `carrel questions dismiss ID` | Dismisses a question; its items stop waiting on it. |
| `carrel questions about ITEM` | Asks Carrel's questions about a filed item (what a photo shows, who someone named in it is, what a project was for, whether it still matters), adding them to the queue with likely answers from a text model when there is one. |

## Discovery

See [Discovering the archive](user-guide.md#discovering-the-archive).

| Command | What it does |
| --- | --- |
| `carrel discover [today] [--date DATE]` | What was made, filed or worked on on this day (or `DATE`) in other years. |
| `carrel discover wander [ITEM] [--seed N] [--steps N] [--next]` | Wanders the stacks from `ITEM` or from somewhere you haven't been in a while, one item at a time, each with why it came next. In a terminal, `Enter` takes the next step. `--next` goes on with the last walk; the same `--seed` takes the same steps. |
| `carrel discover timeline [QUERY...] [--by created\|filed\|worked] [--per year\|month\|day] [--items N]` | Items by date, narrowed by words and search's filters. |
| `carrel discover map [--depth N]` | Collections by total size and number of items, as a tree. |
| `carrel discover activity [--from DATE] [--to DATE]` | Filing and work sessions by day, as a heatmap; the last year by default. |

With `--json`, `carrel discover today` prints `{"date", "years"}`, each year `{"year", "years_ago", "items"}` and each item `{"item", "what", "summary"}`, where `item` is `{"id", "title", "card", "item", "collection", "kind", "sensitivity"}` and `what` lists `created`, `filed` or `worked`. `wander` prints `{"seed", "steps", "visited", "done"}`, each step `{"number", "item", "reason", "summary", "tags", "preview"}`, with `reason.why` one of `start`, `linked`, `near`, `neighbour` or `jump`. `timeline` prints `{"resolution", "by", "periods", "undated"}`, each period `{"period", "items"}` and each item `{"item", "date", "what"}`. `map` prints `{"regions", "total_items", "total_size", "without_size"}`, each region `{"path", "title", "items", "size", "total_items", "total_size", "children"}`. `activity` prints `{"from", "to", "days", "busiest"}`, each day `{"date", "filed", "sessions"}`, listing only days with something. `carrel status --json` has `resurfaced`: `null`, or `{"collection", "title", "last_seen", "days", "items"}`.

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
| `carrel providers` | Lists the providers on this device (text models, text extraction, embedding models) and which Carrel uses for each. |
| `carrel formats` | Lists the file types this device reads and which provider reads each, then the types it couldn't read so far, most frequent first, with what would read them when Carrel knows. |
| `carrel models` | Lists the models Carrel can download, with size, memory needed and license, and the models already downloaded. Text models come first, then the embedding models for search. |
| `carrel models download NAME [--format mlx] [--yes]` | Downloads a model after showing its size and license and asking; `--yes` skips the question. Every file is checked against the checksum Carrel ships with. `--format mlx` downloads the MLX build, a folder, for the MLX provider. |
| `carrel models remove NAME [--format mlx] [--yes]` | Deletes a downloaded model, or its MLX build, after asking. |
| `carrel models embed TEXT... [--query] [--json]` | Embeds texts with the archive's embedding model, as search will, and shows the start of each vector (all of it with `--json`); `--query` embeds them as search queries. Only the archive's model is used, never another. An archive with no model yet asks which to use first. |

## Settings

| Command | What it does |
| --- | --- |
| `carrel settings` | Lists this device's preferences and the archive's shared settings. |
| `carrel settings set KEY VALUE` | Changes one. `picker`, `editor`, `model`, `second_model` and `librarian.model` are this device's (an empty value, or `default`, clears them); `model` takes `provider/offer`, the offer alone, or a provider such as `apple`, and must be one that's available; `second_model` also takes `off`. `librarian.model` takes a model as `model` does. `ingest.originals`, `compressed_files`, `signals.crowded_items`, `questions.skip_days`, `outside_edits`, `embedding.model`, `librarian.changes` (`on` or `off`) and `librarian.confidence` (0.5 to 1) are shared by every device, and each change can be undone from History. `embedding.model` can only be chosen once here; switch models with `carrel index model NAME`, which re-embeds the archive first. |

## The interactive mode

`carrel` with no command (or `carrel tui`) opens it. The screens are numbered, and `?` shows every key.

| Key | Screen |
| --- | --- |
| `1` | **Status**: what `carrel status` shows, including Recent, what's waiting for you, and the search index, as `carrel index status` shows it |
| `2` | **Review**: `a` approve, `h` hold, `m` move, `e` edit, `x` reject, `z` unpack a zip or keep it compressed, `Space` mark, `A` approve all high-confidence |
| `3` | **Active**: what's checked out and on this device's desk, with their pending history entries; `r` return the chosen checkout, `x` cancel it, `o` open the working copy, `a` approve, `c` combine, `d` drop pending entries (all, or those marked with `Space`); `n` put a file or folder on the desk, and on a desk item `x` take it off, `f` archive it |
| `4` | **Questions**: `Enter` to answer, `1`–`9` to pick a suggestion, `s` to skip, `d` to dismiss |
| `5` | **Browse**: collections and items; `/` to search, as `carrel search` does; `Enter` on an item for its changes and links, then `u` to undo the chosen change, `Enter` to go to a linked item; on an item, `o` check it out, `r` rename it, `z` expand a compressed file; `n` new collection |
| `6` | **History**: `u` to undo the chosen change |
| `7` | **Ingest**: the queue; `x` remove, `t` to the front, `r` retry |
| `8` | **Settings**: `Enter` to change the chosen setting. **Search index** rebuilds it with a progress bar (`carrel index rebuild`), switches the archive's model or carries on or stops a switch (`carrel index model NAME`), and downloads an embedding model after asking. **Providers** and **File types** show `carrel providers` and `carrel formats`; **Archive** switches archives |
| `9` | **Backups**: the snapshots, each with its change; `n` snapshot now, `v` verify, `r` restore an item as of the chosen snapshot, `s` set up or connect a repository, choosing its folder with the file picker |

`i` on any screen chooses files to ingest. `Ctrl-O` on any screen switches to another archive: one opened lately, one by its path, as `carrel open` does (commands outside an archive then use it too), or a new one, as `carrel init` does. `Ctrl-R` reads everything again, and finds the providers again, and `q` quits; the worker keeps going.

## Environment

| Variable | Effect |
| --- | --- |
| `CARREL_CONFIG_DIR` | Where Carrel keeps this device's files, instead of the default (see the [User guide](user-guide.md#what-carrel-keeps-on-this-device)) |
| `CARREL_MODELS_DIR` | Where local models are kept |
| `CARREL_WORKER=off` | Never start a worker in the background |
| `CARREL_SERVICE_MANAGER=off` | `carrel agent install` and `uninstall` write and remove the service's file without running `launchctl`, `systemctl` or `schtasks` |
| `CARREL_TRASH_DIR` | Put files Carrel trashes (such as a zip after it's unpacked) in this folder instead of the system Trash |
| `CARREL_OCR_LANGUAGES` | Tesseract's languages for OCR, such as `eng+deu` |
| `CARREL_IMAGE_DESCRIPTIONS=off` | Read images on a Mac without asking Apple's model to describe them, to measure what the descriptions add |
| `VISUAL`, `EDITOR` | The editor for cards, unless the `editor` setting is set |
| `NO_COLOR` | Draw the interactive mode without color |
| `KOPIA_PASSWORD` | The backup password, for scripts; Kopia then doesn't store it |
