//! `carrel sync`: an archive shared between devices through iCloud Drive
//! (`docs/design/sync-and-mobile.md`). `status` (the default) shows who holds the writer lock,
//! each device's inbox backlog, the holding folder, re-checks waiting, files not downloaded
//! and conflicted copies; `download` fetches an item's or a collection's evicted contents;
//! `keep` sets a collection's `sync.keep_downloaded`; `resolve` settles a conflicted copy as the
//! owner chooses; and `move` moves the archive into iCloud Drive.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::cloud_files::{self, Keep};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, WriterLease};
use carrel_core::icloud_move::{self, size};
use carrel_core::sync_status::{SyncStatus, sync_status};
use serde_json::{Value, json};

use crate::commands::{find_item, holder, locate, plural};
use crate::config::Identity;
use crate::{Failure, Report};

/// What `carrel sync` does.
#[derive(Debug, clap::Subcommand)]
pub enum SyncAction {
    /// Who holds the writer lock and until when, each device's inbox backlog, the holding
    /// folder, re-checks waiting, files not downloaded here and conflicted copies.
    Status,
    /// Download an item's or a collection's contents that iCloud evicted from this device.
    Download {
        /// The item (its identifier, or its card's or contents' path) or the collection.
        target: String,
    },
    /// Keep a collection's items downloaded on every device (`sync.keep_downloaded`), or
    /// show whether it does.
    Keep {
        /// The collection, such as Reading or Projects/Hardware.
        collection: String,
        /// on, off, or default to follow the collection above.
        setting: Option<String>,
    },
    /// Settle a conflicted copy iCloud made: keep the original or the copy; the other goes
    /// to the Trash, and undo can reverse it.
    Resolve {
        /// The conflicted copy, as `carrel sync status` lists it.
        copy: String,
        /// Which to keep: original or copy.
        #[arg(long)]
        keep: String,
    },
    /// Move the archive into iCloud Drive: checks there's room, copies it, checks the copy
    /// against a manifest of the original, then uses the copy from now on. The old copy is
    /// kept until you remove it.
    Move {
        /// Where to put it; a folder of the archive's name in iCloud Drive by default.
        #[arg(long)]
        to: Option<PathBuf>,
        /// Move without asking.
        #[arg(long)]
        yes: bool,
    },
}

/// `carrel sync [ACTION]`.
///
/// # Errors
///
/// [`Failure`] when the archive can't be found or the action fails.
pub fn command(archive: Option<&Path>, action: Option<SyncAction>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    match action.unwrap_or(SyncAction::Status) {
        SyncAction::Status => Ok(status(&archive)),
        SyncAction::Download { target } => download(&archive, &target),
        SyncAction::Keep {
            collection,
            setting,
        } => keep(&archive, &collection, setting.as_deref()),
        SyncAction::Resolve { copy, keep } => resolve(&archive, &copy, &keep),
        SyncAction::Move { to, yes } => move_archive(&archive, to.as_deref(), yes),
    }
}

fn lease(archive: &Archive, purpose: &str) -> Result<WriterLease, Failure> {
    let identity = Identity::load_or_create()?;
    Ok(DiskLock::for_archive(archive).acquire(holder(&identity), purpose, DEFAULT_LEASE)?)
}

/// `carrel sync status`.
fn status(archive: &Archive) -> Report {
    let status = sync_status(archive);
    let this_device = Identity::load().ok().flatten().map(|i| i.device);
    let text = status_text(&status, this_device);
    Report::new(text, status_json(&status))
}

