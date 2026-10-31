# Benchmark report

2026-10-31 on macos aarch64, carrel 0.2.0.

## Summary

| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model then llama/gemma-4-e4b.gguf | 100% | 100% | 0% | 100% | 44% | 0% | 0 | 100% | 75% | 12 | 18.1 | yes |

## apple/Apple Foundation Model then llama/gemma-4-e4b.gguf (provider 0.2.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| IMG_1502.jpg | Recipes | ✓ Recipes | sensitive (over): The recipe includes personal ingredients and instructions. | Oat biscuits recipe | A recipe card with handwritten text for making oat biscuits, including ingredients and instructions. |
| IMG_1876.jpg | Travel | ✓ Travel | sensitive (over): The image shows a location and departure details, which may involve personal travel patterns or private data.; has the place it was taken | Departure List Display | A digital display shows a list of departures with times, destinations, and line numbers against a blue and white checkered background. The text is in yellow and white on a black background. |
| IMG_2041.jpg | Receipts | ✓ Receipts | sensitive (over): The receipt contains payment details and personal contact information. | Receipt from HARTLEY & SONS HARDWARE | A receipt from HARTLEY & SONS HARDWARE showing a purchase of an electric kettle and additional items, with a total amount and payment details. |
| IMG_3310.jpg | Work | ✓ Work | sensitive (over): Task management with potential security and operational implications. | Sprint 14 planning tasks | A table of tasks for 'Sprint 14 planning' on '7 Oct' with columns 'TO DO', 'DOING', and 'DONE'. Tasks include 'Export to CSV', 'Fix login timeout', 'Dashboard Alters', 'Release 3.2 notes', and 'On-call rota Q4'. |
| IMG_4127.jpg | Holding | ✓ Holding | personal (exact): has the place it was taken | Illustration of cat | An illustration of a fat orange cat lying on a blue surface with a yellow circle above it. |
| Screenshot 2025-08-28 at 09.14.03.png | Home | ✓ Home | sensitive (exact): mentions a lease | Rent review: 4 Elm Court | Screenshot of an email from Oakfield Lettings to Alex Morgan about a rent increase from $950 to $980 per month starting October 2025. |
| Screenshot 2025-10-03 at 08.41.55.png | Finances | ✓ Finances | restricted (exact): contains what looks like a bank account number (IBAN); mentions pay details | Bank account screenshot | A screenshot showing a bank account with a balance and list of recent transactions. |
| export.png | Projects | ✓ Projects | sensitive (over): Details involve health, energy, and precise technical setups. | Plant monitor v2 - wiring | A technical diagram showing connections between a solar panel, a battery, a microcontroller, a moisture sensor, and a pump, detailing wiring for a plant monitor system. |
| scan0007.png | Letters | ✓ Letters | personal (exact) | Letter to Alex | A printed page of a letter addressed to Alex, dated 3 March 2025, thanking them for a parcel, describing a scarf, and inviting them to help with gardening at Easter. |
