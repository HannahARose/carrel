//! Applying a whole-section proposal's changes (`docs/design/restructure.md`, "A restructure
//! proposal"): creating, renaming and merging collections, moving items, and rewriting design
//! documents, to the archive as it is when the owner approves. None of these records history
//! itself: [`super::approve_with_snapshot`] records them all as one `restructure` change, with
//! every move, so undoing it puts every item, folder, design document and link back.
//!
//! What can't be done safely is left alone and said: an item that's checked out, moved since
//! it was proposed, or whose name is taken where it would go; a collection holding a
//! checked-out item, or files that aren't items. Nothing is deleted: a merged collection's
//! design document and changelog leave with it, and history keeps them for undo. Sealed
//! bundles move whole and are never entered.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use super::{Edit, Outcome, ProposedChange, RestructureError, io};
use crate::archive::{Archive, ItemRef, escape_link_text};
use crate::card::Id;
use crate::card_file::CardFile;
use crate::collections::{NewCollection, write_collection};
use crate::documents::CollectionDoc;
use crate::history::Move;

/// Whether `edit` moves files, so a snapshot is taken first and links are rewritten after.
pub(super) fn moves_files(edit: &Edit) -> bool {
    matches!(
        edit,
        Edit::MoveItems { .. } | Edit::RenameCollection { .. } | Edit::MergeCollection { .. }
    )
}

/// Applies one section change to the archive as it is now. A change that can't be made
/// safely is reported in the outcome (`not_applied`, or `skipped` for single items) instead,
/// so the history commit always records what was done.
pub(super) fn apply(archive: &Archive, change: &ProposedChange) -> Outcome {
    let mut outcome = Outcome {
        change: change.id.clone(),
        ..Outcome::default()
    };
    let result = match &change.change {
        Edit::CreateCollection { path, description } => {
            create(archive, path, description, &mut outcome)
        }
        Edit::MoveItems { to } => move_items(archive, change, to, &mut outcome),
        Edit::RenameCollection { from, to } => rename(archive, from, to, &mut outcome),
        Edit::MergeCollection { from, into } => merge(archive, from, into, &mut outcome),
        Edit::RewriteDesign {
            collection,
            description,
            organization,
        } => rewrite(
            archive,
            collection,
            description.as_deref(),
            organization.as_deref(),
            &mut outcome,
        ),
        Edit::MergeTags { .. } | Edit::SplitTag { .. } | Edit::RetireTag { .. } => {
            Err("it isn't a section change".to_owned())
        }
    };
    if let Err(why) = result {
        outcome.not_applied = Some(why);
    }
    outcome
}

/// After the moves: every card's links point where their items are now, and every listing
/// is rebuilt.
///
/// # Errors
///
/// An error reading or writing a card or a listing.
pub(super) fn finish(archive: &Archive) -> Result<(), RestructureError> {
    relink_everything(archive)?;
    crate::listing::refresh(archive)?;
    Ok(())
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(f, _)| f)
}

fn name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, n)| n)
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// Whether `name` is free in `folder`, compared without case, as case-insensitive disks see it.
fn is_free(folder: &Path, name: &str) -> bool {
    !std::fs::read_dir(folder).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
    })
}

/// Adds a line to the collection's changelog, if it keeps one. A changelog made here inside a
/// collection the same change renames would move back with the folder on undo, untracked, and
/// stay behind.
fn changelog(archive: &Archive, collection: &str, entry: &str) -> Result<(), String> {
    if collection.is_empty()
        || !archive
            .root()
            .join(collection)
            .join("CHANGELOG.md")
            .is_file()
    {
        return Ok(());
    }
    crate::listing::add_to_changelog(archive, collection, &format!("{entry} (restructure)"))
        .map_err(|e| format!("{collection}/CHANGELOG.md: {e}"))
}

