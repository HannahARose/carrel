//! Reminders for the Mac app (`docs/design/reminders.md`): the ones due, for Status; the ones
//! this device hasn't shown yet, for the agent to post as notifications and then mark shown;
//! and whether the agent shows its menu bar list. What was shown is kept in this handle's
//! configuration folder, never in the archive.

use carrel_core::reminders::{self, Shown};

use crate::{ArchiveHandle, CarrelError};

/// A reminder due now.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Reminder {
    /// Which reminder: `idle`, `weekly` or `due-date`.
    pub kind: String,
    /// What makes it this reminder, to mark it shown.
    pub key: String,
    /// A short heading, such as "Due back tomorrow".
    pub title: String,
    /// What it says.
    pub text: String,
    /// The identifier of the item it's about, to open when it's chosen.
    pub item: Option<String>,
}

impl From<reminders::Reminder> for Reminder {
    fn from(r: reminders::Reminder) -> Self {
        Self {
            kind: r.kind.as_str().to_owned(),
            key: r.key,
            title: r.title,
            text: r.text,
            item: r.item.map(|i| i.to_string()),
        }
    }
}

impl ArchiveHandle {
    fn due_reminders(&self, fresh_only: bool) -> Result<Vec<Reminder>, CarrelError> {
        let archive = self.fresh()?;
        let shown = Shown::load(&self.config_dir()?);
        let device = self.identity()?.device;
        let due = reminders::due(&archive, Some(device), &jiff::Zoned::now(), &shown);
        let due = if fresh_only {
            shown.fresh(archive.doc().id, due)
        } else {
            due
        };
        Ok(due.into_iter().map(Reminder::from).collect())
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// The reminders due now, as the `reminders` setting turns them on, for Status.
    ///
    /// # Errors
    ///
    /// [`CarrelError`] if the archive can't be read.
    pub fn reminders(&self) -> Result<Vec<Reminder>, CarrelError> {
        self.due_reminders(false)
    }

    /// The reminders due that this device hasn't shown yet, for the agent to post; once it
    /// has, it marks them shown ([`ArchiveHandle::mark_reminders_shown`]).
    ///
    /// # Errors
    ///
    /// [`CarrelError`] if the archive can't be read.
    pub fn reminders_to_show(&self) -> Result<Vec<Reminder>, CarrelError> {
        self.due_reminders(true)
    }

    /// Records the reminders `keys` as shown on this device, so they aren't shown again.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the record can't be written.
    pub fn mark_reminders_shown(&self, keys: Vec<String>) -> Result<(), CarrelError> {
        reminders::mark_shown(
            &self.config_dir()?,
            self.archive.doc().id,
            &keys,
            jiff::Timestamp::now(),
        )
        .map_err(CarrelError::failed)
    }

    /// Whether the agent shows its menu bar list: the `menu-bar` reminder.
    #[must_use]
    pub fn shows_menu_bar(&self) -> bool {
        self.fresh()
            .map_or(true, |archive| reminders::shows_menu_bar(&archive))
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;

    #[test]
    fn the_agent_shows_each_reminder_once_and_status_keeps_listing_it() {
        let sample = Sample::new();
        let due = sample.handle.reminders().unwrap();
        assert!(due.iter().any(|r| r.kind == "idle"), "{due:?}");
        let idle = due.iter().find(|r| r.kind == "idle").unwrap();
        assert_eq!(
            idle.item.as_deref(),
            Some("01a18701-0500-776a-9867-080924eb019c")
        );
        let to_show = sample.handle.reminders_to_show().unwrap();
        assert_eq!(to_show, due);
        sample
            .handle
            .mark_reminders_shown(to_show.iter().map(|r| r.key.clone()).collect())
            .unwrap();
        assert_eq!(sample.handle.reminders_to_show().unwrap(), []);
        assert_eq!(sample.handle.reminders().unwrap(), due);
        assert!(sample.dir.path().join("config/reminders.json").is_file());
        assert!(sample.handle.shows_menu_bar());
        sample
            .handle
            .set_setting("reminders".into(), "idle".into())
            .unwrap();
        assert!(!sample.handle.shows_menu_bar());
    }
}
