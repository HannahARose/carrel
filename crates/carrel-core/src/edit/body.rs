//! Saving the owner's edit of a card's body against the body their editor opened, so a body
//! saved another way meanwhile isn't overwritten, and a managed section Carrel changed
//! meanwhile, such as a new history entry, isn't taken for the owner's (#393).
//!
//! The three bodies, as opened, as the card has it now and as the owner edited it, are merged
//! line by line, as Git merges a file. Each managed section's content is first replaced by
//! one line naming it, so the merge never looks inside a section and always takes the card's
//! current one.

use super::{Editing, Saved, write};
use crate::actions::ActionError;
use crate::archive::Archive;
use crate::coordinator::Holder;
use crate::sections::{self, SectionError};

/// What a section's content is replaced by while the bodies are merged.
const HELD: &str = "<!-- carrel-held-section:";

/// Saves `body`, the owner's edit of `base`, the body their editor opened, as the card's body,
/// as one history change as `holder`. Carrel's managed sections (FORMAT.md §5.3), such as the
/// history, must be as `base` had them: they're changed through their own actions, such as
/// deciding pending entries on Active. The card as `editing` read it is merged with the edit:
///
/// - what the owner changed since `base` is kept, and so is what was saved another way
///   meanwhile, when the two changed different lines;
/// - the managed sections are the card's, as they are now;
/// - when both changed the same lines, nothing is written: [`Saved::BodyChanged`] has the
///   body as it is now and both versions together, for the owner to choose.
///
/// # Errors
///
/// [`ActionError::Lease`] if another writer holds the archive, or an error writing the card
/// or the history. A body that changes a managed section isn't an error: it comes back as
/// [`Saved::Problems`].
pub fn save_body(
    archive: &Archive,
    editing: Editing,
    base: &str,
    body: &str,
    holder: Holder,
) -> Result<Saved, ActionError> {
    let Editing { item, mut file, .. } = editing;
    if body == base {
        return Ok(Saved::Unchanged);
    }
    let opened = managed(base).map_err(|e| ActionError::Invalid(format!("{}: {e}", item.card)))?;
    match managed(body) {
        Err(e) => return Ok(Saved::Problems(vec![format!("the body's sections: {e}")])),
        Ok(now) if now != opened => {
            return Ok(Saved::Problems(vec![
                "Carrel's own sections, between <!-- carrel:… --> markers, have to stay as they are; change the history's entries on Active"
                    .to_owned(),
            ]));
        }
        Ok(_) => {}
    }
    let current = file.body().to_owned();
    let merged = match merge_body(base, &current, body)
        .map_err(|e| ActionError::Invalid(format!("{}: {e}", item.card)))?
    {
        BodyMerge::Merged(merged) => merged,
        BodyMerge::Clash { both } => return Ok(Saved::BodyChanged { current, both }),
    };
    if merged == current {
        return Ok(Saved::Unchanged);
    }
    file.set_body(&merged);
    let summary = format!("Edit “{}”: notes", file.card().title);
    let commit = write(archive, &item, &file, &summary, holder)?;
    Ok(Saved::Changed {
        fields: vec!["notes"],
        summary,
        commit,
    })
}

/// How the owner's edit and the card's body as it is now come together.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BodyMerge {
    /// They changed different lines, or only one changed: the body to save.
    Merged(String),
    /// They changed the same lines: both versions together, each changed line from each.
    Clash {
        /// The body with both, the card's managed sections as they are now.
        both: String,
    },
}

/// The managed sections of `text`, by name and content, in order.
fn managed(text: &str) -> Result<Vec<(String, String)>, SectionError> {
    Ok(sections::find_all(text)?
        .into_iter()
        .map(|s| (s.name, text[s.content].to_owned()))
        .collect())
}

/// Merges `mine`, an edit of `base`, with `current`, the body as it is now, line by line.
/// The managed sections are `current`'s, whatever `base` and `mine` have in them.
pub(crate) fn merge_body(base: &str, current: &str, mine: &str) -> Result<BodyMerge, SectionError> {
    if current == base {
        return Ok(BodyMerge::Merged(mine.to_owned()));
    }
    let contents = managed(current)?;
    // git2's merge_file doesn't start libgit2 itself, as opening a repository does; making a
    // configuration does, once.
    let _ = git2::Config::new();
    let (base, theirs, ours) = (held(base)?, held(current)?, held(mine)?);
    // `union` keeps each changed line from both sides where they clash, rather than giving up.
    let merge = |union: bool| -> Option<String> {
        let mut options = git2::MergeFileOptions::new();
        if union {
            options.favor(git2::FileFavor::Union);
        }
        let result = git2::merge_file(
            &input(&base),
            &input(&ours),
            &input(&theirs),
            Some(&mut options),
        )
        .ok()?;
        (union || result.is_automergeable())
            .then(|| String::from_utf8_lossy(result.content()).into_owned())
            .map(|text| restored(&text, &contents))
            // A merge that left a section out, or in twice, isn't one to save.
            .filter(|text| managed(text).is_ok_and(|now| now == contents))
    };
    if let Some(merged) = merge(false) {
        return Ok(BodyMerge::Merged(merged));
    }
    let both = merge(true).unwrap_or_else(|| {
        // The lines didn't come together: the card as it is, then the owner's own text.
        let mut both = current.trim_end().to_owned();
        both.push_str("\n\n");
        both.push_str(without_sections(mine).trim());
        both.push('\n');
        both
    });
    Ok(BodyMerge::Clash { both })
}

