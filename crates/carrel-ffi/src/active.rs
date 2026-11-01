//! The Active screen (`docs/design/checkout.md`): what's checked out and what's on this
//! device's desk, with their pending history entries; checking out, returning and cancelling,
//! putting things on the desk and taking them off, and deciding pending entries. Each change
//! takes the writer lease as `carrel checkout`, `carrel return`, `carrel desk` and `carrel
//! sessions` do.

use std::path::PathBuf;

use carrel_core::active::{ActiveItem as CoreItem, Kind};
use carrel_core::archive::Archive;
use carrel_core::card::DateOrTime;
use carrel_core::checkout::{cancel_checkout, check_out, finish_return, plan_return};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::sessions::Decision as CoreDecision;

use crate::records::CardHistoryEntry;
use crate::{ArchiveHandle, CarrelError, text};

/// Whether an active item is checked out of the archive or on the desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ActiveKind {
    /// Checked out: the working copy is a copy, returned to the archive.
    CheckedOut,
    /// On this device's desk: worked on where it is, not archived.
    Desk,
}

/// What changed in a working copy since it went out, by path within it.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct Changes {
    /// Files changed.
    pub changed: Vec<String>,
    /// Files added.
    pub added: Vec<String>,
    /// Files removed.
    pub removed: Vec<String>,
    /// Files renamed, as `from → to`.
    pub renamed: Vec<String>,
}

impl From<&carrel_core::manifest::Diff> for Changes {
    fn from(diff: &carrel_core::manifest::Diff) -> Self {
        Self {
            changed: diff.changed.clone(),
            added: diff.added.clone(),
            removed: diff.removed.clone(),
            renamed: diff
                .renamed
                .iter()
                .map(|(from, to)| format!("{from} → {to}"))
                .collect(),
        }
    }
}

/// An item being worked on.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ActiveItem {
    /// Checked out or on the desk.
    pub kind: ActiveKind,
    /// Its card's identifier.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its card: in the archive for a checkout, beside the item for a desk item.
    pub card: String,
    /// The working copy, or the desk item itself.
    pub working: String,
    /// When it was checked out or put on the desk.
    pub since: String,
    /// When a checkout is due back.
    pub due: Option<String>,
    /// How it compares with how it went out: `unchanged`, `changed`, `missing`, and so on.
    pub state: String,
    /// What changed in a checkout's working copy, when it could tell.
    pub changes: Option<Changes>,
    /// Its pending history entries, in order: the first is number 1.
    pub pending: Vec<CardHistoryEntry>,
}

impl From<&CoreItem> for ActiveItem {
    fn from(item: &CoreItem) -> Self {
        Self {
            kind: match item.kind {
                Kind::CheckedOut => ActiveKind::CheckedOut,
                Kind::Desk => ActiveKind::Desk,
            },
            id: item.id.to_string(),
            title: item.title.clone(),
            card: text(&item.card),
            working: text(&item.working),
            since: item.since.clone(),
            due: item.due.clone(),
            state: item.state.to_owned(),
            changes: item.diff.as_ref().map(Into::into),
            pending: item.pending.iter().map(Into::into).collect(),
        }
    }
}

/// An item checked out.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CheckedOut {
    /// The item.
    pub item: String,
    /// Its title.
    pub title: String,
    /// Where the working copy is.
    pub working: String,
    /// The history commit.
    pub commit: Option<String>,
}

/// What returning an item would do, to show before the owner says yes.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReturnPreview {
    /// The item.
    pub item: String,
    /// Its title.
    pub title: String,
    /// Where the working copy is.
    pub working: String,
    /// What changed in it.
    pub changes: Changes,
    /// For a Git repository, the subjects of the commits since checkout, newest first.
    pub commits: Vec<String>,
    /// For a Git repository, whether there are changes not committed.
    pub uncommitted: bool,
    /// The history entry the return adds, line by line.
    pub entry: Vec<String>,
    /// With backups, how many session snapshots of the working copy were taken while it
    /// was out, which can be forgotten once it's returned.
    pub session_snapshots: u32,
}

