# Crawler benchmark report

2026-11-01 on macos aarch64, carrel 0.6.0, 113 items in the muddled archive.

## Summary

| Model | Found | Recall | Precision | Placed right | Unusable | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model | 18 of 18 | 100% | 41% | 79% | 1 | 3.0 | no: precision 41% |
| llama/granite-4.2-3b.gguf | 7 of 18 | 39% | 100% | 36% | 0 | 4.2 | no: recall 39%, placement 36% |
| llama/qwen3.5-4b.gguf | 17 of 18 | 94% | 68% | 86% | 0 | 5.9 | yes |

The bar: recall at least 70%, precision at least 60%, at least 70% of what it finds put right, and under 30 seconds an item.

## apple/Apple Foundation Model

- its collection missing: found 4 of 4
- dumped in an overfilled collection: found 6 of 6
- scattered: found 8 of 8

Found but put wrong:

- label printer driver.zip → Projects
- pension annual statement.txt → Work
- team meeting notes.md → Projects

False alarms:

- Allotment committee newsletter.txt (in Home) → Letters: Contains personal correspondence from a community group about local events.
- Allotment planting plan.md (in Home) → Projects: Contains instructions for growing vegetables, which is part of making something.
- Allotment seed order.txt (in Home) → Projects: The item is an order for seeds, which fits the Projects collection's scope.
- Calder Building Society travel money card.txt (in Finances) → Travel: Statement about prepaid travel money fits Travel collection's scope.
- EHIC renewal reminder.txt (in Health) → Travel: The text mentions travel, making this a travel-related item.
- LIGO detector primer.txt (in Reading) → Projects: Contains writing about things the creator makes outside work.
- LIGO open data tutorial.md (in Reading) → Projects: Contains instructions for making something: a black hole merger chirp.
- Mariner Travel invoice.txt (in Travel) → Receipts: Contains proof of purchase for a trip expense, matching the collection's purpose.
- ORCA sensor budget.txt (in Work) → Projects: Contains calculations for equipment purchases, typical of projects.
- ORCA site visit report.txt (in Work) → Projects: The report documents activities related to making or managing projects involving the river.
- PhD funding letter.txt (in Research) → Projects: Contains a draft of academic research related to something the user makes.
- PhD thesis outline.md (in Research) → Projects: Contains drafts of work on a doctorate, which is research for a project.
- cell biology review.txt (in Reading) → Projects: Contains writing about something the creator makes outside work.
- claim physio costs.md (in Insurance) → Health: The text mentions medical treatment, which belongs in Health.
- conference registration.txt (in Research) → Projects: Contains details of a conference presentation and registration, which are aspects of personal projects.
- darwin origin chapter 4 notes.md (in Reading) → Projects: Contains notes on a project: code, electronics, 3D prints and writing.
- dataset description.md (in Research) → Projects: Contains code, electronics, 3D prints and writing: dataset description.
- electricity bill september.txt (in Home) → Finances: The document details a payment for electricity, a financial transaction.
- home office expenses.md (in Finances) → Home: Contains items related to a workspace, fitting the Home collection's scope.
- letter from tax office.txt (in Finances) → Letters: Contains personal correspondence about money, matching Letters collection purpose.
- payslip october.txt (in Work) → Finances: Contains salary details, a core component of financial records.
- rfc 9110 http semantics notes.md (in Reading) → Projects: Contains code-related notes about a web protocol specification.
- short story draft.md (in Projects) → Reading: The text is a reference copy of fiction, not a personal project.
- talk slides outline.md (in Research) → Projects: Contains notes for a presentation about projects outside work.
- travel insurance certificate.txt (in Insurance) → Travel: Contains a claim for a travel-related insurance policy
- travel insurance claim lost bag.md (in Insurance) → Travel: The document is a travel claim, not an insurance policy or renewal.

Unusable:

- vaccination record.md: the crawler's model failed: declined: Apple's on-device model declined: Detected content likely to be unsafe

## llama/granite-4.2-3b.gguf

- its collection missing: found 0 of 4
- dumped in an overfilled collection: found 2 of 6
- scattered: found 5 of 8

Missed:

- IMG_1502.jpg (in Home)
- coffee machine descaling.txt (in Home)
- drill safety and warranty.txt (in Home)
- flight booking lisbon.txt (in Work)
- label printer driver.zip (in Home)
- lemon drizzle cake.txt (in Home)
- lentil dal.md (in Home)
- paper kuramoto synchronization.txt (in Projects)
- router quick start.md (in Home)
- sourdough loaf.md (in Home)
- team meeting notes.md (in Insurance)

Found but put wrong:

- pension annual statement.txt → Insurance
- receipt desk chair.txt → Home

## llama/qwen3.5-4b.gguf

- its collection missing: found 3 of 4
- dumped in an overfilled collection: found 6 of 6
- scattered: found 8 of 8

Missed:

- IMG_1502.jpg (in Home)

Found but put wrong:

- drill safety and warranty.txt → Receipts
- paper kuramoto synchronization.txt → Research

False alarms:

- Allotment committee newsletter.txt (in Home) → Letters: This is a newsletter from a garden association, which is personal correspondence.
- Allotment seed order.txt (in Home) → Receipts: It is a proof of purchase for seeds and potatoes.
- LIGO open data tutorial.md (in Reading) → Projects: This is a tutorial on how to process data, which is a project activity.
- Larkspur Road deposit return.txt (in Home) → Receipts: This is proof of purchase/lease payment, not a utility bill or repair notice.
- Mariner Travel invoice.txt (in Travel) → Receipts: The card is an invoice for a trip, which is proof of purchase.
- ORCA sensor budget.txt (in Work) → Finances: This is a budget document, which belongs in Finances.
- conference registration.txt (in Research) → Projects: This is a personal research project outside of formal employment.
- payslip october.txt (in Work) → Finances: Pay slips are financial records, not employment documents.
