//! What happens to a checkout's working copy once it's returned, by `checkout.after_return`
//! ([`carrel_core::after_return`]): the system Trash, never a permanent delete, kept, or the
//! owner asked.

use carrel_core::after_return::{AfterReturn, trash_working_copy};
use carrel_core::archive::Archive;

use crate::{ArchiveHandle, CarrelError};

/// What happened to a returned working copy.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum WorkingCopyAfter {
    /// It's in the Trash.
    Trashed,
    /// It's where it was.
    Kept,
    /// It's where it was, and `checkout.after_return` says to ask the owner whether to put it
    /// in the Trash ([`ArchiveHandle::trash_working_copy`]).
    Ask,
    /// The setting said to put it in the Trash, but it couldn't be; it's where it was.
    NotTrashed {
        /// Why, in words.
        why: String,
    },
}

/// Does what `archive`'s `checkout.after_return` says with the returned working copy at
/// `working`.
pub(crate) fn after_return(archive: &Archive, working: &std::path::Path) -> WorkingCopyAfter {
    match AfterReturn::of(archive.settings()) {
        AfterReturn::Keep => WorkingCopyAfter::Kept,
        AfterReturn::Ask => WorkingCopyAfter::Ask,
        AfterReturn::Trash => match trash_working_copy(archive.root(), working) {
            Ok(()) => WorkingCopyAfter::Trashed,
            Err(e) => WorkingCopyAfter::NotTrashed { why: e.to_string() },
        },
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// Puts the returned working copy at `path` in the system Trash, where it can be
    /// recovered, when the owner says to after a return ([`WorkingCopyAfter::Ask`]). Never
    /// anything inside the archive.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if it's inside the archive or isn't there, or
    /// [`CarrelError::Failed`] if the Trash refuses it.
    pub fn trash_working_copy(&self, path: String) -> Result<(), CarrelError> {
        use carrel_core::after_return::TrashError;
        match trash_working_copy(self.archive.root(), crate::path(&path)) {
            Ok(()) => Ok(()),
            Err(e @ (TrashError::InArchive | TrashError::Missing)) => Err(CarrelError::invalid(e)),
            Err(e) => Err(CarrelError::failed(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorkingCopyAfter;
    use crate::tests::Sample;
    use crate::text;

    /// Sets the sample archive's `checkout.after_return` to `value`.
    fn after_return(sample: &Sample, value: &str) {
        let file = std::path::Path::new(&sample.handle.root()).join("_System/settings.json");
        let mut settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        settings["checkout"]["after_return"] = value.into();
        std::fs::write(&file, settings.to_string()).unwrap();
    }

    #[test]
    fn a_returned_working_copy_goes_to_the_trash_or_the_owner_is_asked() {
        let sample = Sample::new();
        let trash = sample.dir.path().join("Trash");
        carrel_core::trash::redirect(Some(trash.clone()));
        let item = sample.item_titled("Desk lamp parts list");
        let to = sample.dir.path().join("Desktop");
        std::fs::create_dir_all(&to).unwrap();
        let check_out = || {
            let out = sample
                .handle
                .check_out(item.clone(), Some(text(&to)), None)
                .unwrap();
            std::path::PathBuf::from(out.working)
        };

        after_return(&sample, "trash");
        let working = check_out();
        let returned = sample
            .handle
            .return_item(item.clone(), None, false)
            .unwrap();
        assert_eq!(returned.working_copy, WorkingCopyAfter::Trashed);
        assert!(!working.exists());
        assert!(trash.join(working.file_name().unwrap()).exists());

        after_return(&sample, "ask");
        let working = check_out();
        let returned = sample
            .handle
            .return_item(item.clone(), None, false)
            .unwrap();
        assert_eq!(returned.working_copy, WorkingCopyAfter::Ask);
        assert_eq!(returned.working, text(&working));
        assert!(working.exists(), "kept until the owner says");
        sample.handle.trash_working_copy(returned.working).unwrap();
        assert!(!working.exists());
        assert!(
            sample
                .handle
                .trash_working_copy(sample.handle.root())
                .is_err(),
            "never the archive"
        );
        carrel_core::trash::redirect(None);
    }
}
