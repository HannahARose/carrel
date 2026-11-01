# Question value report

2026-11-01 on macos aarch64, carrel 0.5.1.

## Summary

| Model | Items | Questions | Answerable | Routing before | After all | Top 1 (best) | Top 3 (best) | Top 5 (best) | Rank correlation |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model | 116 | 14 | 14 | 73% | 77% | 75% (75%) | 75% (76%) | 76% (77%) | 0.58 |
| llama/granite-4.2-3b.gguf | 116 | 14 | 14 | 62% | 68% | 64% (64%) | 67% (67%) | 68% (69%) | 0.87 |
| llama/qwen3.5-4b.gguf | 116 | 14 | 14 | 82% | 84% | 84% (84%) | 84% (85%) | 84% (85%) | 0.48 |

Top k: the routing after answering the k highest-ranked questions, with the best any k questions give in brackets. Rank correlation: Spearman's, between what the ranking said each answer is worth and the routing points it won.

## apple/Apple Foundation Model

| Rank | Question | Items | Ranked value | Known answer | Moved | Gain |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | These 4 items about “Allotment”: Projects, Home, or a new Projects/Allotment collection? | 4 | 3.20 | Home | Allotment committee newsletter.txt → Home; Allotment planting plan.md → Home; Allotment seed order.txt → Home | +2.0 |
| 2 | What is “ORCA”? | 3 | 3.00 | ORCA: items named for it belong in Work (Employment: contracts, pay, reviews, meetings and applications) | ORCA sensor budget.txt → Projects | +0.0 |
| 3 | These 4 items about “Calder Building Society”: Finances, or a new Finances/Calder Building Society collection? | 4 | 2.80 | Finances | Calder Building Society rate change.txt → Finances | +1.0 |
| 4 | These 3 items about “Larkspur Road”: Home, or a new Home/Larkspur Road collection? | 3 | 2.60 | Home | Larkspur Road deposit return.txt → Home; Larkspur Road keys handover.txt → Home | +1.0 |
| 5 | What is “CNC”? | 2 | 2.00 | CNC: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 6 | These 3 items about “Mariner Travel”: Travel, or a new Travel/Mariner Travel collection? | 3 | 1.80 | Travel |  | +0.0 |
| 7 | What is “BrewLog”? | 2 | 1.60 | BrewLog: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 8 | What is “EHIC”? | 2 | 1.60 | EHIC: items named for it belong in Health (Medical records, prescriptions, test results and appointments) | EHIC application confirmation.txt → Health | +0.5 |
| 9 | What is “LIGO”? | 2 | 1.60 | LIGO: items named for it belong in Reading (Papers, articles and books by others, kept for reference) |  | +0.0 |
| 10 | What is “OSM”? | 2 | 1.60 | OSM: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 11 | What is “PhD”? | 2 | 1.60 | PhD: items named for it belong in Research (My own research: lab notes, drafts, data and talks) | PhD funding letter.txt → Work | +0.0 |
| 12 | What is “QPCR”? | 2 | 1.60 | QPCR: items named for it belong in Research (My own research: lab notes, drafts, data and talks) | QPCR plate layout.txt → Holding | +0.0 |
| 13 | What is “DnD”? | 2 | 1.20 | DnD: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 14 | What is “ISA”? | 2 | 1.20 | ISA: items named for it belong in Finances (Bank accounts, tax, pensions and investments) |  | +0.0 |

## llama/granite-4.2-3b.gguf

