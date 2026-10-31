# Design: typed corrections

Status: accepted 2026-10-30, with the five recommendations below, broad corrections included. Issue #187.

## What this adds

When a card is wrong, the owner can open it in an editor today (`e` in Review, `carrel review edit`), and `carrel edit ITEM` for filed cards is planned (#162). An editor means finding the field, knowing its format, and getting YAML right. Most corrections are quicker to say: "it's from 2019, not 2025", "this is my landlord's letter, not the bank's", "it's not sensitive". Some are broad: "this is a picture of Lake Annecy, not Geneva" changes what the item is, so the title, summary, tags and perhaps the collection all follow from it. This design lets the owner **type a correction in plain words**, both while approving an ingest and later on a filed item, and see exactly what it changes before it's saved.

It's the last piece the owner wants before the v0.2 gate.

## Where

- **In Review:** `c` in the interactive mode, on the proposal shown, and `carrel review correct ID "…"` on the command line.
- **On a filed item:** `carrel correct ITEM "…"`, and `c` wherever the interactive mode shows a card (browsing, history).
- **Without the words on the command line,** a prompt asks for them.

## How a correction is applied

1. **The words go to a text model,** with the card's current fields, the item's text (as at drafting, trimmed to fit), the archive's collections and tag vocabulary, earlier corrections of this card, and the new correction. It answers with **only the fields to change**, in a fixed schema: title, summary, tags (add and remove), created date, sensitivity, destination and a lasting fact, each optional. A broad correction may change many fields at once.
   - **Each changed field says whether the owner dictated it:** the value was stated outright ("it's from 2019", "call it Rent letter"), or the model inferred it from what the owner said ("it's a picture of A" leading to a new title and summary). This decides provenance, below.
   - **The destination is routed again** when the correction changes what the item is, with the same router as at ingest, given the corrected card. In Review, the proposal moves; on a filed item, it's refiled (a move).
2. **Carrel shows the change** as before and after, field by field:

   ```text
   created     2025-03-31  →  2019-03-31
   summary     A bank statement for March.  →  A letter from the landlord about March's rent.
   tags        + housing  − bank
   collection  Finances  →  Home            (refiled)
   ```

   Dictated fields are marked, so the owner can see which values are theirs and which the model worked out.

3. **The owner confirms** (`y`), edits the result in the editor (`e`), or discards it (`n`). Nothing is saved without a yes.
4. **The same checks as drafting:** a title, real dates, tags from the vocabulary (a new one is proposed as for drafting), a summary that isn't the title. A correction that fails a check is shown with the problem, and the model gets one retry with it, as at drafting.
5. **Only the card's fields change.** The body below the frontmatter is the owner's and is never touched, and neither is the item.

**Without a model,** or when the owner wants to be exact, lines of `field: value` are applied directly, with no model call: `created: 2019-03-31`, `tags: +housing -bank`, `sensitivity: personal`, `title: …`. Anything else needs a model, and Carrel says so.

## What the owner's word can do

A correction comes from the owner, so it can do what a model alone can't:

- **Lower sensitivity.** Models and rules may only raise it; the owner may set any level. A restricted value found by the floor rules (such as a card number) still keeps the item at least sensitive, and Carrel says why.
- **Change the destination,** by naming it ("this goes in Home") or by saying what the item is, which routes it again. In Review, the proposal is re-routed, as `move-to` does. On a filed item, it's refiled: moved the way `carrel rename` moves an item and its card together, as part of the same change.
- **Set any date or title,** whatever the text says.

## Keeping the correction

- **Only dictated fields stop being the model's.** A field the owner stated outright, or wrote as a `field: value` line, is removed from the card's `generated` provenance (FORMAT.md §5.2), so later model work never overwrites it. A field the model inferred from a broad correction stays model-written, with new provenance (the model, the time), since it's still the model's wording.
- **The correction is kept with the card,** in its provenance (`corrections`: the owner's words and the date, §5.2), and given to any later model work on this card, such as a re-check by a stronger model. Model-written fields can be rewritten later, but never back to what the owner corrected: "a picture of A, not B" holds.
- **One undoable change in history** per correction, with the owner's words in its message ("Correct “Rent letter”: created, summary, tags. Owner: it's from 2019…"), so `carrel log` shows what was said and `carrel undo` reverses it.
- **Lasting facts become proposals, never silent rules.** When a correction states something true beyond this item ("Acme Property is my landlord", "anything from Dr Shah is medical"), the model can return it as a fact. Carrel offers to add it to the glossary (§7.3), or, when it names a collection, to draft a routing rule. The owner says yes or no, as with questions.

## Building it

1. The correction schema, prompt and checks in `carrel-core`, with `field: value` parsing, and tests with scripted models.
2. `carrel review correct` and `carrel correct`, with the before-and-after and confirmation.
3. `c` in the interactive mode's Review and card views.
4. Sensitivity lowering, re-routing, and refiling filed items.
5. Provenance (dictated fields out of `generated`, `corrections` kept), history messages and undo, and earlier corrections given to later model work.
6. Lasting facts offered as glossary entries or routing rules.
7. The user guide.

## Decisions (accepted 2026-10-30)

1. **Corrections are typed in plain words, in Review and on filed items,** beside the editor (`c` in the interactive mode, `carrel review correct`, `carrel correct`). Recommended: it's how people say what's wrong, and it works at both moments the owner asked for.
2. **A text model turns the words, narrow or broad, into field changes in a fixed schema, re-routing when what the item is changes, shown before and after, saved only on the owner's yes;** `field: value` lines work without a model. Recommended: the owner sees exactly what changes, and simple fixes never depend on a model.
3. **The owner's correction can lower sensitivity, change the destination (a move, for filed items) and set any field,** with restricted values still keeping an item at least sensitive. Recommended: the owner knows best, and the one floor that stays protects against a typed slip exposing a card number.
4. **Only fields the owner dictated leave the `generated` provenance; fields inferred from a broad correction stay model-written, and the correction itself is kept with the card for any later model work.** Each correction is one undoable change with the owner's words in history. Recommended: broad corrections ("a picture of A, not B") can rewrite and refile everything they touch, a later re-check can still improve the wording but never undo what the owner said, and every correction can be traced and reversed.
5. **Lasting facts in a correction are offered as glossary entries or routing rules, never added silently.** Recommended: Carrel learns from corrections, but only what the owner agrees to.
