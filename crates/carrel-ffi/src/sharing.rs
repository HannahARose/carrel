//! Adding from a Mac's share sheet and Quick Action (`docs/design/macos-integration.md`,
//! "Share"): files with the owner's note, and web addresses filed as `url` items. While another
//! device is the writer, what's added goes into this device's inbox (FORMAT.md §7.8).

use std::path::PathBuf;

use carrel_core::ingest::IntakeOptions;
use carrel_core::{inbox, intake};

use crate::intake::{Added, LeftOut};
use crate::{ArchiveHandle, CarrelError, text};

/// What sharing something with the archive did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Shared {
    /// What joined this device's intake queue, and what was left out.
    pub added: Added,
    /// The inbox entry it went into instead, while another device is the writer.
    pub inbox: Option<String>,
    /// That device, when it did.
    pub writer: Option<String>,
}

impl Shared {
    fn sent(path: &std::path::Path, writer: carrel_core::card::Id) -> Self {
        Self {
            added: Added {
                added: Vec::new(),
                left_out: Vec::new(),
            },
            inbox: Some(text(path)),
            writer: Some(writer.to_string()),
        }
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// Adds the files and folders at `paths` to the intake queue, as
    /// [`queue`](ArchiveHandle::queue) does, with the owner's `note` for their drafts and cards.
    /// While another device is the writer, they go into this device's inbox instead.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if a path is missing, or the queue or inbox can't be written.
    pub fn share_paths(
        &self,
        paths: Vec<String>,
        each: bool,
        whole: bool,
        note: Option<String>,
    ) -> Result<Shared, CarrelError> {
        let archive = self.fresh()?;
        let holder = self.holder()?;
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        if let Some(writer) = inbox::writer_elsewhere(&archive, holder.device) {
            let sent = inbox::send_paths(&archive, holder, &paths, each, note.as_deref())?;
            return Ok(Shared::sent(&sent, writer.device));
        }
        let queued = intake::add_with_note(
            &archive,
            &paths,
            IntakeOptions { each, whole },
            holder.device,
            note.as_deref(),
        )?;
        Ok(Shared {
            added: Added {
                added: queued.added.iter().map(Into::into).collect(),
                left_out: queued
                    .skipped
                    .into_iter()
                    .map(|s| LeftOut {
                        path: text(&s.path),
                        reason: s.reason,
                    })
                    .collect(),
            },
            inbox: None,
            writer: None,
        })
    }

    /// Adds the web address `url` to the intake queue, to be filed as a `url` item drafted
    /// from the address, its `title` and the owner's `note`; nothing is fetched. While another
    /// device is the writer, it goes into this device's inbox instead.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if it isn't an `http` or `https` address, or
    /// [`CarrelError::Failed`] if the queue or inbox can't be written.
    pub fn share_url(
        &self,
        url: String,
        title: Option<String>,
        note: Option<String>,
    ) -> Result<Shared, CarrelError> {
        let archive = self.fresh()?;
        let holder = self.holder()?;
        if let Some(writer) = inbox::writer_elsewhere(&archive, holder.device) {
            let sent = inbox::send_url(&archive, holder, &url, title.as_deref(), note.as_deref())?;
            return Ok(Shared::sent(&sent, writer.device));
        }
        let added = intake::add_url(
            &archive,
            &url,
            title.as_deref(),
            note.as_deref(),
            holder.device,
        )?;
        let left_out = added.is_none().then(|| LeftOut {
            path: url.clone(),
            reason: "already queued or waiting for review".into(),
        });
        Ok(Shared {
            added: Added {
                added: added.iter().map(Into::into).collect(),
                left_out: left_out.into_iter().collect(),
            },
            inbox: None,
            writer: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;
    use crate::{CarrelError, text};

    #[test]
    fn web_addresses_and_notes_are_queued_or_sent_to_the_inbox_while_another_device_writes() {
        let sample = Sample::new();
        let shared = sample
            .handle
            .share_url(
                "https://example.org/a".into(),
                Some("A page".into()),
                Some("For later".into()),
            )
            .unwrap();
        assert_eq!(shared.added.added.len(), 1);
        assert_eq!(shared.inbox, None);
        let again = sample
            .handle
            .share_url("https://example.org/a".into(), None, None)
            .unwrap();
        assert_eq!(again.added.left_out.len(), 1);
        assert!(matches!(
            sample.handle.share_url("example.org".into(), None, None),
            Err(CarrelError::Invalid { .. })
        ));

        let note = sample.dir.path().join("note.txt");
        std::fs::write(&note, "A note.\n").unwrap();
        let shared = sample
            .handle
            .share_paths(vec![text(&note)], false, true, Some("Keep".into()))
            .unwrap();
        assert_eq!(shared.added.added.len(), 1);

        let lock = serde_json::json!({
            "token": "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b",
            "person": "01928f39-eeee-7fff-8000-000000000005",
            "device": "01928f3a-0000-7000-8000-00000000d002",
            "purpose": "ingest",
            "program": "carrel 0.1.0",
            "acquired": "2026-10-29T14:02:11-07:00",
            "expires": "2999-01-01T00:00:00Z",
        });
        std::fs::write(sample.root().join("_System/writer.lock"), lock.to_string()).unwrap();
        let sent = sample
            .handle
            .share_url("https://example.org/b".into(), None, None)
            .unwrap();
        assert_eq!(
            sent.writer.as_deref(),
            Some("01928f3a-0000-7000-8000-00000000d002")
        );
        assert!(std::path::Path::new(sent.inbox.as_deref().unwrap()).is_file());
        assert_eq!(sent.added.added.len(), 0);
    }
}
