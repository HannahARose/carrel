# Findability report

2026-10-31 on macos aarch64, carrel 0.3.0.

230 descriptions of 81 items, written by hand (`descriptions.yaml`), each looked up as a search. The items were filed in their expected collections, with the titles and summaries drafted in `docs/bench/2026-10-31/gemma/results.json` (9 items; the rest titled by their file names), and the expected tags, dates and sensitivity. A description counts as found when its item is among the first 5 items that come back; the mean reciprocal rank counts every place up to 100.

## Summary

| Ranking | Model | Top five | Mean reciprocal rank | Missed |
| --- | --- | --- | --- | --- |
| Words alone | none | 92% | 0.85 | 18 |
| Meaning alone | apple-nl-sentence-en | 42% | 0.31 | 133 |
| Words and meaning | apple-nl-sentence-en | 85% | 0.65 | 35 |
| Meaning alone | embeddinggemma-300m | 98% | 0.92 | 5 |
| Words and meaning | embeddinggemma-300m | 96% | 0.89 | 10 |
| Meaning alone | qwen3-embedding-0.6b | 98% | 0.89 | 5 |
| Words and meaning | qwen3-embedding-0.6b | 93% | 0.90 | 15 |

Built an index for apple-nl-sentence-en , embedding 81 items; the next run reuses it.

Built an index for embeddinggemma-300m , embedding 81 items; the next run reuses it.

Built an index for qwen3-embedding-0.6b , embedding 81 items; the next run reuses it.

## Not in the top five

### Words alone

| Item | Description | Place | Came back first |
| --- | --- | --- | --- |
| 2025 bank statement March.txt | my current account transactions for one month in spring | not found | Screenshot 2025-10-03 at 08.41.55.png, tenancy agreement.md, dataset description.md, internet contract.txt, prescription amoxicillin.txt |
| 2025 bank statement March.txt | Northbridge account balance after moving money into savings | not found | Screenshot 2025-10-03 at 08.41.55.png, message to landlord.txt, pension annual statement.txt, tax return summary 2024.md, payslip october.txt |
| card replacement letter.txt | cut up your old card | 7 | car insurance renewal.txt, physio appointment.txt, internet contract.txt, order confirmation laptop.txt, fund report q2.txt |
| coffee machine descaling.txt | how to get the limescale out of the espresso maker | not found | cell biology review.txt, draft paper phase response.md, itinerary portugal.md, hotel booking porto.txt, letter from grandma.txt |
| cv alex morgan.md | my résumé | not found | talk slides outline.md, darwin origin chapter 4 notes.md, sourdough loaf.md |
| flight booking lisbon.txt | my plane tickets to Portugal | not found | train tickets porto.txt, itinerary portugal.md, travel insurance certificate.txt, talk slides outline.md, darwin origin chapter 4 notes.md |
| home contents policy.txt | what my possessions in the flat are insured for | 7 | essay on attention.md, travel insurance certificate.txt, message to landlord.txt, Screenshot 2025-08-28 at 09.14.03.png, talk slides outline.md |
| label printer driver.zip | the installer for the QL-700 | not found |  |
| old us tax form.txt | the 1040 with my social security number | not found | payslip october.txt, talk slides outline.md, darwin origin chapter 4 notes.md, sourdough loaf.md |
| old us tax form.txt | federal return filed in April 2021 | 7 | tax return summary 2024.md, cv alex morgan.md, receipt and manual blender.txt, hotel booking porto.txt, home office expenses.md |
| receipt desk chair.txt | what I paid for my office seat | not found | tax return summary 2024.md, home office expenses.md, letter from tax office.txt, essay on attention.md, train tickets porto.txt |
| scan_0007.stl | the little figurine with a round body and a head | not found | performance review 2025.md, referral letter for physio.txt |
| train tickets porto.txt | the rail journey from Lisbon up north | not found | flight booking lisbon.txt, itinerary portugal.md, amplifier build log.txt, travel insurance claim lost bag.md, travel insurance certificate.txt |
| vaccination record.md | which jabs I've had and when | not found | letter from grandma.txt, short story draft.md, coffee machine descaling.txt, scan0007.png, internet contract.txt |
| vaccination record.md | my immunisation history from the surgery | not found | talk slides outline.md, darwin origin chapter 4 notes.md, sourdough loaf.md |
| wifi and alarm codes.txt | instructions I leave for the house-sitter | not found | kettle manual.zip, internet contract.txt, scan0007.png, drill safety and warranty.txt, IMG_1502.jpg |
| wifi and alarm codes.txt | when the bins go out and how much to feed the cat | not found | letter from grandma.txt, cell biology review.txt, draft paper phase response.md, itinerary portugal.md, coffee machine descaling.txt |
| wifi and alarm codes.txt | the flat's password for guests | not found | message to landlord.txt, Screenshot 2025-08-28 at 09.14.03.png, tenancy agreement.md, home contents policy.txt, IMG_1502.jpg |

### Meaning alone, apple-nl-sentence-en

