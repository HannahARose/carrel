//! Provider programs asked again after a passing failure (#347's review): one that couldn't
//! start once, and one whose offer becomes available; and forgetting them while another thread
//! is asking a program that never answers. In a file of their own, since they set how soon
//! programs are asked again for the whole test process, and hold up discovery for seconds.

use std::path::{Path, PathBuf};
use std::time::Duration;

use carrel_core::local::Prefs;
use carrel_core::providers::Registry;
use carrel_core::providers::device::registry_with;

/// Asks again at once, rather than after a minute.
fn retry_at_once() {
    carrel_core::providers::device::retry_after(Duration::ZERO);
}

/// The providers in `registry` whose programs are in `dir`.
fn from_dir(registry: &Registry, dir: &Path) -> Vec<String> {
    registry
        .providers()
        .filter(|p| {
            p.program()
                .is_some_and(|program| program.path.starts_with(dir))
        })
        .map(|p| p.name().to_owned())
        .collect()
}

/// A shell-script provider named `carrel-provider-<name>` in `dir`, offering one text model
/// `<name>`. `before` runs first: it can exit, or set `avail` (`true` unless it does).
#[cfg(unix)]
fn script_provider(dir: &Path, name: &str, before: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join(format!("carrel-provider-{name}"));
    let text = format!(
        r#"#!/bin/sh
avail=true
{before}
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocol":1,"name":"{name}","offers":[{{"capability":"text-model","id":"{name}","priority":1,"available":%s}}]}}}}\n' "$id" "$avail" ;;
    *)
      printf '{{"jsonrpc":"2.0","id":%s,"result":null}}\n' "$id" ;;
  esac
done
"#
    );
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A provider that couldn't start once, as when it timed out under load, is tried again
/// by a later registry, rather than being remembered as failed for the rest of the session.
#[cfg(unix)]
#[test]
fn a_provider_that_failed_to_start_once_is_tried_again() {
    retry_at_once();

    let dir = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let ready = state.path().join("ready");
    script_provider(
        dir.path(),
        "flaky",
        &format!("[ -e '{}' ] || exit 1", ready.display()),
    );
    let folders = [dir.path().to_owned()];

    let (_, failed) = registry_with(&folders, &Prefs::default());
    assert!(
        failed.iter().any(|f| f.found.name == "flaky"),
        "it fails the first time"
    );

    std::fs::write(&ready, "").unwrap();
    let (registry, failed) = registry_with(&folders, &Prefs::default());
    let failed: Vec<String> = failed.iter().map(|f| f.error.to_string()).collect();
    assert_eq!(
        from_dir(&registry, dir.path()),
        ["flaky"],
        "it can start now, but is still reported as failed: {failed:?}"
    );
}

/// A provider whose offers change while its program and the models folder don't, as Apple's
/// does when its own model finishes downloading or Apple Intelligence is turned on, is seen
/// with its new offers by a later registry.
#[cfg(unix)]
#[test]
fn a_provider_whose_offer_becomes_available_is_seen_again() {
    retry_at_once();

    let dir = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let ready = state.path().join("model-ready");
    script_provider(
        dir.path(),
        "onlater",
        &format!("[ -e '{}' ] || avail=false", ready.display()),
    );
    let folders = [dir.path().to_owned()];
    let available = |registry: &Registry| {
        registry
            .providers()
            .find(|p| p.name() == "onlater")
            .map(|p| p.offers()[0].available)
    };

    let (registry, _) = registry_with(&folders, &Prefs::default());
    assert_eq!(available(&registry), Some(false), "not available at first");

    std::fs::write(&ready, "").unwrap();
    let (registry, _) = registry_with(&folders, &Prefs::default());
    assert_eq!(
        available(&registry),
        Some(true),
        "available once its model is ready"
    );
}

/// Forgetting the providers, as the interactive mode's Ctrl-R does on its key thread,
/// returns at once even while another thread is finding them.
#[cfg(unix)]
#[test]
fn forgetting_the_providers_never_waits_for_a_discovery() {
    use carrel_core::local::Prefs;
    use carrel_core::providers::device::{forget_providers, registry_with};

    let dir = tempfile::tempdir().unwrap();
    // Never answers `initialize`, so finding it takes INITIALIZE_TIMEOUT.
    script_provider(dir.path(), "hung", "exec sleep 30");
    let folders = [dir.path().to_owned()];
    std::thread::scope(|scope| {
        scope.spawn(|| drop(registry_with(&folders, &Prefs::default())));
        std::thread::sleep(Duration::from_millis(300));
        let started = std::time::Instant::now();
        forget_providers();
        let took = started.elapsed();
        assert!(took < Duration::from_secs(1), "Ctrl-R waited {took:?}");
    });
}
