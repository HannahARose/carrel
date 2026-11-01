# Design: restructure proposals (v0.7)

Status: accepted 2026-11-01, with the five recommendations below and the owner's amendment to decision 2 (a setting for how eager Carrel is to rearrange). Issue #34, with #154 (crawlers); built in #322, #323, #324, #325, #154, #326, #327 and #328.

## What this adds

Filing puts each item where it fits best today, but an archive's shape drifts: a collection fills up with forty loose receipts, a project grows a dozen notes filed in three places, Holding collects what didn't fit anywhere, and a tag turns up in five unrelated collections because there's a group the structure doesn't have. v0.7 lets Carrel notice that and propose a better shape: a **restructure proposal** for one section of the archive, as a list of changes you review, approved all, in part or not at all, and undone as one change. **Crawlers** keep reassessing placements in the background with the strongest model the device has, item by item, and feed what they find into both.

**The gate:** the first whole-archive restructure is reviewed and approved (#34), its findability test shows the new shape is easier to navigate, and undoing it puts everything back.

The spec sets the shape. Restructuring "proposes a reorganization of one section of the archive as a reviewable change list, and is proposed only when its benefit clearly beats its cost." Whole items can move, collections can be renamed, merged and split, open folders reorganized inside, and design documents and tags rewritten, but "sealed bundles are never entered." Grouping is "done numerically, from on-device embeddings and tag co-occurrence"; "the model names groups, drafts design-document changes, writes the rationale and runs the findability test." An approved proposal "runs as one change with one snapshot and one history entry, and is undone as a unit." And regardless of settings, "Carrel never deletes, purges, restructures or sends anything off the owner's devices without asking." The signals it starts from have been recorded since v0.2 (`docs/design/signals.md`).

## A restructure proposal

- **One section at a time:** a collection and everything under it, or the whole archive for the first one. A proposal is a file in `_System/proposals/`, readable without Carrel, as FORMAT.md already reserves.
- **What it can change:**
  - move items between collections;
  - rename, merge and split collections, and create new ones;
  - reorganize the inside of open folders;
  - rewrite the affected design documents (`_collection.md`) and tags.

  It never enters sealed bundles, never touches a checked-out item until it's returned, never deletes anything, and leaves fields you dictated alone.
- **Each change says why,** and the proposal as a whole has a rationale, its cost, its benefit and its findability result, so you can judge it without reading every line.
- **Approve all, part or none.** A partial approval drops the changes that depend on the ones you rejected (a split without its new collection, say) and says so before running.
- **One change in history,** with one snapshot first, so undoing it puts every item, folder, design document and link back.

## When one is proposed

- **From the signals** v0.2 records (Holding, low-confidence placements, crowded and single-item collections, tags spread across collections), plus the two v0.2 left for now: items in different branches that are often linked or worked on together, and sibling collections at very different levels of detail.
- **Only when the benefit clearly beats the cost:**
  - **Costs:** moves to check (and possibly undo); data moved between drives; links rewritten; a section restructured recently (a stability penalty, so the archive doesn't churn).
  - **Benefits:** similar items closer together (measured with the embedding index); Holding and low-confidence placements resolved; collections of a sensible size; and above all, a findability gain.
  - **How eager Carrel is** is yours to set (below).
- **The findability test:** a sample of cards is held out, and a model tries to find each one by walking the proposed tree from its summary alone, then the current tree. The proposal goes forward only if the new tree is found more often. It's the same idea as v0.4's findability benchmark, used on your own archive.
- **Asked for, or offered:** `restructure propose` builds one on demand for any section. Otherwise, when the signals for a section cross the bar, Status says a restructure is worth looking at, and builds it when you ask. It's never built or applied unprompted.

## How eager to rearrange

People feel differently about an archive that moves. Some want Carrel to keep refining the organization even when that means moving a lot; others have learned where things are and would rather it stayed put. One setting, `restructure.eagerness`, says which, for the whole archive or per collection (in its `_collection.md`, so a collection you know by heart can stay still while a messy one gets tidied):

| | Settled | Balanced (default) | Eager |
| --- | --- | --- | --- |
| How much better a proposal must be | a large, clear gain | a clear gain | any real gain |
| The stability penalty on recently changed sections | heavy, for a year | moderate, for a few months | light |
| Moves a proposal may make | few; prefers renaming and design-document edits | as many as the gain justifies | many, including splitting and merging freely |
| How often Status offers a restructure | rarely, only for strong signals | when signals cross the bar | whenever a better shape is found |
| Crawler pace | Holding and low-confidence items only | the full order, slowly | the full order, steadily |

- **It changes how readily Carrel proposes, never whether it asks.** Every restructure still waits for your approval, at every setting.
- **It doesn't override the findability test:** even Eager won't propose a shape that's harder to navigate.
- **The numbers behind it** (the margin, the penalty, the move budget) are settings too, for anyone who wants to tune them directly; the three levels are presets.

## How a proposal is built

- **Grouping is numerical, in the core:** clustering the section's items by their card embeddings (the v0.4 index) and their tags' co-occurrence, with literary warrant: a group needs at least `min_collection_items` to become a collection.
- **The model writes the words:** it names each group, drafts the design documents, writes each change's reason and the rationale, and checks the plan against the existing design documents. Every model output is validated, as filing's is, so a dropped or invented item is caught.
- **The strongest model the device has** does this: a 20–30B model on a 32 GB Mac, and on a smaller device a draft that says so, which a stronger device can redo (the spec's quality ladder).
- **Tags restructure the same way, more cheaply:** merging synonyms, splitting overloaded tags and retiring rare ones, with no files moved. These can be proposed on their own (`restructure tags`).

## Crawlers

- **A background pass** in the worker when it's idle, and in the agent from v0.5, looks at items one at a time:
  1. Holding first;
  2. then low-confidence placements;
  3. then crowded collections;
  4. then a slow sweep of everything.
- **The strongest model judges whether each still fits** its collection's design document, and where it would go instead.
- **What it finds:**
  - a misplaced item becomes a move proposal in Review;
  - its verdicts feed the signals, so a collection whose items keep failing makes a restructure worth proposing.
- **Optional, undoable moves:** a setting like the librarian's (`crawler.changes`, off by default) lets it refile the items it's sure of itself, each an ordinary change marked as the crawler's, listed on Status and in History, and undone one at a time or together. Whole-section restructures always wait for you, whatever the setting, as the spec's autonomy limit requires.
- **Quiet and slow by design:** it runs only when the device is idle and on power, a few items at a time, and stops for anything you're doing.

## The tag and link graph

v0.6 left the tag and link graph for now, since it needs the same data as restructuring. v0.7 builds it once, in the core: items and collections as nodes, tags, links and shared work sessions as edges. Restructuring uses it to find groups across branches; discovery shows it as a view (`discover graph`) where you can see which collections connect and why.

## In each interface

| | Command line | Interactive mode | Mac app |
| --- | --- | --- | --- |
| Proposals | `carrel restructure propose [COLLECTION]`, `list`, `show`, `approve [--only …]`, `reject`; `carrel restructure tags`; `--json` | A Restructure section in Review: the change list beside a before-and-after tree, approving all or part | A proposal view: before and after trees side by side, each change with its reason, approve all or part |
| Crawler | `carrel crawl status`, its findings as proposals in `carrel review`; `crawler.changes` in settings | Its findings in Review; its changes on Status and in History | The same, and its activity in the menu bar list |
| Signals | The Structure section of `carrel status` | Status | Status |
| Eagerness | `carrel settings set restructure.eagerness settled\|balanced\|eager`, and per collection in its design document | Settings, and a collection's view in Browse | Settings, and a collection's inspector |
| Graph | `carrel discover graph`, as a list of connections and `--json` | A text view of the strongest connections | A drawn graph in Discover |

## Measuring it

- **A restructure benchmark** on the test set: a deliberately muddled copy of its archive (items scattered, a collection overfilled, a missing collection), scored by whether the proposal recovers the intended shape, how many moves it takes, and the findability gain.
- **Crawler accuracy:** on the same muddled archive, how many misplaced items each model finds and where it would put them, against the known answers, which sets the crawler's default model per memory size.

## Building it

1. The tag and link graph, and the two remaining signals, in the core.
2. Grouping and the findability test in the core.
3. Proposals: building, the file format (FORMAT.md), partial approval, applying as one change with one snapshot, and undo; `carrel restructure`.
4. Tag restructuring.
5. `restructure.eagerness`, archive-wide and per collection, and its presets.
6. Crawlers in the worker and the agent, with the optional undoable moves; `carrel crawl`.
7. The interactive mode's Restructure section and graph view.
8. The Mac app's proposal view and graph.
9. The restructure and crawler benchmarks.
10. The user guide, then the gate checklist.

## Decisions (accepted 2026-11-01)

1. **A restructure proposal covers one section, is a reviewable list of changes approved all, in part or not at all, never enters sealed bundles or touches checked-out items, and runs as one change with one snapshot, undone as a unit.** Recommended: it's the spec's design, and it keeps even large reorganizations safe to try.
2. **Proposals are offered only when the benefit beats the cost by the margin your eagerness setting asks for (Settled, Balanced or Eager, archive-wide or per collection), judged by a findability test on your own archive, and never built or applied without your asking.** Accepted with the owner's amendment adding the eagerness setting: some people want the organization refined even if it means moving a lot, others want a familiar archive left alone. At every level the findability test still applies and every restructure still asks.
3. **Grouping is numerical, from the embedding index and tag co-occurrence, and the strongest model on the device names groups and writes the design documents and reasons, validated like filing's output.** Recommended: numbers decide what belongs together, so the result doesn't hinge on one model's whim, and the model does what it's good at.
4. **Crawlers reassess placements in the background, item by item, as move proposals in Review, with an option (off by default) to make the moves they're sure of themselves, each undoable. Whole-section restructures always wait for you.** Recommended: it follows your amendment to the librarian (autonomy only where it can be undone) and the spec's limit on restructuring.
5. **The tag and link graph is built once, in the core, for restructuring and as a discovery view, and everything is in every interface.** Recommended: one implementation serves both, and it completes what v0.6 left for now.
