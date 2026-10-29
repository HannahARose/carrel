---
carrel: card
format: "1.0"
id: 01a18701-0500-776a-9867-080924eb019c
kind: bundle
title: "oscillator-sim"
item: "oscillator-sim"
sealed: hard
summary: "A tiny Rust program that simulates two coupled phase oscillators and reports their phase difference."
tags: [simulation, oscillators]
links:
  - type: derived-from
    id: 01a18701-0500-776a-9867-0805d715b667
    path: "../Reading/Coupled oscillators note.pdf.md"
dates:
  created: "2026-09-01"
  added: "2026-10-24"
  worked: "2026-10-28"
sensitivity: personal
size: 613
manifest:
  checksum: "sha256:7a9feb9bcd69e47ac76d51369e18a9db7b11db96d7494ab90d939a73cf8dac52"
  path: "_System/manifests/01a18701-0500-776a-9867-080924eb019c.tsv"
checked_out:
  to: "~/Desktop/oscillator-sim"
  device: 01a18701-0500-776a-9867-0800c9e7e287
  person: 01a18701-0500-776a-9867-07ffadd03c6b
  since: "2026-10-27T09:12:00-07:00"
  manifest: "sha256:7a9feb9bcd69e47ac76d51369e18a9db7b11db96d7494ab90d939a73cf8dac52"
---

Sealed because it is a Rust project (`Cargo.toml`). Currently checked out to the Desktop.

<!-- carrel:history -->
## History

> [!pending] 2026-10-28 · 2h 10m · auto-generated
> Added a coupling-strength parameter and printed the phase difference every 50 time units.
> Commits: "add coupling parameter", "print phase difference"

### 2026-10-27 · 45m
Set up the project and the basic Euler integration loop.
My notes: try a smaller time step next.
<!-- /carrel:history -->
