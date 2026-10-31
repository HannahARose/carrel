# Benchmark report

2026-10-30 on macos aarch64, carrel 0.1.0.

## Summary

| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model | 89% | 89% | 11% | 75% | 83% | 3% | 0 | 77% | 82% | 60 | 6.9 | no: valid output 89%, routing 75% |
| llama/gemma-4-e4b.gguf | 100% | 100% | 0% | 94% | 83% | 2% | 0 | 58% | 82% | 16 | 17.9 | yes |
| llama/granite-4.2-3b.gguf | 86% | 82% | 0% | 72% | 78% | 2% | 0 | 62% | 77% | 68 | 18.3 | no: valid output 86%, routing 72% |
| llama/granite-4.2-8b.gguf | 95% | 91% | 0% | 80% | 85% | 3% | 0 | 45% | 68% | 29 | 32.3 | no: 32.3 s an item |
| llama/qwen3.5-4b.gguf | 97% | 94% | 0% | 87% | 95% | 2% | 0 | 63% | 86% | 35 | 21.0 | yes |
| llama/qwen3.5-9b.gguf | 100% | 98% | 0% | 96% | 72% | 2% | 0 | 69% | 85% | 29 | 33.2 | no: 33.2 s an item |

## apple/Apple Foundation Model (provider 0.1.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| 2025 bank statement March.txt | Finances | ✓ Finances | restricted (exact) | 2025 bank statement March | A bank statement showing financial transactions for March 2025, including income, expenses, and closing balance. |
| amplifier build log.txt | Projects | ✓ Projects | personal (exact) | Headphone amplifier build log | A build log for a headphone amplifier, detailing steps like etching the board, drilling holes, soldering components, and adjusting wiring after initial testing. |
| birthday card from sister.txt | Letters | ✓ Letters | personal (exact) | birthday card from sister | A personal message from a sister celebrating a birthday, including a friendly note about chess and a future dinner invitation. |
| blood test results.txt | Health | ✓ Health | sensitive (exact) | blood test results |  |
| boiler repair note.txt | Home | ✓ Home | personal (exact) | Boiler repair note | Engineer's note about boiler pressure loss, valve replacement, system refill, and weekly gauge checks. |
| budget app notes.md | Projects | ✓ Projects | personal (exact) | Budget app: design notes | A small terminal app to categorise bank exports using CSV imports and payee guessing, with rules stored in a TOML file and monthly summaries in a table format. |
| building notice water off.txt | Home | ✓ Home | restricted (over) | Water supply disruption notice | Notice to residents about water supply disruption due to valve replacement. |
| car insurance renewal.txt | Insurance | ✓ Insurance | sensitive (exact) | Car insurance renewal | Car insurance policy renewal with updated premium and no claims discount. |
| card replacement letter.txt | Finances | ✓ Finances | restricted (exact) | Replacement debit card | A letter from Northbridge Bank providing a replacement debit card and instructions to sign it and discard the old card. |
| cell biology review.txt | Reading | ✗ Holding | sensitive (over) | Circadian clocks in single cells: a review | A review of circadian clocks in single cells, focusing on transcriptional feedback loops, single-cell imaging variability, tissue coupling, light and feeding-induced resets. |
| claim physio costs.md | Insurance | ✗ Holding | sensitive (exact) | physio costs | A submission for reimbursement of physiotherapy sessions for lower back pain, including attached invoices and a claim reference note. |
| coffee machine descaling.txt | Manuals | ✓ Manuals | personal (exact) | coffee machine descaling instructions | Instructions for descaling a Barista Mini coffee machine, including steps for cleaning the tank, drip tray, and buttons. |
| conference registration.txt | Research or Work or Travel | ½ Holding | sensitive (over) | conference registration | Confirmation of registration for a conference event including participant details, poster title, and fee information. |
| cv alex morgan.md | Work | ✓ Work | personal (exact) | Alex Morgan: CV | A CV document detailing work experience and education for Alex Morgan, including roles at Lumen Analytics and Riverside Council, and a Master's degree in Applied Statistics. |
| darwin origin chapter 4 notes.md | Reading | ✗ Holding | personal (exact) | darwin origin chapter 4 notes | Summary of Darwin's chapter on natural selection, including key ideas on variation, sexual selection, and the diagram of divergence. |
| dataset description.md | Research | ✓ Research | personal (exact) | Cell imaging dataset | A dataset of cell imaging recordings from 48 wells, with one image taken every 30 minutes for 5 days, stored as TIFF stacks and summarized in a CSV with columns for well, cell, time, and intensity. |
| dishwasher manual.txt | Manuals | ✓ Manuals | personal (exact) | QuietClean 45 slimline dishwasher: user manual | User guide for a dishwasher model detailing setup steps, program options, and maintenance instructions. |
| draft paper phase response.md | Research | ✓ Research | personal (exact) | Phase response curves of weakly coupled cells (draft) | A draft paper discussing phase response curves in weakly coupled cells, including methods and co-authors. |
| drill safety and warranty.txt | Manuals | ✓ Manuals | personal (exact) | Drill safety and warranty | Safety instructions and warranty for a PowerLine 18V cordless drill. |
| electricity bill september.txt | Home | ✓ Home | personal (exact) | electricity bill september | Electricity bill for the period 1 to 30 September 2025 from Brightspark Energy, showing usage, cost, and payment details. |
| email from sam about visit.txt | Letters | ✗ Travel | personal (exact) | email from sam about visit.txt | An email from Sam about a train booking and a walk invitation. |
| employment contract lumen.md | Work | ✓ Work | sensitive (exact) | employment contract | A formal agreement outlining employment terms including job title, salary, hours, holiday, notice period and employer details. |
| essay on attention.md | Reading | ✗ Projects | personal (exact) | essay on attention | An essay about attention as a habit, not a spotlight, with the idea that we become what we keep returning to. |
| flight booking lisbon.txt | Travel | ✓ Travel | personal (exact) | Flight booking lisbon | Flight booking confirmation for a round trip from London Gatwick to Lisbon, including departure and return times and cabin bag inclusion. |
| fund report q2.txt | Finances | ✗ Holding | personal (exact) | fund report q2.txt | Quarterly report for investors on the Global Equity Index fund, detailing holdings, unit price, cost, and index performance. |
| home contents policy.txt | Insurance | ✓ Insurance | personal (under) | home contents policy | Home contents insurance policy for Flat 4, 17 Oakfield Road, covering accidental damage and a policy schedule from 1 February 2025 to 31 January 2026. |
| home office expenses.md | Finances or Receipts | ½ Holding | restricted (over) | Home-working expenses for the tax return | List of home-working expenses for tax return, including monitor arm, desk chair, and electricity share. |
| hotel booking porto.txt | Travel | ✓ Travel | personal (exact) | hotel booking porto | Hotel booking for a double room with river view at Casa do Rio, Porto, including breakfast, with check-in on 18 June 2025 and check-out on 21 June 2025. |
| internet contract.txt | Home | ✗ Holding | restricted (over) | Fibrelink broadband contract | 24-month broadband contract with Fibrelink, including speed, price, and router return instructions. |
| invoice monitor arm.md | Receipts | ✗ Holding | personal (exact) | Invoice for Dual Monitor Arm | A text file listing an invoice for a dual monitor arm with a clamp mount, including price, VAT, and payment details. |
| itinerary portugal.md | Travel | ✓ Travel | personal (exact) | itinerary portugal | Itinerary for a trip to Portugal in June 2025, including travel dates, accommodations, and transport plans. |
| lab notebook oscillators week 12.md | Research | ✓ Research | personal (exact) | Lab notebook, coupled-oscillator simulation | A lab notebook entry detailing a coupled-oscillator simulation with N = 500 and K from 0 to 4, noting a sharp rise in order parameter near K = 1.6 matching Lorentzian theory, with a next step to add noise and check threshold movement. |
| lemon drizzle cake.txt | Recipes | ✓ Recipes | personal (exact) | lemon drizzle cake | Recipe for lemon drizzle cake with ingredients and baking instructions. |
| lentil dal.md | Recipes | ✓ Recipes | personal (exact) | red lentil dal | A recipe for red lentil dal for four servings, including ingredients and preparation steps. The dish freezes well. |
| letter from grandma.txt | Letters | ✓ Letters | personal (exact) | letter from grandma | A personal letter from a grandparent to a family member, discussing the weather, a shared photo, and an invitation to a meal. |
| letter from tax office.txt | Finances | ✓ Finances | restricted (over) | letter from tax office | A letter from the tax office informing about a change in tax code due to employer-reported benefit changes. |
| message to landlord.txt | Home | ✓ Home | personal (exact) | Message to landlord | A message requesting a visit to fix a dripping kitchen tap and a cracked bathroom window, specifying availability after 4 on weekdays next week. |
| misc notes.txt | Holding | ✓ Holding | personal (exact) | misc notes | A list of tasks and questions including a call-back, a number of items marked 3 x AA, and a question about a film mentioned by Priya. |
| old us tax form.txt | Finances | ✗ Holding | restricted (exact) | U.S. Individual Income Tax Return 2020 | A completed tax form for the year worked in Boston, showing total income and filing status. |
| order confirmation laptop.txt | Receipts | ✓ Receipts | personal (exact) | order confirmation laptop | Order confirmation for a laptop and a USB-C dock, including delivery and payment details. |
| paper kuramoto synchronization.txt | Reading | ✗ Holding | personal (exact) | paper kuramoto synchronization.txt | A scientific paper discussing the Kuramoto model and the synchronization of coupled oscillators in populations. |
| payslip october.txt | Work | ✓ Work | restricted (over) | payslip october | A payslip showing pay details including basic pay, tax, national insurance, pension and net pay. |
| pension annual statement.txt | Finances | ✓ Finances | restricted (over) | pension annual statement | Annual benefit statement for a personal pension investment in the Harbour Balanced Growth fund, showing contributions and projected annual pension amount at 67. |
| performance review 2025.md | Work | ✓ Work | personal (exact) | performance review 2025 | A mid-year review document detailing strengths, development tasks, and performance rating for a data-related role. |
| phone repair receipt.txt | Receipts | ✓ Receipts | personal (exact) | phone repair receipt | Receipt for phone repairs including screen replacement and screen protector. |
| physio appointment.txt | Health | ✓ Health | personal (under) | physio appointment | An appointment confirmation for a follow-up physiotherapy session regarding lower back pain. |
| plant monitor readme.md | Projects | ✓ Projects | personal (exact) | plant monitor readme.md | A soil moisture monitor for houseplants: an ESP32 reads a capacitive sensor every 10 minutes and posts the reading to a small web page. |
| prescription amoxicillin.txt | Health | ✓ Health | sensitive (exact) | prescription amoxicillin |  |
| receipt and manual blender.txt | Receipts or Manuals | ✓ Manuals | personal (exact) | PowerBlend 600 blender | A receipt and manual for a PowerBlend 600 blender, including instructions for use and a two-year guarantee. |
| receipt desk chair.txt | Receipts | ✓ Receipts | personal (exact) | Desk chair receipt | A receipt for a desk chair and floor protector from FURNISH & CO., showing a total cost of 161.50 with a 5 year frame guarantee and 30 day return policy. |
| referral letter for physio.txt | Health | ✓ Health | sensitive (exact) | Physiotherapy referral for back pain | A letter requesting a physiotherapy appointment for mechanical lower back pain lasting six weeks, with no red-flag symptoms; mentions ibuprofen has helped slightly and may be used for an insurance claim. |
| rfc 9110 http semantics notes.md | Reading | ✗ Holding | personal (exact) | RFC 9110: HTTP Semantics | A technical document defining methods, status codes and headers shared by every version of HTTP. |
| router quick start.md | Manuals | ✓ Manuals | personal (exact) | router quick start | A quick start guide for setting up the Fibrelink Hub 3, including connection steps, power instructions, and network joining. |
| short story draft.md | Projects | ✗ Holding | personal (exact) | The Lighthouse Keeper's Daughter (draft 2) | A draft of a short story titled 'The Lighthouse Keeper's Daughter', with edits noted for the opening paragraph and a change to the storm's timing. |
| sourdough loaf.md | Recipes | ✓ Recipes | personal (exact) | sourdough loaf | Recipe for sourdough loaf with ingredients and baking instructions. |
| talk slides outline.md | Research | ✗ Work | personal (exact) | Group meeting talk outline | Outline for a group meeting presentation on tissue synchronization, a model threshold, simulations comparing theory and data, and next steps. |
| tax return summary 2024.md | Finances | ✓ Finances | sensitive (exact) | tax return summary 2024 | A summary of tax income and payments for the 2023 to 2024 tax year, showing no balance to pay by January 31. |
| team meeting notes.md | Work | ✓ Work | personal (exact) | team meeting notes | Notes from a team meeting about a churn model, warehouse migration, and hiring for a junior analyst role. |
| tenancy agreement.md | Home | ✗ Holding | sensitive (exact) | Assured shorthold tenancy agreement | A written agreement for a 12-month tenancy starting from 1 August 2023, followed by a month-to-month arrangement, detailing rent, deposit, and terms between landlord and tenant. |
| train tickets porto.txt | Travel | ✓ Travel | personal (exact) | Train tickets porto | Train ticket for a journey from Lisboa Santa Apolonia to Porto Campanha on 18 June 2025, departing at 09:39 and arriving at 12:24. Coach 4, seat 61. |
| travel insurance certificate.txt | Insurance | ✓ Insurance | sensitive (exact) | Travel Insurance Certificate | Travel insurance certificate for a single trip to Portugal from 14 to 24 June 2025, covering medical expenses up to 5,000,000 and cancellation up to 3,000. |
| travel insurance claim lost bag.md | Insurance | ½ Holding | sensitive (exact) | Lost luggage claim | Claim for lost bag containing clothes and a camera, with airline report reference and receipts for the camera. |
| untitled scan text.txt | Holding | ✓ Holding | sensitive (over) | untitled scan text.txt | A text file containing legal text about custody agreement for an item from schedule B. |
| vaccination record.md | Health | ✗ Holding | sensitive (exact) | vaccination record |  |
| wifi and alarm codes.txt | Home | ✓ Home | restricted (exact) | Flat info for house-sitters | Flat info for house-sitters: bins go out Tuesday night, heating is on a timer, cat eats twice a day, half a pouch each time. |