fn create(
    archive: &Archive,
    path: &str,
    description: &str,
    outcome: &mut Outcome,
) -> Result<(), String> {
    let new = NewCollection {
        description: Some(description.to_owned()).filter(|d| !d.trim().is_empty()),
        tags: Vec::new(),
    };
    write_collection(archive, path, &new).map_err(|e| e.to_string())?;
    outcome.documents.push(format!("{path}/_collection.md"));
    Ok(())
}

fn move_items(
    archive: &Archive,
    change: &ProposedChange,
    to: &str,
    outcome: &mut Outcome,
) -> Result<(), String> {
    let to = to.trim_matches('/');
    let collections = archive.collections().map_err(|e| e.to_string())?;
    if !collections.iter().any(|c| c.path == to) {
        return Err(format!("the collection {to} isn't there"));
    }
    let items: HashMap<Id, ItemRef> = archive
        .items()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|i| (i.id, i))
        .collect();
    for touched in &change.items {
        let Some(item) = items.get(&touched.id) else {
            outcome
                .skipped
                .push((touched.id, "it's no longer in the archive".into()));
            continue;
        };
        if item.card != touched.card {
            outcome.skipped.push((
                touched.id,
                "it was moved after the proposal was made".into(),
            ));
            continue;
        }
        if folder_of(&item.card) == to {
            outcome
                .skipped
                .push((touched.id, format!("it's already in {to}")));
            continue;
        }
        match move_one(archive, item, to) {
            Ok(moved) => {
                outcome.items.push(item.id);
                outcome.moves.extend(moved);
            }
            Err(why) => outcome.skipped.push((item.id, why)),
        }
    }
    Ok(())
}

/// Moves one item and its card into the folder `to`, keeping their names. Returns the move of
/// its contents (a card-only item has none); the inner error says why it was left alone.
fn move_one(archive: &Archive, item: &ItemRef, to: &str) -> Result<Option<Move>, String> {
    let root = archive.root();
    let card_path = root.join(&item.card);
    let text = std::fs::read_to_string(&card_path).map_err(|e| e.to_string())?;
    let mut file = CardFile::parse(&text).map_err(|e| e.to_string())?;
    if file.card().checked_out.is_some() {
        return Err("it's checked out; it can move once it's returned".into());
    }
    let from_folder = folder_of(&item.card);
    let card_name = name_of(&item.card);
    let target = root.join(to);
    let contents = item.item.as_deref();
    for name in contents.map(name_of).into_iter().chain([card_name]) {
        if !is_free(&target, name) {
            return Err(format!("{to} already has something named {name}"));
        }
    }
    // The owner approved where it goes, so its placement is no longer in doubt (FORMAT.md §5.8).
    file.card_mut().placement = None;
    let updated = file.to_text().map_err(|e| e.to_string())?;
    let moved = match contents {
        Some(contents) => {
            let new = join(to, name_of(contents));
            std::fs::rename(root.join(contents), root.join(&new)).map_err(|e| e.to_string())?;
            Some(Move {
                item: item.id,
                from: contents.to_owned(),
                to: new,
            })
        }
        None => None,
    };
    let new_card = target.join(card_name);
    let temp = target.join(format!(".{card_name}.tmp-{}", item.id));
    std::fs::write(&temp, updated)
        .and_then(|()| std::fs::rename(&temp, &new_card))
        .and_then(|()| std::fs::remove_file(&card_path))
        .map_err(|e| e.to_string())?;
    let title = file.card().title.clone();
    changelog(
        archive,
        from_folder,
        &format!("moved “{}” to {to}", title.replace('|', "\\|")),
    )?;
    changelog(
        archive,
        to,
        &format!(
            "moved [{}]({}) here from {}",
            escape_link_text(&title),
            crate::listing::link_destination(card_name),
            if from_folder.is_empty() {
                "the top level"
            } else {
                from_folder
            }
        ),
    )?;
    Ok(moved)
}