| Rank | Question | Items | Ranked value | Known answer | Moved | Gain |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | These 4 items about “Allotment”: a new Allotment collection? | 4 | 4.00 | Home | Allotment committee newsletter.txt → Home; Allotment planting plan.md → Home; Allotment plot licence.txt → Home; Allotment seed order.txt → Home | +2.5 |
| 2 | These 4 items about “Calder Building Society”: Finances, or a new Finances/Calder Building Society collection? | 4 | 3.60 | Finances | Calder Building Society branch closure.txt → Finances; Calder Building Society rate change.txt → Finances; Calder Building Society travel money card.txt → Finances | +2.0 |
| 3 | What is “ORCA”? | 3 | 3.00 | ORCA: items named for it belong in Work (Employment: contracts, pay, reviews, meetings and applications) | ORCA sensor budget.txt → Work | +1.0 |
| 4 | These 3 items about “Larkspur Road”: a new Larkspur Road collection? | 3 | 3.00 | Home | Larkspur Road deposit return.txt → Home; Larkspur Road inventory.txt → Home; Larkspur Road keys handover.txt → Home | +1.5 |
| 5 | These 3 items about “Mariner Travel”: Travel, or a new Travel/Mariner Travel collection? | 3 | 2.20 | Travel |  | +0.0 |
| 6 | What is “BrewLog”? | 2 | 2.00 | BrewLog: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 7 | What is “CNC”? | 2 | 2.00 | CNC: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 8 | What is “DnD”? | 2 | 2.00 | DnD: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) | DnD session recap.md → Projects | +1.0 |
| 9 | What is “EHIC”? | 2 | 2.00 | EHIC: items named for it belong in Health (Medical records, prescriptions, test results and appointments) |  | +0.0 |
| 10 | What is “LIGO”? | 2 | 2.00 | LIGO: items named for it belong in Reading (Papers, articles and books by others, kept for reference) | LIGO open data tutorial.md → Holding | +0.0 |
| 11 | What is “OSM”? | 2 | 2.00 | OSM: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 12 | What is “PhD”? | 2 | 2.00 | PhD: items named for it belong in Research (My own research: lab notes, drafts, data and talks) |  | +0.0 |
| 13 | What is “QPCR”? | 2 | 2.00 | QPCR: items named for it belong in Research (My own research: lab notes, drafts, data and talks) |  | +0.0 |
| 14 | What is “ISA”? | 2 | 1.60 | ISA: items named for it belong in Finances (Bank accounts, tax, pensions and investments) | ISA interest summary.txt → Holding | -1.0 |

## llama/qwen3.5-4b.gguf

| Rank | Question | Items | Ranked value | Known answer | Moved | Gain |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | These 4 items about “Allotment”: Receipts, or a new Receipts/Allotment collection? | 4 | 3.60 | Home | Allotment committee newsletter.txt → Home; Allotment planting plan.md → Home; Allotment plot licence.txt → Home; Allotment seed order.txt → Home | +2.0 |
| 2 | These 4 items about “Calder Building Society”: Finances, or a new Finances/Calder Building Society collection? | 4 | 2.80 | Finances | Calder Building Society travel money card.txt → Finances | +0.5 |
| 3 | What is “ORCA”? | 3 | 2.60 | ORCA: items named for it belong in Work (Employment: contracts, pay, reviews, meetings and applications) | ORCA site visit report.txt → Projects | +0.0 |
| 4 | These 3 items about “Mariner Travel”: Travel, or a new Travel/Mariner Travel collection? | 3 | 2.20 | Travel | Mariner Travel invoice.txt → Travel | +0.5 |
| 5 | These 3 items about “Larkspur Road”: Home, or a new Home/Larkspur Road collection? | 3 | 1.80 | Home |  | +0.0 |
| 6 | What is “CNC”? | 2 | 1.60 | CNC: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) | CNC toolpath settings.txt → Holding | -1.0 |
| 7 | What is “EHIC”? | 2 | 1.60 | EHIC: items named for it belong in Health (Medical records, prescriptions, test results and appointments) |  | +0.0 |
| 8 | What is “LIGO”? | 2 | 1.60 | LIGO: items named for it belong in Reading (Papers, articles and books by others, kept for reference) | LIGO open data tutorial.md → Holding | +0.0 |
| 9 | What is “OSM”? | 2 | 1.60 | OSM: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) | OSM footpath edits.md → Projects | +1.0 |
| 10 | What is “PhD”? | 2 | 1.60 | PhD: items named for it belong in Research (My own research: lab notes, drafts, data and talks) | PhD thesis outline.md → Holding | -1.0 |
| 11 | What is “BrewLog”? | 2 | 1.20 | BrewLog: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 12 | What is “DnD”? | 2 | 1.20 | DnD: items named for it belong in Projects (Things I make outside work: code, electronics, 3D prints and writing) |  | +0.0 |
| 13 | What is “ISA”? | 2 | 1.20 | ISA: items named for it belong in Finances (Bank accounts, tax, pensions and investments) |  | +0.0 |
| 14 | What is “QPCR”? | 2 | 1.20 | QPCR: items named for it belong in Research (My own research: lab notes, drafts, data and talks) |  | +0.0 |