## llama/gemma-4-e4b.gguf (provider 0.1.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| 2025 bank statement March.txt | Finances | ✓ Finances | restricted (exact) | 2025 bank statement March.txt | Current account statement for Alex Morgan covering March 2025, showing opening balance, transactions including rent, salary, and transfers, and final balance. |
| amplifier build log.txt | Projects | ✓ Projects | personal (exact) | amplifier build log.txt | Log detailing the construction process of a headphone amplifier, including steps like etching the board, soldering components, and initial testing observations. |
| birthday card from sister.txt | Letters | ✓ Letters | personal (exact) | birthday card from sister.txt | A birthday message from a sister, mentioning dinner in Leeds. |
| blood test results.txt | Health | ✓ Health | sensitive (exact) | blood test results.txt | Lab results reported 7 March 2025 for Alex Morgan, showing low iron stores. |
| boiler repair note.txt | Home | ✓ Home | personal (exact) | boiler repair note.txt | Note from the landlord's engineer regarding a boiler pressure issue, detailing the repair and advice given to the tenant. |
| budget app notes.md | Projects | ✓ Projects | personal (exact) | budget app notes.md | Design notes for a small terminal application intended to categorize bank exports by reading CSV files and storing rules in a TOML file, featuring a monthly summary table. The planned stack is Python and Textual. |
| building notice water off.txt | Home | ✓ Home | restricted (over) | building notice water off.txt | Notice to all residents regarding water supply being turned off on Tuesday 15 July 2025 from 9:00 to 13:00 for main valve replacement. |
| car insurance renewal.txt | Insurance | ✓ Insurance | sensitive (exact) | car insurance renewal.txt | Policy renewal notice for a 2017 Fiesta, detailing the new premium and renewal date. |
| card replacement letter.txt | Finances | ✓ Finances | restricted (exact) | card replacement letter.txt | A letter from Northbridge Bank enclosing a replacement debit card for Alex Morgan. |
| cell biology review.txt | Reading | ✓ Reading | restricted (over) | cell biology review.txt | A review of circadian clocks in single cells, discussing variability, tissue coupling, and light/feeding resets. |
| claim physio costs.md | Insurance | ✓ Insurance | sensitive (exact) | claim physio costs.md | Claim submitted to Brightwell Health Cover for three physiotherapy sessions in September and October 2025, totaling 165.00. The referral diagnosis is lower back pain. |
| coffee machine descaling.txt | Manuals | ✓ Manuals | restricted (over) | coffee machine descaling.txt | Instructions for descaling a Barista Mini coffee machine. |
| conference registration.txt | Research or Work or Travel | ✓ Work | personal (exact) | conference registration.txt | Confirmation of registration for Dynamics Days Europe 2025 in Edinburgh, including poster presentation details. |
| cv alex morgan.md | Work | ✓ Work | personal (exact) | cv alex morgan.md | A curriculum vitae for Alex Morgan, detailing experience as a Data Analyst and Graduate Analyst, and an MSc in Applied Statistics. |
| darwin origin chapter 4 notes.md | Reading | ✓ Reading | personal (exact) | darwin origin chapter 4 notes.md | Notes summarizing Darwin's chapter on natural selection, including ideas on variation, sexual selection, and divergence. |
| dataset description.md | Research | ✓ Research | personal (exact) | dataset description.md | A file describing a cell imaging dataset recorded over 5 days, containing TIFF stacks and a summary CSV. |
| dishwasher manual.txt | Manuals | ✓ Manuals | restricted (over) | dishwasher manual.txt | User manual for QuietClean 45 slimline dishwasher, detailing setup, programmes, and filter cleaning. |
| draft paper phase response.md | Research | ✓ Research | personal (exact) | draft paper phase response.md | A draft of a paper discussing phase response curves measured in cultured cells and their ability to predict cell synchronization rates. |
| drill safety and warranty.txt | Manuals | ✓ Manuals | personal (exact) | drill safety and warranty.txt | Contains safety instructions and warranty information for a PowerLine 18V cordless drill. |
| electricity bill september.txt | Home | ✓ Home | sensitive (over) | electricity bill september.txt | Electricity bill for the period of 1 to 30 September 2025, showing usage, charges, and payment details. |
| email from sam about visit.txt | Letters | ✓ Letters | personal (exact) | email from sam about visit.txt | Email from Sam regarding a visit, mentioning train booking and a planned coastal walk. |
| employment contract lumen.md | Work | ✓ Work | sensitive (exact) | employment contract lumen.md | Statement of main terms of employment for a Data Analyst at Lumen Analytics Ltd. |
| essay on attention.md | Reading | ✗ Projects | personal (exact) | essay on attention.md | A saved essay arguing that attention is a habit of return rather than a fixed spotlight. |
| flight booking lisbon.txt | Travel | ✓ Travel | personal (exact) | flight booking lisbon.txt | Booking confirmation for flights from London Gatwick to Lisbon and return. |
| fund report q2.txt | Finances | ✓ Finances | personal (exact) | fund report q2.txt | Quarterly report for investors regarding Greenfield Index Funds, showing holdings and unit price changes for the quarter ending 30 June 2025. |
| home contents policy.txt | Insurance | ✓ Insurance | sensitive (exact) | home contents policy.txt | Home contents insurance policy schedule for the period of 1 February 2025 to 31 January 2026. |
| home office expenses.md | Finances or Receipts | ½ Holding | sensitive (exact) | home office expenses.md | A file detailing home-working expenses collected for the self-assessment tax return, listing items like a monitor arm, desk chair, and share of electricity with their respective costs. |
| hotel booking porto.txt | Travel | ✓ Travel | personal (exact) | hotel booking porto.txt | Details for a stay at Casa do Rio, Porto, including check-in/out dates and room type. |
| internet contract.txt | Home | ✓ Home | personal (exact) | internet contract.txt | A 24-month broadband contract with Fibrelink, detailing start date, speed, monthly price, and router information. |
| invoice monitor arm.md | Receipts | ✓ Receipts | personal (exact) | Invoice 2025-0819-77 | Invoice for a dual monitor arm, clamp mount, paid in full and claimable as a home-working expense. |
| itinerary portugal.md | Travel | ✓ Travel | personal (exact) | itinerary portugal.md | A travel plan for Portugal in June 2025, detailing locations and dates for stays in Lisbon and Porto, including flight and train information. |
| lab notebook oscillators week 12.md | Research | ✓ Research | personal (exact) | lab notebook oscillators week 12.md | A lab notebook entry detailing a coupled-oscillator simulation run, noting the order parameter's sharp rise near K = 1.6, and outlining the next steps to add noise. |
| lemon drizzle cake.txt | Recipes | ✓ Recipes | personal (exact) | lemon drizzle cake.txt | Recipe for lemon drizzle cake. |
| lentil dal.md | Recipes | ✓ Recipes | restricted (over) | lentil dal.md | Recipe for weeknight red lentil dal. |
| letter from grandma.txt | Letters | ✓ Letters | personal (exact) | letter from grandma.txt | A letter from Grandma mentioning snowdrops, Grandad's gardening, and inviting Alex for Sunday lunch. |
| letter from tax office.txt | Finances | ✓ Finances | sensitive (exact) | letter from tax office.txt | Letter from HM Revenue and Customs regarding a change in tax code due to a change in benefits. |
| message to landlord.txt | Home | ✓ Home | personal (exact) | message to landlord.txt | A message detailing a dripping kitchen tap and a cracked bathroom window seal, requesting a repair next week. |
| misc notes.txt | Holding | ✓ Holding | personal (exact) | misc notes.txt | A list of miscellaneous reminders and to-do items. |
| old us tax form.txt | Finances | ✓ Finances | restricted (exact) | old us tax form.txt | Form 1040, U.S. Individual Income Tax Return, 2020, filed 15 April 2021, from the year worked in Boston. |
| order confirmation laptop.txt | Receipts | ✓ Receipts | personal (exact) | order confirmation laptop.txt | Order confirmation for an Aero 14 laptop and a USB-C dock, including warranty information. |
| paper kuramoto synchronization.txt | Reading | ✓ Reading | restricted (over) | paper kuramoto synchronization.txt | A text file containing the abstract of a paper discussing the Kuramoto model, which describes the synchronization of coupled oscillators. |
| payslip october.txt | Work | ✓ Work | sensitive (exact) | payslip october.txt | A payslip from LUMEN ANALYTICS LTD showing basic pay, income tax, National Insurance, pension contribution, and net pay for the pay date of 31 October 2025. |
| pension annual statement.txt | Finances | ✓ Finances | sensitive (exact) | pension annual statement.txt | Annual benefit statement for a personal pension plan, detailing member contributions, employer contributions, and projected pension value. |
| performance review 2025.md | Work | ✓ Work | personal (exact) | performance review 2025.md | Mid-year review dated 30 June 2025, noting strengths in clear dashboards and reliable delivery on the churn model, with areas for development being presenting to senior leadership and documenting pipelines. The rating is meets expectations, on track for promotion next year. |
| phone repair receipt.txt | Receipts | ✓ Receipts | personal (exact) | phone repair receipt.txt | Receipt for phone screen replacement and screen protector. |
| physio appointment.txt | Health | ✓ Health | personal (under) | physio appointment.txt | Confirmation of a physiotherapy appointment for lower back pain follow-up. |
| plant monitor readme.md | Projects | ✗ Holding | restricted (over) | plant monitor readme.md | A file describing a soil moisture monitor for houseplants, detailing how to build the firmware and wiring instructions. |
| prescription amoxicillin.txt | Health | ✓ Health | sensitive (exact) | prescription amoxicillin.txt | Prescription for Amoxicillin 500 mg capsules, one three times a day for 7 days, prescribed by Dr P. Shah for a chest infection. |
| receipt and manual blender.txt | Receipts or Manuals | ½ Holding | personal (exact) | receipt and manual blender.txt | Receipt and quick guide for a PowerBlend 600 blender, including purchase price and warranty information. |
| receipt desk chair.txt | Receipts | ✓ Receipts | personal (exact) | receipt desk chair.txt | A receipt from FURNISH & CO. dated 03/05/2025 for an ERGO TASK CHAIR GREY and a FLOOR PROTECTOR, totaling 161.50. It includes a 5 year frame guarantee. |
| referral letter for physio.txt | Health | ✓ Health | sensitive (exact) | referral letter for physio.txt | A referral from Oakfield Medical Practice for Alex Morgan to see a physiotherapist for mechanical lower back pain. |
| rfc 9110 http semantics notes.md | Reading | ✓ Reading | restricted (over) | rfc 9110 http semantics notes.md | A file defining HTTP methods, status codes, and headers shared across HTTP versions, with notes on useful sections. |
| router quick start.md | Manuals | ✓ Manuals | restricted (over) | router quick start.md | Instructions for setting up a Fibrelink Hub 3, including connecting cables, powering on, and joining the network. |
| short story draft.md | Projects | ✓ Projects | personal (exact) | short story draft.md | A draft of a short story titled "The Lighthouse Keeper's Daughter," with notes to self regarding structural changes. |
| sourdough loaf.md | Recipes | ✓ Recipes | personal (exact) | sourdough loaf.md | Recipe for a sourdough loaf, including ingredients and baking instructions. A personal note suggests an adjustment to the water amount. |
| talk slides outline.md | Research | ✗ Work | personal (exact) | talk slides outline.md | Outline for a group meeting talk on synchronization in tissues, including discussion points and time allocation. |
| tax return summary 2024.md | Finances | ✓ Finances | sensitive (exact) | tax return summary 2024.md | Summary of self-assessment for 2023 to 2024, filed online on 20 January 2025. Details employment income, interest, tax paid, and balance due. |
| team meeting notes.md | Work | ✓ Work | personal (exact) | team meeting notes.md | Notes from the Data team weekly meeting on 16 September 2025, covering the launch of churn model v2, warehouse migration status, and upcoming interviews for a junior analyst role. |
| tenancy agreement.md | Home | ✓ Home | sensitive (exact) | tenancy agreement.md | Assured shorthold tenancy agreement for Flat 4, 17 Oakfield Road, with a 12-month term starting 1 August 2023, rent of 950 a month, and a deposit of 1,096. |
| train tickets porto.txt | Travel | ✓ Travel | personal (exact) | train tickets porto.txt | A record of a train journey from Lisboa Santa Apolonia to Porto Campanha on 18 June 2025. |
| travel insurance certificate.txt | Insurance | ✓ Insurance | sensitive (exact) | travel insurance certificate.txt | Certificate of insurance for a single trip to Portugal, covering medical expenses and cancellation. |
| travel insurance claim lost bag.md | Insurance | ✓ Insurance | sensitive (exact) | travel insurance claim lost bag.md | A claim submitted to Wayfarer Travel Insurance regarding lost luggage from a flight back from Lisbon, detailing the contents and value of the lost bag. |
| untitled scan text.txt | Holding | ✓ Holding | personal (exact) | untitled scan text.txt | A text file containing a clause regarding the custody of an item described in schedule B. |
| vaccination record.md | Health | ✓ Health | sensitive (exact) | vaccination record.md | A file containing a table of vaccinations with dates. |
| wifi and alarm codes.txt | Home | ✓ Home | restricted (exact) | wifi and alarm codes.txt | File containing flat information for house-sitters, including bin schedules and pet feeding instructions. |

