//! Adding from a Mac's share sheet and Quick Action (`docs/design/macos-integration.md`,
//! "Share"): files with the owner's note, and web addresses filed as `url` items. While another
//! device is the writer, what's added goes into this device's inbox (FORMAT.md §7.8).

use std::path::PathBuf;

use carrel_core::ingest::IntakeOptions;
use carrel_core::written_note::{self, WrittenNoteAdded};
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

    fn queued(queued: intake::Queued) -> Self {
        Self {
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
        Ok(Shared::queued(queued))
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

    /// Adds `text` as a note item of its own, titled `title` (or named by its first words),
    /// with the owner's `note` for its draft and card: kept as a Markdown file in a folder of
    /// its own in `folder` (`notes` in this handle's configuration folder when `None`) and queued as any file is.
    /// While another device is the writer, it goes into this device's inbox instead.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if `text` is empty, or [`CarrelError::Failed`] if the note,
    /// the queue or the inbox can't be written.
    pub fn share_note(
        &self,
        text: String,
        title: Option<String>,
        note: Option<String>,
        folder: Option<String>,
    ) -> Result<Shared, CarrelError> {
        let archive = self.fresh()?;
        let holder = self.holder()?;
        let folder = match folder {
            Some(folder) => PathBuf::from(folder),
            None => self.config_dir()?.join(written_note::WRITTEN_NOTES_FOLDER),
        };
        let added = written_note::add_written_note(
            &archive,
            &folder,
            &text,
            title.as_deref(),
            note.as_deref(),
            holder,
        )?;
        Ok(match added {
            WrittenNoteAdded::Sent { inbox, writer } => Shared::sent(&inbox, writer),
            WrittenNoteAdded::Queued { queued, .. } => Shared::queued(queued),
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

    #[test]
    fn a_written_note_is_kept_in_the_folder_given_and_queued_as_an_item() {
        let sample = Sample::new();
        let folder = sample.dir.path().join("Sent");
        let shared = sample
            .handle
            .share_note(
                "Call about the lamp.".into(),
                Some("Lamp".into()),
                Some("Soon".into()),
                Some(text(&folder)),
            )
            .unwrap();
        assert_eq!(shared.added.added.len(), 1);
        assert!(shared.added.added[0].path.ends_with("/Lamp.md"));
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        assert!(matches!(
            sample
                .handle
                .share_note(" ".into(), None, None, Some(text(&folder))),
            Err(CarrelError::Invalid { .. })
        ));
        let kept = sample
            .handle
            .share_note("In the configuration".into(), None, None, None)
            .unwrap();
        assert!(
            kept.added.added[0].path.contains("/config/notes/"),
            "{:?}",
            kept.added.added[0].path
        );
    }
}
