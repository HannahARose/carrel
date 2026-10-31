//! `carrel backup`: snapshots of the archive's contents with Kopia (`docs/design/backups.md`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::backup::{BackupError, Backups, KOPIA_PREFIX, Kopia, record};
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock};
use carrel_core::history::{Change, Entry, History, Operation};
use carrel_core::ingest::filing::Outcome;
use serde_json::{Value, json};

use crate::commands::{find_item, holder, local_time, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// This archive's backups on this device, and a line to show when there are none: a hint to
/// set them up, or why the ones set up can't be used.
pub fn for_batch(archive: &Archive) -> (Option<Kopia>, Option<String>) {
    match Kopia::for_archive(archive) {
        Ok(kopia) => (Some(kopia), None),
        Err(BackupError::NotSetUp) if archive.settings().backups.is_none() => (
            None,
            Some(
                "No backups are set up, so no snapshot was taken; see `carrel backup setup`."
                    .into(),
            ),
        ),
        Err(error) => (None, Some(format!("No snapshot was taken: {error}"))),
    }
}

/// What a filing batch did about backups, as a line for people.
pub fn describe_outcome(outcome: &Outcome, note: Option<&str>) -> Option<String> {
    if outcome.filed.is_empty() {
        return None;
    }
    if let Some(snapshot) = &outcome.snapshot {
        return Some(format!("Snapshot {} taken.", snapshot.trailer()));
    }
    if let Some(error) = &outcome.snapshot_error {
        return Some(format!(
            "No snapshot was taken ({error}); the next one will cover these items."
        ));
    }
    note.map(str::to_owned)
}

/// The password from `KOPIA_PASSWORD`, when the owner supplies it that way.
fn supplied_password() -> Option<String> {
    std::env::var("KOPIA_PASSWORD")
        .ok()
        .filter(|p| !p.is_empty())
}

/// Asks for the repository password without echoing it, twice for a new repository.
fn password(new: bool) -> Result<String, Failure> {
    let first = rpassword::prompt_password(if new {
        "New backup password: "
    } else {
        "Backup password: "
    })
    .map_err(|e| Failure::new(format!("couldn't read the password: {e}")))?;
    if first.is_empty() {
        return Err(Failure::new("the password can't be empty"));
    }
    if new {
        let again = rpassword::prompt_password("Same password again: ")
            .map_err(|e| Failure::new(format!("couldn't read the password: {e}")))?;
        if again != first {
            return Err(Failure::new(
                "the passwords don't match; nothing was set up",
            ));
        }
    }
    Ok(first)
}

/// Whether `repository` holds a backup repository already, so setting up connects to it
/// rather than creating one.
#[must_use]
pub fn exists(repository: &Path) -> bool {
    repository.join("kopia.repository.f").exists()
}

/// What to tell the owner before they choose a password for a new repository.
pub const PASSWORD_WARNING: &str = "Backups are encrypted with a password. Keep it in a \
    password manager: if it's lost, the backups can't be read.";

/// `carrel backup setup PATH`: creates or connects to a repository for this archive. The
/// password is `given` (the interactive mode asks for it itself), else `KOPIA_PASSWORD`, else
/// asked for on the terminal. Except from `KOPIA_PASSWORD`, it's kept in this device's
/// keychain by Kopia, never by Carrel.
pub fn setup(
    archive: Option<&Path>,
    repository: &Path,
    description: Option<&str>,
    given: Option<&str>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    if Kopia::program().is_none() {
        return Err(BackupError::NotInstalled.into());
    }
    let identity = Identity::load_or_create()?;
    let new = !exists(repository);
    let supplied = given.is_none().then(supplied_password).flatten();
    let password = match (given, &supplied) {
        (Some(given), _) => given.to_owned(),
        (None, Some(password)) => password.clone(),
        (None, None) => {
            if new {
                eprintln!("{PASSWORD_WARNING}");
            }
            password(new)?
        }
    };
    let keep = supplied.is_none();
    let config = Kopia::config_for(&archive)
        .ok_or_else(|| Failure::new("this device has no folder for Carrel's configuration"))?;
    let (_, setup) = Kopia::setup(
        &archive,
        config,
        repository,
        &password,
        keep,
        identity.device,
    )?;
    let description = description.map_or_else(
        || {
            repository
                .file_name()
                .map_or_else(|| "Backups".into(), |n| n.to_string_lossy().into_owned())
        },
        str::to_owned,
    );
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "backup", DEFAULT_LEASE)?;
    let recorded = (|| -> Result<Option<String>, Failure> {
        record(&archive, &setup.repository, &description)
            .map_err(|e| Failure::io(&archive.root().join("_System/settings.json"), &e))?;
        let change = Change::new(
            Operation::Edit,
            format!("Back up to “{description}”"),
            identity.person,
            identity.device,
        );
        Ok(History::open(&archive)?.commit(&archive, &change, &lease)?)
    })();
    lease.release()?;
    recorded?;

    let mut text = format!(
        "{} the backup repository at {}\n",
        if setup.created {
            "Created"
        } else {
            "Connected to"
        },
        repository.display()
    );
    text.push_str(if !keep {
        "The password came from KOPIA_PASSWORD, so it isn't stored; supply it the same way each time.\n"
    } else if cfg!(any(target_os = "macos", windows)) {
        "Kopia keeps the password in this device's keychain.\n"
    } else {
        "Kopia keeps the password in the system keyring, or where there is none, in a file \
         beside its configuration that only you can read.\n"
    });
    if setup.same_disk {
        text.push_str(
            "warning: the repository is on the same disk as the archive. It guards against \
             mistakes, but not against losing the disk; an external drive is safer.\n",
        );
    }
    text.push_str(
        "Each approval batch now takes a snapshot. Take one now with `carrel backup now`.\n",
    );
    Ok(Report::new(
        text,
        json!({
            "created": setup.created,
            "repository": setup.repository,
            "path": repository,
            "same_disk": setup.same_disk,
        }),
    ))
}