## llama/granite-4.2-3b.gguf (provider 0.1.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| 2025 bank statement March.txt | Finances | ✓ Finances | restricted (exact) | 2025 bank statement March.txt | A bank statement for March 2025 showing account holder, transactions, and balance. |
| amplifier build log.txt | Projects | ✓ Projects | personal (exact) | amplifier build log.txt | A text log of building a headphone amplifier, including board etching, soldering, and initial listening notes. |
| birthday card from sister.txt | Letters | ✓ Letters | personal (exact) | birthday card from sister.txt | A text file containing a birthday greeting from a sister, including a message, a dinner invitation, and a date. |
| blood test results.txt | Health | ✓ Health | sensitive (exact) | blood test results.txt | Pathology Department, St Anne's Hospital; Lab results reported 7 March 2025; Patient: Alex Morgan; Full blood count: within normal range; Ferritin: 18 ug/L (low; range 30 to 400); Vitamin D: 52 nmol/L; Comment: low iron stores. Discuss with your GP. |
| boiler repair note.txt | Home | ✗ Holding | personal (exact) | boiler repair note.txt | A note from the landlord's engineer about a boiler repair, including the issue, actions taken, and tenant instructions. |
| budget app notes.md | Projects | ✗ Finances | personal (exact) | budget app notes | a terminal app to categorise bank exports |
| building notice water off.txt | Home | ✗ Holding | personal (exact) | building notice water off.txt | A notice informing residents of a temporary water supply interruption for maintenance work. |
| car insurance renewal.txt | Insurance | ✓ Insurance | restricted (over) | car insurance renewal.txt | A text file containing renewal details for a car insurance policy. |
| card replacement letter.txt | Finances | ✓ Finances | restricted (exact) | card replacement letter.txt | A bank letter informing the user of a replacement debit card, including card number and expiration date. |
| cell biology review.txt | Reading | ✗ Holding | personal (exact) | cell biology review.txt | a file (text/plain) containing a review of circadian clocks in single cells |
| claim physio costs.md | Insurance | ✗ Holding | sensitive (exact) | claim physio costs.md | A claim file for physiotherapy sessions submitted to Brightwell Health Cover, detailing sessions, amount, and attached invoices. |
| coffee machine descaling.txt | Manuals | ✓ Manuals | personal (exact) | coffee machine descaling | Descaling your Barista Mini every 2 to 3 months or when the descale light flashes: empty drip tray, fill tank with water and descaler, hold steam and coffee buttons for 3 seconds, then rinse with two full tanks of water. |
| conference registration.txt | Research or Work or Travel | ½ Holding | personal (exact) | conference registration.txt | Registration for Dynamics Days Europe 2025, Edinburgh, 8 to 12 September 2025, with participant Alex Morgan presenting a poster titled 'Thresholds for synchrony in noisy cell populations'. |
| cv alex morgan.md | Work | ✓ Work | personal (exact) | cv alex morgan.md | a file (text/markdown) containing a CV for Alex Morgan, data analyst with three years' experience in customer analytics. |
| darwin origin chapter 4 notes.md | Reading | ✗ Holding | personal (exact) | darwin origin chapter 4 notes | a file (text/markdown) containing Darwin's chapter on natural selection, with notes summarizing key ideas about variation, sexual selection, and divergence diagrams. |
| dataset description.md | Research | ✓ Research | personal (exact) | dataset description.md | a file (text/markdown) that describes a dataset of cell imaging data recorded on 9 May 2025, containing 48 wells with images taken every 30 minutes for 5 days, stored as TIFF stacks on the lab server, with a summary CSV containing columns well, cell, time, intensity. |
| dishwasher manual.txt | Manuals | ✓ Manuals | personal (exact) | dishwasher manual.txt | A user manual for a slimline dishwasher. It lists programmes and cleaning instructions. |
| draft paper phase response.md | Research | ✗ Holding | personal (exact) | draft paper phase response | a markdown file containing a draft paper about phase response curves of weakly coupled cells, with abstract and methods sections |
| drill safety and warranty.txt | Manuals | ✓ Manuals | personal (exact) | drill safety and warranty.txt | A text file containing safety instructions and warranty details for a cordless drill. |
| electricity bill september.txt | Home | ✗ Holding | personal (exact) | electricity bill september.txt | A bill for electricity usage from 1 to 30 September 2025, with total cost 56.34, paid by direct debit on 15 October. |
| email from sam about visit.txt | Letters | ✓ Letters | personal (exact) | email from sam about visit.txt | A text file containing an email from Sam about a visit, with date, subject, sender, and message details. |
| employment contract lumen.md | Work | ✓ Work | restricted (over) | employment contract lumen.md | employment contract for Alex Morgan as Data Analyst at Lumen Analytics Ltd, with start date 15 May 2023, salary 41,000 a year paid monthly, 37.5 hours per week, 25 days holiday plus public holidays, and 1 month notice period. |
| essay on attention.md | Reading | ✗ Holding | personal (exact) | essay on attention.md | a file (text/markdown) containing an essay about attention as a habit of returning to things rather than focusing on them, saved from a magazine's website in 2024 |
| flight booking lisbon.txt | Travel | ✓ Travel | personal (exact) | flight booking lisbon.txt | A confirmed flight booking from London Gatwick to Lisbon with reference K7Q2PX, passenger Alex Morgan, departure and return dates, and baggage information. |
| fund report q2.txt | Finances | ✓ Finances | personal (exact) | fund report q2.txt | A quarterly report on a global equity index fund for investors, showing holdings, unit price, and annual costs. |
| home contents policy.txt | Insurance | ✓ Insurance | restricted (over) | home contents policy.txt | home contents insurance policy |
| home office expenses.md | Finances or Receipts | ½ Holding | restricted (over) | home office expenses | a file containing home-working expenses for the tax return |
| hotel booking porto.txt | Travel | ✓ Travel | personal (exact) | hotel booking porto.txt | A hotel booking confirmation for Casa do Rio in Porto, with check-in and check-out dates, room type and breakfast inclusion, and cancellation policy. |
| internet contract.txt | Home | ✗ Holding | personal (exact) | internet contract.txt | A 24-month broadband contract with a fixed monthly price of 29.00, starting on 12 November 2024, using a Fibrelink Hub 3 router. |
| invoice monitor arm.md | Receipts | ✓ Receipts | personal (exact) | invoice monitor arm.md | a file (text/markdown) containing an invoice for a dual monitor arm, with details of item, price, VAT, and note that it is claimable as a home-working expense. |
| itinerary portugal.md | Travel | ✓ Travel | personal (exact) | itinerary portugal.md | Travel itinerary for Portugal, covering June 2025, with daily stays in Lisbon and Porto, and travel details. |
| lab notebook oscillators week 12.md | Research | ✗ Holding | personal (exact) | lab notebook oscillators week 12.md | A lab notebook entry for week 12 (17 to 21 March 2025) documenting a coupled-oscillator simulation with N = 500 and K values from 0 to 4, including observations about the order parameter and a planned next step. |
| lemon drizzle cake.txt | Recipes | ✓ Recipes | personal (exact) | lemon drizzle cake | A cake recipe for lemon drizzle cake, including ingredients and baking instructions. |
| lentil dal.md | Recipes | ✓ Recipes | personal (exact) | lentil dal.md | A recipe for red lentil dal, suitable for weeknight meals. It includes cooking instructions and serves 4, freezing well. |
| letter from grandma.txt | Letters | ✓ Letters | personal (exact) | letter from grandma.txt | A personal letter from a family member, containing seasonal observations and a request for a visit. |
| letter from tax office.txt | Finances | ✓ Finances | restricted (over) | letter from tax office.txt | This letter is about your income tax. |
| message to landlord.txt | Home | ✓ Home | personal (exact) | message to landlord.txt | A text message to the landlord about a dripping kitchen tap and cracked bathroom window, requesting repair next week, with contact details. |
| misc notes.txt | Holding | ✓ Holding | personal (exact) | misc notes.txt | A text file containing miscellaneous notes, including tasks, items, and questions. |
| old us tax form.txt | Finances | ✓ Finances | restricted (exact) | old us tax form.txt | a file (text/plain) containing Form 1040, U.S. Individual Income Tax Return, 2020, filed on 15 April 2021, with filing details and income information. |
| order confirmation laptop.txt | Receipts | ✗ Holding | personal (exact) | order confirmation laptop.txt | A confirmation email for an order containing a laptop and dock, with payment details and warranty information. |
| paper kuramoto synchronization.txt | Reading | ✗ Holding | personal (exact) | paper kuramoto synchronization.txt | a file (text/plain) containing an abstract of a Kuramoto model paper |
| payslip october.txt | Work | ✓ Work | restricted (over) | payslip october.txt | A payslip for Alex Morgan showing basic pay, tax, NI, pension deductions and net pay for October 2025. |
| pension annual statement.txt | Finances | ✓ Finances | sensitive (exact) | pension annual statement.txt | Annual benefit statement for a personal pension plan showing member details, statement date, pot value, contributions, and projected pension. |
| performance review 2025.md | Work | ✓ Work | personal (exact) | performance review 2025.md | a file (text/markdown) containing a mid-year review dated 30 June 2025, with reviewer Jordan Blake, head of data, strengths and development areas, and a rating of meets expectations, on track for promotion next year. |
| phone repair receipt.txt | Receipts | ✓ Receipts | personal (exact) | phone repair receipt.txt | A receipt for phone repairs showing services performed, costs, and guarantee period. |
| physio appointment.txt | Health | ✓ Health | personal (under) | physio appointment.txt | Appointment details for a physiotherapy session. |
| plant monitor readme.md | Projects | ✓ Projects | personal (exact) | plant monitor readme.md | a file (text/markdown) that describes a soil moisture monitor for houseplants using an ESP32, a capacitive sensor, and a web page to post readings every 10 minutes. |
| prescription amoxicillin.txt | Health | ✓ Health | sensitive (exact) | prescription amoxicillin.txt | A prescription file for a chest infection, containing medication details and instructions. |
| receipt and manual blender.txt | Receipts or Manuals | ½ Holding | personal (exact) | receipt and manual blender.txt | A receipt and manual blender guide for a PowerBlend 600 blender, showing payment details and safety instructions. |
| receipt desk chair.txt | Receipts | ✓ Receipts | personal (exact) | receipt desk chair.txt | A receipt for a desk chair purchase, showing item details and total cost. |
| referral letter for physio.txt | Health | ✓ Health | restricted (over) | referral letter for physio.txt | A referral letter for physiotherapy from a medical practice, containing details of a patient's condition and request for therapy. |
| rfc 9110 http semantics notes.md | Reading | ✗ Holding | personal (exact) | rfc 9110: http semantics notes.md | a file (text/markdown) defining methods, status codes and headers shared by every version of HTTP. |
| router quick start.md | Manuals | ✓ Manuals | personal (exact) | router quick start.md | A quick start guide for a fibre internet hub. |
| short story draft.md | Projects | ✗ Holding | personal (exact) | short story draft.md | a markdown file containing a short story draft with notes to self |
| sourdough loaf.md | Recipes | ✓ Recipes | personal (exact) | sourdough loaf | A recipe for a sourdough loaf, including ingredient quantities, mixing instructions, folding steps, proofing, baking temperatures and times, and a note about water quantity. |
| talk slides outline.md | Research | ✓ Research | personal (exact) | talk slides outline.md | Group meeting talk, 14 November 2025: outline |
| tax return summary 2024.md | Finances | ✓ Finances | restricted (over) | tax return summary 2024.md | Self assessment 2023 to 2024 summary; filed online on 20 January 2025; employment income 34,450; interest 112; tax paid through pay 4,890; balance to pay by 31 January 0; kept calculation from tax office website; nothing owed this year. |
| team meeting notes.md | Work | ✓ Work | personal (exact) | team meeting notes.md | a file (text/markdown) containing weekly team meeting notes with present members, model status, migration delay, and upcoming hiring interviews. |
| tenancy agreement.md | Home | ✓ Home | restricted (over) | tenancy agreement.md | A tenancy agreement for a flat, with landlord and tenant details, term, rent, and deposit information. |
| train tickets porto.txt | Travel | ✓ Travel | personal (exact) | train tickets porto.txt | CP Comboios de Portugal Alfa Pendular train from Lisboa Santa Apolonia to Porto Campanha on 18 June 2025, departing at 09:39 and arriving at 12:24, in Coach 4, seat 61. |
| travel insurance certificate.txt | Insurance | ✗ Holding | restricted (over) | travel insurance certificate.txt | A travel insurance certificate for a single trip to Portugal covering medical expenses and cancellation. |
| travel insurance claim lost bag.md | Insurance | ½ Holding | restricted (over) | travel insurance claim lost bag | A lost luggage claim filed with Wayfarer Travel Insurance for a bag containing clothes and a camera, with airline report reference LIS-88213. |
| untitled scan text.txt | Holding | ✓ Holding | restricted (over) | untitled scan text.txt | a file (text/plain) containing a partial schedule describing custody of an item from schedule B. |
| vaccination record.md | Health | ✓ Health | restricted (over) | vaccination record.md | A markdown file containing a table of vaccine names and dates. |
| wifi and alarm codes.txt | Home | ½ Holding | restricted (exact) | wifi and alarm codes.txt | Flat info for house-sitters |

