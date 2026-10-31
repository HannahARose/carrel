# Benchmark report

2026-10-31 on macos aarch64, carrel 0.2.0.

## Summary

| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model | 100% | 100% | 0% | 67% | 33% | 0% | 0 | 100% | 62% | 14 | 7.3 | no: routing 67% |

## apple/Apple Foundation Model (provider 0.2.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| IMG_1502.jpg | Recipes | ✓ Recipes | sensitive (over): The recipe involves private ingredients and family-specific instructions. | Gran's oat biscuits | A recipe for oat biscuits with ingredient lists and baking instructions. |
| IMG_1876.jpg | Travel | ✓ Travel | sensitive (over): The image contains a schedule with location and timing details.; has the place it was taken | Train schedule | A train schedule showing departure and arrival times for various stations. |
| IMG_2041.jpg | Receipts | ✓ Receipts | sensitive (over): The receipt includes card details and a personal transaction. | Receipt for Electric Kettle and Descaler Sachets | Receipt for electric kettle and descaler sachets from HARTLEY & SONS HARDWARE. |
| IMG_3310.jpg | Work | ✗ Holding | sensitive (over): The text includes work-related tasks and scheduling details. | Sprint 14 planning | A planning document with tasks, releases, and meeting notes for Sprint 14 and Q4. |
| IMG_4127.jpg | Holding | ✓ Holding | sensitive (over): The image includes a location, which may involve personal data.; has the place it was taken | Photo of location | A photo taken on an iPhone 15 Pro, showing a location. |
| Screenshot 2025-08-28 at 09.14.03.png | Home | ✗ Holding | sensitive (exact): mentions a lease | Rent review update | A document showing a rent review update from Oakfield Lettings regarding a change in rent amount for Flat 4, Elm Court. |
| Screenshot 2025-10-03 at 08.41.55.png | Finances | ✓ Finances | restricted (exact): contains what looks like a bank account number (IBAN); mentions pay details | Screenshot of financial document | A screenshot of a financial document showing account balances and recent transactions. |
| export.png | Projects | ✗ Holding | sensitive (over): Details involve hardware, sensors, and system configurations tied to personal projects. | Plant monitor v2 - wiring | A technical diagram and chart showing a plant monitor system with wiring for a solar panel, soil moisture sensors, and a microcontroller setup. |
| scan0007.png | Letters | ✓ Letters | personal (exact) | Letter to Alex | A personal letter thanking someone for a gift, mentioning gardening, a new job, and a colleague. |