/// `carrel backup now`: a snapshot outside any batch, recorded as a history commit.
pub fn now(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let kopia = Kopia::for_archive(&archive)?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "backup", DEFAULT_LEASE)?;
    let result = (|| -> Result<_, Failure> {
        let snapshot = kopia.snapshot(archive.root(), "Snapshot on request")?;
        let mut change = Change::new(
            Operation::Edit,
            "Snapshot on request",
            identity.person,
            identity.device,
        );
        change.snapshot = Some(snapshot.trailer());
        // History records a snapshot only with a change; with nothing changed, the snapshot
        // still exists in the repository.
        let commit = History::open(&archive)?.commit(&archive, &change, &lease)?;
        Ok((snapshot, commit))
    })();
    lease.release()?;
    let (snapshot, commit) = result?;
    let mut text = format!("Snapshot {} taken", snapshot.trailer());
    if let (Some(size), Some(files)) = (snapshot.size, snapshot.files) {
        write!(text, ": {files} files, {}", human_bytes(size)).unwrap();
    }
    text.push('\n');
    Ok(Report::new(
        text,
        json!({"snapshot": snapshot.id, "time": snapshot.time, "commit": commit}),
    ))
}

/// The history entries that name a snapshot, newest first.
fn snapshot_commits(archive: &Archive) -> Result<Vec<Entry>, Failure> {
    Ok(History::open(archive)?
        .log()?
        .into_iter()
        .filter(|e| {
            e.snapshot
                .as_deref()
                .is_some_and(|s| s.starts_with(KOPIA_PREFIX))
        })
        .collect())
}

/// `carrel backup list`: snapshots, newest first, with the change each was taken with.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let kopia = Kopia::for_archive(&archive)?;
    let mut snapshots = kopia.list(archive.root())?;
    snapshots.reverse();
    let commits = snapshot_commits(&archive)?;
    let commit_of = |id: &str| {
        commits
            .iter()
            .find(|e| e.snapshot.as_deref() == Some(&format!("{KOPIA_PREFIX}{id}")))
    };
    let mut text = String::new();
    if snapshots.is_empty() {
        text.push_str("No snapshots yet\n");
    }
    for snapshot in &snapshots {
        let with = commit_of(&snapshot.id).map_or_else(
            || snapshot.description.clone().unwrap_or_default(),
            |e| format!("{} ({})", e.summary, &e.id[..10]),
        );
        writeln!(text, "{}  {}  {with}", snapshot.time, snapshot.id).unwrap();
    }
    let json: Vec<Value> = snapshots
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "time": s.time,
                "description": s.description,
                "pinned": s.pinned,
                "size": s.size,
                "files": s.files,
                "commit": commit_of(&s.id).map(|e| e.id.clone()),
            })
        })
        .collect();
    Ok(Report::new(
        text,
        json!({"repository": kopia.describe(), "snapshots": json}),
    ))
}

