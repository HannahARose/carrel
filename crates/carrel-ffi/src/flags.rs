//! Flags (FORMAT.md §5.9): marking items and collections that need work, with a note, as
//! `carrel flag` and `carrel unflag` do, and listing them, as `carrel flagged` does. Each flag
//! is one change undo can reverse.

use carrel_core::flags::{Flaggable, Flagged};

use crate::{ArchiveHandle, CarrelError};

/// The owner's flag on an item or a collection.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Flag {
    /// What needs doing, if the owner said.
    pub note: Option<String>,
    /// When it was flagged, as written (FORMAT.md §2.4), if the flag says.
    pub at: Option<String>,
}

impl From<&carrel_core::card::NeedsWork> for Flag {
    fn from(flag: &carrel_core::card::NeedsWork) -> Self {
        Self {
            note: flag.note.clone(),
            at: flag.at.as_ref().map(|at| at.0.clone()),
        }
    }
}

/// Something flagged, as the Flagged list shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FlaggedThing {
    /// Whether it's a collection; otherwise an item.
    pub is_collection: bool,
    /// The item's identifier; none for a collection.
    pub id: Option<String>,
    /// The item's card, or the collection's folder, relative to the archive root.
    pub path: String,
    /// Its title.
    pub title: String,
    /// The flag.
    pub flag: Flag,
}

impl From<Flagged> for FlaggedThing {
    fn from(flagged: Flagged) -> Self {
        let flag = Flag::from(&flagged.flag);
        let (is_collection, id, path) = match flagged.what {
            Flaggable::Item { id, card } => (false, Some(id.to_string()), card),
            Flaggable::Collection { path } => (true, None, path),
        };
        Self {
            is_collection,
            id,
            path,
            title: flagged.title,
            flag,
        }
    }
}

/// What flagging or unflagging did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FlagChange {
    /// Whether anything changed: flagging with the same note again, or unflagging what isn't
    /// flagged, changes nothing.
    pub changed: bool,
    /// The change, as History shows it.
    pub summary: String,
    /// The history commit, when one was made.
    pub commit: Option<String>,
}

impl From<carrel_core::flags::Flagging> for FlagChange {
    fn from(done: carrel_core::flags::Flagging) -> Self {
        Self {
            changed: done.changed,
            summary: done.summary,
            commit: done.commit,
        }
    }
}

impl ArchiveHandle {
    fn flaggable_item(&self, item: &str) -> Result<Flaggable, CarrelError> {
        let archive = self.fresh()?;
        Ok(Flaggable::item(&Self::item(&archive, item)?))
    }

    fn flaggable_collection(&self, path: &str) -> Result<Flaggable, CarrelError> {
        let archive = self.fresh()?;
        let wanted = path.trim().trim_matches('/');
        archive
            .collections()?
            .into_iter()
            .find(|c| c.path == wanted)
            .map(|c| Flaggable::collection(&c.path))
            .ok_or_else(|| CarrelError::invalid(format!("there's no collection {path}")))
    }

