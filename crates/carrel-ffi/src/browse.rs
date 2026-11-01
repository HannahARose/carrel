//! Browse's actions on the archive's structure and its filed items: creating a collection,
//! renaming an item and expanding a filed compressed file. Each takes the writer lease and runs
//! the core function `carrel collection new`, `carrel rename` and `carrel expand` run, so each
//! is one change that undo can reverse.

use carrel_core::collections::{CollectionError, NewCollection};
use carrel_core::compressed::item::{ExpandError, examine, expand_item};
use carrel_core::rename::RenameError;

use crate::{ArchiveHandle, CarrelError, ItemKind};

/// A tag in a collection's vocabulary, and what it means.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CollectionTag {
    /// The tag: lowercase words joined by hyphens.
    pub name: String,
    /// What it means.
    pub meaning: String,
}

/// A collection that was created.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CreatedCollection {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// Its identifier.
    pub id: String,
    /// Its design document, relative to the archive root, for the owner to fill in.
    pub design_document: String,
    /// The history commit.
    pub commit: Option<String>,
}

/// An item's name now and the one `carrel rename` suggests for it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NameSuggestion {
    /// Its name now.
    pub current: String,
    /// A name that works on every system: the same as `current` when it already does.
    pub suggested: String,
}

/// An item that was renamed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RenamedItem {
    /// Its path before, relative to the archive root.
    pub from: String,
    /// Its path after.
    pub to: String,
    /// Its card's path after.
    pub card: String,
    /// The cards whose links to it were updated.
    pub relinked: Vec<String>,
    /// The history commit.
    pub commit: Option<String>,
}

/// Whether a filed item can be expanded, to ask before expanding it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Expandable {
    /// It expands as it is.
    Yes,
    /// It's kept compressed for this reason, but expands all the same when asked.
    AllTheSame {
        /// Why it's kept compressed.
        reason: String,
    },
    /// It can't be expanded.
    No {
        /// Why not.
        reason: String,
    },
}

/// A filed compressed file that was expanded.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ExpandedItem {
    /// The compressed file's path before, relative to the archive root.
    pub from: String,
    /// What it held, now the item.
    pub to: String,
    /// The card's path after.
    pub card: String,
    /// What the item is now.
    pub kind: ItemKind,
    /// How many files were written.
    pub files: u64,
    /// How many bytes.
    pub size: u64,
    /// Why the compressed file couldn't go to the Trash, when it couldn't; it's then left
    /// beside the item.
    pub trash_problem: Option<String>,
    /// The history commit.
    pub commit: Option<String>,
    /// The compressed file, kept in the holding folder instead of the Trash on a device
    /// without Kopia, until a snapshot has it.
    pub held: Option<crate::inbox::HeldVersion>,
}

impl From<CollectionError> for CarrelError {
    fn from(error: CollectionError) -> Self {
        match error {
            CollectionError::Io { .. }
            | CollectionError::Archive(_)
            | CollectionError::History(_) => Self::failed(error),
            error => Self::invalid(error),
        }
    }
}

impl From<RenameError> for CarrelError {
    fn from(error: RenameError) -> Self {
        match error {
            RenameError::NotPortable(_) | RenameError::Taken(_) | RenameError::CheckedOut => {
                Self::invalid(error)
            }
            error => Self::failed(error),
        }
    }
}

impl From<ExpandError> for CarrelError {
    fn from(error: ExpandError) -> Self {
        match error {
            ExpandError::NotCompressed(_) | ExpandError::CheckedOut | ExpandError::Kept { .. } => {
                Self::invalid(error)
            }
            error => Self::failed(error),
        }
    }
}