/// `carrel backup verify [--percent N]`.
pub fn verify(archive: Option<&Path>, percent: u8) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let kopia = Kopia::for_archive(&archive)?;
    let found = kopia.verify(percent)?;
    Ok(Report::new(
        format!(
            "Backups check out ({}, reading back {percent}% of files)\n{found}\n",
            kopia.describe()
        ),
        json!({"ok": true, "percent": percent, "details": found}),
    ))
}

/// `carrel backup restore ITEM [--at COMMIT] [--to FOLDER]`: an item's contents as they were,
/// restored beside the archive. Nothing in the archive is changed.
pub fn restore(
    archive: Option<&Path>,
    query: &str,
    at: Option<&str>,
    to: Option<&Path>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let kopia = Kopia::for_archive(&archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    let path = item
        .item
        .clone()
        .ok_or_else(|| Failure::new(format!("{} has no contents to restore", item.card)))?;
    let commits = snapshot_commits(&archive)?;
    let entry = match at {
        Some(at) => {
            let log = History::open(&archive)?.log()?;
            let position = log
                .iter()
                .position(|e| e.id.starts_with(at))
                .ok_or_else(|| Failure::new(format!("no change {at} in the history")))?;
            // The snapshot taken with that change, or the latest one before it.
            log[position..]
                .iter()
                .find(|e| e.snapshot.is_some())
                .cloned()
                .ok_or_else(|| Failure::new(format!("no snapshot was taken by change {at}")))?
        }
        None => commits
            .first()
            .cloned()
            .ok_or_else(|| Failure::new("no snapshot has been taken yet"))?,
    };
    let snapshot = entry
        .snapshot
        .as_deref()
        .and_then(|s| s.strip_prefix(KOPIA_PREFIX))
        .ok_or_else(|| Failure::new("that change's snapshot isn't a Kopia snapshot"))?;
    let target = match to {
        Some(to) => to.to_owned(),
        None => beside(&archive, &path, &entry),
    };
    kopia.restore(snapshot, &path, &target)?;
    Ok(Report::new(
        format!(
            "Restored {path} as of “{}” ({}) to {}\nNothing in the archive changed; compare, then \
             move it in yourself if you want it.\n",
            entry.summary,
            local_time(entry.time),
            target.display()
        ),
        json!({"item": item.id, "snapshot": snapshot, "commit": entry.id, "to": target}),
    ))
}

/// A free folder beside the archive for a restored item: `NAME (restored 2026-10-29)`.
fn beside(archive: &Archive, path: &str, entry: &Entry) -> PathBuf {
    let parent = archive
        .root()
        .parent()
        .map_or_else(|| archive.root().to_owned(), Path::to_owned);
    let name = path.rsplit('/').next().unwrap_or(path);
    let date = jiff::Timestamp::from_second(entry.time)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default();
    let mut target = parent.join(format!("{name} (restored {date})"));
    let mut n = 2;
    while target.exists() {
        target = parent.join(format!("{name} (restored {date}, {n})"));
        n += 1;
    }
    target
}

/// The line `carrel status` shows about backups, from history alone so it needs no drive.
pub fn status_line(archive: &Archive, history: Option<&[Entry]>) -> String {
    let last = history.and_then(|log| {
        log.iter().find(|e| {
            e.snapshot
                .as_deref()
                .is_some_and(|s| s.starts_with(KOPIA_PREFIX))
        })
    });
    match (archive.settings().backups.as_ref(), last) {
        (None, _) => "No backups set up (see `carrel backup setup`)\n".into(),
        (Some(_), None) => "Backups set up; no snapshot yet\n".into(),
        (Some(backups), Some(entry)) => format!(
            "Last snapshot {} to {}\n",
            local_time(entry.time),
            backups["description"]
                .as_str()
                .unwrap_or("the backup repository")
        ),
    }
}

/// `1.2 GB`.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    #[allow(clippy::cast_precision_loss)] // a display value
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