/// What returning an item did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Returned {
    /// The history commit.
    pub commit: Option<String>,
    /// The backup snapshot taken first, if there are backups.
    pub snapshot: Option<String>,
    /// With session snapshots, the one taken of the returned version afterwards.
    pub returned_snapshot: Option<String>,
    /// How many session snapshots were forgotten.
    pub snapshots_forgotten: u32,
    /// How many are kept.
    pub snapshots_kept: u32,
    /// Why some session snapshots couldn't be forgotten, or all were kept, if they were.
    pub snapshot_problems: Vec<String>,
}

/// What to do with an item's pending history entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EntryDecision {
    /// Approve them.
    Approve,
    /// Drop them.
    Drop,
    /// Combine them into one, still pending.
    Combine,
}

impl From<EntryDecision> for CoreDecision {
    fn from(decision: EntryDecision) -> Self {
        match decision {
            EntryDecision::Approve => Self::Approve,
            EntryDecision::Drop => Self::Drop,
            EntryDecision::Combine => Self::Combine,
        }
    }
}

/// A decision on pending entries, made.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EntriesDecided {
    /// The change's summary, as History shows it.
    pub summary: String,
    /// How many entries it concerned.
    pub entries: u32,
    /// The history commit.
    pub commit: Option<String>,
}

impl From<carrel_core::sessions::Decided> for EntriesDecided {
    fn from(decided: carrel_core::sessions::Decided) -> Self {
        Self {
            summary: decided.summary,
            entries: u32::try_from(decided.entries).unwrap_or(u32::MAX),
            commit: decided.commit,
        }
    }
}

/// A desk item.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeskItem {
    /// Its card's identifier.
    pub id: String,
    /// Where it is.
    pub path: String,
    /// Where its card is.
    pub card: String,
    /// When it was put on the desk.
    pub added: String,
}

impl From<carrel_core::desk::DeskEntry> for DeskItem {
    fn from(entry: carrel_core::desk::DeskEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            path: text(&entry.path),
            card: text(&entry.card),
            added: entry.added.0,
        }
    }
}

impl ArchiveHandle {
    /// The active item `id`, checked out or on this device's desk.
    fn active_item(&self, archive: &Archive, id: &str) -> Result<CoreItem, CarrelError> {
        let device = self.identity()?.device;
        carrel_core::active::items(archive, Some(device))
            .into_iter()
            .find(|i| i.id.to_string() == id.trim().to_lowercase())
            .ok_or_else(|| CarrelError::invalid(format!("{id} isn't checked out or on this desk")))
    }

    /// Takes the writer lease for `purpose`, runs `change` with it, and lets go.
    pub(crate) fn with_lease<T>(
        &self,
        archive: &Archive,
        purpose: &str,
        change: impl FnOnce(&carrel_core::coordinator::WriterLease) -> Result<T, CarrelError>,
    ) -> Result<T, CarrelError> {
        let lease =
            DiskLock::for_archive(archive).acquire(self.holder()?, purpose, DEFAULT_LEASE)?;
        let result = change(&lease);
        lease.release()?;
        result
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// What's checked out, then what's on this device's desk, each with its pending history
    /// entries.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive or this device's identity can't be read.
    pub fn active(&self) -> Result<Vec<ActiveItem>, CarrelError> {
        let archive = self.fresh()?;
        let device = self.identity()?.device;
        Ok(carrel_core::active::items(&archive, Some(device))
            .iter()
            .map(Into::into)
            .collect())
    }

    /// Checks out the item `item` to the folder `to` (the Desktop when `None`), due back on
    /// `due` (`YYYY-MM-DD`) if given. The archive copy is read-only until it's returned.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item or a date that isn't one,
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it can't come out.
    pub fn check_out(
        &self,
        item: String,
        to: Option<String>,
        due: Option<String>,
    ) -> Result<CheckedOut, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let destination = match to {
            Some(to) => PathBuf::from(to),
            None => self
                .home()
                .map(|home| home.join("Desktop"))
                .ok_or_else(|| CarrelError::invalid("no home folder to check out to"))?,
        };
        let due = due
            .map(|d| {
                d.parse::<jiff::civil::Date>()
                    .map(|date| DateOrTime(date.to_string()))
                    .map_err(|_| CarrelError::invalid(format!("{d} isn't a date (YYYY-MM-DD)")))
            })
            .transpose()?;
        let out = self.with_lease(&archive, "checkout", |lease| {
            Ok(check_out(&archive, &item, &destination, due, lease)?)
        })?;
        Ok(CheckedOut {
            item: out.item.to_string(),
            title: out.title,
            working: text(&out.working),
            commit: out.commit,
        })
    }

