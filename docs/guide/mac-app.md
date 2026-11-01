# The Mac app

Carrel for Mac is a native app over the same archive, and the same core, as the `carrel` command and its interactive mode. It has the interactive mode's screens in a window's sidebar, adds items by dragging, shows previews of what you've filed, and comes with a background agent that keeps working while the app is closed. If you haven't used Carrel yet, [Getting started](getting-started.md) explains the ideas; the [User guide](user-guide.md) explains each part in depth, and everything there applies to the app too.

## Getting the app

The app needs **macOS 26 or later on a Mac with Apple silicon**. Apple's on-device model and the look of the app need macOS 26, and Carrel's core is built for Apple silicon only.

Until v1.0 there are two ways to get it. **Signed and notarized builds**, which open like any other app, come with v1.0.

### A development build from CI

Every push to `dev` builds the app. To download the latest:

1. Open the repository's [Actions](https://github.com/Horizon-Labs/carrel/actions/workflows/ci.yml?query=branch%3Adev), and choose the newest successful run on `dev`.
2. Under **Artifacts**, download **Carrel**. It's a zip holding `Carrel.app`, kept for 30 days. You need to be signed in to GitHub to download it.
3. Unzip it and move `Carrel.app` to Applications.

The build is signed **ad hoc**, not with a developer's certificate, and isn't notarized, so macOS won't open it the first time. The usual way past that, right-click (or Control-click) `Carrel.app` and choose **Open**, no longer works on its own in macOS 26. Instead:

1. Double-click `Carrel.app`. macOS says it can't check it; choose **Done**, not Move to Trash.
2. Open System Settings → Privacy & Security, scroll to Security, and choose **Open Anyway** beside the message about Carrel.
3. Confirm with **Open Anyway** and your password.

You do this once for each build you download; after that it opens normally, and so does its agent.

### Building it with Xcode

You need Xcode 26, Rust and a clone of the repository, as in [Getting started](getting-started.md#1-install). From the repository's folder, build Carrel's core for the app (again after each update):

```bash
PROFILE=dev scripts/build-xcframework.sh
```

Then open `apple/CarrelApp/CarrelApp.xcodeproj` in Xcode and run the **Carrel** scheme (⌘R). It's signed to run on your own Mac; to sign with your own team instead, choose it under Signing & Capabilities. [The app's README](../../apple/CarrelApp/README.md) has the details.

### Models and Kopia

The app uses the same models, providers and Kopia as the command line, and the same models folder, so a model downloaded from either is there for both. The development build doesn't carry the provider programs inside it yet. **Settings → Providers** shows which ones the app found; if it lists fewer than `carrel providers` does in your terminal, that's why.

## Opening and creating archives

With no archive open, the app shows its **Welcome** window: **New Archive…**, **Open Archive…**, and the archives you've opened lately. The same commands are in the File menu:

| Command | Key | What it does |
| --- | --- | --- |
| New Archive… | ⇧⌘N | Makes an archive in a new or empty folder you choose, named after the folder |
| Open Archive… | ⇧⌘O | Opens the archive in, or containing, the folder you choose |
| Open Recent | | The archives opened lately; Clear Menu forgets them |

Each archive opens in **its own window**, and you can have several open at once. An archive written by a newer version of Carrel opens **read only**: you can look, but not change anything, and the title bar says so.