## llama/granite-4.2-8b.gguf (provider 0.1.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| 2025 bank statement March.txt | Finances | ✓ Finances | restricted (exact) | 2025 bank statement March.txt | A bank statement for March 2025 showing account activity for Alex Morgan's current account at Northbridge Bank. |
| amplifier build log.txt | Projects | ✓ Projects | personal (exact) | amplifier build log.txt | A text file recording steps and observations for building a headphone amplifier. |
| birthday card from sister.txt | Letters | ✓ Letters | personal (exact) | birthday card from sister.txt | A handwritten birthday card from sister expressing affection and a promise to treat brother to dinner in Leeds. |
| blood test results.txt | Health | ✓ Health | sensitive (exact) | blood test results.txt | Lab results for Alex Morgan from Pathology Department, St Anne's Hospital reported 7 March 2025, indicating low iron stores (ferritin 18 ug/L). |
| boiler repair note.txt | Home | ✓ Home | personal (exact) | boiler repair note.txt | A note from the landlord's engineer about a boiler repair, stating the pressure relief valve was replaced and the system refilled to 1.5 bar, advising the tenant to check the gauge weekly and call if pressure drops below 1 bar. No charge to tenant. |
| budget app notes.md | Projects | ✓ Projects | personal (exact) | budget app notes.md | A markdown file containing design notes for a small terminal app that categorises bank exports. |
| building notice water off.txt | Home | ✓ Home | personal (exact) | building notice water off.txt | A notice informing residents that the water supply will be turned off on Tuesday 15 July 2025 from 9:00 to 13:00 for main valve replacement. |
| car insurance renewal.txt | Insurance | ✓ Insurance | sensitive (exact) | car insurance renewal.txt | A text file containing the renewal details for a car insurance policy for a 2017 Fiesta, including the renewal date, new premium amount, and note that no action is required if satisfied. |
| card replacement letter.txt | Finances | ✓ Finances | restricted (exact) | card replacement letter.txt | A bank letter informing Alex Morgan that a replacement debit card is enclosed, providing the new card number and expiry date. |
| cell biology review.txt | Reading | ✗ Holding | personal (exact) | cell biology review.txt | A text file containing a review abstract about circadian clocks in single cells, mentioning variability between cells, coupling within tissues, and clock resetting by light and feeding. Published 12 February 2024. |
| claim physio costs.md | Insurance | ✗ Holding | sensitive (exact) | claim physio costs.md | A markdown file submitting a health insurance claim for three physiotherapy sessions in September and October 2025 for lower back pain, amount 165.00. |
| coffee machine descaling.txt | Manuals | ✓ Manuals | personal (exact) | coffee machine descaling.txt | A text file containing instructions for descaling the Barista Mini coffee machine. |
| conference registration.txt | Research or Work or Travel | ½ Holding | personal (exact) | conference registration.txt | Confirmation of registration for Dynamics Days Europe 2025 in Edinburgh, 8–12 September 2025, as a poster presenter; fee of 280 paid; accommodation not included. |
| cv alex morgan.md | Work | ✓ Work | personal (exact) | cv alex morgan.md | A CV in markdown format for Alex Morgan, listing work experience and education. |
| darwin origin chapter 4 notes.md | Reading | ✗ Holding | personal (exact) | darwin origin chapter 4 notes.md | Notes on Darwin's chapter 4 of On the Origin of Species, summarizing key ideas about variation, sexual selection, and divergence diagram. |
| dataset description.md | Research | ✗ Holding | personal (exact) | dataset description.md | A markdown file describing a cell imaging dataset recorded on 9 May 2025. It includes details about 48 wells, one image every 30 minutes for 5 days, file format (TIFF stacks per well, ~2 GB each), and the CSV summary columns (well, cell, time, intensity). |
| dishwasher manual.txt | Manuals | ✓ Manuals | personal (exact) | dishwasher manual.txt | A user manual for a QuietClean 45 slimline dishwasher describing setup before first use, programmes (Eco 50, Intensive 70, Quick 35), and weekly filter cleaning. |
| draft paper phase response.md | Research | ✗ Holding | personal (exact) | draft paper phase response.md | A draft paper in markdown format discussing phase response curves of weakly coupled cells, with a rough abstract and methods section that notes a TODO for imaging setup and lists co-authors Priya and Dr Lin for review. |
| drill safety and warranty.txt | Manuals | ✓ Manuals | personal (exact) | drill safety and warranty.txt | A text file containing safety instructions and warranty information for a PowerLine 18V cordless drill. |
| electricity bill september.txt | Home | ✓ Home | personal (exact) | electricity bill september.txt | A text file containing an electricity bill from Brightspark Energy for the period 1 to 30 September 2025, showing usage, charges, and payment details. |
| email from sam about visit.txt | Letters | ✗ Holding | personal (exact) | email from sam about visit.txt | An email from Sam confirming a train booking for arrival on Friday 12 September 2025 and asking if Alex is still up for a coastal walk on Saturday. |
| employment contract lumen.md | Work | ✓ Work | restricted (over) | employment contract lumen.md | A markdown file stating the main terms of an employment contract for Alex Morgan as a Data Analyst at Lumen Analytics Ltd, including start date, salary, hours, holiday entitlement, and notice period. |
| essay on attention.md | Reading | ✗ Holding | personal (exact) | essay on attention.md | A markdown file containing an essay about attention, stating that attention is less a spotlight than a habit and quoting 'we become what we keep coming back to'. Saved from a magazine's website in 2024. |
| flight booking lisbon.txt | Travel | ✓ Travel | personal (exact) | flight booking lisbon.txt | A confirmed flight booking from London Gatwick to Lisbon with passenger Alex Morgan, outbound 14 June 2025 and return 24 June 2025. |
| fund report q2.txt | Finances | ✓ Finances | personal (exact) | fund report q2.txt | A quarterly report for investors about the Global Equity Index fund, showing the holding units, unit price, and cost percentage. |
| home contents policy.txt | Insurance | ✓ Insurance | personal (under) | home contents policy.txt | A home contents insurance policy document for Alex Morgan covering flat 4, 17 Oakfield Road, valid 1 February 2025 to 31 January 2026, with contents limit 30,000 and excess 100, premium 11.40 per month. |
| home office expenses.md | Finances or Receipts | ½ Holding | restricted (over) | home office expenses.md | A markdown file listing home‑working expenses for the tax return, including a monitor arm, desk chair, and share of electricity. |
| hotel booking porto.txt | Travel | ✓ Travel | personal (exact) | hotel booking porto.txt | A text file containing details of a hotel booking for Casa do Rio, Porto, including check-in and check-out dates, room type, and cancellation policy. |
| internet contract.txt | Home | ✗ Holding | personal (exact) | internet contract.txt | A 24-month broadband contract with Fibrelink starting 12 November 2024, offering up to 500 Mb/s speed at a fixed monthly price of 29.00, including a Fibrelink Hub 3 router that must be returned upon leaving. |
| invoice monitor arm.md | Receipts | ✓ Receipts | personal (exact) | invoice monitor arm.md | A markdown file containing an invoice for a dual monitor arm with price 74.99 and VAT 12.50, sold to Alex Morgan on 19 August 2025, paid in full and claimable as a home-working expense. |
| itinerary portugal.md | Travel | ✓ Travel | personal (exact) | itinerary portugal.md | A travel itinerary for Portugal in June 2025 describing days, locations, and accommodations. |
| lab notebook oscillators week 12.md | Research | ✓ Research | personal (exact) | lab notebook oscillators week 12.md | A lab notebook entry for week 12 (17–21 March 2025) describing a coupled-oscillator simulation with N = 500 and K from 0 to 4, noting the order parameter rise near K = 1.6 matching Lorentzian spread theory, and a plan to add noise and check threshold movement. |
| lemon drizzle cake.txt | Recipes | ✓ Recipes | personal (exact) | lemon drizzle cake.txt | A text file containing a lemon drizzle cake recipe from Gran. |
| lentil dal.md | Recipes | ✓ Recipes | personal (exact) | lentil dal.md | A markdown file containing a recipe for a weeknight red lentil dal, serving 4 and suitable for freezing. |
| letter from grandma.txt | Letters | ✓ Letters | personal (exact) | letter from grandma.txt | A handwritten letter from Grandma dated 14 February 2025 thanking Alex for birthday photos, mentioning snowdrops and Grandad's vegetable patch, and inviting Alex to Sunday lunch. |
| letter from tax office.txt | Finances | ✓ Finances | restricted (over) | letter from tax office.txt | A letter from HM Revenue and Customs informing Alex Morgan that their tax code for the tax year starting 6 April 2025 has changed to 1257L due to a change in benefits reported by their employer. No action is required unless Alex believes the change is incorrect. |
| message to landlord.txt | Home | ✓ Home | personal (exact) | message to landlord.txt | A tenant informs the landlord about a kitchen tap drip and a cracked bathroom window seal, requesting a visit next week after 4 pm on weekdays. |
| misc notes.txt | Holding | ✓ Holding | personal (exact) | misc notes.txt | A text file containing a list of items and questions. |
| old us tax form.txt | Finances | ✓ Finances | restricted (exact) | old us tax form.txt | A text file containing a U.S. Individual Income Tax Return (Form 1040) for tax year 2020, filed April 15, 2021, for the year the author worked in Boston. Includes name Alex Morgan and total income of $52,300. |
| order confirmation laptop.txt | Receipts | ✓ Receipts | personal (exact) | order confirmation laptop.txt | Order confirmation for a laptop and USB-C dock, paid by card, includes warranty information. |
| paper kuramoto synchronization.txt | Reading | ✗ Holding | personal (exact) | paper kuramoto synchronization.txt | A text file containing an abstract and title of a paper on the Kuramoto model and synchronization of coupled oscillators, downloaded for a literature review. |
| payslip october.txt | Work | ✓ Work | restricted (over) | payslip october.txt | A payslip for October 2025 showing basic pay, deductions and net pay for employee Alex Morgan. |
| pension annual statement.txt | Finances | ✓ Finances | sensitive (exact) | pension annual statement.txt | Annual benefit statement from Harbour Pensions for Alex Morgan's Lumen Analytics group personal pension, showing pot value, contributions, and projected pension. |
| performance review 2025.md | Work | ✓ Work | personal (exact) | performance review 2025.md | A mid-year performance review document for 2025 written in markdown format, containing strengths, areas for development, and a rating. |
| phone repair receipt.txt | Receipts | ✓ Receipts | personal (exact) | phone repair receipt.txt | Receipt for phone screen replacement and screen protector purchase with total cost and 90-day guarantee. |
| physio appointment.txt | Health | ✓ Health | personal (under) | physio appointment.txt | A confirmed physiotherapy appointment for lower back pain follow-up with Sam Okafor on Thursday 9 October 2025 at 10:30 at Riverside Physiotherapy. Patient should arrive 10 minutes early. |
| plant monitor readme.md | Projects | ✓ Projects | personal (exact) | plant monitor readme.md | A README file for a soil moisture monitor project using an ESP32 and a capacitive sensor. It includes building instructions and a TODO list. |
| prescription amoxicillin.txt | Health | ✓ Health | sensitive (exact) | prescription amoxicillin.txt | A prescription file for amoxicillin capsules for Alex Morgan, dated 14 January 2025, prescribed by Dr P. Shah for a chest infection. |
| receipt and manual blender.txt | Receipts or Manuals | ½ Holding | personal (exact) | receipt and manual blender.txt | A receipt for a PowerBlend 600 blender purchased on 12 April 2025 and a quick guide for operating the blender. |
| receipt desk chair.txt | Receipts | ✓ Receipts | personal (exact) | receipt desk chair.txt | A receipt for a grey ergonomic task chair and floor protector purchased from FURNISH & CO., Store 22, Riverside Retail Park on 03/05/2025, total 161.50, includes return policy and 5-year frame guarantee. |
| referral letter for physio.txt | Health | ✓ Health | restricted (over) | referral letter for physio.txt | A referral letter from Oakfield Medical Practice for Alex Morgan to see a physiotherapist for mechanical lower back pain lasting six weeks, with no red-flag symptoms and ibuprofen providing slight relief. The letter notes it may be used for an insurance claim. |
| rfc 9110 http semantics notes.md | Reading | ✗ Holding | personal (exact) | rfc 9110 http semantics notes.md | A markdown file summarizing RFC 9110 published June 2022 by the IETF, covering HTTP methods, status codes, and headers common to all HTTP versions. Useful sections include 9 (methods), 15 (status codes), and 14.2 (range requests). |
| router quick start.md | Manuals | ✓ Manuals | personal (exact) | router quick start.md | A quick start guide for the Fibrelink Hub 3 router written in markdown. |
| short story draft.md | Projects | ✓ Projects | personal (exact) | short story draft.md | A draft of a short story titled 'The Lighthouse Keeper's Daughter' in markdown format, containing narrative text and author notes about cutting the opening paragraph and moving the storm earlier. |
| sourdough loaf.md | Recipes | ✓ Recipes | personal (exact) | sourdough loaf.md | A markdown file containing a sourdough bread recipe with ingredient amounts, mixing instructions, folding sets, shaping, proofing, and baking details, plus a personal note about water amount. |
| talk slides outline.md | Research | ✗ Holding | personal (exact) | talk slides outline.md | A markdown file outlining a group meeting talk scheduled for 14 November 2025, including four sections and a total time of 15 minutes plus questions. |
| tax return summary 2024.md | Finances | ✓ Finances | restricted (over) | tax return summary 2024.md | A tax return summary file for 2024 covering self assessment from 2023 to 2024, filed online on 20 January 2025. It shows employment income of 34,450, interest of 112, tax paid through pay of 4,890, and a balance to pay of 0, indicating nothing owed for the year. |
| team meeting notes.md | Work | ✓ Work | personal (exact) | team meeting notes.md | Meeting notes for the Data team weekly meeting on 16 September 2025, listing present attendees, topics discussed (churn model v2 live status, warehouse migration delay, hiring for junior analyst role), and next meeting date. |
| tenancy agreement.md | Home | ✓ Home | restricted (over) | tenancy agreement.md | A markdown file containing an assured shorthold tenancy agreement for Flat 4, 17 Oakfield Road between Oakfield Lettings Ltd (landlord) and Alex Morgan (tenant). Term is 12 months from 1 August 2023, then month to month. Rent is 950 per month due on the 1st. Deposit is 1,096 held in a protection scheme. |
| train tickets porto.txt | Travel | ✓ Travel | personal (exact) | train tickets porto.txt | A text file containing details of a train ticket for a CP Alfa Pendular journey from Lisboa Santa Apolonia to Porto Campanha on 18 June 2025, departing at 09:39 and arriving at 12:24, with coach 4 seat 61. |
| travel insurance certificate.txt | Insurance | ✓ Insurance | sensitive (exact) | travel insurance certificate.txt | A travel insurance certificate for a single trip to Portugal from 14 to 24 June 2025, covering medical expenses up to 5,000,000 and cancellation up to 3,000. Includes an emergency assistance line on the back. |
| travel insurance claim lost bag.md | Insurance | ✓ Insurance | sensitive (exact) | travel insurance claim lost bag.md | A markdown file documenting a lost luggage claim for Wayfarer Travel Insurance regarding a bag lost on a flight back from Lisbon on 24 June 2025, with contents listed as clothes and a camera and receipts attached. |
| untitled scan text.txt | Holding | ✓ Holding | restricted (over) | untitled scan text.txt | A scanned text file containing part of a document that references a schedule and an agreement about custody of an item. |
| vaccination record.md | Health | ✓ Health | sensitive (exact) | vaccination record.md | A markdown file listing vaccination dates for Tetanus booster, Hepatitis A, and two Flu vaccinations. |
| wifi and alarm codes.txt | Home | ½ Holding | restricted (exact) | wifi and alarm codes.txt | A text file containing house-sitter instructions for bins, heating, and cat feeding, plus a password. |

