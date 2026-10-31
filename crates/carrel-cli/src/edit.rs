//! `carrel edit ITEM`: a filed card in the owner's editor, checked as proposals are, with
//! the body kept exactly, as one undoable change (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::actions::{self, EditedCard};
use carrel_core::card::Sensitivity;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::history::{Change, History, Operation};
use serde_json::json;

use crate::commands::{find_item, holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// `carrel edit ITEM`.
///
/// # Errors
///
/// [`Failure`] if the item isn't found, the editor can't run, or the card can't be written.
pub fn edit(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?.clone();
    let card_path = archive.root().join(&item.card);
    let mut file = CardFile::parse(&std::fs::read_to_string(&card_path)?)?;
    if file.card().checked_out.is_some() {
        return Err(Failure::new(
            "it's checked out; edit the card when it's returned, so the return can't overwrite it",
        ));
    }
    let collection = item.card.rsplit_once('/').map_or("", |(f, _)| f).to_owned();
    let floor = floor(&archive, &item);
    let temp = std::env::temp_dir().join(format!("carrel-card-{}.yaml", item.id));
    let mut card = file.card().clone();
    let mut text = actions::card_text(&archive, &card, &collection, floor, &[])?;
    let changed = loop {
        std::fs::write(&temp, &text).map_err(|e| Failure::io(&temp, &e))?;
        crate::review::run_editor(&temp)?;
        let edited = std::fs::read_to_string(&temp).map_err(|e| Failure::io(&temp, &e))?;
        if edited == text {
            break None;
        }
        match actions::apply_card(&archive, &mut card, &collection, floor, &edited)? {
            EditedCard::Unchanged => break None,
            EditedCard::Changed(changed) => break Some(changed),
            EditedCard::Problems(problems) => {
                // The owner's edits stay; the problems go on top, replacing earlier ones.
                let mut again = String::new();
                for problem in &problems {
                    writeln!(again, "# To fix: {problem}").unwrap();
                }
                again.push_str("#\n");
                for line in edited
                    .lines()
                    .skip_while(|l| l.starts_with("# To fix: ") || *l == "#")
                {
                    again.push_str(line);
                    again.push('\n');
                }
                text = again;
            }
        }
    };
    let _ = std::fs::remove_file(&temp);
    let Some(changed) = changed else {
        return Ok(Report::new(
            "Nothing changed\n",
            json!({"item": item.id, "changed": [], "commit": null}),
        ));
    };
    *file.card_mut() = card;
    let summary = format!("Edit “{}”: {}", file.card().title, changed.join(", "));
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "edit", DEFAULT_LEASE)?;
    let result = (|| -> Result<Option<String>, Failure> {
        let written = card_path.with_extension("md.tmp-edit");
        std::fs::write(&written, file.to_text()?)?;
        std::fs::rename(&written, &card_path)?;
        let mut change = Change::new(
            Operation::Edit,
            summary.clone(),
            lease.info().person,
            lease.info().device,
        );
        change.items = vec![item.id];
        Ok(History::open(&archive)?.commit(&archive, &change, &lease)?)
    })();
    lease.release()?;
    let commit = result?;
    Ok(Report::new(
        format!("{summary}; undo it from History or with `carrel undo`.\n"),
        json!({"item": item.id, "changed": changed, "commit": commit}),
    ))
}

/// The lowest sensitivity the owner may give `item`: sensitive when a rule finds a restricted
/// value in it (a card number, a key, recovery codes), otherwise public.
fn floor(
    archive: &carrel_core::archive::Archive,
    item: &carrel_core::archive::ItemRef,
) -> Sensitivity {
    use carrel_core::ingest::sensitivity::{floor, restricted_by_rule};
    let Some(contents) = item.item.as_ref().map(|p| archive.root().join(p)) else {
        return Sensitivity::Public;
    };
    let (registry, _) = crate::providers::registry();
    let text = crate::correct::item_text(archive, &registry, &contents, item.kind);
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
