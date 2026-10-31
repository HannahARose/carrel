# Model benchmark results

How each text model does at Carrel's work: drafting a card, choosing a collection, and rating sensitivity, on the benchmark's test set of 65 invented documents with known answers (`fixtures/bench`). The design, the scoring and the bar are in [docs/design/benchmark.md](../design/benchmark.md). Each run's full report, with every item's draft, is kept in a folder named by its date.

## 2026-10-30

On the reference machine, an M4 Max MacBook Pro with 36 GB, with Carrel 0.1.0 from `dev` after #146, one model at a time with nothing else compiling. [Full report](2026-10-30/report.md) · [results.json](2026-10-30/results.json)

| Model | Size | Valid | Routing | Sensitivity exact | Under-rated | Dates | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Gemma 4 E4B** (llama.cpp) | 5.2 GB | 100% | 94% | 83% | 2% | 58% | 17.9 | **yes** |
| **Qwen3.5 4B** (llama.cpp) | 2.7 GB | 97% | 87% | 95% | 2% | 63% | 21.0 | **yes** |
| Qwen3.5 9B (llama.cpp) | 5.7 GB | 100% | 96% | 72% | 2% | 69% | 33.2 | no: too slow |
| Granite 4.2 8B (llama.cpp) | 5.4 GB | 95% | 80% | 85% | 3% | 45% | 32.3 | no: too slow |
| Apple Foundation Model | shared | 89% | 75% | 83% | 3% | 77% | 6.9 | no: declined 11%, routing |
| Granite 4.2 3B (llama.cpp) | 2.2 GB | 86% | 72% | 78% | 2% | 62% | 18.3 | no: valid, routing |

The bar: valid output for at least 95% of items, routing at least 80%, no restricted item under-rated and at most 5% under-rated, under 30 seconds an item. No model under-rated a restricted item.

### Recommended

| Memory | Model | Why |
| --- | --- | --- |
| 8 GB | `qwen3.5-4b` | The smallest that meets the bar; the most exact sensitivity ratings (95%) |
| 16 GB or more | `gemma-4-e4b` | Every answer valid on the first try, the best routing that's fast enough (94%), and the fastest open model |
| A Mac with Apple Intelligence and 16 GB | Apple's model, with `gemma-4-e4b` installed | Apple's model drafts first and Gemma gives a second opinion (#168): it meets the bar at 9.7 s an item |

`carrel models` marks them, and `carrel models download NAME` fetches one.

### What the numbers say

- **Apple's on-device model** is the fastest by far and needs no download, but it declined 7 of the 65 items, 6 of them health documents its guardrails refuse. Left out of the count, its answers were all valid, but its routing (77%) is still below the bar. In use, Carrel passes a declined item to the next model, so on a Mac with Apple Intelligence it works best with a recommended open model installed beside it, as the second opinion below shows.
- **Larger isn't better at the bar's speed:** Qwen3.5 9B routes best (96%), but at 33 seconds an item a batch of 200 takes almost two hours, and its sensitivity ratings were the least exact (72%, mostly rating up, which is the safe direction).
- **Dates are the weak spot** for every model (45–77% right), since many documents give several dates; the owner sees the date in review.
- **Extra tags** (tags proposed outside the vocabulary) were highest for Apple's model and Granite 3B, which is why their tag scores look fine while their proposals are noisier.

### Apple's model with a second opinion

Apple's model drafts, routes and rates every item, and Gemma 4 E4B is asked again for what it would send to Holding or place with low confidence, and for anything it declines (#168). [Report](2026-10-30/apple-then-gemma/report.md)

| | Valid | Routing | Sensitivity exact | Dates | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- |
| Apple's model alone | 89% | 75% | 83% | 77% | 6.9 | no |
| Gemma 4 E4B alone | 100% | 94% | 83% | 58% | 17.9 | yes |
| **Apple, then Gemma** | **100%** | **89%** | **89%** | **74%** | **9.7** | **yes** |

Nearly Gemma's accuracy at about half its time, and Apple's model's better dates. An earlier run of the same pair routed 92%, so a few points either way is normal.

### MLX

The same Granite 4.2 3B through `carrel-provider-mlx` (IBM's 4-bit MLX build), after the fixes in #164. [Report](2026-10-30/mlx/report.md)

| Granite 4.2 3B | Valid | Routing | Dates | s/item |
| --- | --- | --- | --- | --- |
| llama.cpp (Q4_K_M) | 86% | 72% | 62% | 18.3 |
| MLX (4-bit) | 100% | 36% | 54% | 10.8 |

MLX is about 40% faster and every answer was valid, but it routes much worse: its second routing pass, with the collections in the other order, rates good choices as poor fits, so most items go to Holding. The likeliest cause is the quantization (plain 4-bit against llama.cpp's mixed Q4_K_M); the 8-bit build would tell. Until then no MLX build is recommended.

### Not measured yet

- **A larger model for 32 GB Macs,** as the design planned, needs a download of about 20 GB, which hasn't been asked for.

## Running it again

```bash
carrel bench --out bench-results
```

It's resumable (`--fresh` starts over), shows progress, and `--compare` puts a run beside an earlier `results.json`. The on-device runner runs it weekly ([docs/dev/on-device-runner.md](../dev/on-device-runner.md)). Results change with the prompts, so record a new dated folder here when a release changes them.
