//! Editing a card's fields in the inspector: a filed card as one undoable change, as `carrel
//! edit` does ([`carrel_core::edit`]), or a proposal's card before it's filed, as `e` in
//! Review does. Both are checked the same way; problems come back to show beside the fields.

use carrel_core::actions::{self, Edited};
use carrel_core::review::ReviewQueue;

use crate::records::{CardFields, EditOutcome, Sensitivity};
use crate::{ArchiveHandle, CarrelError};

/// A card's fields ready to edit, with what applies to them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EditableCard {
    /// The fields as they are.
    pub fields: CardFields,
    /// The tags that apply where it is or goes, to choose from. A proposal may add one new
    /// tag; a filed card only uses these.
    pub tags: Vec<String>,
    /// The lowest sensitivity it may be given.
    pub floor: Sensitivity,
}

#[uniffi::export]
impl ArchiveHandle {
    /// The filed item `item`'s card, to edit.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item or it's checked out (edit it when
    /// it's returned), or [`CarrelError::Failed`].
    pub fn editable_card(&self, item: String) -> Result<EditableCard, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let editing = carrel_core::edit::open(&archive, &self.registry(), &item)?;
        Ok(EditableCard {
            fields: (&editing.fields()).into(),
            tags: actions::tags_at(&archive, &editing.collection)?,
            floor: editing.floor.into(),
        })
    }

    /// Saves `fields` to the filed item `item`'s card, checked as `carrel edit` checks
    /// them, as one change undone from History. The card's body is kept exactly.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item or it's checked out,
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`]. Fields that don't check out aren't an error: they come back
    /// as [`EditOutcome::Problems`].
    pub fn edit_card(&self, item: String, fields: CardFields) -> Result<EditOutcome, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let editing = carrel_core::edit::open(&archive, &self.registry(), &item)?;
        let fields = fields.into_core(&editing.fields());
        Ok(
            match carrel_core::edit::save(&archive, editing, fields, self.holder()?)? {
                carrel_core::edit::Saved::Unchanged => EditOutcome::Unchanged,
                carrel_core::edit::Saved::Changed { fields, commit, .. } => EditOutcome::Saved {
                    fields: fields.iter().map(|&f| f.to_owned()).collect(),
                    commit,
                },
                carrel_core::edit::Saved::Problems(problems) => EditOutcome::Problems { problems },
            },
        )
    }

    /// The proposal `id`'s card, to edit before it's filed.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found.
    pub fn editable_proposal(&self, id: String) -> Result<EditableCard, CarrelError> {
        let archive = self.fresh()?;
        let entry = ReviewQueue::of(&archive).find(&id)?;
        Ok(EditableCard {
            fields: (&actions::draft_fields(&entry)).into(),
            tags: actions::tags_at(&archive, &entry.destination.path)?,
            floor: entry.card.sensitivity.into(),
        })
    }

    /// Saves `fields` to the proposal `id`'s card, checked as `e` in Review checks them.
    /// Nothing is filed until it's approved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a decided proposal, or [`CarrelError::Failed`].
    pub fn edit_proposal(
        &self,
        id: String,
        fields: CardFields,
    ) -> Result<EditOutcome, CarrelError> {
        let archive = self.fresh()?;
        let mut entry = actions::proposals(&archive, &[id])?
            .pop()
            .ok_or_else(|| CarrelError::invalid("no such proposal"))?;
        let was = actions::draft_fields(&entry);
        let before = was.clone();
        Ok(
            match actions::apply_draft_fields(&archive, &mut entry, fields.into_core(&was))? {
                Edited::Unchanged => EditOutcome::Unchanged,
                Edited::Problems(problems) => EditOutcome::Problems { problems },
                Edited::Changed => {
                    ReviewQueue::of(&archive).save(&entry)?;
                    let after = actions::draft_fields(&entry);
                    EditOutcome::Saved {
                        fields: changed_fields(&before, &after),
                        commit: None,
                    }
                }
            },
        )
    }
}

/// The names of the fields that differ between `before` and `after`.
fn changed_fields(before: &actions::CardFields, after: &actions::CardFields) -> Vec<String> {
    [
        ("title", before.title != after.title),
        ("summary", before.summary != after.summary),
        ("tags", before.tags != after.tags),
        ("dates", before.dates != after.dates),
        ("sensitivity", before.sensitivity != after.sensitivity),
    ]
    .into_iter()
    .filter(|&(_, differs)| differs)
    .map(|(field, _)| field.to_owned())
    .collect()
}