/// Checks that the collection at `path` can move: it exists, isn't Holding, and holds no
/// checked-out item. Returns its identifier.
fn movable(archive: &Archive, path: &str) -> Result<Id, String> {
    if path.eq_ignore_ascii_case("Holding") {
        return Err("Holding stays where it is".into());
    }
    let collections = archive.collections().map_err(|e| e.to_string())?;
    let collection = collections
        .iter()
        .find(|c| c.path == path)
        .ok_or_else(|| format!("the collection {path} isn't there"))?;
    for item in archive.items().map_err(|e| e.to_string())? {
        if crate::graph::is_within(folder_of(&item.card), path) {
            let text = std::fs::read_to_string(archive.root().join(&item.card))
                .map_err(|e| e.to_string())?;
            if CardFile::parse(&text).is_ok_and(|f| f.card().checked_out.is_some()) {
                return Err(format!(
                    "{path} holds a checked-out item; it can change once that's returned"
                ));
            }
        }
    }
    Ok(collection.doc.id)
}

/// Reads a design document's frontmatter, lets `edit` change it and writes it back, keeping its
/// body exactly unless `body` replaces it.
fn edit_design(
    archive: &Archive,
    collection: &str,
    edit: impl FnOnce(&mut CollectionDoc),
    body: Option<&dyn Fn(&str) -> String>,
) -> Result<(), String> {
    let path = archive.root().join(collection).join("_collection.md");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let split = crate::frontmatter::split(&text).map_err(|e| e.to_string())?;
    let mut doc: CollectionDoc =
        serde_saphyr::from_str(split.yaml).map_err(|e| format!("{collection}: {e}"))?;
    let before = doc.clone();
    edit(&mut doc);
    let yaml = if doc == before {
        split.yaml.to_owned()
    } else {
        serde_saphyr::to_string(&doc).map_err(|e| e.to_string())?
    };
    let new_body = body.map_or_else(|| split.body.to_owned(), |b| b(split.body));
    let updated = crate::frontmatter::join(&yaml, &new_body);
    if updated != text {
        std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn rename(archive: &Archive, from: &str, to: &str, outcome: &mut Outcome) -> Result<(), String> {
    let (from, to) = (from.trim_matches('/'), to.trim_matches('/'));
    let id = movable(archive, from)?;
    let problems = crate::names::problems(name_of(to));
    if !problems.is_empty() {
        return Err(format!("the new name {}", problems.join(" and ")));
    }
    if super::hidden_name(name_of(to)) {
        return Err(format!(
            "{} starts with a dot or an underscore, which would hide the collection",
            name_of(to)
        ));
    }
    let parent = folder_of(to);
    if !parent.is_empty()
        && !archive
            .collections()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|c| c.path == parent)
    {
        return Err(format!("{parent} isn't a collection"));
    }
    let root = archive.root();
    let only_case = from.eq_ignore_ascii_case(to);
    if !only_case && !is_free(&root.join(parent), name_of(to)) {
        return Err(format!("{to} is taken"));
    }
    if only_case {
        // A rename that only changes case goes through a free name on case-insensitive disks.
        let step = root.join(parent).join(format!(".carrel-rename-{id}"));
        std::fs::rename(root.join(from), &step)
            .and_then(|()| std::fs::rename(&step, root.join(to)))
            .map_err(|e| e.to_string())?;
    } else {
        std::fs::rename(root.join(from), root.join(to)).map_err(|e| e.to_string())?;
    }
    outcome.moves.push(Move {
        item: id,
        from: from.to_owned(),
        to: to.to_owned(),
    });
    let title = name_of(to).to_owned();
    edit_design(archive, to, |doc| doc.title = title, None)?;
    changelog(archive, to, &format!("renamed from {from}"))?;
    outcome.documents.push(format!("{to}/_collection.md"));
    Ok(())
}

/// What's in a collection's folder, sorted out for merging it.
struct Contents {
    /// Items directly in it.
    items: Vec<ItemRef>,
    /// Its subcollections' folder names, with their identifiers.
    subcollections: Vec<(String, Id)>,
    /// Names that are neither, nor its design document and changelog.
    strays: Vec<String>,
}

fn contents_of(archive: &Archive, path: &str) -> Result<Contents, String> {
    let items: Vec<ItemRef> = archive
        .items()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|i| folder_of(&i.card) == path)
        .collect();
    let subcollections: Vec<(String, Id)> = archive
        .collections()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|c| folder_of(&c.path) == path && c.path != path)
        .map(|c| (name_of(&c.path).to_owned(), c.doc.id))
        .collect();
    let mut known: Vec<String> = vec!["_collection.md".into(), "CHANGELOG.md".into()];
    for item in &items {
        known.push(name_of(&item.card).to_owned());
        known.extend(item.item.as_deref().map(|p| name_of(p).to_owned()));
    }
    known.extend(subcollections.iter().map(|(n, _)| n.clone()));
    let folder = archive.root().join(path);
    let mut strays = Vec::new();
    for entry in std::fs::read_dir(&folder).map_err(|e| e.to_string())? {
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if !crate::detect::is_ignored(&name) && !known.contains(&name) {
            strays.push(name);
        }
    }
    strays.sort();
    Ok(Contents {
        items,
        subcollections,
        strays,
    })
}