| Item | Description | Place | Came back first |
| --- | --- | --- | --- |
| 2025 bank statement March.txt | my current account transactions for one month in spring | 12 | pension annual statement.txt, employment contract lumen.md, tenancy agreement.md, performance review 2025.md, email from sam about visit.txt |
| 2025 bank statement March.txt | the statement that shows the rent going out and my salary coming in | 17 | IMG_2041.jpg, essay on attention.md, misc notes.txt, Screenshot 2025-10-03 at 08.41.55.png, pension annual statement.txt |
| 2025 bank statement March.txt | Northbridge account balance after moving money into savings | 11 | Screenshot 2025-10-03 at 08.41.55.png, vaccination record.md, IMG_2041.jpg, pension annual statement.txt, draft paper phase response.md |
| IMG_1876.jpg | the departures board at the station in Porto | 61 | misc notes.txt, wifi and alarm codes.txt, untitled scan text.txt, drill safety and warranty.txt, itinerary portugal.md |
| IMG_2041.jpg | Hartley & Sons receipt with descaler sachets | 13 | lentil dal.md, vaccination record.md, drill safety and warranty.txt, dishwasher manual.txt, kettle manual.zip |
| IMG_3310.jpg | the whiteboard from sprint planning at work | 80 | itinerary portugal.md, untitled scan text.txt, lentil dal.md, misc notes.txt, dishwasher manual.txt |
| IMG_3310.jpg | to do, doing and done columns from the October meeting | 79 | misc notes.txt, lentil dal.md, 2025 bank statement March.txt, hotel booking porto.txt, draft paper phase response.md |
| Screenshot 2025-08-28 at 09.14.03.png | the email from the letting agent about the rent going up | 45 | email from sam about visit.txt, IMG_2041.jpg, Screenshot 2025-10-03 at 08.41.55.png, itinerary portugal.md, misc notes.txt |
| Screenshot 2025-10-03 at 08.41.55.png | my banking app showing the balance | 9 | coffee machine descaling.txt, label printer driver.zip, budget app notes.md, kettle manual.zip, misc notes.txt |
| amplifier build log.txt | notes from building my headphone amp | 28 | lamp dimmer firmware.zip, untitled scan text.txt, misc notes.txt, label printer driver.zip, budget app notes.md |
| amplifier build log.txt | the hum from the transformer, move the input wiring | 31 | lamp dimmer firmware.zip, misc notes.txt, plant monitor readme.md, drill safety and warranty.txt, dishwasher manual.txt |
| amplifier build log.txt | etching the board and soldering the op-amp socket | 24 | kettle manual.zip, coffee machine descaling.txt, dishwasher manual.txt, drill safety and warranty.txt, lamp dimmer firmware.zip |
| blood test results.txt | lab results that showed my iron was low | 14 | router quick start.md, kettle manual.zip, plant monitor readme.md, referral letter for physio.txt, coffee machine descaling.txt |
| budget app notes.md | the Python and Textual app I haven't started | 20 | untitled scan text.txt, misc notes.txt, coffee machine descaling.txt, part_v3_final.stl, kettle manual.zip |
| building notice water off.txt | the day the water supply was going to be cut for the valve work | 18 | drill safety and warranty.txt, kettle manual.zip, draft paper phase response.md, plant monitor readme.md, dishwasher manual.txt |
| building notice water off.txt | notice to residents from the managing agent | 25 | Screenshot 2025-10-03 at 08.41.55.png, wifi and alarm codes.txt, budget app notes.md, itinerary portugal.md, email from sam about visit.txt |
| building notice water off.txt | fill a kettle beforehand | 10 | untitled scan text.txt, lentil dal.md, misc notes.txt, receipt and manual blender.txt, coffee machine descaling.txt |
| car insurance renewal.txt | how much it costs to insure the Fiesta next year | 45 | pension annual statement.txt, performance review 2025.md, payslip october.txt, 2025 bank statement March.txt, tax return summary 2024.md |
| car insurance renewal.txt | my motor premium going up from 389 to 412 | 20 | travel insurance certificate.txt, IMG_2041.jpg, phone repair receipt.txt, payslip october.txt, home contents policy.txt |
| car insurance renewal.txt | letter about the no claims discount from Lighthouse | 53 | untitled scan text.txt, travel insurance claim lost bag.md, message to landlord.txt, letter from grandma.txt, coffee machine descaling.txt |
| card replacement letter.txt | the letter that came with my new debit card | 8 | email from sam about visit.txt, IMG_2041.jpg, misc notes.txt, birthday card from sister.txt, referral letter for physio.txt |
| card replacement letter.txt | Northbridge sending a replacement card that expires 09/29 | 17 | internet contract.txt, receipt and manual blender.txt, IMG_2041.jpg, drill safety and warranty.txt, phone repair receipt.txt |
| card replacement letter.txt | cut up your old card | 23 | lentil dal.md, lamp dimmer firmware.zip, misc notes.txt, wifi and alarm codes.txt, short story draft.md |
| cell biology review.txt | review of body clocks in individual cells | 31 | drill safety and warranty.txt, wifi and alarm codes.txt, kettle manual.zip, dishwasher manual.txt, plant monitor readme.md |
| cell biology review.txt | how clocks get reset by light and feeding | 56 | lentil dal.md, kettle manual.zip, router quick start.md, drill safety and warranty.txt, misc notes.txt |
| cell biology review.txt | single-cell imaging of circadian rhythms | 8 | untitled scan text.txt, prescription amoxicillin.txt, darwin origin chapter 4 notes.md, coffee machine descaling.txt, lamp dimmer firmware.zip |
| claim physio costs.md | health cover reimbursement for three sessions | 51 | vaccination record.md, lentil dal.md, Screenshot 2025-10-03 at 08.41.55.png, travel insurance certificate.txt, itinerary portugal.md |
| conference registration.txt | confirmation for Dynamics Days in Edinburgh | 12 | vaccination record.md, darwin origin chapter 4 notes.md, pension annual statement.txt, lab notebook oscillators week 12.md, payslip october.txt |
| conference registration.txt | where I'm presenting the poster on synchrony thresholds | 47 | lentil dal.md, budget app notes.md, team meeting notes.md, misc notes.txt, untitled scan text.txt |
| conference registration.txt | the September meeting I paid 280 for | 14 | pension annual statement.txt, travel insurance certificate.txt, phone repair receipt.txt, tax return summary 2024.md, home contents policy.txt |
| cv alex morgan.md | my résumé | 41 | untitled scan text.txt, lentil dal.md, misc notes.txt, dishwasher manual.txt, kettle manual.zip |
| cv alex morgan.md | list of my jobs and my MSc in applied statistics | 36 | budget app notes.md, draft paper phase response.md, kettle manual.zip, dishwasher manual.txt, pension annual statement.txt |
| cv alex morgan.md | what I send when applying for analyst jobs | 54 | drill safety and warranty.txt, kettle manual.zip, wifi and alarm codes.txt, dishwasher manual.txt, lentil dal.md |
| darwin origin chapter 4 notes.md | my summary of the natural selection chapter | 13 | misc notes.txt, untitled scan text.txt, budget app notes.md, wifi and alarm codes.txt, draft paper phase response.md |
| darwin origin chapter 4 notes.md | the diagram of divergence where varieties branch | 17 | misc notes.txt, untitled scan text.txt, lamp dimmer firmware.zip, kettle manual.zip, wifi and alarm codes.txt |
| darwin origin chapter 4 notes.md | notes on sexual selection from the 1859 book | 22 | essay on attention.md, letter from grandma.txt, misc notes.txt, receipt desk chair.txt, physio appointment.txt |
| dataset description.md | what's in the imaging data from the 48 wells | 21 | boiler repair note.txt, flight booking lisbon.txt, Screenshot 2025-10-03 at 08.41.55.png, phone repair receipt.txt, travel insurance certificate.txt |
| dataset description.md | TIFF stacks on the lab server, one per well | 13 | lamp dimmer firmware.zip, plant monitor readme.md, budget app notes.md, label printer driver.zip, wifi and alarm codes.txt |
| dataset description.md | columns of the summary CSV for the May recording | 36 | misc notes.txt, draft paper phase response.md, performance review 2025.md, email from sam about visit.txt, pension annual statement.txt |
| draft paper phase response.md | the manuscript I'm writing about how cells synchronize in pairs | 35 | lentil dal.md, sourdough loaf.md, misc notes.txt, untitled scan text.txt, label printer driver.zip |
| draft paper phase response.md | unfinished paper where the methods still say TODO | 9 | untitled scan text.txt, kettle manual.zip, coffee machine descaling.txt, misc notes.txt, dishwasher manual.txt |
| drill safety and warranty.txt | register within 28 days for three years cover | 56 | phone repair receipt.txt, receipt and manual blender.txt, employment contract lumen.md, payslip october.txt, performance review 2025.md |
| electricity bill september.txt | how much power I used last month and what it cost | 25 | essay on attention.md, talk slides outline.md, message to landlord.txt, email from sam about visit.txt, car insurance renewal.txt |
| electricity bill september.txt | Brightspark energy invoice paid by direct debit | 35 | IMG_2041.jpg, pension annual statement.txt, travel insurance certificate.txt, vaccination record.md, Screenshot 2025-10-03 at 08.41.55.png |
| email from sam about visit.txt | when my friend is coming to stay in September | 7 | birthday card from sister.txt, untitled scan text.txt, message to landlord.txt, team meeting notes.md, physio appointment.txt |
| email from sam about visit.txt | Sam bringing the good waterproofs | 22 | lemon drizzle cake.txt, sourdough loaf.md, lentil dal.md, coffee machine descaling.txt, kettle manual.zip |
| employment contract lumen.md | the document with my starting salary of 41,000 | 8 | pension annual statement.txt, travel insurance certificate.txt, IMG_2041.jpg, Screenshot 2025-10-03 at 08.41.55.png, phone repair receipt.txt |
| export.png | deep sleep 15 minutes between readings | 75 | physio appointment.txt, prescription amoxicillin.txt, payslip october.txt, talk slides outline.md, sourdough loaf.md |
| flight booking lisbon.txt | my plane tickets to Portugal | 19 | itinerary portugal.md, coffee machine descaling.txt, travel insurance certificate.txt, budget app notes.md, misc notes.txt |
| flight booking lisbon.txt | booking reference K7Q2PX | 10 | untitled scan text.txt, misc notes.txt, wifi and alarm codes.txt, budget app notes.md, drill safety and warranty.txt |
| fund report q2.txt | how my index fund investment did over the summer quarter | 7 | draft paper phase response.md, pension annual statement.txt, IMG_2041.jpg, old us tax form.txt, Screenshot 2025-10-03 at 08.41.55.png |
| fund report q2.txt | units held in the global equity tracker and their price | 25 | IMG_2041.jpg, budget app notes.md, pension annual statement.txt, Screenshot 2025-10-03 at 08.41.55.png, car insurance renewal.txt |
| fund report q2.txt | Greenfield investor update | 27 | vaccination record.md, label printer driver.zip, misc notes.txt, untitled scan text.txt, plant monitor readme.md |
| home contents policy.txt | what my possessions in the flat are insured for | 56 | essay on attention.md, untitled scan text.txt, misc notes.txt, lentil dal.md, wifi and alarm codes.txt |
| home contents policy.txt | Cedar Mutual cover with accidental damage | 15 | misc notes.txt, IMG_4127.jpg, coffee machine descaling.txt, sourdough loaf.md, boiler repair note.txt |
| home office expenses.md | what I spent working from home, for the tax return | 13 | itinerary portugal.md, IMG_2041.jpg, misc notes.txt, pension annual statement.txt, essay on attention.md |
| home office expenses.md | list of the monitor arm, chair and share of electricity | 30 | Screenshot 2025-10-03 at 08.41.55.png, misc notes.txt, wifi and alarm codes.txt, plant monitor readme.md, budget app notes.md |
| hotel booking porto.txt | where we're staying in Porto | 16 | untitled scan text.txt, misc notes.txt, lentil dal.md, wifi and alarm codes.txt, print_0412.stl |
| hotel booking porto.txt | room with a river view and breakfast | 7 | message to landlord.txt, lentil dal.md, scan0007.png, email from sam about visit.txt, sourdough loaf.md |
| internet contract.txt | my broadband deal and how fast it is | 22 | router quick start.md, plant monitor readme.md, lamp dimmer firmware.zip, coffee machine descaling.txt, budget app notes.md |
| internet contract.txt | the welcome letter that says I have to give the router back | 33 | misc notes.txt, scan0007.png, email from sam about visit.txt, IMG_2041.jpg, wifi and alarm codes.txt |
| invoice monitor arm.md | bill for the screen mount that clamps to my desk | 32 | label printer driver.zip, plant monitor readme.md, router quick start.md, lamp dimmer firmware.zip, budget app notes.md |
| invoice monitor arm.md | the dual display bracket I can claim as a home-working expense | 21 | wifi and alarm codes.txt, drill safety and warranty.txt, lamp dimmer firmware.zip, budget app notes.md, label printer driver.zip |
| invoice monitor arm.md | August purchase of something to hold two monitors | 51 | IMG_2041.jpg, misc notes.txt, label printer driver.zip, untitled scan text.txt, Screenshot 2025-10-03 at 08.41.55.png |
| itinerary portugal.md | the plan for the June holiday, which nights where | 47 | misc notes.txt, email from sam about visit.txt, employment contract lumen.md, hotel booking porto.txt, team meeting notes.md |
| itinerary portugal.md | staying in Alfama and then Casa do Rio | 10 | train tickets porto.txt, hotel booking porto.txt, IMG_1876.jpg, email from sam about visit.txt, flight booking lisbon.txt |
| itinerary portugal.md | day by day plan of the trip | 36 | message to landlord.txt, pension annual statement.txt, tenancy agreement.md, boiler repair note.txt, payslip october.txt |
| kettle manual.zip | instructions for the BrewSwift | 9 | IMG_1876.jpg, lentil dal.md, itinerary portugal.md, wifi and alarm codes.txt, drill safety and warranty.txt |
| kettle manual.zip | how to descale with white vinegar | 18 | lentil dal.md, lemon drizzle cake.txt, IMG_1502.jpg, sourdough loaf.md, IMG_4127.jpg |
| lab notebook oscillators week 12.md | order parameter jumps near K = 1.6 | 7 | ring_v2.stl, scan_0007.stl, print_0412.stl, part_v3_final.stl, export.png |
| lab notebook oscillators week 12.md | my March notes on the threshold with a Lorentzian spread | 41 | misc notes.txt, Screenshot 2025-10-03 at 08.41.55.png, 2025 bank statement March.txt, pension annual statement.txt, IMG_2041.jpg |
| label printer driver.zip | the installer for the QL-700 | 9 | dishwasher manual.txt, kettle manual.zip, electricity bill september.txt, IMG_2041.jpg, receipt and manual blender.txt |
| lentil dal.md | something to cook on a weeknight that freezes well | 9 | misc notes.txt, message to landlord.txt, IMG_1502.jpg, IMG_2041.jpg, lemon drizzle cake.txt |
| letter from grandma.txt | the letter where she says the snowdrops are out | 8 | untitled scan text.txt, lentil dal.md, sourdough loaf.md, misc notes.txt, lemon drizzle cake.txt |
| letter from grandma.txt | Gran asking me to come for Sunday lunch | 8 | employment contract lumen.md, email from sam about visit.txt, birthday card from sister.txt, misc notes.txt, lentil dal.md |
| letter from grandma.txt | grandad working on the vegetable patch again | 32 | lemon drizzle cake.txt, lentil dal.md, sourdough loaf.md, IMG_1502.jpg, coffee machine descaling.txt |
| letter from tax office.txt | HMRC telling me my tax code changed | 11 | budget app notes.md, draft paper phase response.md, untitled scan text.txt, vaccination record.md, plant monitor readme.md |
| letter from tax office.txt | notice that my code is now 1257L because of a change in work benefits | 26 | Screenshot 2025-10-03 at 08.41.55.png, budget app notes.md, IMG_2041.jpg, draft paper phase response.md, claim physio costs.md |
| letter from tax office.txt | official letter about my income tax from June | 7 | pension annual statement.txt, travel insurance certificate.txt, tax return summary 2024.md, tenancy agreement.md, employment contract lumen.md |
| message to landlord.txt | what I wrote asking for the dripping tap to be fixed | 26 | misc notes.txt, essay on attention.md, untitled scan text.txt, IMG_2041.jpg, coffee machine descaling.txt |
| old us tax form.txt | my American tax return from when I lived in Boston | 30 | birthday card from sister.txt, pension annual statement.txt, travel insurance certificate.txt, employment contract lumen.md, tax return summary 2024.md |
| old us tax form.txt | the 1040 with my social security number | 39 | phone repair receipt.txt, travel insurance certificate.txt, pension annual statement.txt, employment contract lumen.md, performance review 2025.md |
| old us tax form.txt | federal return filed in April 2021 | 27 | travel insurance certificate.txt, tax return summary 2024.md, 2025 bank statement March.txt, phone repair receipt.txt, Screenshot 2025-10-03 at 08.41.55.png |
| order confirmation laptop.txt | order for the Aero 14 and a USB-C dock | 7 | train tickets porto.txt, dishwasher manual.txt, IMG_2041.jpg, kettle manual.zip, scan_0007.stl |
| order confirmation laptop.txt | the email confirming what I paid for my notebook PC | 35 | misc notes.txt, essay on attention.md, IMG_1502.jpg, email from sam about visit.txt, IMG_2041.jpg |
| paper kuramoto synchronization.txt | Strogatz's review of the onset of synchrony | 23 | draft paper phase response.md, vaccination record.md, lentil dal.md, performance review 2025.md, budget app notes.md |
| paper kuramoto synchronization.txt | coupled limit-cycle oscillators and the phase transition | 24 | draft paper phase response.md, router quick start.md, budget app notes.md, plant monitor readme.md, lamp dimmer firmware.zip |
| paper kuramoto synchronization.txt | the Physica D article I downloaded for my literature review | 12 | essay on attention.md, lentil dal.md, IMG_1502.jpg, lemon drizzle cake.txt, untitled scan text.txt |
| part_v3_final.stl | the L-shaped bracket I designed in 3 mm plate | 11 | ring_v2.stl, receipt and manual blender.txt, export.png, boiler repair note.txt, scan_0007.stl |
| part_v3_final.stl | 3D model of a right-angle mounting bracket | 10 | router quick start.md, lamp dimmer firmware.zip, misc notes.txt, dishwasher manual.txt, kettle manual.zip |
| payslip october.txt | how much National Insurance and pension came out of my salary | 10 | pension annual statement.txt, vaccination record.md, tax return summary 2024.md, letter from tax office.txt, travel insurance certificate.txt |
| pension annual statement.txt | how much is in my retirement pot | 32 | lentil dal.md, coffee machine descaling.txt, sourdough loaf.md, lemon drizzle cake.txt, kettle manual.zip |
| pension annual statement.txt | yearly letter from Harbour about contributions from me and my employer | 12 | dishwasher manual.txt, untitled scan text.txt, kettle manual.zip, birthday card from sister.txt, lentil dal.md |
| performance review 2025.md | what Jordan said about my work this summer | 26 | misc notes.txt, draft paper phase response.md, lentil dal.md, essay on attention.md, vaccination record.md |
| phone repair receipt.txt | when my Pixel screen was fixed | 31 | router quick start.md, kettle manual.zip, lentil dal.md, misc notes.txt, label printer driver.zip |
| phone repair receipt.txt | cost of replacing a cracked phone display | 22 | untitled scan text.txt, misc notes.txt, label printer driver.zip, router quick start.md, lamp dimmer firmware.zip |
| physio appointment.txt | when my back follow-up session is booked | 22 | misc notes.txt, lentil dal.md, essay on attention.md, team meeting notes.md, draft paper phase response.md |
| physio appointment.txt | Riverside booking with Sam Okafor | 49 | sourdough loaf.md, lentil dal.md, misc notes.txt, lemon drizzle cake.txt, untitled scan text.txt |
| plant monitor readme.md | ESP32 firmware that posts readings to a web page | 9 | IMG_1502.jpg, email from sam about visit.txt, untitled scan text.txt, label printer driver.zip, order confirmation laptop.txt |
| print_0412.stl | the square tray I 3D printed | 8 | untitled scan text.txt, lamp dimmer firmware.zip, coffee machine descaling.txt, misc notes.txt, label printer driver.zip |
| receipt and manual blender.txt | proof of purchase for the PowerBlend and its quick guide | 14 | IMG_2041.jpg, drill safety and warranty.txt, router quick start.md, lamp dimmer firmware.zip, label printer driver.zip |
| receipt and manual blender.txt | Kitchen Plus purchase with a two year guarantee | 6 | IMG_2041.jpg, misc notes.txt, Screenshot 2025-10-03 at 08.41.55.png, label printer driver.zip, pension annual statement.txt |
| receipt desk chair.txt | what I paid for my office seat | 21 | misc notes.txt, untitled scan text.txt, essay on attention.md, wifi and alarm codes.txt, lentil dal.md |
| receipt desk chair.txt | Furnish & Co till slip with the floor protector | 32 | misc notes.txt, untitled scan text.txt, sourdough loaf.md, label printer driver.zip, lentil dal.md |
| receipt desk chair.txt | proof of purchase for the ergonomic chair, five year frame guarantee | 13 | IMG_2041.jpg, 2025 bank statement March.txt, Screenshot 2025-10-03 at 08.41.55.png, pension annual statement.txt, receipt and manual blender.txt |
| referral letter for physio.txt | the GP's letter sending me to have my back treated | 23 | misc notes.txt, untitled scan text.txt, lentil dal.md, draft paper phase response.md, wifi and alarm codes.txt |
| referral letter for physio.txt | doctor's note I can use for the health insurance claim | 8 | vaccination record.md, travel insurance certificate.txt, Screenshot 2025-10-03 at 08.41.55.png, car insurance renewal.txt, drill safety and warranty.txt |
| referral letter for physio.txt | six weeks of lower back pain, no red flags | 60 | sourdough loaf.md, car insurance renewal.txt, phone repair receipt.txt, IMG_4127.jpg, tax return summary 2024.md |
| rfc 9110 http semantics notes.md | which section covers range requests | 52 | budget app notes.md, draft paper phase response.md, misc notes.txt, drill safety and warranty.txt, wifi and alarm codes.txt |
| rfc 9110 http semantics notes.md | IETF standard from 2022 | 56 | misc notes.txt, performance review 2025.md, electricity bill september.txt, untitled scan text.txt, internet contract.txt |
| ring_v2.stl | the finger ring model for the printer | 18 | label printer driver.zip, lamp dimmer firmware.zip, kettle manual.zip, dishwasher manual.txt, untitled scan text.txt |
| router quick start.md | what it means when the light on the Hub 3 is red | 7 | misc notes.txt, scan_0007.stl, part_v3_final.stl, IMG_1876.jpg, print_0412.stl |
| router quick start.md | which cable goes into the WAN port | 10 | plant monitor readme.md, wifi and alarm codes.txt, untitled scan text.txt, misc notes.txt, lentil dal.md |
| scan0007.png | the letter thanking me for the scarf | 42 | lentil dal.md, sourdough loaf.md, untitled scan text.txt, coffee machine descaling.txt, essay on attention.md |
| scan0007.png | scanned note from my friend in Hebden Bridge | 33 | email from sam about visit.txt, label printer driver.zip, essay on attention.md, dishwasher manual.txt, lentil dal.md |
| scan_0007.stl | the little figurine with a round body and a head | 13 | sourdough loaf.md, lemon drizzle cake.txt, coffee machine descaling.txt, IMG_4127.jpg, misc notes.txt |
| scan_0007.stl | 3D printable statuette on a base | 10 | IMG_4127.jpg, coffee machine descaling.txt, lemon drizzle cake.txt, phone repair receipt.txt, untitled scan text.txt |
| short story draft.md | Mara climbing the stairs to the lamp | 56 | misc notes.txt, message to landlord.txt, email from sam about visit.txt, itinerary portugal.md, hotel booking porto.txt |
| talk slides outline.md | plan for my 15 minute presentation to the group | 18 | misc notes.txt, travel insurance certificate.txt, phone repair receipt.txt, birthday card from sister.txt, pension annual statement.txt |
| talk slides outline.md | outline of what I'm going to say at group meeting | 30 | misc notes.txt, essay on attention.md, lentil dal.md, untitled scan text.txt, team meeting notes.md |
| tax return summary 2024.md | self assessment figures for employment income and interest | 42 | vaccination record.md, essay on attention.md, drill safety and warranty.txt, wifi and alarm codes.txt, budget app notes.md |
| tax return summary 2024.md | the overview of my filed return from January | 14 | misc notes.txt, untitled scan text.txt, claim physio costs.md, travel insurance certificate.txt, draft paper phase response.md |
| team meeting notes.md | interviews for the junior analyst role | 9 | essay on attention.md, email from sam about visit.txt, untitled scan text.txt, misc notes.txt, label printer driver.zip |
| tenancy agreement.md | the lease for my flat | 11 | untitled scan text.txt, IMG_2041.jpg, misc notes.txt, kettle manual.zip, boiler repair note.txt |
| tenancy agreement.md | my rental contract with Oakfield Lettings and the deposit amount | 12 | pension annual statement.txt, travel insurance certificate.txt, budget app notes.md, employment contract lumen.md, drill safety and warranty.txt |
| tenancy agreement.md | how much rent I pay and when the term started | 17 | pension annual statement.txt, untitled scan text.txt, vaccination record.md, old us tax form.txt, lentil dal.md |
| travel insurance certificate.txt | proof I was covered for the Portugal trip | 11 | misc notes.txt, itinerary portugal.md, hotel booking porto.txt, IMG_2041.jpg, Screenshot 2025-10-03 at 08.41.55.png |
| travel insurance claim lost bag.md | the claim for my suitcase that went missing on the way home | 20 | essay on attention.md, message to landlord.txt, boiler repair note.txt, misc notes.txt, letter from grandma.txt |
| travel insurance claim lost bag.md | LIS-88213 | 63 | print_0412.stl, part_v3_final.stl, scan_0007.stl, ring_v2.stl, cv alex morgan.md |
| untitled scan text.txt | page from a contract about custody of an item in schedule B | 13 | Screenshot 2025-10-03 at 08.41.55.png, email from sam about visit.txt, misc notes.txt, IMG_2041.jpg, budget app notes.md |
| untitled scan text.txt | a stray second page of some legal agreement | 7 | Screenshot 2025-10-03 at 08.41.55.png, pension annual statement.txt, 2025 bank statement March.txt, travel insurance claim lost bag.md, card replacement letter.txt |
| vaccination record.md | which jabs I've had and when | 6 | misc notes.txt, essay on attention.md, lentil dal.md, sourdough loaf.md, untitled scan text.txt |
| wifi and alarm codes.txt | when the bins go out and how much to feed the cat | 8 | lentil dal.md, lemon drizzle cake.txt, sourdough loaf.md, vaccination record.md, IMG_1502.jpg |

