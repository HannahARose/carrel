# Design: the librarian and discovery (v0.6)

Status: accepted 2026-10-31, with the five recommendations below and the owner's amendment to decision 1 (the librarian may make undoable changes, as an option). Issue #6, with #195 (better questions during ingest) and #48 (coverage and performance benchmarks); built in #287, #288, #289, #290, #291, #292, #293, #48 and #294.

## What this adds

So far Carrel files what you give it and finds what you ask for. v0.6 makes the archive something you can talk to and wander through. A **librarian** answers questions about what's in your archive, in conversation: what you have on a subject, what a document says, how two things connect. It suggests directions as well as finding things, and along the way it asks you about the items it comes across, so the catalog gets better as you use it. **Discovery views** show the archive itself: what happened on this day in other years, a random walk through the stacks, a timeline, a map of what takes up the space, and when you worked on what. **Questions** get better everywhere, at ingest as much as from the librarian.

**The gate:** the owner spends an hour with the librarian and the discovery views, and finds something they'd forgotten, learns something about the archive they didn't know, and answers questions that improve at least ten cards.

The spec sets the shape. The librarian is "a chat panel that searches cards through the shared embedding index, follows links, opens items, and asks the on-device model about a document's contents", "prompted to be curious and suggest interesting directions as well as find specific documents", which "asks questions from the question queue at natural moments", whose "conversations can be saved to the Journal", and which "runs on the strongest local model available". Discovery is "on this day, a wander-the-stacks random walk, related items beside every item, a timeline, occasional resurfacing of forgotten collections", with "a size map, the collection tree with design documents, a drive view, the activity heatmap, and the tag and link graph". And from the owner (#6): part of the librarian's wandering should be asking the owner to elaborate on items it visits.

## The librarian

- **A conversation over the archive.** You ask in plain words ("what do I have about the flat on Cowley Road?", "what did I work on last spring?", "summarize the lease"), and it answers with the items it found, each a link you can open, and the passages it read.
- **It works through a small, read-only set of tools,** run by the core:
  - `search` (the v0.4 search, words and meaning, with filters);
  - `show` (an item's card);
  - `related` (links and nearest by meaning);
  - `read` (passages of an item's text, as the index stores them);
  - `history` (an item's changes and work sessions);
  - `collection` (a collection's design document and items);
  - `on_this_day` and `timeline` (the discovery data, below).

  The model picks a tool by answering in a fixed JSON form, the core runs it and hands back the result, and so on, up to a limit per answer. This works with every text provider Carrel has, since all of them can answer to a schema, and it needs no tool-calling support from the model. The final answer gets more room than a step naming a tool, and a step whose reply was cut off is asked again with that room. A tool call the model already made isn't run again or counted as a step: it gets the earlier result back, marked as already done. What the tools returned is trimmed to fit the model's context, the oldest results first.
- **By default it changes nothing itself.** When it suggests a change, such as "this receipt looks misfiled" or "these five items could be a collection", the suggestion becomes a proposal or a correction you confirm, through the paths that already exist: review, `docs/design/corrections.md`, and restructuring once v0.7 adds it.
- **Optionally, it may make changes that can be undone.** A setting, off unless you turn it on (`librarian.changes`), lets it make the changes it's sure of without asking first:
  - refiling an item, correcting a card's fields, renaming an item, and creating a collection;
  - each one is an ordinary change in history, marked as the librarian's, that undo reverses;
  - it never touches fields you dictated, never acts on restricted items, and only acts above a confidence threshold you can set.
  - Even with the setting on, it never deletes anything, never edits a file's contents, never sends anything off the device, and never changes settings or rules.
  - Everything it did is listed on Status and in History ("the librarian's changes this week"), where you can undo any one or all of them together.
- **Curious, not just obedient.** Its instructions ask it to notice and offer: a related item you didn't ask about, a collection you haven't opened in a year, a gap ("you have the 2023 and 2025 statements but not 2024"). It answers first and suggests after, never instead.
- **Restricted items stay restricted.** It finds them by their card, as search does, and never reads or quotes their text. Sensitive items are read like any other, since everything stays on your device.
- **The strongest local model on the device:** the largest open model installed that fits in memory (on a 32 GB Mac, a 9B or a 20–30B model), else Apple's model, else Gemma or Qwen at 4B. You can choose one in Settings. A small model keeps its answers short and leans on search; a large one reads and connects more. Each answer says which model gave it.
- **Conversations are kept only if you save them.** A saved one goes to the Journal as Markdown, with the question, the answer and links to the items, so it's readable without Carrel. Unsaved ones are gone when you close them, and nothing about them is written into the archive.

## Questions

Today ingest asks two kinds of question, what a term means and which collection a term's items go in, and the owner flagged at the v0.2 gate that they could be better (#195). v0.6 gives questions one queue, more kinds, and a sense of which matter.

- **New kinds:**
  - **About an item** ("what does this photo show?", "who is J. Okafor?", "what was this project for?", "does this still matter?"), from the librarian or from browsing. The answer goes through the corrections path: the model turns it into card changes you see and confirm, and fields you dictate stay yours.
  - **About a group** ("these 12 receipts from the same restaurant: Finances, or a new Food collection?").
  - **About a sender or project** ("should Northbridge Bank have its own collection?"), which can lead to a routing rule or, in v0.7, a restructure proposal.
- **Ranked by what an answer settles:** how many items it would place or improve, and how unsure Carrel is about them now. Questions a rule or the glossary already answers are dropped, and near-duplicates merge.
- **Easier to answer:** each question shows the items it's about and offers likely answers to pick from, with a free answer and Skip always there. Skipping is cheap and remembered, so a skipped question doesn't come back for a while.
- **Learned only on your yes:** an answer can become a glossary fact or a routing rule, and Carrel asks before saving either (the corrections path's "learn this?").
- **Asked at natural moments,** never as pop-ups: in Review and Questions as today, beside an item while you browse it, by the librarian in conversation when the item comes up, and in a **wander** session that visits items and asks about some of them. Outside those, at most a few new questions a day appear on Status.

## Discovery

The core computes each view as plain data, and each interface draws it in its own way (the rule that every interface is fully capable, `docs/dev/interfaces.md`).

- **On this day:** what was filed, created or worked on on this date in other years.
- **Wander the stacks:** a random walk from an item to a related one, a link, a neighbour in its collection, or a jump to somewhere you haven't been in a while. It shows one item at a time, with its preview, card and why it came next, and now and then a question about it.
- **Timeline:** items by their date (created, filed or worked on), by year, month and day, filterable like search.
- **The archive map:** collections by size and number of items, as a treemap in the app and a sized tree in the terminal; and which drive holds what, once offline collections exist.
- **Activity:** when you filed and worked on things, as a heatmap by day.
- **Resurfacing:** now and then, Status mentions a collection you haven't opened in a long time, with an item or two from it.
- **The tag and link graph** comes later, with v0.7's restructuring, which needs the same data.

## In each interface

| | Command line | Interactive mode | Mac app |
| --- | --- | --- | --- |
| The librarian | `carrel ask "…"` for one answer; `carrel ask` alone for a conversation; `--save` to the Journal; `--json` | A Librarian screen: the conversation on the left, the item it's discussing on the right | A Librarian panel beside the window, and "Ask about this" on any item |
| With no model to run on | Why, then `carrel models download NAME` for the model that suits this device's memory (it asks first), or `carrel settings set librarian.model default` when the setting names a model that's gone | Why, and the download confirmation Settings uses, for the model that suits this device | Why, with Download… (its size, confirmed first) or Use, for the model the librarian would answer with |
| Questions | `carrel questions`, with the new kinds and suggested answers | Questions, and a question beside an item in Browse | Questions, and a question in the inspector |
| On this day, timeline | `carrel discover today`, `carrel discover timeline` | A Discover screen | A Discover sidebar section |
| Wander | `carrel discover wander`, one item at a time | Wander in Discover, one key per step | Wander in Discover, with previews |
| Map, activity | `carrel discover map`, `carrel discover activity`: tables, and `--json` | A sized tree and a block heatmap | A treemap and a heatmap |

## Measuring it

- **Librarian questions in the benchmark:** for the test set, questions with known answers (which items, which facts), written by hand, scored by whether the librarian finds the right items and says the right thing, for each model. This picks the default model per memory size, as the filing benchmark did.
- **Question value:** for the test set, which questions, once answered with the known answer, improve routing the most. That checks the ranking.
- **Coverage and performance** (#48): coverage on pull requests, and benchmarks for card parsing, manifest diffing and indexing at large scale, tracked over time.

## Building it

1. The question queue's new kinds, ranking, suggested answers and skipping in the core, with `carrel questions` and the interactive mode updated (#195's ideas).
2. The librarian's loop and toolset in the core, with `carrel ask`, and the optional undoable changes (`librarian.changes`) with their record on Status and in History.
3. The discovery data in the core, with `carrel discover`.
4. The interactive mode's Librarian and Discover screens.
5. The Mac app's Librarian panel, "Ask about this", and Discover.
6. Questions while browsing and wandering, in every interface.
7. The librarian and question-value benchmarks, then the default models.
8. Coverage and performance benchmarks (#48).
9. The user guide, then the gate checklist.

## Decisions (accepted 2026-10-31)

1. **The librarian is a conversation run by the core over a fixed set of tools, chosen by the model in a JSON form. By default it changes nothing itself:** what it suggests becomes a proposal or correction you confirm. **As an option, off unless turned on, it may make the changes it's sure of directly** (refiling, correcting fields, renaming, creating collections), each one undoable and marked as the librarian's, and never deletions, file edits, settings or anything leaving the device. Accepted with the owner's amendment to allow undoable changes as an option. It works with every model Carrel has, and every change, asked for or not, goes through the same checked, undoable paths.
2. **It runs on the strongest local model the device has, chosen automatically and changeable in Settings, with each answer saying which model gave it.** Recommended: the spec asks for the strongest model, and naming it makes the quality of an answer understandable. The benchmark sets the defaults per memory size.
3. **Questions get one queue with new kinds (about an item, a group, a sender or project), ranked by what an answer would settle, with suggested answers, cheap skipping, and learning only on your yes. They're asked at natural moments: review, browsing, the librarian, and wander sessions, plus a few a day on Status.** Recommended: it's what #195 and your idea in #6 ask for, and it keeps questions from becoming nagging.
4. **Conversations are kept only when you save them, to the Journal as Markdown.** Recommended: nothing you say to the librarian is written down unless you choose, and what you keep is readable without Carrel. The alternative is keeping every conversation, which helps the librarian remember but writes more of your words into the archive.
5. **Discovery (on this day, wander, timeline, map, activity, resurfacing) is computed in the core and shown in every interface in its own way, with the tag and link graph left for v0.7.** Recommended: one implementation, a fully capable command line and interactive mode, and the graph built once with the restructuring that needs it.
