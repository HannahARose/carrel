# Benchmark report

2026-10-31 on macos aarch64, carrel 0.2.0.

## Summary

| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model then llama/gemma-4-e4b.gguf | 100% | 100% | 0% | 100% | 33% | 0% | 0 | 100% | 62% | 13 | 13.5 | yes |

## apple/Apple Foundation Model then llama/gemma-4-e4b.gguf (provider 0.2.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| IMG_1502.jpg | Recipes | ✓ Recipes | sensitive (over): The recipe involves private ingredients and methods. | Gran's oat biscuits | Recipe for oat biscuits including ingredients, preparation steps, and baking instructions. |
| IMG_1876.jpg | Travel | ✓ Travel | sensitive (over): The image shows a schedule with location and time details, suggesting private travel information.; has the place it was taken | Train schedule | A train schedule showing departure and arrival times for various stations, including Porto Sao Bento, Braga, Guimaraes, Aveiro, Marco de Canaveses, and Regua. |
| IMG_2041.jpg | Receipts | ✓ Receipts | sensitive (over): Includes financial details and personal transaction information. | Receipt for kettle and descaler sachets | A receipt for a kettle and descaler sachets from HARTLEY & SONS HARDWARE. |
| IMG_3310.jpg | Work | ✓ Work | sensitive (over): The text includes work-related tasks and deadlines. | Sprint 14 planning | A planning document for a sprint with tasks, notes, and a client demo scheduled. |
| IMG_4127.jpg | Holding | ✓ Holding | sensitive (over): Location data may involve personal privacy concerns.; has the place it was taken | Photo on iPhone | A photo taken on an iPhone 15 Pro with a location. |
| Screenshot 2025-08-28 at 09.14.03.png | Home | ✓ Home | sensitive (exact): mentions a lease | Rent review email | Screenshot of a rent review email regarding a change in rent amount from 950.00 to 985.00 a month from 1 October 2025. |
| Screenshot 2025-10-03 at 08.41.55.png | Finances | ✓ Finances | restricted (exact): contains what looks like a bank account number (IBAN); mentions pay details | Screenshot of financial transactions | A screenshot of a financial document showing account balances and recent transactions. |
| export.png | Projects | ✓ Projects | sensitive (over): Details involve health, legal, and private technical setups. | Plant monitor v2 - wiring | Diagram of a plant monitor system with wiring details for solar panel, soil moisture sensor, ESP32-C3, relay module, TP4056 charger, pump, and LDO regulator. |
| scan0007.png | Letters | ✓ Letters | personal (exact) | Letter from Jo | A handwritten letter on a printed page, thanking someone for a gift, mentioning a walk, gardening, and a new job, with a request to share updates. |