### Words and meaning, apple-nl-sentence-en

| Item | Description | Place | Came back first |
| --- | --- | --- | --- |
| 2025 bank statement March.txt | my current account transactions for one month in spring | 25 | tenancy agreement.md, employment contract lumen.md, payslip october.txt, Screenshot 2025-10-03 at 08.41.55.png, prescription amoxicillin.txt |
| 2025 bank statement March.txt | Northbridge account balance after moving money into savings | 15 | Screenshot 2025-10-03 at 08.41.55.png, pension annual statement.txt, payslip october.txt, message to landlord.txt, tax return summary 2024.md |
| IMG_3310.jpg | the whiteboard from sprint planning at work | 7 | sourdough loaf.md, employment contract lumen.md, payslip october.txt, cv alex morgan.md, team meeting notes.md |
| IMG_3310.jpg | to do, doing and done columns from the October meeting | 11 | team meeting notes.md, payslip october.txt, hotel booking porto.txt, electricity bill september.txt, message to landlord.txt |
| Screenshot 2025-08-28 at 09.14.03.png | the email from the letting agent about the rent going up | 6 | email from sam about visit.txt, misc notes.txt, travel insurance certificate.txt, order confirmation laptop.txt, building notice water off.txt |
| car insurance renewal.txt | how much it costs to insure the Fiesta next year | 11 | performance review 2025.md, pension annual statement.txt, electricity bill september.txt, claim physio costs.md, team meeting notes.md |
| car insurance renewal.txt | letter about the no claims discount from Lighthouse | 11 | travel insurance claim lost bag.md, letter from tax office.txt, letter from grandma.txt, referral letter for physio.txt, email from sam about visit.txt |
| card replacement letter.txt | Northbridge sending a replacement card that expires 09/29 | 6 | phone repair receipt.txt, internet contract.txt, boiler repair note.txt, IMG_2041.jpg, Screenshot 2025-08-28 at 09.14.03.png |
| claim physio costs.md | health cover reimbursement for three sessions | 7 | travel insurance certificate.txt, vaccination record.md, physio appointment.txt, prescription amoxicillin.txt, referral letter for physio.txt |
| coffee machine descaling.txt | how to get the limescale out of the espresso maker | 10 | draft paper phase response.md, itinerary portugal.md, cell biology review.txt, hotel booking porto.txt, flight booking lisbon.txt |
| cv alex morgan.md | my résumé | 43 | sourdough loaf.md, darwin origin chapter 4 notes.md, talk slides outline.md, untitled scan text.txt, lentil dal.md |
| cv alex morgan.md | what I send when applying for analyst jobs | 9 | essay on attention.md, drill safety and warranty.txt, kettle manual.zip, coffee machine descaling.txt, employment contract lumen.md |
| drill safety and warranty.txt | register within 28 days for three years cover | 10 | receipt and manual blender.txt, employment contract lumen.md, receipt desk chair.txt, phone repair receipt.txt, IMG_2041.jpg |
| electricity bill september.txt | Brightspark energy invoice paid by direct debit | 6 | IMG_2041.jpg, claim physio costs.md, tax return summary 2024.md, employment contract lumen.md, payslip october.txt |
| export.png | deep sleep 15 minutes between readings | 14 | talk slides outline.md, physio appointment.txt, lentil dal.md, lemon drizzle cake.txt, dataset description.md |
| flight booking lisbon.txt | my plane tickets to Portugal | 23 | itinerary portugal.md, travel insurance certificate.txt, train tickets porto.txt, sourdough loaf.md, talk slides outline.md |
| home contents policy.txt | what my possessions in the flat are insured for | 9 | essay on attention.md, sourdough loaf.md, message to landlord.txt, travel insurance certificate.txt, IMG_1502.jpg |
| internet contract.txt | the welcome letter that says I have to give the router back | 9 | scan0007.png, misc notes.txt, email from sam about visit.txt, router quick start.md, card replacement letter.txt |
| invoice monitor arm.md | August purchase of something to hold two monitors | 7 | IMG_2041.jpg, home office expenses.md, drill safety and warranty.txt, order confirmation laptop.txt, coffee machine descaling.txt |
| itinerary portugal.md | the plan for the June holiday, which nights where | 7 | employment contract lumen.md, hotel booking porto.txt, pension annual statement.txt, flight booking lisbon.txt, travel insurance certificate.txt |
| itinerary portugal.md | day by day plan of the trip | 8 | pension annual statement.txt, receipt desk chair.txt, drill safety and warranty.txt, employment contract lumen.md, conference registration.txt |
| label printer driver.zip | the installer for the QL-700 | 9 | dishwasher manual.txt, kettle manual.zip, electricity bill september.txt, IMG_2041.jpg, receipt and manual blender.txt |
| old us tax form.txt | the 1040 with my social security number | 41 | payslip october.txt, talk slides outline.md, sourdough loaf.md, darwin origin chapter 4 notes.md, phone repair receipt.txt |
| old us tax form.txt | federal return filed in April 2021 | 7 | tax return summary 2024.md, receipt and manual blender.txt, pension annual statement.txt, home office expenses.md, letter from tax office.txt |
| performance review 2025.md | what Jordan said about my work this summer | 6 | essay on attention.md, misc notes.txt, sourdough loaf.md, team meeting notes.md, email from sam about visit.txt |
| receipt desk chair.txt | what I paid for my office seat | 35 | essay on attention.md, home office expenses.md, letter from tax office.txt, sourdough loaf.md, tax return summary 2024.md |
| referral letter for physio.txt | six weeks of lower back pain, no red flags | 11 | physio appointment.txt, car insurance renewal.txt, lentil dal.md, payslip october.txt, boiler repair note.txt |
| scan0007.png | the letter thanking me for the scarf | 6 | letter from grandma.txt, label printer driver.zip, email from sam about visit.txt, birthday card from sister.txt, referral letter for physio.txt |
| scan0007.png | scanned note from my friend in Hebden Bridge | 6 | email from sam about visit.txt, misc notes.txt, sourdough loaf.md, budget app notes.md, darwin origin chapter 4 notes.md |
| scan_0007.stl | the little figurine with a round body and a head | 15 | referral letter for physio.txt, performance review 2025.md, sourdough loaf.md, lemon drizzle cake.txt, coffee machine descaling.txt |
| train tickets porto.txt | the rail journey from Lisbon up north | 13 | flight booking lisbon.txt, itinerary portugal.md, email from sam about visit.txt, travel insurance certificate.txt, internet contract.txt |
| vaccination record.md | which jabs I've had and when | 14 | coffee machine descaling.txt, kettle manual.zip, short story draft.md, letter from grandma.txt, drill safety and warranty.txt |
| wifi and alarm codes.txt | instructions I leave for the house-sitter | 7 | drill safety and warranty.txt, kettle manual.zip, IMG_1502.jpg, internet contract.txt, scan0007.png |
| wifi and alarm codes.txt | when the bins go out and how much to feed the cat | 21 | letter from grandma.txt, coffee machine descaling.txt, itinerary portugal.md, draft paper phase response.md, scan0007.png |
| wifi and alarm codes.txt | the flat's password for guests | 10 | message to landlord.txt, tenancy agreement.md, IMG_1502.jpg, Screenshot 2025-08-28 at 09.14.03.png, home contents policy.txt |

