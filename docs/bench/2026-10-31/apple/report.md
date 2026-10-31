# Benchmark report

2026-10-31 on macos aarch64, carrel 0.2.0.

## Summary

| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model | 100% | 100% | 0% | 89% | 44% | 0% | 0 | 100% | 75% | 12 | 9.5 | yes |

## apple/Apple Foundation Model (provider 0.2.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| IMG_1502.jpg | Recipes | ✓ Recipes | sensitive (over): The recipe includes private family-specific ingredients and instructions. | Gran's oat biscuits | A handwritten recipe card for making oat biscuits, listing ingredients and step-by-step instructions. |
| IMG_1876.jpg | Travel | ✓ Travel | sensitive (over): The image contains a departure list with times, destinations, and lines, which may involve personal travel data.; has the place it was taken | Departure list | A departure list showing times, destinations, and lines for various locations. |
| IMG_2041.jpg | Receipts | ✓ Receipts | sensitive (over): Contains financial transaction and personal contact details. | Receipt for electric kettle | A receipt from HARTLEY & SONS HARDWARE for an electric kettle purchase, showing a total of 39.49 pounds with contactless payment approved and a 2-year guarantee. |
| IMG_3310.jpg | Work | ✗ Holding | sensitive (over): The image contains a work-related task list with deadlines and team responsibilities. | Sprint 14 planning - 7 Oct | A slide showing a table titled 'Sprint 14 planning - 7 Oct' with columns 'TO DO', 'DOING', and 'DONE', listing tasks such as 'Export to CSV', 'Fix login', and 'Demo to client 21 Oct'. |
| IMG_4127.jpg | Holding | ✓ Holding | personal (exact): has the place it was taken | Cartoon cat on blue surface | A cartoon cat with closed eyes lies on a blue surface next to a yellow circle. |
| Screenshot 2025-08-28 at 09.14.03.png | Home | ✓ Home | sensitive (exact): mentions a lease | Rent review: 4 Elm Court | A screenshot of a document showing a rent review letter from Oakfield Lettings regarding a change in rent from $950 to $985 starting October 2025. |
| Screenshot 2025-10-03 at 08.41.55.png | Finances | ✓ Finances | restricted (exact): contains what looks like a bank account number (IBAN); mentions pay details | Banking application screenshot | A screenshot of a banking application showing a balance and a list of transactions. |
| export.png | Projects | ✓ Projects | sensitive (over): Details involve hardware, connections, and operational specifics. | Plant monitor wiring diagram | Wiring diagram for a plant monitor showing connections between a solar panel, ESP32-C3 dev board, soil moisture sensor, relay module, and 18650 cell with TP4056 charger. Includes labels for ADC GPIO2, sensor (capacitive), charge, GPI05, and details on power, deep sleep, and pump behavior. |
| scan0007.png | Letters | ✓ Letters | personal (exact) | Letter to Alex | A letter on a white background with black text, addressed to Alex and signed by Jo, thanking them for a parcel of a scarf and mentioning a new job and gardening. |
