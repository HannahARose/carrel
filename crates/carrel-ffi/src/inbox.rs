//! This device's inbox and the holding folder (FORMAT.md §7.8 and §7.12,
//! `docs/design/sync-and-mobile.md`). While another device is the writer, every change made
//! from here goes into this device's inbox as a request, for the writer to apply as this
//! device's: the call then fails with [`CarrelError::SentToInbox`], whose message says so,
//! since nothing changed yet. Status shows what's waiting, and what the writer skipped and
//! why. On a device without Kopia, content a change replaces is kept in the holding folder
//! until a Mac's snapshot has it; the versions there can be listed and restored.

use carrel_core::archive::Archive;
use carrel_core::coordinator::Holder;
use carrel_core::holding;
use carrel_core::inbox::{self as core_inbox, Request, Sending};

use crate::{ArchiveHandle, CarrelError, text};

impl CarrelError {
    /// That a change went into this device's inbox, for `sending`'s reason.
    pub(crate) fn sent(sending: &Sending) -> Self {
        Self::SentToInbox {
            message: sending.describe(),
        }
    }
}

impl ArchiveHandle {
    /// Who this device acts as, when it acts through its inbox in `archive` now, and why.
    pub(crate) fn sending(
        &self,
        archive: &Archive,
    ) -> Result<Option<(Holder, Sending)>, CarrelError> {
        let holder = self.holder()?;
        Ok(core_inbox::sending(archive, holder.device).map(|sending| (holder, sending)))
    }

    /// Before a change: when this device acts through its inbox, puts the request `request`
    /// makes there and fails with [`CarrelError::SentToInbox`]; otherwise does nothing, for
    /// the change to be made here.
    pub(crate) fn through_inbox(
        &self,
        archive: &Archive,
        request: impl FnOnce() -> Result<Request, CarrelError>,
    ) -> Result<(), CarrelError> {
        let Some((holder, sending)) = self.sending(archive)? else {
            return Ok(());
        };
        core_inbox::send(archive, holder, request()?)?;
        Err(CarrelError::sent(&sending))
    }
}

/// A change waiting in this device's inbox for the writer.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InboxWaiting {
    /// What it asks, in a few words.
    pub what: String,
    /// When it was asked (RFC 3339).
    pub at: String,
}

/// A change this device sent that the writer skipped, because it no longer applied.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InboxSkipped {
    /// What it asked, in a few words.
    pub what: String,
    /// When it was asked (RFC 3339).
    pub at: String,
    /// Why it was skipped, in words.
    pub reason: String,
    /// The writer device that skipped it.
    pub by: String,
    /// When it was skipped (RFC 3339).
    pub skipped: String,
}

/// This device's inbox, for Status.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct InboxStatus {
    /// Why changes made here go into the inbox now, when they do.
    pub sending: Option<String>,
    /// The writer device they wait for, when another device is the writer.
    pub writer: Option<String>,
    /// The changes waiting for the writer, oldest first.
    pub waiting: Vec<InboxWaiting>,
    /// The changes the writer skipped lately, newest first, each with why.
    pub skipped: Vec<InboxSkipped>,
    /// All of it in lines, as `carrel status` says it.
    pub lines: Vec<String>,
}

/// What waits in the holding folder for a snapshot, for Status.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct HoldingStatus {
    /// How many versions.
    pub count: u64,
    /// Their size in bytes.
    pub size: u64,
    /// How many days the oldest has waited.
    pub oldest_days: Option<i64>,
    /// What's worth warning about: too much, or waiting too long.
    pub warnings: Vec<String>,
}

/// A version kept in the holding folder.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HeldVersion {
    /// Its identifier.
    pub id: String,
    /// The item it was the contents of, when there's one.
    pub item: Option<String>,
    /// The item's title.
    pub title: String,
    /// Where it was, relative to the archive root.
    pub path: String,
    /// Its name.
    pub name: String,
    /// Why it was kept, in words.
    pub why: String,
    /// Its size in bytes.
    pub size: u64,
    /// The device that kept it.
    pub device: String,
    /// When (RFC 3339).
    pub at: String,
    /// How many days it has waited.
    pub age_days: i64,
}