    /// What returning the checked-out item `item` would bring back, and the history entry it
    /// adds, from where it was checked out to, or from `from` when the working copy has
    /// moved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item, or [`CarrelError::Failed`] if it isn't
    /// checked out or its working copy can't be read.
    pub fn return_preview(
        &self,
        item: String,
        from: Option<String>,
    ) -> Result<ReturnPreview, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let plan = plan_return(&archive, &item, from.as_deref().map(crate::path))?;
        let sessions = crate::backups(&archive).map_or(0, |b| {
            carrel_core::sessions::return_snapshots(&archive, &item, &*b).len()
        });
        Ok(ReturnPreview {
            item: plan.item.to_string(),
            title: plan.title,
            working: text(&plan.working),
            changes: (&plan.diff).into(),
            commits: plan.commits,
            uncommitted: plan.uncommitted,
            entry: plan.entry.lines,
            session_snapshots: u32::try_from(sessions).unwrap_or(u32::MAX),
        })
    }

    /// Returns the checked-out item `item`, from where it was checked out to or from `from`,
    /// replacing the archive copy with the working copy, with a snapshot first when backups
    /// are set up. Afterwards the returned version is snapshotted, and the checkout's session
    /// snapshots are forgotten when `trim_snapshots` says so, as the owner chose. Ask the
    /// owner first, with [`ArchiveHandle::return_preview`].
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it isn't checked out or can't be returned.
    pub fn return_item(
        &self,
        item: String,
        from: Option<String>,
        trim_snapshots: bool,
    ) -> Result<Returned, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let plan = plan_return(&archive, &item, from.as_deref().map(crate::path))?;
        let backups = crate::backups(&archive);
        let sessions = backups
            .as_deref()
            .map(|b| carrel_core::sessions::return_snapshots(&archive, &item, b))
            .unwrap_or_default();
        let returned = self.with_lease(&archive, "return", |lease| {
            Ok(finish_return(&archive, &plan, lease, backups.as_deref())?)
        })?;
        let mut done = Returned {
            commit: returned.commit,
            snapshot: returned.snapshot,
            returned_snapshot: None,
            snapshots_forgotten: 0,
            snapshots_kept: 0,
            snapshot_problems: Vec::new(),
        };
        if let Some(backups) = backups.as_deref().filter(|_| !sessions.is_empty()) {
            let kept = carrel_core::sessions::keep_returned_version(
                backups,
                &plan.working,
                &plan.title,
                plan.item,
                &sessions,
                trim_snapshots,
            );
            let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
            done.snapshots_forgotten = count(kept.forgotten);
            done.snapshots_kept = count(kept.kept);
            match kept.returned {
                Ok(snapshot) => done.returned_snapshot = Some(snapshot.trailer()),
                Err(e) => done.snapshot_problems.push(format!(
                    "the returned version couldn't be snapshotted, so the session snapshots are kept: {e}"
                )),
            }
            done.snapshot_problems.extend(
                kept.failures
                    .into_iter()
                    .map(|(id, e)| format!("couldn't forget snapshot {id}: {e}")),
            );
        }
        Ok(done)
    }

    /// Cancels the checkout of `item`: the archive copy is unlocked as it was, and the
    /// working copy's changes aren't brought back. Ask the owner first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it isn't checked out.
    pub fn cancel_checkout(&self, item: String) -> Result<Option<String>, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        self.with_lease(&archive, "return", |lease| {
            Ok(cancel_checkout(&archive, &item, lease)?)
        })
    }

    /// Puts the file or folder at `path` on this device's desk, its card drafted by this
    /// device's models unless `title` and `summary` are both given. Drafting can take a
    /// while; it's done before the writer lease is taken.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it's missing or already on the desk.
    pub fn add_to_desk(
        &self,
        path: String,
        title: Option<String>,
        summary: Option<String>,
    ) -> Result<DeskItem, CarrelError> {
        let archive = self.fresh()?;
        let device = self.identity()?.device;
        let drafted = if title.is_some() && summary.is_some() {
            None
        } else {
            Some(carrel_core::desk::draft(
                &archive,
                &self.registry(),
                crate::path(&path),
                device,
            )?)
        };
        let entry = self.with_lease(&archive, "desk", |lease| {
            Ok(carrel_core::desk::add(
                &archive,
                crate::path(&path),
                title.as_deref(),
                summary.as_deref(),
                drafted.as_ref(),
                lease,
            )?)
        })?;
        Ok(entry.into())
    }

    /// Takes the desk item at `path` off this device's desk, deleting its card when
    /// `delete_card` says so.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it isn't on the desk.
    pub fn remove_from_desk(
        &self,
        path: String,
        delete_card: bool,
    ) -> Result<DeskItem, CarrelError> {
        let archive = self.fresh()?;
        let entry = self.with_lease(&archive, "desk", |lease| {
            Ok(carrel_core::desk::remove(
                &archive,
                crate::path(&path),
                delete_card,
                lease,
            )?)
        })?;
        Ok(entry.into())
    }

    /// Archives the desk item at `path`: queues it for ingest, keeping its card and history,
    /// and takes it off the desk. The worker drafts its proposal. Returns the intake entry's
    /// identifier.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it isn't on the desk or can't be queued.
    pub fn archive_desk_item(&self, path: String) -> Result<String, CarrelError> {
        let archive = self.fresh()?;
        let (_, intake) = self.with_lease(&archive, "desk", |lease| {
            Ok(carrel_core::desk::archive(
                &archive,
                crate::path(&path),
                lease,
            )?)
        })?;
        Ok(intake.id.to_string())
    }

    /// Approves, drops or combines the pending history entries `numbers` (from 1; none means
    /// all of them) of the active item `item`, as one change.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an item that isn't active or an entry that isn't
    /// pending, [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`].
    pub fn decide_entries(
        &self,
        item: String,
        numbers: Vec<u32>,
        decision: EntryDecision,
    ) -> Result<EntriesDecided, CarrelError> {
        let archive = self.fresh()?;
        let active = self.active_item(&archive, &item)?;
        let numbers: Vec<usize> = numbers.into_iter().map(|n| n as usize).collect();
        Ok(carrel_core::sessions::decide(
            &archive,
            &active.card,
            active.id,
            &numbers,
            decision.into(),
            self.holder()?,
        )?
        .into())
    }

    /// Replaces the text of the active item `item`'s pending entry `number` (from 1) with
    /// `text`, still pending, as one change. `None` when the text is the same.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::decide_entries`], and [`CarrelError::Invalid`] if the text would
    /// break the card's history section.
    pub fn edit_entry(
        &self,
        item: String,
        number: u32,
        text: String,
    ) -> Result<Option<EntriesDecided>, CarrelError> {
        let archive = self.fresh()?;
        let active = self.active_item(&archive, &item)?;
        let was = active
            .pending
            .get((number as usize).wrapping_sub(1))
            .ok_or_else(|| CarrelError::invalid(format!("there's no pending entry {number}")))?;
        Ok(carrel_core::sessions::edit_entry(
            &archive,
            &active.card,
            active.id,
            was,
            &text,
            self.holder()?,
        )?
        .map(Into::into))
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;
    use crate::text;

    #[test]
    fn a_checkout_returns_from_where_its_working_copy_moved() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let to = sample.dir.path().join("Desktop");
        std::fs::create_dir_all(&to).unwrap();
        let out = sample
            .handle
            .check_out(item.clone(), Some(text(&to)), None)
            .unwrap();
        let moved = sample.dir.path().join("moved.csv");
        std::fs::rename(&out.working, &moved).unwrap();
        std::fs::write(&moved, "part,source\nLED strip,shop\n").unwrap();
        assert!(sample.handle.return_preview(item.clone(), None).is_err());
        let preview = sample
            .handle
            .return_preview(item.clone(), Some(text(&moved)))
            .unwrap();
        assert_eq!(preview.working, text(&moved));
        assert_eq!(preview.session_snapshots, 0);
        let returned = sample
            .handle
            .return_item(item.clone(), Some(text(&moved)), true)
            .unwrap();
        assert!(returned.commit.is_some());
        assert_eq!(returned.returned_snapshot, None);
        assert!(!sample.handle.active().unwrap().iter().any(|a| a.id == item));
    }
}