fn merge(archive: &Archive, from: &str, into: &str, outcome: &mut Outcome) -> Result<(), String> {
    let (from, into) = (from.trim_matches('/'), into.trim_matches('/'));
    if from == into || crate::graph::is_within(into, from) {
        return Err(format!("{from} can't be merged into {into}"));
    }
    movable(archive, from)?;
    let collections = archive.collections().map_err(|e| e.to_string())?;
    let Some(source) = collections.iter().find(|c| c.path == from) else {
        return Err(format!("the collection {from} isn't there"));
    };
    if !collections.iter().any(|c| c.path == into) {
        return Err(format!("the collection {into} isn't there"));
    }
    let contents = contents_of(archive, from)?;
    if !contents.strays.is_empty() {
        return Err(format!(
            "{from} holds files that aren't items ({}), so it stays",
            contents.strays.join(", ")
        ));
    }
    let target = archive.root().join(into);
    let mut names: Vec<&str> = contents
        .subcollections
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    for item in &contents.items {
        names.push(name_of(&item.card));
        names.extend(item.item.as_deref().map(name_of));
    }
    if let Some(taken) = names.iter().find(|n| !is_free(&target, n)) {
        return Err(format!("{into} already has something named {taken}"));
    }
    for item in &contents.items {
        let moved = move_one(archive, item, into)?;
        outcome.items.push(item.id);
        outcome.moves.extend(moved);
    }
    for (name, id) in &contents.subcollections {
        let (old, new) = (join(from, name), join(into, name));
        std::fs::rename(archive.root().join(&old), archive.root().join(&new))
            .map_err(|e| e.to_string())?;
        outcome.moves.push(Move {
            item: *id,
            from: old,
            to: new,
        });
    }
    // Its vocabulary joins the other's, so the tags its items carry stay defined.
    let tags = source.doc.tags.clone();
    edit_design(
        archive,
        into,
        |doc| {
            for def in tags {
                if !doc.tags.iter().any(|d| d.name == def.name) {
                    doc.tags.push(def);
                }
            }
        },
        None,
    )?;
    retire_folder(archive, from)?;
    changelog(
        archive,
        into,
        &format!("merged {from} into this collection"),
    )?;
    outcome.documents.push(format!("{into}/_collection.md"));
    outcome.documents.push(format!("{from}/_collection.md"));
    Ok(())
}

/// Takes away an emptied collection's design document and changelog (history keeps both, for
/// undo) and its folder, if nothing else is left in it.
fn retire_folder(archive: &Archive, path: &str) -> Result<(), String> {
    let folder = archive.root().join(path);
    for name in ["_collection.md", "CHANGELOG.md"] {
        let file = folder.join(name);
        if file.exists() {
            std::fs::remove_file(&file).map_err(|e| e.to_string())?;
        }
    }
    // Fails, and leaves the folder, if anything the owner keeps there is left.
    let _ = std::fs::remove_dir(&folder);
    Ok(())
}