impl From<&holding::Held> for HeldVersion {
    fn from(held: &holding::Held) -> Self {
        Self {
            id: held.id.to_string(),
            item: held.item.map(|i| i.to_string()),
            title: held.title.clone(),
            path: held.path.clone(),
            name: held.name.clone(),
            why: held.why.describe().to_owned(),
            size: held.size,
            device: held.device.to_string(),
            at: held.at.clone(),
            age_days: held.age_days(),
        }
    }
}

/// `archive`'s inbox for `device`.
pub(crate) fn inbox_status(archive: &Archive, device: carrel_core::card::Id) -> InboxStatus {
    let sending = core_inbox::sending(archive, device);
    InboxStatus {
        sending: sending.as_ref().map(Sending::describe),
        writer: sending
            .as_ref()
            .and_then(Sending::writer)
            .map(|d| d.to_string()),
        waiting: core_inbox::waiting_from(archive, device)
            .into_iter()
            .map(|(_, entry)| InboxWaiting {
                what: entry.request.describe(),
                at: entry.at,
            })
            .collect(),
        skipped: core_inbox::skipped(archive, device)
            .into_iter()
            .map(|skip| InboxSkipped {
                what: skip.entry.request.describe(),
                at: skip.entry.at,
                reason: skip.reason,
                by: skip.by.to_string(),
                skipped: skip.skipped,
            })
            .collect(),
        lines: core_inbox::status_lines(archive, device),
    }
}

