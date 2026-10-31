# Benchmark report

2026-10-30 on macos aarch64, carrel 0.1.0.

## Summary

| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | s/item | Meets the bar |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| apple/Apple Foundation Model then llama/gemma-4-e4b.gguf | 100% | 89% | 11% | 89% | 89% | 2% | 0 | 74% | 85% | 68 | 9.7 | yes |

## apple/Apple Foundation Model then llama/gemma-4-e4b.gguf (provider 0.1.0)

| Item | Expected | Proposed | Sensitivity | Title | Summary |
| --- | --- | --- | --- | --- | --- |
| 2025 bank statement March.txt | Finances | ✓ Finances | restricted (exact) | 2025 bank statement March | A bank statement showing financial transactions for the period from 1 March 2025 to 31 March 2025, including rent, salary, grocery expenses, and a transfer to savings. |
| amplifier build log.txt | Projects | ✓ Projects | personal (exact) | Headphone amplifier build log | A build log for a headphone amplifier, detailing soldering steps, board etching, hole drilling, and initial testing with a slight hum. |
| birthday card from sister.txt | Letters | ✓ Letters | personal (exact) | Birthday card from sister | A personal message from a sister for a birthday, including a chess reference and an invitation for dinner. |
| blood test results.txt | Health | ✓ Health | sensitive (exact) | blood test results.txt | Lab results reported 7 March 2025 for Alex Morgan, showing low iron stores. |
| boiler repair note.txt | Home | ✓ Home | personal (exact) | Boiler repair note | Engineer's note about boiler pressure loss, valve replacement, system refill, and weekly gauge checks. |
| budget app notes.md | Projects | ✓ Projects | personal (exact) | budget app: design notes | A small terminal app to categorise bank exports by reading CSV exports, guessing categories from the payee, and storing rules in a TOML file with a monthly summary as a table. |
| building notice water off.txt | Home | ✓ Home | personal (exact) | water off.txt | Notice to residents about water supply interruption for valve replacement. |
| car insurance renewal.txt | Insurance | ✓ Insurance | sensitive (exact) | Car insurance renewal | Car insurance renewal notice with new premium and policy details. |
| card replacement letter.txt | Finances | ✓ Finances | restricted (exact) | Replacement debit card | A letter from Northbridge Bank containing a replacement debit card and instructions to sign it and discard the old card. |
| cell biology review.txt | Reading | ✓ Reading | personal (exact) | Circadian clocks in single cells: a review | A review of circadian clocks in single cells, detailing transcriptional feedback loops, single-cell imaging variability, tissue coupling, light and feeding resets. |
| claim physio costs.md | Insurance | ✗ Holding | sensitive (exact) | Claim for physio costs | A claim file detailing a request for reimbursement of 3 physiotherapy sessions for lower back pain, with attached invoices and a reference to be sent by email. |
| coffee machine descaling.txt | Manuals | ✓ Manuals | personal (exact) | Barista Mini descaling instructions | Instructions for descaling a Barista Mini coffee machine, including steps for emptying the drip tray, filling the tank, using descaler, holding buttons, and rinsing. |
| conference registration.txt | Research or Work or Travel | ✓ Work | personal (exact) | Conference registration | Confirmation of registration for a conference event including participant details, poster title, and fee information. |
| cv alex morgan.md | Work | ✓ Work | personal (exact) | cv alex morgan | A CV for Alex Morgan detailing experience as a data analyst and education in applied statistics. |
| darwin origin chapter 4 notes.md | Reading | ✓ Reading | personal (exact) | Darwin origin chapter 4 notes | Summary of Darwin's chapter on natural selection from 'On the Origin of Species', including key ideas on variation, sexual selection, and the diagram of divergence. |
| dataset description.md | Research | ✓ Research | personal (exact) | Cell imaging dataset | A dataset of cell imaging records from May 2025, showing 48 wells with one image every 30 minutes for 5 days, stored as TIFF stacks and summarized in a CSV with columns for well, cell, time, intensity. |
| dishwasher manual.txt | Manuals | ✓ Manuals | personal (exact) | QuietClean 45 slimline dishwasher: user manual | User manual for QuietClean 45 slimline dishwasher, detailing setup steps and cleaning instructions for different programs. |
| draft paper phase response.md | Research | ✓ Research | personal (exact) | phase response curves of weakly coupled cells (draft) | A draft document outlining phase response curves in cultured cells, including methods and co-authors. |
| drill safety and warranty.txt | Manuals | ✓ Manuals | personal (exact) | drill safety and warranty | Safety instructions and warranty for a powerLine 18V cordless drill, including battery usage and warranty details. |
| electricity bill september.txt | Home | ✓ Home | personal (exact) | electricity bill september | Electricity bill for the period 1 to 30 September 2025, showing usage, cost and payment details. |
| email from sam about visit.txt | Letters | ✗ Travel | personal (exact) | email from sam about visit.txt | An email from Sam confirming a train booking and suggesting a walk along the coast. |
| employment contract lumen.md | Work | ✓ Work | sensitive (exact) | Employment terms agreement | A formal agreement outlining employment terms, including job title, salary, hours, holiday entitlement, and notice period. |
| essay on attention.md | Reading | ✗ Projects | personal (exact) | essay on attention | An essay about attention as a habit rather than a spotlight, with the idea that we become what we keep returning to. |
| flight booking lisbon.txt | Travel | ✓ Travel | personal (exact) | Flight booking lisbon | Flight booking for London Gatwick to Lisbon, confirmed with departure and return times and cabin bag included. |
| fund report q2.txt | Finances | ✓ Finances | personal (exact) | fund report q2.txt | Quarterly report for investors on the Global Equity Index fund, including unit price, cost, and index details. |
| home contents policy.txt | Insurance | ✓ Insurance | sensitive (exact) | home contents policy | Home contents insurance policy for a flat, covering contents up to 30,000 with accidental damage included, premium 11.40 a month from 1 February 2025 to 31 January 2026. |
| home office expenses.md | Finances or Receipts | ½ Holding | restricted (over) | Home-working expenses for the tax return | List of home office expenses for the tax return, including monitor arm, desk chair, and share of electricity. |
| hotel booking porto.txt | Travel | ✓ Travel | personal (exact) | Hotel booking porto | Hotel stay at Casa do Rio, Porto from 18 June 2025 to 21 June 2025, double room with river view and breakfast included. |
| internet contract.txt | Home | ✓ Home | restricted (over) | Internet contract | A 24-month broadband contract with Fibrelink, including speed, price, and router details. |
| invoice monitor arm.md | Receipts | ✓ Receipts | personal (exact) | Dual monitor arm receipt | A receipt for a dual monitor arm with clamp mount, including VAT and a note that it is payable as a home-working expense. |
| itinerary portugal.md | Travel | ✓ Travel | personal (exact) | itinerary portugal | Itinerary for a trip to Portugal in June 2025, including travel dates, accommodations, and transport plans. |
| lab notebook oscillators week 12.md | Research | ✓ Research | personal (exact) | Lab notebook oscillators week 12 | A text/markdown file detailing a coupled-oscillator simulation run with N = 500 and K from 0 to 4, noting a sharp rise in order parameter near K = 1.6 matching Lorentzian theory, and a next step to add noise and test threshold movement. |
| lemon drizzle cake.txt | Recipes | ✓ Recipes | personal (exact) | lemon drizzle cake | Recipe for lemon drizzle cake with ingredients and baking instructions. |
| lentil dal.md | Recipes | ✓ Recipes | personal (exact) | Weeknight red lentil dal | Recipe for red lentil dal for 4 servings, including ingredients and cooking steps. |
| letter from grandma.txt | Letters | ✓ Letters | personal (exact) | letter from grandma | A personal letter from a grandparent to a family member, mentioning a vegetable patch, photos, and a request for a Sunday lunch. |
| letter from tax office.txt | Finances | ✓ Finances | sensitive (exact) | Letter from tax office | A notice from the tax office informing about a change in the tax code due to employer information about benefits. |
| message to landlord.txt | Home | ✓ Home | personal (exact) | Message to landlord | A request to fix a dripping kitchen tap and a cracked bathroom window, with a time requirement for a visit. |
| misc notes.txt | Holding | ✓ Holding | personal (exact) | misc notes | A list of tasks and questions to address, including a call back about a specific item, a reminder about 3 AA, and a question about a film mentioned by Priya. |
| old us tax form.txt | Finances | ✓ Finances | restricted (exact) | U.S. Individual Income Tax Return 2020 | U.S. Individual Income Tax Return for 2020, filed from the year worked in Boston. |
| order confirmation laptop.txt | Receipts | ✗ Manuals | personal (exact) | Order confirmation laptop | A confirmation for a laptop and dock purchase, including delivery and payment details, with a two-year warranty. |
| paper kuramoto synchronization.txt | Reading | ✓ Reading | personal (exact) | paper kuramoto synchronization.txt | A scientific paper discussing the Kuramoto model and synchronization in populations of coupled oscillators, including a phase transition description. |
| payslip october.txt | Work | ✓ Work | restricted (over) | payslip october | A payslip showing basic pay, income tax, National Insurance, pension deductions and net pay for the employee. |
| pension annual statement.txt | Finances | ✓ Finances | restricted (over) | pension annual statement | Annual benefit statement for a personal pension investment in the Harbour Balanced Growth fund, showing contributions and projected annual income at age 67. |
| performance review 2025.md | Work | ✓ Work | personal (exact) | performance review 2025 | A mid-year performance review document outlining strengths, development tasks, and expectations for promotion. |
| phone repair receipt.txt | Receipts | ✓ Receipts | personal (exact) | phone repair receipt | Receipt for phone repairs including screen replacement and screen protector. |
| physio appointment.txt | Health | ✓ Health | personal (under) | physio appointment | A confirmed appointment for a follow-up with a physiotherapist regarding lower back pain. |
| plant monitor readme.md | Projects | ✓ Projects | personal (exact) | plant monitor | A soil moisture monitor for houseplants: an ESP32 reads a capacitive sensor every 10 minutes and posts the reading to a small web page. Includes instructions for building the firmware and wiring the sensor. |
| prescription amoxicillin.txt | Health | ✓ Health | sensitive (exact) | prescription amoxicillin.txt | Prescription for Amoxicillin 500 mg capsules, one three times a day for 7 days, prescribed by Dr P. Shah for a chest infection. |
| receipt and manual blender.txt | Receipts or Manuals | ✓ Manuals | personal (exact) | PowerBlend 600 blender | A receipt and manual for a PowerBlend 600 blender, including instructions and a two-year guarantee. |
| receipt desk chair.txt | Receipts | ✓ Receipts | personal (exact) | receipt for chair and floor protector | A receipt for a chair and floor protector purchase from FURNISH & CO., including a total cost and return policy. |
| referral letter for physio.txt | Health | ½ Holding | sensitive (exact) | Referral letter for physio | A letter requesting physiotherapy for mechanical lower back pain of six weeks' duration, noting no red-flag symptoms and that ibuprofen has helped slightly. The letter may be used for an insurance claim. |
| rfc 9110 http semantics notes.md | Reading | ✓ Reading | restricted (over) | HTTP Semantics | Defines methods, status codes and headers shared by every version of HTTP. |
| router quick start.md | Manuals | ✓ Manuals | personal (exact) | Fibrelink Hub 3: quick start | A quick start guide for setting up the Fibrelink Hub 3, including connection steps and troubleshooting. |
| short story draft.md | Projects | ✓ Projects | personal (exact) | The Lighthouse Keeper's Daughter (draft 2) | A draft of a short story about a lighthouse keeper's daughter, with notes to cut the opening paragraph and move the storm earlier. |
| sourdough loaf.md | Recipes | ✓ Recipes | personal (exact) | sourdough loaf | Recipe for sourdough loaf including ingredients, mixing steps, resting time, folding instructions, shaping, proofing, baking conditions and water adjustment note. |
| talk slides outline.md | Research | ✗ Work | personal (exact) | Group meeting talk outline | Outline for a group meeting talk on 14 November 2025 about tissue synchronization, a model threshold, simulations, and next steps. |
| tax return summary 2024.md | Finances | ✓ Finances | sensitive (exact) | tax return summary 2024 | A summary of tax income and payments for the 2023 to 2024 period, showing no balance to pay. |
| team meeting notes.md | Work | ✓ Work | personal (exact) | team meeting notes | Team meeting notes for a weekly discussion on churn model status, warehouse migration delay, and hiring for junior analyst role. |
| tenancy agreement.md | Home | ✓ Home | sensitive (exact) | Assured shorthold tenancy agreement | A written agreement detailing the terms of a 12-month tenancy from 1 August 2023, followed by a month-to-month arrangement, including rent, deposit, and payment schedule. |
| train tickets porto.txt | Travel | ✓ Travel | personal (exact) | Train ticket porto | Train ticket for a journey from Lisboa Santa Apolonia to Porto Campanha on 18 June 2025, departing at 09:39 and arriving at 12:24. Coach 4, seat 61. |
| travel insurance certificate.txt | Insurance | ✓ Insurance | sensitive (exact) | Travel insurance certificate | Travel insurance certificate for a single trip to Portugal, covering medical expenses and cancellation. |
| travel insurance claim lost bag.md | Insurance | ✓ Insurance | sensitive (exact) | Lost luggage claim | Claim for lost bag containing clothes and a camera, reported on the flight back from Lisbon on 24 June. |
| untitled scan text.txt | Holding | ✗ Work | sensitive (over) | Custody agreement - schedule B | A legal agreement regarding custody of an item from schedule B. |
| vaccination record.md | Health | ✓ Health | sensitive (exact) | vaccination record.md | A file containing a table of vaccinations with dates. |
| wifi and alarm codes.txt | Home | ✓ Home | restricted (exact) | Flat info for house-sitters | Flat info for house-sitters: bins go out Tuesday night, heating on timer, cat eats half a pouch twice a day. |