### Meaning alone, embeddinggemma-300m

| Item | Description | Place | Came back first |
| --- | --- | --- | --- |
| 2025 bank statement March.txt | the statement that shows the rent going out and my salary coming in | 14 | Screenshot 2025-08-28 at 09.14.03.png, tenancy agreement.md, payslip october.txt, tax return summary 2024.md, home office expenses.md |
| 2025 bank statement March.txt | Northbridge account balance after moving money into savings | 12 | Screenshot 2025-10-03 at 08.41.55.png, pension annual statement.txt, tax return summary 2024.md, fund report q2.txt, electricity bill september.txt |
| conference registration.txt | the September meeting I paid 280 for | 10 | talk slides outline.md, electricity bill september.txt, team meeting notes.md, email from sam about visit.txt, claim physio costs.md |
| wifi and alarm codes.txt | instructions I leave for the house-sitter | 15 | dishwasher manual.txt, scan0007.png, boiler repair note.txt, message to landlord.txt, coffee machine descaling.txt |
| wifi and alarm codes.txt | when the bins go out and how much to feed the cat | 50 | kettle manual.zip, lemon drizzle cake.txt, IMG_4127.jpg, scan0007.png, dishwasher manual.txt |

### Words and meaning, embeddinggemma-300m

| Item | Description | Place | Came back first |
| --- | --- | --- | --- |
| 2025 bank statement March.txt | my current account transactions for one month in spring | 17 | Screenshot 2025-10-03 at 08.41.55.png, dataset description.md, tenancy agreement.md, home contents policy.txt, payslip october.txt |
| 2025 bank statement March.txt | Northbridge account balance after moving money into savings | 13 | Screenshot 2025-10-03 at 08.41.55.png, pension annual statement.txt, tax return summary 2024.md, payslip october.txt, Screenshot 2025-08-28 at 09.14.03.png |
| coffee machine descaling.txt | how to get the limescale out of the espresso maker | 8 | itinerary portugal.md, hotel booking porto.txt, draft paper phase response.md, scan0007.png, cell biology review.txt |
| flight booking lisbon.txt | my plane tickets to Portugal | 7 | train tickets porto.txt, itinerary portugal.md, travel insurance certificate.txt, darwin origin chapter 4 notes.md, talk slides outline.md |
| receipt desk chair.txt | what I paid for my office seat | 18 | home office expenses.md, tax return summary 2024.md, invoice monitor arm.md, order confirmation laptop.txt, letter from tax office.txt |
| train tickets porto.txt | the rail journey from Lisbon up north | 13 | itinerary portugal.md, flight booking lisbon.txt, travel insurance claim lost bag.md, travel insurance certificate.txt, email from sam about visit.txt |
| vaccination record.md | which jabs I've had and when | 9 | letter from grandma.txt, scan0007.png, drill safety and warranty.txt, coffee machine descaling.txt, short story draft.md |
| wifi and alarm codes.txt | instructions I leave for the house-sitter | 16 | kettle manual.zip, scan0007.png, IMG_1502.jpg, drill safety and warranty.txt, internet contract.txt |
| wifi and alarm codes.txt | when the bins go out and how much to feed the cat | 55 | letter from grandma.txt, coffee machine descaling.txt, kettle manual.zip, scan0007.png, IMG_4127.jpg |
| wifi and alarm codes.txt | the flat's password for guests | 6 | message to landlord.txt, Screenshot 2025-08-28 at 09.14.03.png, tenancy agreement.md, home contents policy.txt, IMG_1502.jpg |

