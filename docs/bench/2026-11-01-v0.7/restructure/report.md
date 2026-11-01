# Restructure benchmark report

2026-11-01 on macos aarch64, carrel 0.6.0: a whole-archive proposal for the muddled test archive at each eagerness, with numbers alone and with each text model.

## Summary

| Wording | Eagerness | Changes | Shape before → after | Fixed | Broken | Moves (weighted) | Findability gain | s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| numbers alone | settled | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| numbers alone | balanced | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| numbers alone | eager | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| apple/Apple Foundation Model | settled | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| apple/Apple Foundation Model | balanced | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| apple/Apple Foundation Model | eager | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| llama/granite-4.2-3b.gguf | settled | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| llama/granite-4.2-3b.gguf | balanced | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| llama/granite-4.2-3b.gguf | eager | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| llama/qwen3.5-4b.gguf | settled | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| llama/qwen3.5-4b.gguf | balanced | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |
| llama/qwen3.5-4b.gguf | eager | 0 | 77% → 77% | 0 of 18 | 0 | 0 (0) | untested | 0.0 |

**Shape** is how closely the items that end up together match the intended shape (the pairs' F1), before and after the proposal; it recovers the shape when it rises. **Fixed** is the misplaced items it puts in an acceptable collection or one inside it, and **broken** the well-placed items it moves out of one. **Moves** are items moved, and weighted by each change's invasiveness (1 for rewriting to 5 for top-level collections). **Findability gain** is its own test's held-out cards found in the proposed tree less those found in the current one; numbers alone can't run it.

Items were grouped by their cards' embeddinggemma-300m vectors and the tag and link graph.

## numbers alone, settled

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.15 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.45 moving or merging top-level collections needs at this eagerness

## numbers alone, balanced

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.07 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.22 moving or merging top-level collections needs at this eagerness

## numbers alone, eager

No changes proposed.
- Held back: a new collection of 4 items in Projects: it costs as much as it gains
- Held back: a new top-level collection of 4 items: it costs as much as it gains

## apple/Apple Foundation Model, settled

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.15 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.45 moving or merging top-level collections needs at this eagerness

## apple/Apple Foundation Model, balanced

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.07 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.22 moving or merging top-level collections needs at this eagerness

## apple/Apple Foundation Model, eager

No changes proposed.
- Held back: a new collection of 4 items in Projects: it costs as much as it gains
- Held back: a new top-level collection of 4 items: it costs as much as it gains

## llama/granite-4.2-3b.gguf, settled

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.15 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.45 moving or merging top-level collections needs at this eagerness

## llama/granite-4.2-3b.gguf, balanced

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.07 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.22 moving or merging top-level collections needs at this eagerness

## llama/granite-4.2-3b.gguf, eager

No changes proposed.
- Held back: a new collection of 4 items in Projects: it costs as much as it gains
- Held back: a new top-level collection of 4 items: it costs as much as it gains

## llama/qwen3.5-4b.gguf, settled

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.15 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.45 moving or merging top-level collections needs at this eagerness

## llama/qwen3.5-4b.gguf, balanced

No changes proposed.
- Held back: a new collection of 4 items in Projects: its gain (-0.02) isn't the 0.07 subdividing a collection needs at this eagerness
- Held back: a new top-level collection of 4 items: its gain (-0.40) isn't the 0.22 moving or merging top-level collections needs at this eagerness

## llama/qwen3.5-4b.gguf, eager

No changes proposed.
- Held back: a new collection of 4 items in Projects: it costs as much as it gains
- Held back: a new top-level collection of 4 items: it costs as much as it gains