/// What waits in `archive`'s holding folder.
pub(crate) fn holding_status(archive: &Archive) -> HoldingStatus {
    let waiting = holding::waiting(archive);
    HoldingStatus {
        count: waiting.count as u64,
        size: waiting.size,
        oldest_days: waiting.oldest_days,
        warnings: waiting.warnings,
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// This device's inbox: why changes go there now, if they do, what waits for the
    /// writer, and what it skipped, and why.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive or this device's identity can't be read.
    pub fn inbox_status(&self) -> Result<InboxStatus, CarrelError> {
        let archive = self.fresh()?;
        Ok(inbox_status(&archive, self.identity()?.device))
    }

    /// What waits in the holding folder for a snapshot, with warnings when it's a lot or
    /// has waited long.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn holding_status(&self) -> Result<HoldingStatus, CarrelError> {
        Ok(holding_status(&self.fresh()?))
    }

    /// The versions kept in the holding folder, oldest first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn held_versions(&self) -> Result<Vec<HeldVersion>, CarrelError> {
        Ok(holding::list(&self.fresh()?)
            .iter()
            .map(Into::into)
            .collect())
    }

    /// The versions kept in the holding folder from the item `item`, oldest first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn held_versions_of(&self, item: String) -> Result<Vec<HeldVersion>, CarrelError> {
        let wanted = item.trim().to_lowercase();
        Ok(holding::list(&self.fresh()?)
            .iter()
            .filter(|h| h.item.is_some_and(|i| i.to_string() == wanted))
            .map(Into::into)
            .collect())
    }

    /// Copies the held version `id` (its identifier, or the start of it) to `to`, which
    /// mustn't exist yet, or, when it's `None`, beside the archive as `NAME (held DATE)`.
    /// Nothing in the archive changes; the version stays held until a snapshot clears it.
    /// Returns where it went.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if no held version or several match, or `to` exists, or
    /// [`CarrelError::Failed`] if it can't be copied.
    pub fn restore_held(&self, id: String, to: Option<String>) -> Result<String, CarrelError> {
        let archive = self.fresh()?;
        let held = holding::find(&archive, &id).map_err(CarrelError::invalid)?;
        let to = to.map_or_else(
            || holding::restore_beside(&archive, &held),
            std::path::PathBuf::from,
        );
        holding::restore(&archive, &held, &to).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => CarrelError::invalid(e),
            _ => CarrelError::failed(e),
        })?;
        Ok(text(&to))
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::archive::Archive;
    use carrel_core::card::Id;
    use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};

    use crate::CarrelError;
    use crate::tests::Sample;

    /// Another device holding the writer lease of `sample`'s archive.
    fn another_writer(sample: &Sample) -> carrel_core::coordinator::WriterLease {
        let archive = Archive::open(&sample.root()).unwrap();
        let other = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        DiskLock::for_archive(&archive)
            .acquire(other, "ingest", DEFAULT_LEASE)
            .unwrap()
    }

    fn sent(result: Result<impl std::fmt::Debug, CarrelError>) -> String {
        match result {
            Err(CarrelError::SentToInbox { message }) => message,
            other => panic!("expected it sent to the inbox, got {other:?}"),
        }
    }

    #[test]
    fn changes_go_into_the_inbox_while_another_device_writes() {
        let sample = Sample::new();
        let proposal = sample.first_proposal();
        let item = sample.item_titled("Desk lamp parts list");
        let lease = another_writer(&sample);
        assert!(sample.handle.inbox_status().unwrap().sending.is_some());

        let message = sent(sample.handle.reject(vec![proposal.clone()]));
        assert!(message.contains("inbox"), "{message}");
        sent(sample.handle.approve(vec![proposal.clone()]));
        sent(sample.handle.flag_item(item.clone(), Some("Check".into())));
        let base = sample.handle.editable_card(item.clone()).unwrap().fields;
        let mut fields = base.clone();
        fields.title = "Parts".into();
        sent(sample.handle.edit_card(item.clone(), base, fields));

        let status = sample.handle.status().unwrap();
        assert_eq!(status.inbox.waiting.len(), 4);
        assert!(status.inbox.writer.is_some());
        assert_eq!(status.inbox.waiting[0].what, "reject 1 proposal");
        assert_eq!(status.inbox.lines.len(), 1);
        assert_eq!(
            sample.handle.proposal(proposal).unwrap().state,
            crate::review::ProposalState::Proposed,
            "nothing is decided here"
        );
        lease.release().unwrap();

        assert!(sample.handle.inbox_status().unwrap().sending.is_none());
        assert!(
            sample.handle.flag_item(item, None).is_ok(),
            "done here again"
        );
    }

    #[test]
    fn a_checkout_while_another_device_writes_is_made_here_and_recorded_later() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let to = sample.dir.path().join("Desktop");
        std::fs::create_dir_all(&to).unwrap();
        let lease = another_writer(&sample);
        let out = sample
            .handle
            .check_out(item, Some(crate::text(&to)), None)
            .unwrap();
        assert!(std::path::Path::new(&out.working).exists());
        assert_eq!(out.commit, None);
        assert!(out.inbox.is_some());
        assert_eq!(sample.handle.inbox_status().unwrap().waiting.len(), 1);
        lease.release().unwrap();
    }

    #[test]
    fn a_held_version_is_listed_counted_and_restored() {
        let sample = Sample::new();
        let archive = Archive::open(&sample.root()).unwrap();
        let file = sample.dir.path().join("old.txt");
        std::fs::write(&file, "the old version").unwrap();
        let item = Id::new_v7();
        let keeping = carrel_core::holding::Keeping {
            item: Some(item),
            title: "Old notes",
            why: carrel_core::holding::Why::Return,
            name: None,
        };
        let holder = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        carrel_core::holding::keep(&archive, &file, &keeping, holder).unwrap();

        let status = sample.handle.status().unwrap();
        assert_eq!(status.holding.count, 1);
        assert_eq!(status.holding.warnings, Vec::<String>::new());
        let held = sample.handle.held_versions().unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].title, "Old notes");
        assert_eq!(held[0].why, "replaced by a return");
        assert_eq!(
            sample.handle.held_versions_of(item.to_string()).unwrap(),
            held
        );

        let to = sample.dir.path().join("restored.txt");
        let restored = sample
            .handle
            .restore_held(held[0].id[..8].to_owned(), Some(crate::text(&to)))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&restored).unwrap(),
            "the old version"
        );
        assert!(matches!(
            sample
                .handle
                .restore_held(held[0].id.clone(), Some(crate::text(&to))),
            Err(CarrelError::Invalid { .. })
        ));
        let beside = sample
            .handle
            .restore_held(held[0].id.clone(), None)
            .unwrap();
        assert!(beside.contains("(held "), "{beside}");
        assert!(matches!(
            sample.handle.restore_held("ffffffff".into(), None),
            Err(CarrelError::Invalid { .. })
        ));
    }
}
