# Design: on-device tests on a self-hosted Mac (v0.2)

Status: accepted 2026-10-30, with the five recommendations below. Issue #45.

## What this adds

GitHub's hosted Mac runners have no Apple Intelligence and too little memory for open models, so CI never runs Apple's model, MLX or a real llama.cpp model. Those paths are tested only when someone remembers to, and the benchmark (#33) runs by hand. This design registers the owner's Mac as a self-hosted runner for the jobs that need it.

## Safety

A self-hosted runner runs whatever a workflow tells it to, on the owner's machine. So:

- **Never for pull requests.** The jobs run only on a schedule, by manual dispatch, and on pushes to `dev` (code already reviewed and merged). No workflow with a `pull_request` or `pull_request_target` trigger may name the runner's label; a check in CI's workflow audit refuses one that does.
- **Its own label:** `carrel-on-device`, so no other job lands on it.
- **Its own user account:** the runner runs as a standard (not administrator) macOS user, `carrel-ci`, with no access to the owner's files or keychain, and with its own models folder and test archives. The owner creates the account and registers the runner; the steps are in `docs/dev/on-device-runner.md`.
- **Repository secrets aren't passed** to these jobs, and the checkout keeps no credentials (`persist-credentials: false`).
- **One job at a time,** and the runner's service can be stopped from the menu bar when the owner needs the machine.

## The jobs

A new workflow, `.github/workflows/on-device.yml`:

| Job | When | What | About how long |
| --- | --- | --- | --- |
| **On-device tests** | pushes to `dev`, and manual | `scripts/on-device-tests.sh`: the tests that skip themselves unless pointed at a real provider or model (`CARREL_TEST_APPLE_PROVIDER`, `CARREL_TEST_LLAMA_MODELS`), the Swift provider tests, and a five-item benchmark that drafts with every installed model, MLX included once #24 lands | 5 minutes |
| **Benchmark** | weekly, Sunday 03:00 local time, and manual | `carrel bench` over the full test set, for every installed model | 3–4 hours |

- **Clean timings:** the benchmark job takes a lock the on-device tests also wait on, and a push to `dev` that arrives while it runs queues its tests until the benchmark finishes, so nothing compiles during a benchmark.
- **Progress:** the benchmark job's log shows the same per-model progress lines as `carrel bench`, and a job summary updates after each model.

## Comparing runs

- **Results as artifacts:** each benchmark run uploads `bench-results/` (the per-item JSON lines and the summary), kept for 90 days.
- **Compared with the last run:** the job downloads the previous run's artifact and writes a table to the job summary: each model's valid, routing, under-rated and seconds-per-item shares, with the change since last time.
- **A regression fails the job** when a model that met the bar no longer does, so it shows red on the Actions page. A change in a model's score that doesn't cross the bar is reported, not failed.
- **Results in the repository** stay a deliberate step: `docs/bench/` is updated by a pull request when a release's recommendation changes, not by CI.

## Building it

1. Make the on-device tests runnable with one command (`scripts/on-device-tests.sh`).
2. `on-device.yml` with both jobs, and a guard in CI's workflow audit against `pull_request` workflows using the label.
3. `carrel bench --compare PREVIOUS` for the comparison table and the regression check.
4. `docs/dev/on-device-runner.md`: creating the account, registering the runner, installing models for it, and stopping it.
5. The owner registers the runner, and a manual run of both jobs.

## Decisions (accepted 2026-10-30)

1. **Only schedule, manual dispatch and pushes to `dev`, guarded by a check,** with a dedicated label, a standard user account and no secrets. Recommended.
2. **On-device tests on every push to `dev`; the full benchmark weekly** (Sunday 03:00) and by hand, not on every push. Recommended: the benchmark takes hours and would hold the Mac.
3. **No compiling during a benchmark:** tests queue behind it. Recommended, as the benchmark's clean-timing rule requires.
4. **Compare each run with the last, and fail only when a model drops below the bar.** Recommended.
5. **You create the account and register the runner;** Carrel's repository holds only the workflow and the steps. Recommended: it's your machine and a system change.
