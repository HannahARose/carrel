# Design: reminders (v0.6)

Status: accepted 2026-11-01, with the five recommendations below. Issue #352.

## What this adds

The archive's `reminders` setting (FORMAT.md §7.2) turns on any of four reminders: `idle`, `weekly`, `menu-bar` and `due-date`, with the first three on by default. Since #342 every interface can change it, but nothing reminds yet. The checkout design left reminders and notifications for the Mac app and its agent (`docs/design/checkout.md`, decision 5). The agent exists now (v0.5), so this design says what each reminder does, and how each interface shows it.

## The reminders

- **`idle`: a work session has ended.** When the agent records a session (it has seen no changes for `sessions.idle_minutes`), it reminds the owner that the entry is waiting: "You worked on “oscillator-sim” for 1h 10m. Its entry is waiting for you." Choosing it opens the item on Active, where the pending entry is approved, edited or deleted. One reminder per session, and none for a session whose entry is already approved.
- **`weekly`: a weekly look at what's waiting.** Once a week, on the first day Carrel runs in that week, one reminder sums up what waits for the owner: proposals, questions, pending history entries, checkouts out for more than two weeks, and backups not taken for a week. It's skipped when nothing is waiting.
- **`due-date`: a checkout is due back.** A checkout with a due date (`carrel checkout --due`) is reminded the day before, on the day, and once a day while it's overdue: "“parts-list” is due back tomorrow." Choosing it opens the item on Active, where it's returned.
- **`menu-bar`: the agent's menu bar list.** On a Mac the agent shows what's checked out, on the desk and pending in the menu bar. With `menu-bar` off it runs without the menu bar icon, and is managed from the app's Settings → Agent.

## In each interface

The logic is in `carrel-core`: `reminders::due(archive, now, shown)` returns the reminders due now, each with its kind, its words, the item it's about and a key that says when it was last shown. Each interface presents them its own way:

- **The command line:** `carrel status` lists the reminders due under "Reminders", and `carrel reminders` lists them alone (`--json` for scripts). `carrel agent` (the background agent on Linux and Windows) shows each as a desktop notification: `notify-send` on Linux when it's installed, and a toast through PowerShell on Windows. Without either, the reminders wait in `carrel status`.
- **The interactive mode:** Status shows the same "Reminders" lines at the top, and Enter on one goes to its item.
- **The Mac app:** the agent posts each as a notification (`UserNotifications`), with the app's permission asked the first time a reminder is due, not at launch. Choosing one opens the item in the app, through the same `carrel://` links the menu bar uses. The app's Status shows them too.

Where a reminder was last shown is kept per device, in the configuration folder (`reminders.json`), not in the archive. So each device reminds its own owner once, and the archive's history isn't filled with "reminded" changes. A reminder dismissed on one Mac may still show on another; for one owner with one or two devices that's acceptable.

## Building it

1. `carrel_core::reminders`: `due`, the per-device record of what was shown, and tests for each kind and the `reminders` setting.
2. `carrel status` and `carrel reminders`; the interactive mode's Status lines.
3. The FFI's `reminders()` and `markShown`; the Mac agent's notifications and the app's Status.
4. `menu-bar` hiding the agent's menu bar icon.
5. `carrel agent`'s notifications on Linux and Windows.

## Decisions (accepted 2026-11-01)

1. **Recommended: the four reminders do what's described above:** `idle` after each recorded session with a pending entry, `weekly` once a week when something waits, `due-date` the day before, on the day and daily while overdue, and `menu-bar` showing the agent's menu bar list. The alternative is fewer kinds, such as `due-date` alone, and dropping the others from FORMAT.md.
2. **Recommended: notifications come only from the agents** (the Mac's login item, and `carrel agent` elsewhere), and every interface lists the reminders due in Status. Commands never interrupt with a notification. The alternative is the command line printing reminders after every command, which is noisy in scripts.
3. **Recommended: what was shown is kept per device, outside the archive.** The alternative is recording it in the archive, so a reminder seen on one device is gone from all of them, at the cost of a history change for each reminder.
4. **Recommended: on Linux, notifications use `notify-send` when it's installed, and on Windows a PowerShell toast; without either, Status is the reminder.** Carrel installs nothing for this. The alternative is no notifications outside the Mac until the Linux and Windows apps (v2.0).
5. **Recommended: the `weekly` reminder comes on the first day Carrel runs in a new week (weeks start on Monday)**, rather than at a fixed hour, since the agent may not be running then. The alternative is a chosen day and hour, as a new setting.