/// `body` with `text` added at the end of its Organization section, or a new Organization
/// section before the listing; unchanged if it already says it.
fn with_organization(body: &str, text: &str) -> String {
    if body.contains(text.trim()) {
        return body.to_owned();
    }
    let addition = format!("{}\n\n", text.trim());
    if let Some(start) = body.find("\n## Organization") {
        let after = start + "\n## Organization".len();
        // The section ends at the next heading or the managed listing, whichever comes first.
        let end = [
            body[after..].find("\n## "),
            body[after..].find("\n<!-- carrel:listing -->"),
        ]
        .into_iter()
        .flatten()
        .min()
        .map_or(body.len(), |e| after + e + 1);
        let mut out = body[..end].trim_end().to_owned();
        out.push_str("\n\n");
        out.push_str(&addition);
        out.push_str(&body[end..]);
        return out;
    }
    let at = body.find("<!-- carrel:listing -->").unwrap_or(body.len());
    format!(
        "{}## Organization\n\n{addition}{}",
        &body[..at],
        &body[at..]
    )
}

fn rewrite(
    archive: &Archive,
    collection: &str,
    description: Option<&str>,
    organization: Option<&str>,
    outcome: &mut Outcome,
) -> Result<(), String> {
    let collection = collection.trim_matches('/');
    if !archive
        .root()
        .join(collection)
        .join("_collection.md")
        .is_file()
    {
        return Err(format!("the collection {collection} isn't there"));
    }
    let description = description.map(str::to_owned);
    let organize = organization.map(|text| move |body: &str| with_organization(body, text));
    edit_design(
        archive,
        collection,
        |doc| {
            if let Some(d) = description {
                doc.description = Some(d);
            }
        },
        organize.as_ref().map(|f| f as &dyn Fn(&str) -> String),
    )?;
    changelog(archive, collection, "rewrote the design document")?;
    outcome
        .documents
        .push(format!("{collection}/_collection.md"));
    Ok(())
}

/// Points every card's links at where their items are now.
fn relink_everything(archive: &Archive) -> Result<(), RestructureError> {
    let items = archive.items()?;
    let cards: BTreeMap<Id, &str> = items.iter().map(|i| (i.id, i.card.as_str())).collect();
    for item in &items {
        let path = archive.root().join(&item.card);
        let text = std::fs::read_to_string(&path).map_err(io(&path))?;
        let Ok(mut file) = CardFile::parse(&text) else {
            continue;
        };
        let folder = folder_of(&item.card);
        let mut changed = false;
        for link in &mut file.card_mut().links {
            let Some(target) = cards.get(&link.id) else {
                continue;
            };
            let wanted = crate::listing::relative(folder, target);
            if link.path.is_some() && link.path.as_deref() != Some(wanted.as_str()) {
                link.path = Some(wanted);
                changed = true;
            }
        }
        if changed {
            let updated = file
                .to_text()
                .map_err(|e| RestructureError::Invalid(format!("{}: {e}", item.card)))?;
            std::fs::write(&path, updated).map_err(io(&path))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_text_goes_at_the_end_of_its_section() {
        let body = "# R\n\n## Organization\n\nFlat.\n\n## Examples\n\nx\n";
        let out = with_organization(body, "- **Birds**: birds");
        assert_eq!(
            out,
            "# R\n\n## Organization\n\nFlat.\n\n- **Birds**: birds\n\n## Examples\n\nx\n"
        );
        assert_eq!(with_organization(&out, "- **Birds**: birds"), out);
        let listed = "# R\n\n## Organization\n\nFlat.\n\n<!-- carrel:listing -->\n## Contents\n";
        assert_eq!(
            with_organization(listed, "- **A**: a"),
            "# R\n\n## Organization\n\nFlat.\n\n- **A**: a\n\n<!-- carrel:listing -->\n## Contents\n"
        );
        let bare = "# R\n\n<!-- carrel:listing -->\nlist\n";
        assert_eq!(
            with_organization(bare, "- **A**: a"),
            "# R\n\n## Organization\n\n- **A**: a\n\n<!-- carrel:listing -->\nlist\n"
        );
    }
}