/// The sync status in words, for `carrel sync status` and the short form in `carrel status`.
pub(crate) fn status_text(
    status: &SyncStatus,
    this_device: Option<carrel_core::card::Id>,
) -> String {
    let mut text = String::new();
    if !status.synced {
        text.push_str("Not in a synced folder; `carrel sync move` moves it into iCloud Drive\n");
    }
    text.push_str(&crate::commands::writer_line(
        status.writer.as_ref(),
        this_device,
    ));
    if status.inboxes.is_empty() {
        text.push_str("No inbox entries waiting\n");
    }
    for inbox in &status.inboxes {
        let whose = if this_device.is_some_and(|d| d.to_string() == inbox.device) {
            "this device".to_owned()
        } else {
            format!("device {}", inbox.device)
        };
        writeln!(
            text,
            "Inbox of {whose}: {} waiting",
            plural(inbox.waiting, "entry").replace("entrys", "entries")
        )
        .unwrap();
    }
    let held = &status.holding;
    if held.files > 0 {
        writeln!(
            text,
            "Holding folder: {} ({}), the oldest from {}, waiting for a snapshot{}",
            plural(held.files, "file"),
            size(held.bytes),
            held.oldest.as_deref().unwrap_or("?"),
            if held.warn {
                " — take one on a Mac soon"
            } else {
                ""
            }
        )
        .unwrap();
    }
    if status.rechecks > 0 {
        writeln!(
            text,
            "{} waiting for a stronger device",
            plural(status.rechecks, "re-check")
        )
        .unwrap();
    }
    let away = &status.not_downloaded;
    if away.files.is_empty() {
        text.push_str("Everything is downloaded on this device\n");
    } else {
        writeln!(
            text,
            "{} not downloaded here ({}); they download when needed",
            plural(away.files.len(), "file"),
            size(away.bytes)
        )
        .unwrap();
    }
    if !status.kept.is_empty() {
        writeln!(text, "Kept downloaded: {}", status.kept.join(", ")).unwrap();
    }
    if !status.conflicts.is_empty() {
        writeln!(
            text,
            "\n{} for you to settle with `carrel sync resolve COPY --keep original|copy`:",
            plural(status.conflicts.len(), "conflicted copy").replace("copys", "copies")
        )
        .unwrap();
        for c in &status.conflicts {
            writeln!(text, "  {}  (a version of {})", c.copy, c.original).unwrap();
        }
    }
    text
}

/// What `carrel status` says about syncing: that this is an old copy left behind by a move,
/// and in a synced folder, files not downloaded and conflicted copies.
pub(crate) fn status_lines(archive: &Archive, synced: bool) -> String {
    let mut text = String::new();
    if let Some(to) = icloud_move::moved_to(archive.root()) {
        writeln!(
            text,
            "This is the old copy: the archive was moved to {}",
            to.display()
        )
        .unwrap();
    }
    if synced {
        let away = cloud_files::not_downloaded(archive.root());
        let conflicts = cloud_files::conflicts(archive.root());
        writeln!(
            text,
            "Synced: {} not downloaded here, {}; see `carrel sync status`",
            plural(away.files.len(), "file"),
            plural(conflicts.len(), "conflicted copy").replace("copys", "copies")
        )
        .unwrap();
    }
    text
}

/// The sync status as JSON.
pub(crate) fn status_json(status: &SyncStatus) -> Value {
    json!({
        "synced": status.synced,
        "writer": status.writer.as_ref().map(|w| json!({
            "device": w.device,
            "purpose": w.purpose,
            "expires": w.expires,
            "expired": w.is_expired(),
        })),
        "inboxes": status.inboxes.iter().map(|i| json!({"device": i.device, "waiting": i.waiting})).collect::<Vec<_>>(),
        "holding": {
            "files": status.holding.files,
            "bytes": status.holding.bytes,
            "oldest": status.holding.oldest,
            "warn": status.holding.warn,
        },
        "rechecks": status.rechecks,
        "not_downloaded": {
            "files": status.not_downloaded.files,
            "bytes": status.not_downloaded.bytes,
        },
        "kept_downloaded": status.kept,
        "conflicts": status.conflicts.iter().map(|c| json!({"copy": c.copy, "original": c.original})).collect::<Vec<_>>(),
    })
}

/// The collection `query` names, by its folder, if it is one.
fn collection_named(archive: &Archive, query: &str) -> Result<Option<String>, Failure> {
    let wanted = query.replace('\\', "/");
    let wanted = wanted.trim_end_matches("/_collection.md").trim_matches('/');
    Ok(archive
        .collections()?
        .into_iter()
        .find(|c| c.path.eq_ignore_ascii_case(wanted))
        .map(|c| c.path))
}

/// `carrel sync download ITEM|COLLECTION`.
fn download(archive: &Archive, target: &str) -> Result<Report, Failure> {
    let path = if let Some(collection) = collection_named(archive, target)? {
        archive.root().join(collection)
    } else {
        let items = archive.items()?;
        let item = find_item(archive, &items, target)?;
        let contents = item
            .item
            .as_ref()
            .ok_or_else(|| Failure::new(format!("{} has no contents to download", item.card)))?;
        archive.root().join(contents)
    };
    let provider = cloud_files::provider();
    let waiting = cloud_files::not_downloaded_in(&path, provider.as_ref());
    cloud_files::ensure_downloaded(&path)?;
    let text = if waiting.is_empty() {
        "It's all downloaded already.\n".to_owned()
    } else {
        format!("Downloaded {}.\n", plural(waiting.len(), "file"))
    };
    Ok(Report::new(
        text,
        json!({"downloaded": waiting.len(), "path": path}),
    ))
}

