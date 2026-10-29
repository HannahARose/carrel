---
carrel: archive
format: "1.0"
id: 01a18701-0500-776a-9867-07fe6eb3d36a
name: "Sample archive"
link_types:
  - name: supersedes
    description: "A newer document that replaces an older one"
tags:
  - name: draft
    description: "Work in progress"
  - name: reference
    description: "Material kept for looking things up"
  - name: oscillators
    description: "Coupled oscillators and synchronization, across reading and projects"
sensitivity_default: personal
size_threshold: 10737418240
min_collection_items: 5
---

# Sample archive

A small archive that exercises every part of the Carrel format: each item kind, sealing, links, checkout, history entries, a tombstone and a naming clash.

## Conventions

Collection and file names are plain words. Dates in file names are written `YYYY-MM-DD`. Subcollections go at most three levels deep.

<!-- carrel:collections -->
## Collections

| Collection | Items | Description |
| --- | --- | --- |
| [Holding](Holding/_collection.md) | 1 | Items waiting for a better home |
| [Projects](Projects/_collection.md) | 3 (+4 in subcollections) | Things being made: code, notes and data |
| [Reading](Reading/_collection.md) | 4 | Papers, books and web pages kept for reference |
<!-- /carrel:collections -->