/// The last part of an item's path.
fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether the item at `path` (or its card's) is named as a compressed file, so Browse
/// offers to expand it. [`ArchiveHandle::expandable`] says whether it would.
#[uniffi::export]
#[must_use]
pub fn looks_compressed(path: String) -> bool {
    let name = name_of(&path);
    let name = name.strip_suffix(".md").unwrap_or(name);
    carrel_core::detect::Compressed::from_name(name).is_some()
}

#[uniffi::export]
impl ArchiveHandle {
    /// Creates the collection `path` (such as `Finances/Tax`, inside an existing collection),
    /// saying what belongs there and with its tag vocabulary, as `carrel collection new` does.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a path or tag that can't be used, or one that's taken,
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it can't be written.
    pub fn create_collection(
        &self,
        path: String,
        description: Option<String>,
        tags: Vec<CollectionTag>,
    ) -> Result<CreatedCollection, CarrelError> {
        let archive = self.fresh()?;
        let new = NewCollection {
            description: description.filter(|d| !d.trim().is_empty()),
            tags: tags
                .into_iter()
                .map(|t| (t.name.trim().to_owned(), t.meaning.trim().to_owned()))
                .collect(),
        };
        let created = self.with_lease(&archive, "collection", |lease| {
            Ok(carrel_core::collections::create(
                &archive, &path, &new, lease,
            )?)
        })?;
        Ok(CreatedCollection {
            path: created.path,
            id: created.id.to_string(),
            design_document: created.design_document,
            commit: created.commit,
        })
    }

    /// The item `item`'s name, and the name `carrel rename` suggests for it: one that works
    /// on every system.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item, or [`CarrelError::Failed`] if the
    /// archive can't be read.
    pub fn name_suggestion(&self, item: String) -> Result<NameSuggestion, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let current = name_of(item.item.as_deref().unwrap_or(&item.card)).to_owned();
        Ok(NameSuggestion {
            suggested: carrel_core::names::portable(&current),
            current,
        })
    }

    /// Renames the item `item` and its card together to `new_name`, updating links to it, as
    /// `carrel rename` does.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item, a name that isn't portable or is taken,
    /// or an item that's checked out, [`CarrelError::Busy`] if another writer holds the
    /// archive, or [`CarrelError::Failed`] if it can't be renamed.
    pub fn rename_item(&self, item: String, new_name: String) -> Result<RenamedItem, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(CarrelError::invalid("give the item a name"));
        }
        let renamed = self.with_lease(&archive, "rename", |lease| {
            Ok(carrel_core::rename::rename(
                &archive, &item, new_name, lease,
            )?)
        })?;
        Ok(RenamedItem {
            from: renamed.from,
            to: renamed.to,
            card: renamed.card,
            relinked: renamed.relinked,
            commit: renamed.commit,
        })
    }

    /// Whether the filed item `item` would expand, changing nothing, so the app can ask first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item, or [`CarrelError::Failed`] if the
    /// archive can't be read.
    pub fn expandable(&self, item: String) -> Result<Expandable, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        Ok(match examine(&archive, &item) {
            Ok(None) => Expandable::Yes,
            Ok(Some(reason)) => Expandable::AllTheSame { reason },
            Err(
                e @ (ExpandError::NotCompressed(_)
                | ExpandError::CheckedOut
                | ExpandError::Kept { .. }),
            ) => Expandable::No {
                reason: e.to_string(),
            },
            Err(e) => return Err(e.into()),
        })
    }

    /// Expands the filed compressed file `item` in place, as `carrel expand` does: what it
    /// held becomes the item, its card follows, and the compressed file goes to the Trash.
    /// `anyway` expands one kept compressed for a reason it overrides ([`Expandable`]).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown item or one that won't expand,
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if expanding fails; nothing has changed then.
    pub fn expand_item(&self, item: String, anyway: bool) -> Result<ExpandedItem, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        self.through_inbox(&archive, || {
            // What won't expand is said here, rather than sent for the writer to skip.
            match examine(&archive, &item) {
                Err(e @ (ExpandError::NotCompressed(_) | ExpandError::CheckedOut)) => {
                    return Err(e.into());
                }
                Err(e @ ExpandError::Kept { .. }) if !anyway => return Err(e.into()),
                _ => {}
            }
            Ok(carrel_core::inbox::Request::Expand {
                item: item.id,
                anyway,
            })
        })?;
        let expanded = self.with_lease(&archive, "expand", |lease| {
            Ok(expand_item(&archive, &item, anyway, lease)?)
        })?;
        Ok(ExpandedItem {
            from: expanded.from,
            to: expanded.to,
            card: expanded.card,
            kind: expanded.kind.into(),
            files: u64::try_from(expanded.files).unwrap_or(u64::MAX),
            size: expanded.size,
            trash_problem: expanded.trash_problem,
            commit: expanded.commit,
            held: expanded.held.as_ref().map(Into::into),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Sample;

    #[test]
    fn a_collection_is_created_with_its_tags_as_one_change() {
        let sample = Sample::new();
        let created = sample
            .handle
            .create_collection(
                "Projects/Optics".into(),
                Some("Lenses and spectrometers".into()),
                vec![CollectionTag {
                    name: "lab".into(),
                    meaning: "made in the lab".into(),
                }],
            )
            .unwrap();
        assert_eq!(created.path, "Projects/Optics");
        assert!(created.commit.is_some());
        assert!(sample.root().join(&created.design_document).is_file());
        assert!(
            sample
                .handle
                .collections()
                .unwrap()
                .iter()
                .any(|c| c.path == "Projects/Optics")
        );
        let again = sample
            .handle
            .create_collection("Projects/Optics".into(), None, Vec::new())
            .unwrap_err();
        assert!(matches!(again, CarrelError::Invalid { .. }), "{again:?}");
    }

    #[test]
    fn an_item_is_renamed_from_the_suggested_name() {
        let sample = Sample::new();
        let item = sample.handle.items().unwrap()[0].clone();
        let suggestion = sample.handle.name_suggestion(item.id.clone()).unwrap();
        assert_ne!(suggestion.current, "");
        let renamed = sample
            .handle
            .rename_item(item.id.clone(), format!("renamed-{}", suggestion.current))
            .unwrap();
        assert!(renamed.to.contains("renamed-"), "{renamed:?}");
        assert!(renamed.commit.is_some());
        assert!(sample.root().join(&renamed.card).is_file());
        let empty = sample.handle.rename_item(item.id, "  ".into()).unwrap_err();
        assert!(matches!(empty, CarrelError::Invalid { .. }));
    }

    #[test]
    fn compressed_files_are_known_by_name() {
        assert!(looks_compressed("Projects/Spectra.zip".into()));
        assert!(looks_compressed("Projects/Export.tar.gz.md".into()));
        assert!(!looks_compressed("Projects/Thesis.pdf".into()));
    }

    #[test]
    fn an_item_that_is_not_compressed_does_not_expand() {
        let sample = Sample::new();
        let item = sample.handle.items().unwrap()[0].id.clone();
        assert!(matches!(
            sample.handle.expandable(item.clone()).unwrap(),
            Expandable::No { .. }
        ));
        let refused = sample.handle.expand_item(item, false).unwrap_err();
        assert!(
            matches!(refused, CarrelError::Invalid { .. }),
            "{refused:?}"
        );
    }
}
