# The on-device runner

GitHub's hosted Macs have no Apple Intelligence and too little memory for local models, so the tests that need them, and the weekly model benchmark, run on the owner's Mac, registered as a self-hosted runner. The design is `docs/design/on-device-ci.md`; the workflow is [`.github/workflows/on-device.yml`](../../.github/workflows/on-device.yml).

| Job | When | What |
| --- | --- | --- |
| On-device tests | every push to `dev` that changes code, and by hand | [`scripts/on-device-tests.sh`](../../scripts/on-device-tests.sh): the Apple provider with its on-device model, the llama.cpp provider with a real model, and a five-item benchmark with every installed model |
| Benchmark | Sundays at 03:00, and by hand | `carrel bench` over the whole test set, compared with the last scheduled or manual run |

The jobs never run for pull requests, so code nobody has reviewed never runs on the Mac. CI's workflow audit fails any workflow that names the runner's label and runs on pull requests.

## Setting it up

This is done once, by the owner, on the Mac. It needs an administrator account for the first two steps.

1. **Create a standard user** for the runner, such as `carrel-ci`, in System Settings → Users & Groups. Don't make it an administrator, and don't sign it into your Apple Account or iCloud. The runner then can't read your files or your keychain.
2. **Turn on Apple Intelligence for it:** sign in as `carrel-ci` once, and turn it on in System Settings → Apple Intelligence & Siri, so the on-device model is available to that user.
3. **Install the tools** as `carrel-ci`: the Xcode command-line tools (`xcode-select --install`), [rustup](https://rustup.rs) and the [GitHub CLI](https://cli.github.com) (`gh`, used to fetch the previous benchmark results). Carrel's `rust-toolchain.toml` picks the Rust version.
4. **Download the models** as `carrel-ci`, so they're in its own models folder:

   ```bash
   carrel models download granite-4.2-3b
   carrel models download qwen3.5-4b
   ```

   Download the rest of the catalog too if the weekly benchmark should cover it.
5. **Register the runner:** on GitHub, open the repository's Settings → Actions → Runners → New self-hosted runner, choose macOS and ARM64, and follow its download and `config.sh` steps as `carrel-ci`. When it asks for labels, add `carrel-on-device`.
6. **Run it as a service, logged in as `carrel-ci`:** `./svc.sh install`, then `./svc.sh start`, in the runner's folder. That makes a LaunchAgent, which runs in `carrel-ci`'s session, as `xcodebuild test` needs for the MLX provider's tests. Keep `carrel-ci` logged in: log in as it, then switch back to your own account with Fast User Switching. With FileVault on, it can't be logged in automatically, so after a restart log it in again; until then, jobs wait in the queue.

   If you'd rather the runner start with the Mac, logged in or not, run it as a LaunchDaemon instead: `sudo sh scripts/install-runner-daemon.sh`, after `./svc.sh uninstall`. Don't run both, since they'd share one work folder. Outside a session, the MLX provider's tests with real models are skipped, with a warning; `scripts/on-device-tests.sh` run by hand includes them.
7. **Try it:** in Actions → On-device → Run workflow, run `tests`, then `benchmark`.

## Living with it

- **One job at a time.** A push that arrives while the benchmark runs waits for it, so nothing compiles during a benchmark. Only the newest waiting run is kept.
- **Your own work affects it.** The benchmark times models, so heavy work on the Mac while it runs, such as building Carrel, makes its timings slower. Sunday at 03:00 is chosen to avoid that. If you need the Mac, stop the runner (`./svc.sh stop`, or for the LaunchDaemon `sudo launchctl bootout system/com.horizon-labs.carrel-runner`), and start it again later; a stopped runner's jobs wait in the queue.
- **Results** are kept as the run's `bench-results` artifact for 90 days, and the run's summary shows each model's scores with the change since the last run. A run fails only when a model that met the bar no longer does.
- **`docs/bench/`** is updated by a pull request when a release changes the recommended models, not by CI.
- **Removing it:** `./svc.sh uninstall` (or, for the LaunchDaemon, `sudo launchctl bootout system/com.horizon-labs.carrel-runner` and delete its plist in `/Library/LaunchDaemons`), then remove the runner in the repository's settings and delete the `carrel-ci` user.