Collections are made in the app too (see [Browse](#browse)), but their design documents are Markdown files you write in your own editor, as before.

## The window

The window has three columns:

- **The sidebar:** the screens, in the interactive mode's order, with counts beside Review, Questions and Active: **Status**, **Review**, **Questions**, **Active**, then your **Collections** (Browse), then **Discover**'s views, then **History** and **Backups**.
- **The list** for the chosen screen.
- **The inspector:** the chosen thing in full, such as a proposal's card, an item's preview and card, or what a change did.

Beside them, the **Librarian** panel opens on the right when you want it (see [The librarian](#the-librarian)).

Every action is in the menus, so you can find it there and see its key. In lists, single letters work too, as in the interactive mode, where they can't be mistaken for typing. Right-click anything in a list for its actions.

### Status

What the archive holds: its name and folder, whether it's read only, its format version, how many items, collections and items in Holding it has, and items by kind.

**Intake** shows the worker drafting new items, with its progress, how many are waiting, and how many failed (hover over the count to see why). When items are waiting and nothing is drafting them, because the agent is off or paused, **Draft Now** drafts them in the app.

**Not Opened in a While** appears now and then: a collection you haven't opened in six months, with an item or two from it, as `carrel status` names it ([Discovering the archive](user-guide.md#discovering-the-archive)). Choose the collection or an item to open it in Browse, which counts as opening it.

**The Librarian's Changes This Week** lists what the librarian changed on its own, when you've let it ([What the librarian may change](user-guide.md#what-the-librarian-may-change)), with **Undo All of Them…**.

Status doesn't show Recent, structure signals or suggestions yet; `carrel status` and the interactive mode do.

### Review

Proposals waiting for your decision, grouped by the collection each would go to. Choose one to see its card, destination, confidence and why it's as sensitive as it is in the inspector, where you can also edit its fields before filing it. Choose several (⇧-click or ⌘-click) to decide them together.

| Key | Review menu | What it does |
| --- | --- | --- |
| `a` | Approve | Files the chosen proposals where proposed |
| | Approve Batch | Files every proposal from the chosen one's batch together |
| | Approve All High Confidence | Files every proposal Carrel is sure of |
| `h` | Hold | Files them in Holding, for later |
| `m` | Move To… | Points them at another collection, chosen from a list you can filter by typing |
| `r` | Reject | Leaves their files where they are and drops the proposals |
| `z` | Expand When Filed, Keep Compressed | For a zip or other compressed file, whether it's expanded on approval |
| `v` | File as Version | For a duplicate, files it as a new version of the item it duplicates |
| `b` | Keep Both | For a duplicate, files it beside the item it duplicates |

A duplicate shows the archived item beside it in the inspector, so you can compare them. A bar at the bottom of the list says what the last decision did. Approving and holding file items, which is a change in [History](#history), so it can be undone there. Rejecting changes nothing in the archive, so there's nothing to undo; add the file again instead.

### Questions

Carrel asks here instead of guessing, most useful first: what a term means, where items go (alone or as a group), whether a sender or project should have its own collection, and things about a single item. Choose a question to see what it's about and the items it holds up, then:

- **Pick a suggested answer:** the likely answers come first, with why each is likely; a click answers with it. One marked "a new collection" creates it.
- **Answer it yourself:** type what a term means, choose where items go, or say **Yes** or **No** to a sender's collection. After a sender's answer, Carrel asks before adding a routing rule for future items. An item question's answer opens the correction sheet: you see what it changes on the card, and it's saved, and the question answered, only when you confirm (**Mark Answered** when it changes nothing).
- **Skip** (`s`): put it aside for a while (`questions.skip_days`); **Show Skipped** in the toolbar lists those resting.
- **Dismiss** (`d`): drop the question.

All of these are in the Questions menu ([Questions and the glossary](user-guide.md#questions-and-the-glossary)).

Carrel also asks about items as you go, never in a pop-up ([Questions as you go](user-guide.md#questions-as-you-go)): under **Carrel Asks** below an item's card in the inspector, now and then on a wander step in Discover, after the Librarian's answer ("While we're here…"), and a few a day under **Today's Questions** on Status. Each is answered there, the same way.

### Active

What you're working on: items **checked out** to a working copy, and folders and files **on the desk**, where they are. Each shows when it was checked out, when it's due back and whether it has changed. Under each are its **pending entries**: work sessions Carrel noticed, waiting for you to approve them into the item's Journal. See [Working on items](user-guide.md#working-on-items) for how these work.

| Key | Active menu | What it does |
| --- | --- | --- |
| | Check Out… (⌥⌘C) | Checks out the item chosen in Browse: choose where the working copy goes and when it's due back. The archive copy is read only until you return it |
| | Add to Desk… (⌥⌘D) | Puts a folder or file on the desk, to work on where it is |
| `r` | Return… (⌥⌘R) | Shows what comes back, and the entry it adds, then returns it |
| `x` | Cancel Checkout… (⌥⌘⌫) | Asks first, then unlocks the archive copy as it was. Changes in the working copy aren't brought back, and the working copy stays where it is |
| `o` | Show Working Copy in Finder (⌥⌘O) | |
| `a` | Approve Entries (⌥⌘A) | Adds the chosen entries to the Journal |
| `c` | Combine Entries (⌥⌘J) | Joins the chosen entries into one |
| `e` | Edit Entry… (⌥⌘E) | Changes an entry's words; it stays pending until you approve it |
| `d` | Drop Entries (⌥⇧⌘⌫) | Discards the chosen entries |

A desk item can be taken off the desk from its context menu.

### Browse

Choose a collection in the sidebar to see its items. The toolbar switches between a **list** and a **grid** of thumbnails, and the app remembers which you chose. In the grid, the arrow keys move between items.

Choose an item to see it in the inspector: a **preview** above its **card**. The preview is what Quick Look shows for PDFs, images, documents, video and 3D models such as USDZ and STL; Carrel's own drawing for STEP, IGES and 3MF files; and the contents of a folder or bundle. Until a thumbnail is ready, an item shows its kind's icon. A **restricted** item shows only its card, with **Show Preview** to see it once. Thumbnails are kept on this Mac only, never in the archive (see [What Carrel keeps on this device](user-guide.md#what-carrel-keeps-on-this-device)).

The card shows the item's fields, your notes, its history and the items it links to. **Show in Finder** reveals the item.

| Command | Key | What it does |
| --- | --- | --- |
| Edit Card | ⌘E | Edits the chosen item's card in the inspector. Saving (⌘S) runs the same checks `carrel edit` does, and shows problems beside the field |
| Correct… | ⌘K | Opens a sheet: say what's wrong in your own words, such as a title, a date or where it belongs. The model works out the changes, you see them, then confirm or change your words |
| Check Out… | ⌥⌘C | As on [Active](#active) |
| New Collection… | ⌥⌘N | Asks for its path, what belongs there and its tags; it's made inside the collection you're in |
| Rename… | ⇧⌘R | Renames the item, starting from a name that works on every system |
| Expand… | ⇧⌘E | For a filed zip or other compressed file, asks first, then expands it in place |

Edit Card and Correct… are in the Edit menu; New Collection, Rename and Expand in the File menu; Check Out in the Active menu. They're also in each item's context menu, with **Ask About This** (⇧⌘L), which asks [the librarian](#the-librarian). ⌘K works on a proposal in Review too.

### Search

**⌘F** (Edit → Search Archive) puts the cursor in the search field in the toolbar. It's the same search as `carrel search`, filters included, such as `tag:receipts`, `after:2025` or `collection:Finances`: see [Searching](user-guide.md#searching). Results replace the list, best first, each with its collection, a mark if it's sensitive or restricted, and the matching passage with your words highlighted. Choose one to see it in the inspector; a matching collection has **Show** to open it.

Results by words appear as you type. Search by meaning runs when you pause, with a small spinner while it does, so typing never waits on the model. If search by meaning can't run, say because the index is behind, a note below the results says why. Changing screens clears the search.

### History

Every change to the archive, newest first. Choose one to see what it did: the operation, the device, the items it touched, and the backup snapshot taken with it.

A change the librarian made on its own says **by the librarian**, and its details name the librarian as who made it.

**Undo Change…** (`u`, or ⌥⌘Z in the History menu) asks first, saying what undoing restores and what it leaves alone, then undoes it. Files the change moved go back. Undoing is itself a change, so it can be undone too. (Only the change that created the archive's history can't be.)

### The librarian

The **Librarian** panel answers questions about your archive in your own words, as `carrel ask` does ([Asking the librarian](user-guide.md#asking-the-librarian)). Open it with the **Librarian** button in the toolbar, or **View → Show Librarian** (⌥⌘L), and it stays beside the window as you move between screens.

- **Ask:** type a question and press Return (or ⌘Return). While it works, the panel shows its steps ("Step 2 of up to 8 · searched…"). Each question can build on the ones before.
- **Each answer** lists the **Items** it's about, the **Passages** it read, quoted from the items, and its **Suggestions**. Choose an item or a passage's title to show the item in Browse; hover over an item for its card's path. Under the answer is the model that gave it and how many steps it took, and hovering there lists each step.
- **Ask About This** (⇧⌘L, in the View menu, an item's context menu, and on a wander step) opens the panel with a question about the chosen item, for you to finish and ask.
- **Save to Journal** in the panel's toolbar writes the conversation to this month's Journal, as one change in History; the panel says where. **New Conversation** starts again. A conversation you don't save is gone when you start a new one or close the window, and nothing about it is written into the archive.
- **Which model:** the line under the question field names the model that answers, and whether the librarian only suggests changes or may make them. Choose a model in **Settings → Models → Librarian**.

**What it changes.** By default it changes nothing: each suggestion has **Review This Change…**, which opens the sheet that makes it, worked out already. A refile or a correction is shown before and after in the correction sheet, a rename in the rename sheet and a new collection in the new collection sheet, and nothing is saved until you confirm it there. It's your own change.

With `librarian.changes` on in **Settings → General**, it makes the changes it's at least as sure of as `librarian.confidence` itself. Each says **Done by the librarian**, with **Undo**. The panel lists **The Librarian's Changes This Week** below the conversation, each with **Undo** and all of them with **Undo All…**, as Status does; History marks them **by the librarian**. Undoing is a new change, newest first, and any that can't be undone, because you've changed the same card since, are left and listed. The librarian never changes a field you wrote yourself, never acts on restricted items, and never deletes anything, edits a file, changes settings or sends anything off the Mac.

A **restricted** item is found by its card only: the librarian can say it exists, but never reads or quotes its text.

### Discover

The **Discover** section of the sidebar has the discovery views ([Discovering the archive](user-guide.md#discovering-the-archive)). Choose an item in any of them to see its preview and card in the inspector; its context menu has **Show in Browse** and **Ask About This**.

- **On This Day:** what was made, filed or worked on on today's date in other years, by year. The date picker in the toolbar looks back from another day.
- **Wander:** a walk through the stacks, one item at a time: its title, why it came next, its preview, its card's summary, the start of its text and its tags, with **Show in Browse** and **Ask About This**. **Next** (→) takes the next step, and **Start Again** starts a new walk. The items visited so far are along the bottom, to go back to. A restricted item shows its card only.
- **Timeline:** items by date, newest period first, each period with a bar for how many it has. Type words or search's filters, such as `tag:receipts after:2024`, and press Return; **By** places items by when they were made, filed or worked on, and **Per** groups them by year, month or day. Below, it says how many items have no such date.
- **Archive Map:** collections as a treemap, each sized by how much space it takes or how many items it holds (**Measure** in the toolbar). Hover for a collection's items and size. Click one to zoom into it (or, with nothing inside, to show it in Browse), and double-click to show it in Browse; the path above the map takes you back out.
- **Activity:** when you filed and worked on things over the last year, as a heatmap, a column a week, darker for busier days. Hover over a day for what happened on it.

Which collections you opened and your walk are kept on this Mac, never in the archive, so looking around never changes it.

### Backups

What `carrel backup` does (see [Backups](user-guide.md#backups)), using the Kopia you installed:

- **Set Up Backups…:** choose a folder or drive for the backups. An empty folder makes new backups, and asks for a password twice; an existing repository connects to it, and asks for its password. Kopia keeps the password in this Mac's keychain. Keep it in a password manager too: without it, the backups can't be read.
- **The snapshots**, newest first: one is taken with each approved batch. They're read from history, so they show even when the drive isn't connected.
- **Snapshot Now** takes one.
- **Verify** checks every snapshot, reading back 1% of the files.
- **Restore an Item…:** choose a snapshot, then an item. Its contents as they were then go into a new folder beside the archive. Nothing in the archive changes.

### Settings

**Carrel → Settings…** (⌘,) shows this Mac's settings and those of the archive whose window was last in front:

- **General:** this Mac's settings, and the archive's own, shared by every device that opens it (see [Settings](user-guide.md#settings)), including `librarian.changes`, which lets the librarian make the changes it's sure of, and `librarian.confidence`, how sure it must be. Changing an archive setting is a change in history; **Undo** beside it takes it back.
- **Models:** which model drafts cards, which gives a second opinion and which the **Librarian** answers with (the strongest that fits this Mac, unless you choose one), the text and embedding models with their sizes, and **Download…** and **Delete…**, which ask first and show the size. Nothing downloads without your OK.
- **Search:** the archive's index for search by meaning: rebuilding it, and switching to another embedding model, which asks first and can be stopped and carried on.
- **Providers:** the providers this Mac found, the file types it reads, and those it couldn't read so far.
- **Backups:** whether Kopia is installed and backups are set up for this archive. Set them up from the [Backups](#backups) screen.
- **Agent:** the background agent's switch (below).

## Adding items

Drag files or folders onto an archive's window, or choose **File → Add to Archive…** (⌘O). They join the intake queue, just as `carrel ingest` adds them. Nothing moves yet.

- A **folder** goes in as one item. For a pile such as Downloads, where each file should be its own item, choose the files inside it, or use `carrel ingest --each`.
- An alert says what was added, and what was left out and why: duplicates of items already in the archive, items already waiting, files your rules ignore.
- **Drafts arrive by themselves.** The agent, or the command line's worker, drafts each item in the background; Status → Intake shows the progress. Each proposal appears in Review as soon as it's ready, with the count beside Review going up. If nothing is drafting, Status offers **Draft Now**.

## The background agent

**Carrel Agent** is a small helper inside the app that keeps working while the app is closed. It's what `carrel agent` is for the command line, as a Mac login item:

- it drafts new items as soon as they're added, from the app, the command line or the interactive mode;
- it notices work sessions on checked-out and desk items as they finish;
- it keeps search by meaning current after every change;
- it takes a snapshot every day once backups are set up.

It looks after the archives the app has open or opened lately.

### Turning it on and off

Turn it on in **Settings → Agent** with **Run the Carrel agent at login**. It starts at once, and at every login after. macOS may ask you to allow it: Settings then says so, with a button to open System Settings → General → Login Items, where you switch it on. Turn the switch off to stop it and keep it from starting at login.

With the agent off, the app drafts new items while it's open (Draft Now on Status), as the command line does, and work sessions are noticed when the app or the command line runs.

### The menu bar list

While it runs, the agent has a menu in the menu bar. For each archive it lists:

- what's **checked out** and **on the desk**, with how many pending entries each has;
- how many **entries** wait to be approved;
- how many items are **waiting to be drafted**.

Choosing an item opens it on Active in the app. Below the archives is what the agent is doing now, then:

- **Pause:** stops the agent's work without quitting it; **Resume** starts it again. While it's paused, the command line's worker drafts and indexes again.
- **Open Carrel:** opens the app.
- **Quit Carrel Agent:** quits it until you next log in. To stop it for good, use the switch in Settings.

### The agent and `carrel agent`

The command line has its own agent, [`carrel agent`](user-guide.md#keeping-work-noticed-without-the-app), for macOS, Linux and Windows. **One agent looks after an archive on each device:** while the app's agent runs, `carrel agent` steps aside and says so, and takes over within a minute after the app's agent stops or is paused. You don't need both on a Mac; if you use the app, its agent is the one to turn on. `carrel agent status` says which agent looks after each archive now.

While either agent runs, `carrel ingest` and the interactive mode start no worker of their own: the agent drafts what they add.

## Working alongside the command line and the interactive mode

The app, `carrel` and its interactive mode are three ways into the **same archive**, through the same core. Use whichever suits the moment; you can have all three open on one archive at once.

- **They agree.** Each screen in the app does what its interactive-mode counterpart does, through the same code, so a decision, a search or an undo gives the same result wherever you make it. A change made in one shows in the others once they read the archive again.
- **One writer at a time.** Only one program changes the archive at any moment: it holds the [writer lock](user-guide.md#writing-safely) while it writes, usually for a moment. If the app can't take it, because the command line is filing a batch say, it tells you who holds it; try again when that's done.
- **One history.** Every change, from any of them, is in History, with the device it was made on, and can be undone from any of them.
- **This Mac's settings are shared.** The app reads and writes the same configuration folder as `carrel`, so the models, backups and this device's preferences are the same in both. The app keeps its own list of recent archives.

Some things are only in one interface for now, and some always will be, such as `carrel bench`; [Interfaces](../dev/interfaces.md) lists where each capability stands.