/// `text` as one side of a merge.
fn input(text: &str) -> git2::MergeFileInput<'_> {
    let mut input = git2::MergeFileInput::new();
    input.content(text.as_bytes());
    input
}

/// `text` with each managed section's content replaced by one line naming it.
fn held(text: &str) -> Result<String, SectionError> {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for section in sections::find_all(text)? {
        out.push_str(&text[at..section.content.start]);
        out.push_str(HELD);
        out.push_str(&section.name);
        out.push_str(" -->\n");
        at = section.content.end;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

/// `text` with each held line replaced by the content `contents` gives its section.
fn restored(text: &str, contents: &[(String, String)]) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            line.strip_prefix(HELD)
                .and_then(|rest| rest.trim_end().strip_suffix(" -->"))
                .map_or(line, |name| {
                    contents
                        .iter()
                        .find(|(n, _)| n == name)
                        .map_or("", |(_, content)| content.as_str())
                })
        })
        .collect()
}

/// `text` without its managed sections, markers and all.
fn without_sections(text: &str) -> String {
    let Ok(found) = sections::find_all(text) else {
        return text.to_owned();
    };
    let mut out = String::new();
    let mut at = 0;
    for section in found {
        out.push_str(&text[at..section.whole.start]);
        at = section.whole.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HISTORY: &str =
        "<!-- carrel:history -->\n## History\n\n- 2026-10-01: filed\n<!-- /carrel:history -->\n";

    fn body(notes: &str, history: &str) -> String {
        format!("{notes}\n{history}")
    }

    #[test]
    fn an_edit_of_an_unchanged_body_is_saved_as_it_is() {
        let base = body("Bought at the market.\n", HISTORY);
        let mine = body("Bought at the market, Saturday.\n", HISTORY);
        assert_eq!(merge_body(&base, &base, &mine), Ok(BodyMerge::Merged(mine)));
    }

    #[test]
    fn notes_saved_meanwhile_are_kept_beside_the_owners() {
        let base = body("First line.\n\nLast line.\n", HISTORY);
        let current = body("Their note.\n\nFirst line.\n\nLast line.\n", HISTORY);
        let mine = body("First line.\n\nLast line.\n\nMy note.\n", HISTORY);
        let BodyMerge::Merged(merged) = merge_body(&base, &current, &mine).unwrap() else {
            panic!("expected a clean merge");
        };
        assert!(
            merged.contains("Their note.") && merged.contains("My note."),
            "{merged}"
        );
    }

    #[test]
    fn a_history_entry_added_meanwhile_is_kept() {
        let newer = HISTORY.replace(
            "- 2026-10-01: filed\n",
            "- 2026-10-01: filed\n- 2026-10-20: returned\n",
        );
        let base = body("Notes.\n", HISTORY);
        let current = body("Notes.\n", &newer);
        let mine = body("Notes, and more.\n", HISTORY);
        assert_eq!(
            merge_body(&base, &current, &mine),
            Ok(BodyMerge::Merged(body("Notes, and more.\n", &newer)))
        );
    }

    #[test]
    fn the_same_line_changed_both_ways_keeps_both_for_the_owner() {
        let base = body("The lamp needs a strip.\n", HISTORY);
        let current = body("The lamp needs a warm strip.\n", HISTORY);
        let mine = body("The lamp needs a 2 m strip.\n", HISTORY);
        let BodyMerge::Clash { both } = merge_body(&base, &current, &mine).unwrap() else {
            panic!("expected a clash");
        };
        assert!(
            both.contains("warm strip") && both.contains("2 m strip"),
            "{both}"
        );
        assert_eq!(managed(&both).unwrap(), managed(&current).unwrap());
        assert!(!both.contains(HELD));
    }

    #[test]
    fn sections_are_taken_out_whole() {
        assert_eq!(without_sections(&body("Notes.\n", HISTORY)), "Notes.\n\n");
    }
}
