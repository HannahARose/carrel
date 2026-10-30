# Design: restructure signals (v0.2)

Status: draft for the owner. Issue #122.

## What this adds

The design spec says every poor fit, low-confidence placement and Holding item is recorded as a signal for restructuring, so that a restructure (v1.2) is proposed only when the structure has clearly stopped fitting. v0.2 doesn't propose restructures yet, but it should start keeping the evidence now and show the owner where it gathers, so a collection that needs splitting, or a missing collection, is visible early.

## Recording placements

Filing removes an item's review entry (FORMAT.md §7.9), and with it the routing confidence. So the card keeps it:

```yaml
placement:
  confidence: low          # low, medium or high, as routing gave it
  by: model                # model or rule
  at: "2026-10-30T14:02:11-04:00"
```

- **Written when an item is filed** from a proposal whose confidence was `low` or `medium`. High-confidence placements (a routing rule) aren't recorded: they say nothing about the structure.
- **Removed when the owner places the item themselves:** moving it in review (`m`), or moving or renaming it later. An item the owner has put somewhere is no longer a sign the structure doesn't fit.
- **Items in Holding need no record:** being there is the signal.
- It's a new optional card field in FORMAT.md §5.2, preserved by programs that don't know it, as every unknown field is.

## Which signals, and when they show

Signals are computed from the archive as it is, so they're never stale:

| Signal | Shown when |
| --- | --- |
| Items in Holding | at least `min_collection_items` (§6.2, 5 by default) |
| Low-confidence placements in one collection | at least `min_collection_items` |
| A crowded collection: many loose items and no subcollections | more than 30 items (`signals.crowded_items` in settings) |
| A collection with a single item | always, if it has no subcollections |
| A tag used in three or more top-level collections | always |

The thresholds follow literary warrant: a group is worth a collection only when it has enough items, so fewer than `min_collection_items` uncertain items isn't yet a pattern. Medium-confidence placements count as half, since a second routing pass agreed.

The spec's other signals, items in different branches often linked or worked on together and siblings at very different levels of detail, need link and session history to judge well. They arrive with restructure proposals.

## Where the owner sees them

- **`carrel status`** gets a **Structure** section listing each signal shown, most items first, with the collection and a count, such as "Receipts: 7 items placed with low confidence" or "Holding: 12 items". With `--json`, the same as a list.
- **The interactive mode's Status screen** shows the same section.
- **No nagging:** signals are information. Nothing is moved or proposed until restructure proposals arrive (v1.2), which will start from these signals.

## Building it

1. `placement` in FORMAT.md §5.2, the card type and its JSON Schema, written by filing and removed by moves and renames.
2. `carrel_core::signals`: computing the signals and their thresholds, with `signals.crowded_items` in settings.
3. The Structure section in `carrel status` and the Status screen.

## Decisions for the owner

1. **Record low- and medium-confidence placements on the card** (`placement`), removed once the owner places the item. Recommended: it travels with the item, can't go stale, and is readable in any Markdown viewer. The alternative is a separate log in `_System/`, which would need cleaning up as items move.
2. **Compute signals from the archive's current state**, not from a running count. Recommended.
3. **Thresholds from literary warrant:** `min_collection_items` for Holding and low-confidence clusters, 30 for crowded collections (a setting), with medium confidence counting half. Recommended as a starting point.
4. **v0.2 shows signals only**, in `carrel status` and the Status screen; restructure proposals use them in v1.2. Recommended.
5. **Leave the link and level-of-detail signals for v1.2**, when the history they need exists. Recommended.