/// `carrel sync keep COLLECTION [on|off|default]`.
fn keep(archive: &Archive, query: &str, setting: Option<&str>) -> Result<Report, Failure> {
    let collection = collection_named(archive, query)?
        .ok_or_else(|| Failure::new(format!("there's no collection {query}")))?;
    let Some(setting) = setting else {
        let collections = archive.collections()?;
        let on = cloud_files::keeps_downloaded(&collections, &collection);
        let own = collections
            .iter()
            .find(|c| c.path == collection)
            .and_then(cloud_files::own_setting);
        let text = format!(
            "{collection}'s items are {}kept downloaded on every device{}.\n",
            if on { "" } else { "not " },
            if own.is_none() {
                " (as the collection above says, or by default)"
            } else {
                ""
            }
        );
        return Ok(Report::new(
            text,
            json!({"collection": collection, "keep_downloaded": on, "own": own}),
        ));
    };
    let on = match setting.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" => Some(true),
        "off" | "false" | "no" => Some(false),
        "default" => None,
        other => {
            return Err(Failure::new(format!(
                "{other} isn't a setting: use on, off or default"
            )));
        }
    };
    let lease = lease(archive, "sync.keep_downloaded")?;
    let commit = cloud_files::set_keep_downloaded(archive, &collection, on, &lease)?;
    let mut text = match commit {
        Some(_) => format!("Set sync.keep_downloaded for {collection}; undo can reverse it.\n"),
        None => "It's already so.\n".to_owned(),
    };
    if on == Some(true) {
        let pass = cloud_files::keep_downloaded_pass(&Archive::open(archive.root())?)?;
        if !pass.downloaded.is_empty() {
            writeln!(
                text,
                "Downloaded {}.",
                plural(pass.downloaded.len(), "file")
            )
            .unwrap();
        }
        for (file, why) in &pass.failed {
            writeln!(text, "Couldn't download {file}: {why}").unwrap();
        }
    }
    Ok(Report::new(
        text,
        json!({"collection": collection, "keep_downloaded": on, "commit": commit}),
    ))
}

/// `carrel sync resolve COPY --keep original|copy`.
fn resolve(archive: &Archive, copy: &str, keep: &str) -> Result<Report, Failure> {
    let keep = match keep.to_ascii_lowercase().as_str() {
        "original" => Keep::Original,
        "copy" => Keep::Copy,
        other => {
            return Err(Failure::new(format!(
                "--keep {other}: say original or copy"
            )));
        }
    };
    let lease = lease(archive, "resolve")?;
    let commit = cloud_files::resolve(archive, copy, keep, &lease)?;
    let text = match keep {
        Keep::Original => "Kept the original; the copy is in the Trash.\n",
        Keep::Copy => "Kept the copy in the original's place; the original is in the Trash.\n",
    };
    Ok(Report::new(
        text.to_owned(),
        json!({"copy": copy, "kept": if keep == Keep::Copy { "copy" } else { "original" }, "commit": commit}),
    ))
}

/// `carrel sync move [--to FOLDER] [--yes]`.
fn move_archive(archive: &Archive, to: Option<&Path>, yes: bool) -> Result<Report, Failure> {
    let to = match to {
        Some(to) => to.to_owned(),
        None => icloud_move::default_destination(archive).ok_or_else(|| {
            Failure::new("iCloud Drive isn't turned on here; choose a folder with --to")
        })?,
    };
    let plan = icloud_move::plan_move(archive, &to)?;
    let mut question = format!(
        "Copy {} ({}) to {}{}, check the copy, and use it from now on? The old copy stays \
         where it is until you remove it.",
        plural(plan.files, "file"),
        size(plan.bytes),
        plan.to.display(),
        plan.free
            .map(|f| format!(", where {} is free", size(f)))
            .unwrap_or_default()
    );
    if !plan.synced {
        question.push_str(" (That folder isn't in iCloud Drive.)");
    }
    if !crate::models::confirm(&question, yes)? {
        return Ok(Report::new(
            "Nothing moved.\n".to_owned(),
            json!({"moved": false}),
        ));
    }
    let lease = lease(archive, "move")?;
    icloud_move::copy_archive(&plan, &lease, &mut |done, total| {
        if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            eprint!("\rCopied {} of {}   ", size(done), size(total));
        }
    })?;
    eprintln!();
    drop(lease);
    icloud_move::switch_over(&plan, &carrel_core::local::config_folder()?)?;
    let text = format!(
        "Moved: the archive is now {}, checked against the original. The old copy is still at \
         {}; remove it when you're ready. Restart the agent so it uses the new place.\n",
        plan.to.display(),
        plan.from.display()
    );
    Ok(Report::new(
        text,
        json!({"moved": true, "from": plan.from, "to": plan.to, "files": plan.files, "bytes": plan.bytes}),
    ))
}