### Meaning alone, qwen3-embedding-0.6b

| Item | Description | Place | Came back first |
| --- | --- | --- | --- |
| 2025 bank statement March.txt | the statement that shows the rent going out and my salary coming in | 24 | Screenshot 2025-08-28 at 09.14.03.png, employment contract lumen.md, tax return summary 2024.md, pension annual statement.txt, message to landlord.txt |
| sourdough loaf.md | my note that 330 g water works better | 48 | building notice water off.txt, performance review 2025.md, boiler repair note.txt, team meeting notes.md, dataset description.md |
| wifi and alarm codes.txt | instructions I leave for the house-sitter | 12 | letter from grandma.txt, message to landlord.txt, scan0007.png, kettle manual.zip, Screenshot 2025-08-28 at 09.14.03.png |
| wifi and alarm codes.txt | when the bins go out and how much to feed the cat | 24 | IMG_4127.jpg, budget app notes.md, letter from grandma.txt, coffee machine descaling.txt, building notice water off.txt |
| wifi and alarm codes.txt | the flat's password for guests | 8 | tenancy agreement.md, Screenshot 2025-08-28 at 09.14.03.png, message to landlord.txt, card replacement letter.txt, hotel booking porto.txt |

### Words and meaning, qwen3-embedding-0.6b

