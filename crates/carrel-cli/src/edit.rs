//! `carrel edit ITEM`: a filed card in the owner's editor, checked as proposals are, with
//! the body kept exactly, as one undoable change (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::actions::{self, EditedCard};
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
    let (registry, _) = crate::providers::registry();
    let editing = carrel_core::edit::open(&archive, &registry, &item)?;
    let (collection, floor) = (editing.collection.clone(), editing.floor);
    let temp = std::env::temp_dir().join(format!("carrel-card-{}.yaml", item.id));
    let mut card = editing.file.card().clone();
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
    let mut file = editing.file;
    *file.card_mut() = card;
    let summary = format!("Edit “{}”: {}", file.card().title, changed.join(", "));
    let identity = Identity::load_or_create()?;
    let commit = carrel_core::edit::write(&archive, &item, &file, &summary, holder(&identity))?;
    Ok(Report::new(
        format!("{summary}; undo it from History or with `carrel undo`.\n"),
        json!({"item": item.id, "changed": changed, "commit": commit}),
    ))
}