    /// Flags `what` with `note` when `set`, else clears its flag.
    fn set_flag(
        &self,
        what: &Flaggable,
        set: bool,
        note: Option<&str>,
    ) -> Result<FlagChange, CarrelError> {
        let archive = self.fresh()?;
        let holder = self.holder()?;
        self.through_inbox(&archive, || {
            let target = carrel_core::inbox::FlagTarget::of(what);
            Ok(if set {
                carrel_core::inbox::Request::Flag {
                    target,
                    note: note.map(str::to_owned),
                }
            } else {
                carrel_core::inbox::Request::Unflag { target }
            })
        })?;
        let done = if set {
            carrel_core::flags::flag(&archive, what, note, holder)?
        } else {
            carrel_core::flags::unflag(&archive, what, holder)?
        };
        Ok(done.into())
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// Flags the item `item` (its identifier) as needing work, with `note` saying what, as
    /// `carrel flag` does. Flagging it again replaces the note.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item, a note that's too long, or an item
    /// that's checked out; [`CarrelError::Busy`] if another writer holds the archive; or
    /// [`CarrelError::Failed`] if it can't be written.
    pub fn flag_item(&self, item: String, note: Option<String>) -> Result<FlagChange, CarrelError> {
        self.set_flag(&self.flaggable_item(&item)?, true, note.as_deref())
    }

    /// Clears the item `item`'s flag, as `carrel unflag` does.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::flag_item`].
    pub fn unflag_item(&self, item: String) -> Result<FlagChange, CarrelError> {
        self.set_flag(&self.flaggable_item(&item)?, false, None)
    }

    /// Flags the collection at `path` as needing work, with `note` saying what.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown collection or a note that's too long;
    /// [`CarrelError::Busy`] if another writer holds the archive; or
    /// [`CarrelError::Failed`] if it can't be written.
    pub fn flag_collection(
        &self,
        path: String,
        note: Option<String>,
    ) -> Result<FlagChange, CarrelError> {
        self.set_flag(&self.flaggable_collection(&path)?, true, note.as_deref())
    }

    /// Clears the collection at `path`'s flag.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::flag_collection`].
    pub fn unflag_collection(&self, path: String) -> Result<FlagChange, CarrelError> {
        self.set_flag(&self.flaggable_collection(&path)?, false, None)
    }

    /// Everything flagged: collections by path, then items, as `carrel flagged` lists them.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn flagged(&self) -> Result<Vec<FlaggedThing>, CarrelError> {
        let archive = self.fresh()?;
        Ok(carrel_core::flags::all(&archive)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// The item `item`'s flag, if it has one: to offer clearing it once an edit or a
    /// correction is saved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item, or [`CarrelError::Failed`] if the
    /// archive can't be read.
    pub fn item_flag(&self, item: String) -> Result<Option<Flag>, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        Ok(carrel_core::flags::of_item(&archive, &item)
            .as_ref()
            .map(Flag::from))
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;

    #[test]
    fn items_and_collections_are_flagged_listed_and_cleared() {
        let sample = Sample::new();
        let item = sample.handle.items().unwrap()[0].clone();
        assert_eq!(item.flag, None);
        let flagged = sample
            .handle
            .flag_item(item.id.clone(), Some("wrong title".into()))
            .unwrap();
        assert!(flagged.changed);
        assert!(flagged.commit.is_some());
        let flag = sample.handle.item_flag(item.id.clone()).unwrap().unwrap();
        assert_eq!(flag.note.as_deref(), Some("wrong title"));
        assert!(flag.at.is_some());
        let listed = sample.handle.items().unwrap();
        assert_eq!(
            listed.iter().find(|i| i.id == item.id).unwrap().flag,
            Some(flag.clone())
        );
        assert_eq!(
            sample.handle.item_detail(item.id.clone()).unwrap().flag,
            Some(flag)
        );

        sample
            .handle
            .flag_collection("Projects".into(), None)
            .unwrap();
        let collections = sample.handle.collections().unwrap();
        let projects = collections.iter().find(|c| c.path == "Projects").unwrap();
        assert!(projects.flag.is_some());
        let all = sample.handle.flagged().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all[0].is_collection);
        assert_eq!(all[1].id.as_deref(), Some(item.id.as_str()));
        assert_eq!(sample.handle.status().unwrap().flagged, all);

        assert!(sample.handle.unflag_item(item.id.clone()).unwrap().changed);
        assert!(!sample.handle.unflag_item(item.id.clone()).unwrap().changed);
        assert!(
            sample
                .handle
                .unflag_collection("Projects".into())
                .unwrap()
                .changed
        );
        assert_eq!(sample.handle.flagged().unwrap(), []);
        assert!(
            sample
                .handle
                .flag_collection("Nowhere".into(), None)
                .is_err()
        );
    }
}
