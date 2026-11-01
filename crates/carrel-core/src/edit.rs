//! Editing a filed card's fields as one undoable change, the body kept exactly
//! (`docs/design/checkout.md`): what `carrel edit` and the apps' inspector do. The fields are
//! checked as a proposal's are ([`crate::actions::apply_card_fields`]).

use crate::actions::{self, ActionError, CardFields, EditedCard};
use crate::archive::{Archive, ItemRef};
use crate::card::Sensitivity;
use crate::card_file::CardFile;
use crate::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use crate::history::{Change, History, Operation};
use crate::providers::Registry;

/// A filed card, read for editing.
#[derive(Debug, Clone)]
pub struct Editing {
    /// The item.
    pub item: ItemRef,
    /// The card as it is.
    pub file: CardFile,
    /// The collection it's in: the card's folder, empty at the archive root.
    pub collection: String,
    /// The lowest sensitivity it may have ([`floor`]).
    pub floor: Sensitivity,
}

impl Editing {
    /// The card's editable fields.
    #[must_use]
    pub fn fields(&self) -> CardFields {
        actions::card_fields(self.file.card())
    }
}

/// What saving an edit did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// Nothing changed, so nothing was written.
    Unchanged,
    /// These fields changed, in this history commit.
    Changed {
        /// The fields, by name.
        fields: Vec<&'static str>,
        /// The change's summary, as the history shows it.
        summary: String,
        /// The commit.
        commit: Option<String>,
    },
    /// The edit doesn't check out, for these reasons; nothing was written.
    Problems(Vec<String>),
}

fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> ActionError + '_ {
    move |source| ActionError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Reads `item`'s card for editing, with `registry` to find its [`floor`].
///
/// # Errors
///
/// [`ActionError::Invalid`] if it's checked out (edit it when it's returned, so the return
/// can't overwrite it) or its card can't be parsed, or [`ActionError::Io`].
pub fn open(
    archive: &Archive,
    registry: &Registry,
    item: &ItemRef,
) -> Result<Editing, ActionError> {
    let path = archive.root().join(&item.card);
    let text = std::fs::read_to_string(&path).map_err(io(&path))?;
    let file =
        CardFile::parse(&text).map_err(|e| ActionError::Invalid(format!("{}: {e}", item.card)))?;
    if file.card().checked_out.is_some() {
        return Err(ActionError::Invalid(
            "it's checked out; edit the card when it's returned, so the return can't overwrite it"
                .into(),
        ));
    }
    Ok(Editing {
        item: item.clone(),
        file,
        collection: item.card.rsplit_once('/').map_or("", |(f, _)| f).to_owned(),
        floor: floor(archive, registry, item),
    })
}

/// The lowest sensitivity the owner may give `item`: sensitive when a rule finds a restricted
/// value in it (a card number, a key, recovery codes), otherwise public.
#[must_use]
pub fn floor(archive: &Archive, registry: &Registry, item: &ItemRef) -> Sensitivity {
    use crate::ingest::sensitivity::{floor, restricted_by_rule};
    let Some(contents) = item.item.as_ref().map(|p| archive.root().join(p)) else {
        return Sensitivity::Public;
    };
    let text = crate::correct::prepare::item_text(archive, registry, &contents, item.kind);
    let name = contents
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if floor(&name, &text, Sensitivity::Public)
        .reasons
        .iter()
        .any(|r| restricted_by_rule(r))
    {
        Sensitivity::Sensitive
    } else {
        Sensitivity::Public
    }
}

/// Checks `fields` and, when they change the card, writes it and records one history change
/// as `holder`, holding the writer lease.
///
/// # Errors
///
/// [`ActionError::Lease`] if another writer holds the archive, or an error writing the card
/// or the history.
pub fn save(
    archive: &Archive,
    editing: Editing,
    fields: CardFields,
    holder: Holder,
) -> Result<Saved, ActionError> {
    let Editing {
        item,
        mut file,
        collection,
        floor,
    } = editing;
    let mut card = file.card().clone();
    let changed = match actions::apply_card_fields(archive, &mut card, &collection, floor, fields)?
    {
        EditedCard::Unchanged => return Ok(Saved::Unchanged),
        EditedCard::Problems(problems) => return Ok(Saved::Problems(problems)),
        EditedCard::Changed(changed) => changed,
    };
    *file.card_mut() = card;
    let summary = format!("Edit “{}”: {}", file.card().title, changed.join(", "));
    let commit = write(archive, &item, &file, &summary, holder)?;
    Ok(Saved::Changed {
        fields: changed,
        summary,
        commit,
    })
}

/// Saves the owner's edit of the card's whole body, its Notes and anything else they wrote
/// below the fields, as one history change as `holder`. Carrel's managed sections (FORMAT.md
/// §5.3), such as the history, must come back exactly as they were: they're changed through
/// their own actions, such as deciding pending entries on Active.
///
/// # Errors
///
/// [`ActionError::Lease`] if another writer holds the archive, or an error writing the card
/// or the history. A body that changes a managed section isn't an error: it comes back as
/// [`Saved::Problems`].
pub fn save_body(
    archive: &Archive,
    editing: Editing,
    body: &str,
    holder: Holder,
) -> Result<Saved, ActionError> {
    let Editing { item, mut file, .. } = editing;
    if body == file.body() {
        return Ok(Saved::Unchanged);
    }
    let managed = |text: &str| {
        crate::sections::find_all(text).map(|found| {
            found
                .into_iter()
                .map(|s| (s.name, text[s.content].to_owned()))
                .collect::<Vec<_>>()
        })
    };
    let was = managed(file.body())
        .map_err(|e| ActionError::Invalid(format!("{}: {e}", item.card)))?;
    match managed(body) {
        Err(e) => return Ok(Saved::Problems(vec![format!("the body's sections: {e}")])),
        Ok(now) if now != was => {
            return Ok(Saved::Problems(vec![
                "Carrel's own sections, between <!-- carrel:… --> markers, have to stay as they are; change the history's entries on Active"
                    .to_owned(),
            ]));
        }
        Ok(_) => {}
    }
    file.set_body(body);
    let summary = format!("Edit “{}”: notes", file.card().title);
    let commit = write(archive, &item, &file, &summary, holder)?;
    Ok(Saved::Changed {
        fields: vec!["notes"],
        summary,
        commit,
    })
}

/// Writes the edited card `file` for `item` and records it as one change, holding the
/// writer lease as `holder`.
///
/// # Errors
///
/// [`ActionError::Lease`], or an error writing the card or the history.
pub fn write(
    archive: &Archive,
    item: &ItemRef,
    file: &CardFile,
    summary: &str,
    holder: Holder,
) -> Result<Option<String>, ActionError> {
    let card_path = archive.root().join(&item.card);
    let text = file
        .to_text()
        .map_err(|e| ActionError::Invalid(format!("{}: {e}", item.card)))?;
    let lease = DiskLock::for_archive(archive).acquire(holder, "edit", DEFAULT_LEASE)?;
    let result = (|| -> Result<Option<String>, ActionError> {
        let written = card_path.with_extension("md.tmp-edit");
        std::fs::write(&written, text).map_err(io(&written))?;
        std::fs::rename(&written, &card_path).map_err(io(&card_path))?;
        let mut change = Change::new(
            Operation::Edit,
            summary,
            lease.info().person,
            lease.info().device,
        );
        change.items = vec![item.id];
        Ok(History::open(archive)?.commit(archive, &change, &lease)?)
    })();
    lease.release()?;
    result
}