| Item | Description | Place | Came back first |
| --- | --- | --- | --- |
| 2025 bank statement March.txt | my current account transactions for one month in spring | 17 | Screenshot 2025-10-03 at 08.41.55.png, internet contract.txt, tenancy agreement.md, payslip october.txt, dataset description.md |
| 2025 bank statement March.txt | the statement that shows the rent going out and my salary coming in | 6 | Screenshot 2025-08-28 at 09.14.03.png, employment contract lumen.md, pension annual statement.txt, tenancy agreement.md, essay on attention.md |
| 2025 bank statement March.txt | Northbridge account balance after moving money into savings | 8 | Screenshot 2025-10-03 at 08.41.55.png, tax return summary 2024.md, payslip october.txt, Screenshot 2025-08-28 at 09.14.03.png, pension annual statement.txt |
| coffee machine descaling.txt | how to get the limescale out of the espresso maker | 8 | itinerary portugal.md, scan0007.png, draft paper phase response.md, flight booking lisbon.txt, hotel booking porto.txt |
| cv alex morgan.md | my résumé | 7 | talk slides outline.md, darwin origin chapter 4 notes.md, sourdough loaf.md, performance review 2025.md, employment contract lumen.md |
| flight booking lisbon.txt | my plane tickets to Portugal | 8 | train tickets porto.txt, itinerary portugal.md, travel insurance certificate.txt, darwin origin chapter 4 notes.md, talk slides outline.md |
| old us tax form.txt | the 1040 with my social security number | 6 | payslip october.txt, talk slides outline.md, sourdough loaf.md, darwin origin chapter 4 notes.md, tax return summary 2024.md |
| receipt desk chair.txt | what I paid for my office seat | 18 | home office expenses.md, tax return summary 2024.md, order confirmation laptop.txt, letter from tax office.txt, essay on attention.md |
| scan_0007.stl | the little figurine with a round body and a head | 7 | referral letter for physio.txt, performance review 2025.md, IMG_4127.jpg, part_v3_final.stl, print_0412.stl |
| sourdough loaf.md | my note that 330 g water works better | 12 | building notice water off.txt, team meeting notes.md, boiler repair note.txt, dataset description.md, budget app notes.md |
| train tickets porto.txt | the rail journey from Lisbon up north | 13 | flight booking lisbon.txt, itinerary portugal.md, travel insurance claim lost bag.md, travel insurance certificate.txt, email from sam about visit.txt |
| vaccination record.md | which jabs I've had and when | 9 | scan0007.png, drill safety and warranty.txt, internet contract.txt, coffee machine descaling.txt, short story draft.md |
| wifi and alarm codes.txt | instructions I leave for the house-sitter | 15 | kettle manual.zip, scan0007.png, internet contract.txt, drill safety and warranty.txt, IMG_1502.jpg |
| wifi and alarm codes.txt | when the bins go out and how much to feed the cat | 32 | letter from grandma.txt, coffee machine descaling.txt, IMG_4127.jpg, internet contract.txt, kettle manual.zip |
| wifi and alarm codes.txt | the flat's password for guests | 10 | message to landlord.txt, Screenshot 2025-08-28 at 09.14.03.png, tenancy agreement.md, home contents policy.txt, IMG_1502.jpg |