## llama/qwen3.5-4b.gguf (provider 0.1.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| 2025 bank statement March.txt | Finances | ✓ Finances | restricted (exact) | 2025 bank statement March.txt | March 2025 bank statement for Alex Morgan's current account |
| amplifier build log.txt | Projects | ✓ Projects | personal (exact) | Amplifier build log | Log of building a headphone amplifier |
| birthday card from sister.txt | Letters | ✓ Letters | personal (exact) | birthday card from sister.txt | Birthday card from sister |
| blood test results.txt | Health | ✓ Health | sensitive (exact) | Blood test results | Full blood count and vitamin levels showing low ferritin |
| boiler repair note.txt | Home | ✓ Home | personal (exact) | Boiler repair note.txt | A note from a landlord's engineer detailing a boiler pressure issue and repair. |
| budget app notes.md | Projects | ✓ Projects | personal (exact) | Budget app notes | Design notes for a budgeting application |
| building notice water off.txt | Home | ✓ Home | personal (exact) | Building notice: water off | Notice of water supply interruption for maintenance |
| car insurance renewal.txt | Insurance | ✓ Insurance | sensitive (exact) | Car insurance renewal | Policy renewal notice for a 2017 Fiesta |
| card replacement letter.txt | Finances | ✓ Finances | restricted (exact) | Card replacement letter | Letter from Northbridge Bank confirming receipt of a new debit card |
| cell biology review.txt | Reading | ✓ Reading | personal (exact) | cell biology review.txt | Review of circadian clocks in single cells |
| claim physio costs.md | Insurance | ✓ Insurance | sensitive (exact) | Claim physio costs.md | A claim for physiotherapy sessions submitted to Brightwell Health Cover. |
| coffee machine descaling.txt | Manuals | ✓ Manuals | personal (exact) | coffee machine descaling.txt | Instructions for descaling a coffee machine |
| conference registration.txt | Research or Work or Travel | ✓ Research | personal (exact) | Conference registration | Registration confirmation for Dynamics Days Europe 2025 in Edinburgh |
| cv alex morgan.md | Work | ✓ Work | personal (exact) | Alex Morgan: CV | Resume for data analyst roles |
| darwin origin chapter 4 notes.md | Reading | ✗ Holding | personal (exact) | Darwin Origin chapter 4 notes | Notes on natural selection and sexual selection from Darwin's *On the Origin of Species*, chapter 4 |
| dataset description.md | Research | ✓ Research | personal (exact) | Dataset: cell imaging, May 202025 | A file describing a dataset of cell images recorded over five days. |
| dishwasher manual.txt | Manuals | ✓ Manuals | personal (exact) | Dishwasher manual | User manual for a QuietClean 45 dishwasher |
| draft paper phase response.md | Research | ✓ Research | personal (exact) | draft paper phase response.md | Draft of a paper on phase response curves in cultured cells |
| drill safety and warranty.txt | Manuals | ✓ Manuals | personal (exact) | Drill safety and warranty | Safety instructions and warranty for a PowerLine 18V cordless drill |
| electricity bill september.txt | Home | ✓ Home | personal (exact) | electricity bill september.txt | September electricity bill from Brightspark Energy |
| email from sam about visit.txt | Letters | ✗ Travel | personal (exact) | email from sam about visit.txt | Arrival details for a trip to the coast |
| employment contract lumen.md | Work | ✓ Work | sensitive (exact) | employment contract lumen.md | Employment terms for Data Analyst role at Lumen Analytics Ltd |
| essay on attention.md | Reading | ✗ Holding | personal (exact) | On paying attention | Essay on attention as a habit rather than a spotlight |
| flight booking lisbon.txt | Travel | ✓ Travel | personal (exact) | Flight booking: Lisbon | Confirmed flight reservation from London Gatwick to Lisbon |
| fund report q2.txt | Finances | ✓ Finances | personal (exact) | Fund report Q2 | Quarterly report on Global Equity Index fund holdings |
| home contents policy.txt | Insurance | ✓ Insurance | sensitive (exact) | home contents policy.txt | Home contents insurance policy |
| home office expenses.md | Finances or Receipts | ½ Holding | sensitive (exact) | Home office expenses | A list of home office expenses collected for the tax return. |
| hotel booking porto.txt | Travel | ✓ Travel | personal (exact) | hotel booking porto.txt | Hotel reservation for a stay in Porto |
| internet contract.txt | Home | ✗ Holding | personal (exact) | internet contract.txt | 24-month broadband contract with Fibrelink |
| invoice monitor arm.md | Receipts | ✓ Receipts | personal (exact) | Invoice monitor arm.md | Invoice for a monitor arm sold to Alex Morgan on 19 August 2025, including VAT and note that it is claimable as a home-working expense. |
| itinerary portugal.md | Travel | ✓ Travel | personal (exact) | Portugal, June 2025 | Itinerary for a trip to Portugal in June 2025, including dates, locations, and accommodation details. |
| lab notebook oscillators week 12.md | Research | ✓ Research | personal (exact) | Lab notebook, week 12 | Notes on coupled-oscillator simulation results and next steps |
| lemon drizzle cake.txt | Recipes | ✓ Recipes | personal (exact) | Lemon drizzle cake (Gran's) | Recipe for lemon drizzle cake |
| lentil dal.md | Recipes | ✗ Holding | personal (exact) | lentil dal.md | Recipe for red lentil dal |
| letter from grandma.txt | Letters | ✓ Letters | personal (exact) | letter from grandma.txt | A text file containing a letter from grandma |
| letter from tax office.txt | Finances | ✓ Finances | sensitive (exact) | letter from tax office.txt | Tax code change notification |
| message to landlord.txt | Home | ✓ Home | personal (exact) | message to landlord.txt | Request for repairs to kitchen tap and bathroom window |
| misc notes.txt | Holding | ✓ Holding | personal (exact) | misc notes.txt | A text file containing miscellaneous reminders and questions. |
| old us tax form.txt | Finances | ✓ Finances | restricted (exact) | US Tax Form 1040, 2020 | 2020 U.S. Individual Income Tax Return filed April 2021 for Boston employment |
| order confirmation laptop.txt | Receipts | ✓ Receipts | restricted (over) | order confirmation laptop.txt | Order confirmation for a laptop and USB-C dock |
| paper kuramoto synchronization.txt | Reading | ✓ Reading | personal (exact) | Kuramoto synchronization paper | A paper on the Kuramoto model of coupled oscillators |
| payslip october.txt | Work | ✗ Holding | restricted (over) | payslip october.txt | October 2025 payslip for Alex Morgan at Lumen Analytics Ltd |
| pension annual statement.txt | Finances | ✗ Holding | sensitive (exact) | Pension annual statement | Annual benefit statement from Harbour Pensions showing member Alex Morgan's pot value, contributions, and projected pension income. |
| performance review 2025.md | Work | ✓ Work | personal (exact) | performance review 2025.md | mid-year performance review |
| phone repair receipt.txt | Receipts | ✓ Receipts | personal (exact) | Phone repair receipt | Receipt for screen replacement and screen protector for a Pixel 8 |
| physio appointment.txt | Health | ✓ Health | personal (under) | Physio appointment confirmation | Confirmation of a physiotherapy appointment with Sam Okafor for lower back pain follow-up |
| plant monitor readme.md | Projects | ✓ Projects | personal (exact) | plant monitor readme.md | Instructions for building a soil moisture monitor for houseplants |
| prescription amoxicillin.txt | Health | ✓ Health | sensitive (exact) | prescription amoxicillin.txt | A prescription for amoxicillin for a chest infection |
| receipt and manual blender.txt | Receipts or Manuals | ½ Holding | personal (exact) | receipt and manual blender.txt | Receipt and manual for a PowerBlend 600 blender |
| receipt desk chair.txt | Receipts | ✓ Receipts | personal (exact) | receipt desk chair.txt | Receipt for a desk chair and floor protector |
| referral letter for physio.txt | Health | ✓ Health | sensitive (exact) | Referral letter for physiotherapy | Referral from Oakfield Medical Practice for Alex Morgan's mechanical lower back pain |
| rfc 9110 http semantics notes.md | Reading | ✓ Reading | personal (exact) | RFC 9110: HTTP Semantics | Notes on the IETF standard defining HTTP methods, status codes, and headers. |
| router quick start.md | Manuals | ✓ Manuals | personal (exact) | router quick start.md | Quick start guide for a Fibrelink Hub 3 router |
| short story draft.md | Projects | ✓ Projects | personal (exact) | The Lighthouse Keeper's Daughter (draft 2) | A short story draft about a lighthouse keeper's daughter returning to her father's lighthouse after eleven years. |
| sourdough loaf.md | Recipes | ✓ Recipes | personal (exact) | Sourdough loaf | Recipe for sourdough bread |
| talk slides outline.md | Research | ✓ Research | personal (exact) | Talk slides outline | Outline for a group meeting talk on tissue synchronization, simulations, and next steps |
| tax return summary 2024.md | Finances | ✓ Finances | sensitive (exact) | Tax return summary 2024 | Self-assessment tax return summary for 2023–2024, showing income, tax paid, and zero balance due. |
| team meeting notes.md | Work | ✓ Work | personal (exact) | Data team weekly, 16 September 2025 | Notes from a weekly meeting about the data team's progress, including a live churn model, warehouse migration delays, and hiring plans. |
| tenancy agreement.md | Home | ✓ Home | sensitive (exact) | Assured shorthold tenancy agreement | A tenancy agreement for a flat at 17 Oakfield Road |
| train tickets porto.txt | Travel | ✓ Travel | personal (exact) | Train tickets Porto | Train tickets from Lisbon to Porto |
| travel insurance certificate.txt | Insurance | ✓ Insurance | sensitive (exact) | travel insurance certificate.txt | Travel insurance certificate for a trip to Portugal |
| travel insurance claim lost bag.md | Insurance | ½ Holding | sensitive (exact) | Lost luggage claim | Claim for lost bag on flight from Lisbon |
| untitled scan text.txt | Holding | ✓ Holding | personal (exact) | untitled scan text.txt | A text file containing a partial excerpt of a legal document regarding item custody. |
| vaccination record.md | Health | ✓ Health | sensitive (exact) | Vaccination record | A list of vaccines and dates |
| wifi and alarm codes.txt | Home | ✓ Home | restricted (exact) | wifi and alarm codes.txt | House-sitter instructions and access codes |

## llama/qwen3.5-9b.gguf (provider 0.1.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| 2025 bank statement March.txt | Finances | ✓ Finances | restricted (exact) | 2025 bank statement March | A text file containing a bank statement for March 2025, showing transactions including rent, salary, and savings transfers. |
| amplifier build log.txt | Projects | ✓ Projects | personal (exact) | Headphone amplifier build log | A text file documenting the construction and testing of a headphone amplifier, including dates, steps taken, and initial audio performance notes. |
| birthday card from sister.txt | Letters | ✓ Letters | personal (exact) | Birthday card from sister | A text file containing a birthday greeting from a sister, mentioning a chess challenge and a dinner offer. |
| blood test results.txt | Health | ✓ Health | restricted (over) | Blood test results | Lab results for a full blood count, ferritin, and vitamin D levels, indicating low iron stores. |
| boiler repair note.txt | Home | ✓ Home | personal (exact) | Boiler repair note | A note from a landlord's engineer detailing a boiler pressure loss repair, including valve replacement and system refill instructions. |
| budget app notes.md | Projects | ✓ Projects | personal (exact) | Budget app: design notes | Design notes for a Python terminal application that categorises bank exports by payee and generates monthly summaries. |
| building notice water off.txt | Home | ✓ Home | personal (exact) | Building notice: water off | A notice informing residents of a scheduled water supply interruption for maintenance work. |
| car insurance renewal.txt | Insurance | ✓ Insurance | sensitive (exact) | Car insurance renewal | A notification that a car insurance policy is renewing on 1 December 2025, with the new premium and discount details. |
| card replacement letter.txt | Finances | ✓ Finances | restricted (exact) | Northbridge Bank debit card replacement letter | A letter confirming the issuance of a replacement debit card, including the new card number and expiry date. |
| cell biology review.txt | Reading | ✓ Reading | personal (exact) | Circadian clocks in single cells: a review | A text file reviewing how single-cell imaging reveals variability, tissue coupling, and resetting mechanisms of circadian clocks. |
| claim physio costs.md | Insurance | ✓ Insurance | restricted (over) | Claim submitted to Brightwell Health Cover | A record of a submitted claim for physiotherapy sessions, including the amount and attached invoices. |
| coffee machine descaling.txt | Manuals | ✓ Manuals | personal (exact) | coffee machine descaling.txt | Instructions for descaling a Barista Mini coffee machine. |
| conference registration.txt | Research or Work or Travel | ½ Holding | personal (exact) | Conference registration | Confirmation of registration for Dynamics Days Europe 2025 in Edinburgh, including poster presentation details and fee payment status. |
| cv alex morgan.md | Work | ✓ Work | personal (exact) | Alex Morgan CV | A markdown file containing a curriculum vitae for Alex Morgan, detailing professional experience as a data analyst and education. |
| darwin origin chapter 4 notes.md | Reading | ✗ Holding | personal (exact) | On the Origin of Species, chapter 4 notes | Personal summary of Darwin's chapter on natural selection, including key ideas and a diagram description. |
| dataset description.md | Research | ✓ Research | personal (exact) | cell imaging dataset | A collection of 48 well images recorded over 5 days, with intensity data tracked in a summary CSV. |
| dishwasher manual.txt | Manuals | ✓ Manuals | personal (exact) | QuietClean 45 slimline dishwasher: user manual | Instructions for operating the QuietClean 45 slimline dishwasher, including salt filling, water hardness settings, and programme descriptions. |
| draft paper phase response.md | Research | ✓ Research | personal (exact) | draft paper phase response.md | A draft document outlining a study on phase response curves in weakly coupled cells, including an abstract and placeholders for methods. |
| drill safety and warranty.txt | Manuals | ✓ Manuals | personal (exact) | PowerLine 18V cordless drill safety and warranty | Safety instructions and warranty details for the PowerLine 18V cordless drill. |
| electricity bill september.txt | Home | ✓ Home | sensitive (over) | Brightspark Energy electricity bill | A text file containing an electricity bill from Brightspark Energy for the period 1 to 30 September 2025, showing usage of 162 kWh and a total cost of 56.34. |
| email from sam about visit.txt | Letters | ✓ Letters | personal (exact) | email from sam about visit.txt | A text file containing an email from Sam confirming a train arrival on Friday, 12 August and proposing a coastal walk on Saturday. |
| employment contract lumen.md | Work | ✓ Work | restricted (over) | Employment contract with Lumen Analytics Ltd | A statement of main terms of employment for a Data Analyst role, including salary, hours, holiday entitlement, and notice period. |
| essay on attention.md | Reading | ✓ Reading | personal (exact) | On paying attention | An essay arguing that attention is a habit rather than a spotlight, saved from a magazine website in 2024. |
| flight booking lisbon.txt | Travel | ✓ Travel | personal (exact) | Flight booking: London Gatwick to Lisbon | Confirmed booking details for a round-trip flight from London Gatwick to Lisbon, including dates, times, and cabin bag allowance. |
| fund report q2.txt | Finances | ✓ Finances | personal (exact) | Greenfield Index Funds: Q2 2025 report | Quarterly performance report for a Global Equity Index fund holding, showing unit price, quarterly return, and annual costs. |
| home contents policy.txt | Insurance | ✓ Insurance | restricted (over) | Home contents insurance policy | A text file containing the policy schedule for home contents insurance, covering contents up to 30,000 with an excess of 100, valid from 1 February 2025 to 31 January 2026. |
| home office expenses.md | Finances or Receipts | ✓ Finances | restricted (over) | Home office expenses | A list of costs for home-working equipment and utilities collected for a self-assessment tax return. |
| hotel booking porto.txt | Travel | ✓ Travel | personal (exact) | Hotel booking Porto | A text file containing a booking confirmation for a double room with a river view and breakfast at Casa do Rio in Porto. |
| internet contract.txt | Home | ✓ Home | personal (exact) | Fibrelink broadband contract | A 24-month broadband contract starting on 12 November 2024, with a speed of up to 500 Mb/s and a monthly price of 29.00. |
| invoice monitor arm.md | Receipts | ✓ Receipts | personal (exact) | Invoice for monitor arm | Invoice for a dual monitor arm purchased on 19 August 2025, showing item price and VAT. Notes that the purchase is paid and claimable as a home-working expense. |
| itinerary portugal.md | Travel | ✓ Travel | personal (exact) | Portugal trip itinerary | A travel plan for a June 2025 trip to Portugal, listing dates, locations, and accommodation details. |
| lab notebook oscillators week 12.md | Research | ✓ Research | personal (exact) | Lab notebook, week 12 | A Markdown file recording results from a coupled-oscillator simulation, including observations on the order parameter and plans to add noise. |
| lemon drizzle cake.txt | Recipes | ✓ Recipes | personal (exact) | Lemon drizzle cake recipe | A recipe for lemon drizzle cake, including ingredients and baking instructions. |
| lentil dal.md | Recipes | ✓ Recipes | personal (exact) | Weeknight red lentil dal | A recipe for red lentil dal with instructions for ingredients and cooking steps. |
| letter from grandma.txt | Letters | ✓ Letters | personal (exact) | Letter from Grandma | A personal letter from the user's grandmother, mentioning snowdrops, the grandad's vegetable patch, and an invitation for Sunday lunch. |
| letter from tax office.txt | Finances | ✓ Finances | restricted (over) | Tax code change notification | A letter from HM Revenue and Customs informing Alex Morgan of a change in their tax code for the 2025 tax year due to a change in benefits. |
| message to landlord.txt | Home | ✓ Home | personal (exact) | Message to landlord.txt | A text message requesting repairs for a dripping kitchen tap and a cracked bathroom window seal. |
| misc notes.txt | Holding | ✓ Holding | personal (exact) | misc notes.txt | A text file containing brief reminders and questions, including a request for a callback, a quantity of batteries, and an inquiry about a film. |
| old us tax form.txt | Finances | ✓ Finances | restricted (exact) | 2020 U.S. Individual Income Tax Return | A 2020 U.S. Individual Income Tax Return (Form 1040) filed on April 15, 2021, for work done in Boston. It includes personal details and total income. |
| order confirmation laptop.txt | Receipts | ✓ Receipts | restricted (over) | Laptop order confirmation | Confirmation of a purchase for an Aero 14 laptop and a USB-C dock, including payment details and warranty information. |
| paper kuramoto synchronization.txt | Reading | ✓ Reading | personal (exact) | paper kuramoto synchronization.txt | A text file containing an abstract and description of a scientific paper on coupled oscillators and synchronization. |
| payslip october.txt | Work | ✓ Work | restricted (over) | October 2025 payslip | A payslip from Lumen Analytics Ltd showing pay details for October 2025, including basic pay, tax, National Insurance, and pension deductions, with net pay and bank account information. |
| pension annual statement.txt | Finances | ✓ Finances | restricted (over) | Harbour Pensions annual benefit statement | Annual statement showing the value of the pension pot, contributions made this year, and projected annual pension income at age 67. |
| performance review 2025.md | Work | ✓ Work | personal (exact) | Performance review 2025 | A mid-year performance review document outlining strengths, areas for development, and a rating from a reviewer. |
| phone repair receipt.txt | Receipts | ✓ Receipts | personal (exact) | Phone repair receipt | A receipt for a screen replacement and screen protector on a Pixel 8 phone, with a 90-day guarantee. |
| physio appointment.txt | Health | ✓ Health | personal (under) | Physiotherapy appointment confirmation | A text file confirming a physiotherapy appointment at Riverside Physiotherapy on Thursday 9 October 2025 at 10:30 with Sam Okafor for lower back pain follow-up. |
| plant monitor readme.md | Projects | ✓ Projects | personal (exact) | plant monitor readme.md | Instructions for building and using a soil moisture monitor for houseplants based on an ESP32 and capacitive sensor. |
| prescription amoxicillin.txt | Health | ✓ Health | restricted (over) | Amoxicillin prescription | A text file containing a prescription for amoxicillin capsules, dated 14 January 2025, issued by Oakfield Medical Practice for patient Alex Morgan. |
| receipt and manual blender.txt | Receipts or Manuals | ½ Holding | personal (exact) | PowerBlend 600 receipt and manual | A receipt and quick guide for a Kitchen Plus PowerBlend 600 blender purchased on 12 April 2025, including usage instructions and warranty information. |
| receipt desk chair.txt | Receipts | ✓ Receipts | personal (exact) | Receipt for Ergo Task Chair | A receipt from Furnish & Co. for a grey Ergo Task Chair and a floor protector, totaling £161.50. It notes a 30-day return policy and a 5-year frame guarantee. |
| referral letter for physio.txt | Health | ✓ Health | restricted (over) | Referral letter for physiotherapy | A letter from Oakfield Medical Practice requesting physiotherapy for Alex Morgan's lower back pain. |
| rfc 9110 http semantics notes.md | Reading | ✓ Reading | restricted (over) | RFC 9110: HTTP Semantics | Notes on HTTP methods, status codes, and headers defined by the IETF. |
| router quick start.md | Manuals | ✓ Manuals | personal (exact) | router quick start.md | Instructions for setting up a Fibrelink Hub 3 router, including connecting cables, powering on, and joining the network. |
| short story draft.md | Projects | ✓ Projects | personal (exact) | The Lighthouse Keeper's Daughter (draft 2) | A short story draft about a lighthouse keeper's daughter returning to her father's abandoned lighthouse. |
| sourdough loaf.md | Recipes | ✓ Recipes | personal (exact) | Sourdough loaf recipe | A recipe for a sourdough loaf, including ingredients, mixing, folding, shaping, proving, and baking instructions, with a personal note on water quantity. |
| talk slides outline.md | Research | ✓ Research | personal (exact) | Group meeting talk outline | An outline for a 15-minute presentation on tissue synchronization, covering models, simulations, and next steps. |
| tax return summary 2024.md | Finances | ✓ Finances | restricted (over) | Tax return summary 2024 | A summary of the 2023 to 2024 self-assessment tax return, showing employment income, interest, tax paid, and confirming no balance is owed. |
| team meeting notes.md | Work | ✓ Work | personal (exact) | Data team weekly meeting notes | Notes from a weekly meeting of the data team, covering the status of the churn model, warehouse migration timeline, and hiring plans. |
| tenancy agreement.md | Home | ✓ Home | restricted (over) | Tenancy agreement for Flat 4, 17 Oakfield Road | A 12-month assured shorthold tenancy agreement between Alex Morgan and Oakfield Lettings Ltd, specifying rent of £950/month and a deposit of £1,096 held in a protection scheme. |
| train tickets porto.txt | Travel | ✓ Travel | personal (exact) | Train tickets Porto | A text file containing a train ticket from Lisbon to Porto. |
| travel insurance certificate.txt | Insurance | ✓ Insurance | sensitive (exact) | Travel insurance certificate | A certificate of insurance for a single trip to Portugal in June 2025, covering medical expenses and cancellation fees. |
| travel insurance claim lost bag.md | Insurance | ✓ Insurance | restricted (over) | Lost luggage claim | A claim to Wayfarer Travel Insurance regarding a lost bag on a flight from Lisbon, including details of contents and attached receipts. |
| untitled scan text.txt | Holding | ✓ Holding | personal (exact) | untitled scan text.txt | A text file containing a fragment of a legal agreement regarding item custody. |
| vaccination record.md | Health | ✓ Health | restricted (over) | Vaccination record | A list of vaccines received with their dates, copied from a medical practice record for travel purposes. |
| wifi and alarm codes.txt | Home | ½ Holding | restricted (exact) | wifi and alarm codes.txt | House-sitting instructions including bin schedule, heating timer, cat feeding, and access codes. |
