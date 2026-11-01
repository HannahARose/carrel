# Question value report

2026-11-01 on macos aarch64, carrel 0.5.1.

## Summary

| Model | Items | Questions | Answerable | Routing before | After all | Top 1 (best) | Top 3 (best) | Top 5 (best) | Rank correlation |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model | 81 | 0 | 0 | 79% | 79% | – | – | – | – |
| llama/granite-4.2-3b.gguf | 81 | 0 | 0 | 72% | 72% | – | – | – | – |
| llama/qwen3.5-4b.gguf | 81 | 0 | 0 | 88% | 88% | – | – | – | – |

Top k: the routing after answering the k highest-ranked questions, with the best any k questions give in brackets. Rank correlation: Spearman's, between what the ranking said each answer is worth and the routing points it won.

## apple/Apple Foundation Model

The batch raised no questions.

## llama/granite-4.2-3b.gguf

The batch raised no questions.

## llama/qwen3.5-4b.gguf

The batch raised no questions.
