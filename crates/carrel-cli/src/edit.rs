//! `carrel edit ITEM`: a filed card in the owner's editor, checked as proposals are, with
//! the body kept exactly, as one undoable change (`docs/design/checkout.md`). With `--body`,
//! the body instead: its notes and anything below the fields, Carrel's sections kept.

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

/// The line `carrel edit --body` puts above the body for each problem to fix, as an HTML
/// comment so Markdown editors leave it alone.
const BODY_PROBLEM: &str = "<!-- To fix: ";

/// The problem shown when the body was saved another way, on the same lines, while it was
/// being edited.
const BODY_CHANGED: &str = "the notes were saved another way while you edited them, so both versions are below; tidy them and save, or save without changing anything to keep the card as it is now";

/// `carrel edit ITEM --body`: the card's body in the owner's editor, saved as one change
/// undo can reverse ([`carrel_core::edit::save_body`]). Carrel's own sections, such as the
/// history, have to stay as they are; when they don't, the editor opens again saying so.
/// What was saved another way while the editor was open is kept beside the edit, and when the
/// two changed the same lines, the editor opens again with both.
///
/// # Errors
///
/// [`Failure`] if the item isn't found or is checked out, the editor can't run, or the card
/// can't be written.
pub fn edit_body(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?.clone();
    let (registry, _) = crate::providers::registry();
    let identity = Identity::load_or_create()?;
    // The process's own, so two edits of one item at once don't share a file.
    let temp =
        std::env::temp_dir().join(format!("carrel-body-{}-{}.md", item.id, std::process::id()));
    // `base` is the body the editor last opened, which the save is checked against; `text` is
    // what it opened, with any problems on top.
    let mut base = carrel_core::edit::open(&archive, &registry, &item)?
        .file
        .body()
        .to_owned();
    let mut text = base.clone();
    let saved = loop {
        std::fs::write(&temp, &text).map_err(|e| Failure::io(&temp, &e))?;
        crate::review::run_editor(&temp)?;
        let edited = std::fs::read_to_string(&temp).map_err(|e| Failure::io(&temp, &e))?;
        if edited == text {
            // Saved as it was opened, problems and all: the owner gave up, so nothing changes.
            break carrel_core::edit::Saved::Unchanged;
        }
        let body = without_body_problems(&edited);
        let editing = carrel_core::edit::open(&archive, &registry, &item)?;
        match carrel_core::edit::save_body(&archive, editing, &base, &body, holder(&identity))? {
            carrel_core::edit::Saved::Problems(problems) => {
                // The owner's edits stay; the problems go on top, replacing earlier ones.
                text = with_body_problems(&problems, &body);
            }
            carrel_core::edit::Saved::BodyChanged { current, both } => {
                // Saved another way meanwhile: both versions, to tidy and save against the
                // body as it is now, or to leave as they are, keeping the card's.
                text = with_body_problems(&[BODY_CHANGED.to_owned()], &both);
                base = current;
            }
            outcome => break outcome,
        }
    };
    let _ = std::fs::remove_file(&temp);
    match saved {
        carrel_core::edit::Saved::Changed {
            summary, commit, ..
        } => Ok(Report::new(
            format!("{summary}; undo it from History or with `carrel undo`.\n"),
            json!({"item": item.id, "changed": ["notes"], "commit": commit}),
        )),
        _ => Ok(Report::new(
            "Nothing changed\n",
            json!({"item": item.id, "changed": [], "commit": null}),
        )),
    }
}

/// `body` with a line on top for each of `problems`, as `carrel edit --body` shows them.
fn with_body_problems(problems: &[String], body: &str) -> String {
    let mut text = String::new();
    for problem in problems {
        writeln!(text, "{BODY_PROBLEM}{problem} -->").unwrap();
    }
    text.push_str(body);
    text
}

/// `edited` without the problem lines `carrel edit --body` put above it.
fn without_body_problems(edited: &str) -> String {
    let mut rest = edited;
    while rest.starts_with(BODY_PROBLEM) {
        rest = rest.split_once('\n').map_or("", |(_, after)| after);
    }
    rest.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_problems_above_a_body_are_taken_off_again() {
        let body = "Notes.\n\n<!-- carrel:history -->\n## History\n<!-- /carrel:history -->\n";
        let marked =
            format!("{BODY_PROBLEM}keep the history -->\n{BODY_PROBLEM}and this -->\n{body}");
        assert_eq!(without_body_problems(&marked), body);
        assert_eq!(without_body_problems(body), body);
    }
}
