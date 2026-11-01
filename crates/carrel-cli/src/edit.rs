//! `carrel edit ITEM`: a filed card's fields in the owner's editor, every one the owner may
//! set, checked as the core checks them, with the body kept exactly, as one undoable change
//! (`docs/design/checkout.md`). With `--body`, the body instead: its notes and anything below
//! the fields, Carrel's sections kept.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::actions;
use carrel_core::edit::Saved;
use serde_json::json;

use crate::commands::{find_item, holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// `carrel edit ITEM [--body]`: [`edit`] or [`edit_body`], then, when the card is flagged as
/// needing work, the offer to clear its flag. The interactive mode offers it its own way.
///
/// # Errors
///
/// As [`edit`] and [`edit_body`], or if the flag can't be cleared.
pub fn edit_command(archive: Option<&Path>, query: &str, body: bool) -> Result<Report, Failure> {
    let mut report = if body {
        edit_body(archive, query)?
    } else {
        edit(archive, query)?
    };
    let id = report.json["item"]
        .as_str()
        .and_then(|id| uuid::Uuid::parse_str(id).ok());
    if let Some(id) = id
        && report.json["commit"].is_string()
    {
        crate::flag::offer_to_clear(&locate(archive)?, carrel_core::card::Id(id), &mut report)?;
    }
    Ok(report)
}

/// `carrel edit ITEM`: the card's fields in the owner's editor, saved against the fields it
/// opened ([`carrel_core::edit::save`]), so a field saved another way meanwhile isn't
/// overwritten. When one the owner changed was, the editor opens again saying so.
///
/// # Errors
///
/// [`Failure`] if the item isn't found, the editor can't run, or the card can't be written.
pub fn edit(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?.clone();
    let (registry, _) = crate::providers::registry();
    let identity = Identity::load_or_create()?;
    let editing = carrel_core::edit::open(&archive, &registry, &item)?;
    // `base` is the fields the editor last opened, which the save is checked against.
    let mut base = editing.fields();
    let mut text = actions::card_text(
        &archive,
        editing.file.card(),
        &editing.collection,
        editing.floor,
        &[],
    )?;
    let temp = std::env::temp_dir().join(format!(
        "carrel-card-{}-{}.yaml",
        item.id,
        std::process::id()
    ));
    let saved = loop {
        std::fs::write(&temp, &text).map_err(|e| Failure::io(&temp, &e))?;
        crate::review::run_editor(&temp)?;
        let edited = std::fs::read_to_string(&temp).map_err(|e| Failure::io(&temp, &e))?;
        if edited == text {
            break Saved::Unchanged;
        }
        let fields = match actions::card_fields_from_text(&edited) {
            Ok(fields) => fields,
            Err(problem) => {
                text = with_field_problems(&[problem], &edited);
                continue;
            }
        };
        let editing = carrel_core::edit::open(&archive, &registry, &item)?;
        match carrel_core::edit::save(&archive, editing, &base, fields, holder(&identity))? {
            Saved::Problems(problems) => text = with_field_problems(&problems, &edited),
            Saved::FieldsChanged { fields, current } => {
                // Saved another way meanwhile: saving again keeps the owner's.
                text = with_field_problems(&[fields_changed(&fields, &current)], &edited);
                base = *current;
            }
            outcome => break outcome,
        }
    };
    let _ = std::fs::remove_file(&temp);
    match saved {
        Saved::Changed {
            fields,
            summary,
            commit,
        } => Ok(Report::new(
            format!("{summary}; undo it from History or with `carrel undo`.\n"),
            json!({"item": item.id, "changed": fields, "commit": commit}),
        )),
        _ => Ok(Report::new(
            "Nothing changed\n",
            json!({"item": item.id, "changed": [], "commit": null}),
        )),
    }
}

/// `edited` with a line on top for each of `problems`, replacing the problems put there
/// before; the owner's edits stay.
fn with_field_problems(problems: &[String], edited: &str) -> String {
    let mut again = String::new();
    for problem in problems {
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
    again
}

/// The problem shown when `fields` were saved another way while they were edited: what each
/// is now, as `current` has it.
fn fields_changed(fields: &[&str], current: &actions::CardFields) -> String {
    let now = serde_json::to_value(current).unwrap_or_default();
    let values: Vec<String> = fields
        .iter()
        .map(|f| {
            let value = now
                .get(f)
                .map_or_else(|| "nothing".to_owned(), ToString::to_string);
            format!("{f} is now {value}")
        })
        .collect();
    format!(
        "{} saved another way while you edited: {}. To keep yours, delete this line and save; to keep the card as it is now, save without changing anything",
        if fields.len() == 1 {
            "one field was"
        } else {
            "fields were"
        },
        values.join(", ")
    )
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
            break Saved::Unchanged;
        }
        let body = without_body_problems(&edited);
        let editing = carrel_core::edit::open(&archive, &registry, &item)?;
        match carrel_core::edit::save_body(&archive, editing, &base, &body, holder(&identity))? {
            Saved::Problems(problems) => {
                // The owner's edits stay; the problems go on top, replacing earlier ones.
                text = with_body_problems(&problems, &body);
            }
            Saved::BodyChanged { current, both } => {
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
        Saved::Changed {
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
