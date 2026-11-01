//! Editing a card's fields in the inspector: a filed card as one undoable change, as `carrel
//! edit` does ([`carrel_core::edit`]), or a proposal's card before it's filed, as `e` in
//! Review does. Both are checked the same way; problems come back to show beside the fields.

use carrel_core::actions::{self, Edited};
use carrel_core::card::{Kind, LinkType};
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
    /// The link types it may have links of: the built-in ones, then those `ARCHIVE.md`
    /// declares.
    pub link_types: Vec<String>,
    /// Whether it needs a location: a physical object or an external reference.
    pub needs_location: bool,
    /// Whether it needs a URL: a URL item.
    pub needs_url: bool,
    /// Whether it's checked out, and so may take snapshots: a file, folder or bundle.
    pub takes_snapshots: bool,
}

/// The link types of `archive`: the built-in ones, then those `ARCHIVE.md` declares.
fn link_types(archive: &carrel_core::archive::Archive) -> Vec<String> {
    let mut types: Vec<String> = LinkType::BUILT_IN.map(str::to_owned).to_vec();
    types.extend(archive.doc().link_types.iter().map(|t| t.name.clone()));
    types
}

/// The edit's outcome, for the app.
fn outcome(saved: carrel_core::edit::Saved) -> EditOutcome {
    match saved {
        carrel_core::edit::Saved::Unchanged => EditOutcome::Unchanged,
        carrel_core::edit::Saved::Changed { fields, commit, .. } => EditOutcome::Saved {
            fields: fields.iter().map(|&f| f.to_owned()).collect(),
            commit,
        },
        carrel_core::edit::Saved::Problems(problems) => EditOutcome::Problems { problems },
        carrel_core::edit::Saved::BodyChanged { current, both } => {
            EditOutcome::BodyChanged { current, both }
        }
        carrel_core::edit::Saved::FieldsChanged { fields, current } => EditOutcome::FieldsChanged {
            fields: fields.iter().map(|&f| f.to_owned()).collect(),
            current: (&*current).into(),
        },
    }
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
        let kind = editing.file.card().kind;
        Ok(EditableCard {
            fields: (&editing.fields()).into(),
            tags: actions::tags_at(&archive, &editing.collection)?,
            floor: editing.floor.into(),
            link_types: link_types(&archive),
            needs_location: matches!(kind, Kind::Physical | Kind::External),
            needs_url: kind == Kind::Url,
            takes_snapshots: !kind.is_card_only(),
        })
    }

    /// Saves `fields`, the owner's edit of `base`, the fields [`ArchiveHandle::editable_card`]
    /// gave the editor, to the filed item `item`'s card, checked as `carrel edit` checks them,
    /// as one change undone from History. The card's body is kept exactly, and so is a field
    /// saved another way meanwhile that the edit didn't change.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item or it's checked out,
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`]. Fields that don't check out aren't an error: they come back
    /// as [`EditOutcome::Problems`], and ones the edit changed that were saved another way
    /// meanwhile as [`EditOutcome::FieldsChanged`].
    pub fn edit_card(
        &self,
        item: String,
        base: CardFields,
        fields: CardFields,
    ) -> Result<EditOutcome, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let editing = carrel_core::edit::open(&archive, &self.registry(), &item)?;
        let was = editing.fields();
        let (base, fields) = (base.into_core(&was), fields.into_core(&was));
        Ok(outcome(carrel_core::edit::save(
            &archive,
            editing,
            &base,
            fields,
            self.holder()?,
        )?))
    }

    /// The filed item `item`'s card body, its Notes and Carrel's sections, to edit.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::editable_card`].
    pub fn card_body(&self, item: String) -> Result<String, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let editing = carrel_core::edit::open(&archive, &self.registry(), &item)?;
        Ok(editing.file.body().to_owned())
    }

    /// Saves `body`, the owner's edit of `base`, the body [`ArchiveHandle::card_body`] gave
    /// the editor, as the filed item `item`'s card body, as one change undone from History.
    /// Carrel's own sections, such as the history, have to be as `base` had them; what was
    /// saved another way meanwhile is kept beside the edit.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::edit_card`]; a body that changes Carrel's sections comes back as
    /// [`EditOutcome::Problems`], and one whose lines were changed meanwhile too as
    /// [`EditOutcome::BodyChanged`].
    pub fn edit_card_body(
        &self,
        item: String,
        base: String,
        body: String,
    ) -> Result<EditOutcome, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let editing = carrel_core::edit::open(&archive, &self.registry(), &item)?;
        Ok(outcome(carrel_core::edit::save_body(
            &archive,
            editing,
            &base,
            &body,
            self.holder()?,
        )?))
    }

    /// The proposal `id`'s card, to edit before it's filed.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found.
    pub fn editable_proposal(&self, id: String) -> Result<EditableCard, CarrelError> {
        let archive = self.fresh()?;
        let entry = ReviewQueue::of(&archive).find(&id)?;
        // Its links, location, URL and snapshots are set once it's filed.
        Ok(EditableCard {
            fields: (&actions::draft_fields(&entry)).into(),
            tags: actions::tags_at(&archive, &entry.destination.path)?,
            floor: entry.card.sensitivity.into(),
            link_types: Vec::new(),
            needs_location: false,
            needs_url: false,
            takes_snapshots: false,
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

#[cfg(test)]
mod tests {
    use crate::records::{CardLink, EditOutcome};
    use crate::tests::Sample;

    fn problems(outcome: EditOutcome) -> Vec<String> {
        match outcome {
            EditOutcome::Problems { problems } => problems,
            other => panic!("expected problems, got {other:?}"),
        }
    }

    fn saved(outcome: EditOutcome) -> Vec<String> {
        match outcome {
            EditOutcome::Saved { fields, .. } => fields,
            other => panic!("expected it saved, got {other:?}"),
        }
    }

    #[test]
    fn a_location_and_an_address_are_edited_and_checked() {
        let sample = Sample::new();
        let book = sample.item_titled("On the Origin of Species (1859), paperback");
        let editable = sample.handle.editable_card(book.clone()).unwrap();
        assert!(editable.needs_location && !editable.needs_url && !editable.takes_snapshots);
        let base = editable.fields;
        let mut fields = base.clone();
        fields.location = None;
        let found = problems(
            sample
                .handle
                .edit_card(book.clone(), base.clone(), fields)
                .unwrap(),
        );
        assert!(found.iter().any(|p| p.contains("location")), "{found:?}");
        let mut fields = base.clone();
        fields.location = Some("  Study,\n  top shelf ".into());
        fields.snapshot_on_return_only = true;
        let found = problems(
            sample
                .handle
                .edit_card(book.clone(), base.clone(), fields)
                .unwrap(),
        );
        assert!(found.iter().any(|p| p.contains("snapshots")), "{found:?}");
        let mut fields = base.clone();
        fields.location = Some("  Study,\n  top shelf ".into());
        assert_eq!(
            saved(sample.handle.edit_card(book.clone(), base, fields).unwrap()),
            ["location"]
        );
        let now = sample.handle.editable_card(book).unwrap().fields;
        assert_eq!(now.location.as_deref(), Some("Study, top shelf"));

        let page = sample.item_titled("RFC 9562: Universally Unique IDentifiers (UUIDs)");
        let editable = sample.handle.editable_card(page.clone()).unwrap();
        assert!(editable.needs_url);
        let base = editable.fields;
        let mut fields = base.clone();
        fields.url = Some("www.rfc-editor.org/rfc/rfc9562".into());
        let found = problems(
            sample
                .handle
                .edit_card(page.clone(), base.clone(), fields)
                .unwrap(),
        );
        assert!(found.iter().any(|p| p.contains("url")), "{found:?}");
        let mut fields = base.clone();
        fields.url = Some("https://datatracker.ietf.org/doc/rfc9562/".into());
        assert_eq!(
            saved(sample.handle.edit_card(page, base, fields).unwrap()),
            ["url"]
        );
    }

    #[test]
    fn links_are_checked_against_the_archive_and_snapshots_set() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let editable = sample.handle.editable_card(item.clone()).unwrap();
        assert!(editable.takes_snapshots);
        assert!(editable.link_types.contains(&"supersedes".to_owned()));
        let base = editable.fields;
        assert_eq!(base.links.len(), 1, "the fixture's part-of link");
        let book = sample.item_titled("On the Origin of Species (1859), paperback");
        let link = |link_type: &str, to: &str| CardLink {
            link_type: link_type.into(),
            item: to.into(),
            note: None,
        };
        let mut fields = base.clone();
        fields.links.push(link("replaces", &book));
        fields.links.push(link("related", &item));
        fields.links.push(link("related", "ffffffffffff"));
        let found = problems(
            sample
                .handle
                .edit_card(item.clone(), base.clone(), fields)
                .unwrap(),
        );
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].contains("replaces") && found[0].contains("supersedes"));
        assert!(found[1].contains("itself") && found[2].contains("no item"));
        let mut fields = base.clone();
        // The end of its identifier is enough to find it.
        fields.links.push(CardLink {
            note: Some(" Where the idea came from ".into()),
            ..link("supersedes", &book[book.len() - 12..])
        });
        fields.snapshot_on_return_only = true;
        assert_eq!(
            saved(
                sample
                    .handle
                    .edit_card(item.clone(), base.clone(), fields)
                    .unwrap()
            ),
            ["links", "snapshots"]
        );
        let now = sample.handle.editable_card(item.clone()).unwrap().fields;
        assert_eq!(now.links[0], base.links[0]);
        assert_eq!(
            now.links[1],
            CardLink {
                note: Some("Where the idea came from".into()),
                ..link("supersedes", &book)
            }
        );
        assert!(now.snapshot_on_return_only);
        let card =
            std::fs::read_to_string(sample.root().join("Projects/Hardware/parts-list.csv.md"))
                .unwrap();
        assert!(
            card.contains("path: lamp-build.md")
                && card.contains("../../Reading/On the Origin of Species.md")
                && card.contains("snapshots: return-only"),
            "{card}"
        );
    }

    #[test]
    fn a_field_saved_another_way_meanwhile_is_kept_or_reported() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        // The app's editor opens the card; meanwhile `carrel edit` changes the title.
        let opened = sample.handle.editable_card(item.clone()).unwrap().fields;
        let mut theirs = opened.clone();
        theirs.title = "Lamp parts".into();
        saved(
            sample
                .handle
                .edit_card(item.clone(), opened.clone(), theirs)
                .unwrap(),
        );
        // The app saves a summary, against the fields it opened: the title stays theirs.
        let mut mine = opened.clone();
        mine.summary = Some("Every part, with where it came from.".into());
        assert_eq!(
            saved(
                sample
                    .handle
                    .edit_card(item.clone(), opened.clone(), mine.clone())
                    .unwrap()
            ),
            ["summary"]
        );
        let now = sample.handle.editable_card(item.clone()).unwrap().fields;
        assert_eq!(now.title, "Lamp parts");
        assert_eq!(now.summary, mine.summary);
        // A title of its own, though, is reported, and nothing is written.
        mine.title = "Desk lamp bill of materials".into();
        let EditOutcome::FieldsChanged { fields, current } = sample
            .handle
            .edit_card(item.clone(), opened, mine.clone())
            .unwrap()
        else {
            panic!("expected the clash to be reported");
        };
        assert_eq!(fields, ["title"]);
        assert_eq!(current, now);
        assert_eq!(
            sample.handle.editable_card(item.clone()).unwrap().fields,
            now
        );
        // Keeping it is saving it again against the fields as they are now.
        assert_eq!(
            saved(sample.handle.edit_card(item, current, mine).unwrap()),
            ["title"]
        );
    }

    #[test]
    fn a_card_body_is_edited_but_carrels_sections_are_kept() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let body = sample.handle.card_body(item.clone()).unwrap();
        let edited = format!("A note of my own.\n\n{body}");
        let saved = sample
            .handle
            .edit_card_body(item.clone(), body.clone(), edited.clone())
            .unwrap();
        assert!(
            matches!(saved, EditOutcome::Saved { ref fields, commit: Some(_) } if fields == &["notes"])
        );
        assert_eq!(sample.handle.card_body(item.clone()).unwrap(), edited);
        assert_eq!(
            sample
                .handle
                .edit_card_body(item.clone(), edited.clone(), edited.clone())
                .unwrap(),
            EditOutcome::Unchanged
        );
        if edited.contains("<!-- carrel:history -->") {
            let broken = edited.replace("## History", "## Not the history");
            assert!(matches!(
                sample.handle.edit_card_body(item, edited, broken).unwrap(),
                EditOutcome::Problems { .. }
            ));
        }
    }

    #[test]
    fn a_stale_body_edit_does_not_overwrite_a_newer_one() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        // The Mac app's editor reads the body (CardEditModel.load).
        let opened = sample.handle.card_body(item.clone()).unwrap();
        // Meanwhile another interface (carrel edit --body, Browse's E) saves a note.
        let theirs = format!("Their note.\n\n{opened}");
        assert!(matches!(
            sample
                .handle
                .edit_card_body(item.clone(), opened.clone(), theirs)
                .unwrap(),
            EditOutcome::Saved { .. }
        ));
        // The app saves the owner's edit of the body it read, a note at the end.
        let mine = format!("{}\n\nMy note.\n", opened.trim_end());
        let outcome = sample
            .handle
            .edit_card_body(item.clone(), opened, mine)
            .unwrap();
        let now = sample.handle.card_body(item).unwrap();
        assert!(matches!(outcome, EditOutcome::Saved { .. }), "{outcome:?}");
        assert!(
            now.contains("Their note.") && now.contains("My note."),
            "both notes are kept; the card now reads:\n{now}"
        );
    }

    #[test]
    fn the_same_lines_saved_both_ways_are_kept_for_the_owner_to_choose() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let opened = sample.handle.card_body(item.clone()).unwrap();
        let theirs = format!("Their note.\n\n{opened}");
        sample
            .handle
            .edit_card_body(item.clone(), opened.clone(), theirs.clone())
            .unwrap();
        let mine = format!("My note.\n\n{opened}");
        let EditOutcome::BodyChanged { current, both } = sample
            .handle
            .edit_card_body(item.clone(), opened, mine)
            .unwrap()
        else {
            panic!("expected the clash to be reported");
        };
        assert_eq!(current, theirs, "nothing was written");
        assert_eq!(sample.handle.card_body(item.clone()).unwrap(), theirs);
        assert!(
            both.contains("Their note.") && both.contains("My note."),
            "{both}"
        );
        // Keeping both is saving them against the body as it is now.
        assert!(matches!(
            sample.handle.edit_card_body(item, current, both).unwrap(),
            EditOutcome::Saved { .. }
        ));
    }

    #[test]
    fn a_body_edit_survives_a_history_entry_added_meanwhile() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        // The app's editor reads the body.
        let opened = sample.handle.card_body(item.clone()).unwrap();
        // Meanwhile the item is checked out and returned, which adds a history entry.
        let to = sample.dir.path().join("Desktop");
        std::fs::create_dir_all(&to).unwrap();
        let out = sample
            .handle
            .check_out(item.clone(), Some(crate::text(&to)), None)
            .unwrap();
        std::fs::write(&out.working, "part,source\nLED strip,shop\n").unwrap();
        sample
            .handle
            .return_item(item.clone(), None, false)
            .unwrap();
        let returned = sample.handle.card_body(item.clone()).unwrap();
        assert_ne!(returned, opened, "the return added to the history");
        // The owner, who touched only their notes, saves.
        let mine = format!("My note.\n\n{opened}");
        let outcome = sample
            .handle
            .edit_card_body(item.clone(), opened, mine)
            .unwrap();
        assert!(
            matches!(outcome, EditOutcome::Saved { .. }),
            "the owner's notes weren't saved: {outcome:?}"
        );
        let now = sample.handle.card_body(item).unwrap();
        assert_eq!(
            now,
            format!("My note.\n\n{returned}"),
            "and the return's entry is kept"
        );
    }
}
