//! `carrel restructure`: restructure proposals (`docs/design/restructure.md`). Tag
//! restructuring is the first kind: `carrel restructure tags` proposes merging synonym tags,
//! splitting overloaded ones and retiring rare ones, then shows, approves (all or part) or
//! rejects the proposal. The logic is the core's ([`carrel_core::restructure`]); this file
//! only presents it.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::card::Id;
use carrel_core::restructure::tags::{TagOptions, propose};
use carrel_core::restructure::{self, Approved, Proposal, ProposalKind, ProposedChange, Touched};
use carrel_core::review::short_id;
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// How many items a change lists by name before saying how many more.
const LISTED: usize = 5;

/// `carrel restructure tags propose`: builds a tag restructure proposal from the archive's
/// tags and its meaning index, and puts it in the queue in place of any earlier one.
///
/// # Errors
///
/// [`Failure`] if the archive can't be read or the proposal can't be saved.
pub fn propose_tags(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let index = carrel_core::index::related::open(&archive);
    let vectors = |id: Id| index.as_ref().ok().and_then(|i| i.card_vector(id));
    let proposal = propose(&archive, &vectors, &TagOptions::default())?;
    let without_index = index.as_ref().err().map(|why| {
        let why = why.trim_start_matches("Only its links are listed: ");
        format!(
            "Without the meaning index, only tags written differently and rare tags are \
             proposed: {why}"
        )
    });
    let mut text = String::new();
    if let Some(note) = &without_index {
        writeln!(text, "{note}\n").unwrap();
    }
    if proposal.changes.is_empty() {
        writeln!(text, "{}", proposal.rationale).unwrap();
        return Ok(Report::new(
            text,
            json!({"proposal": null, "rationale": proposal.rationale, "note": without_index}),
        ));
    }
    let replaced = restructure::queue(&archive, &proposal)?;
    text.push_str(&describe(&proposal));
    if replaced > 0 {
        writeln!(text, "\nIt replaces the tag proposal that was waiting.").unwrap();
    }
    text.push_str(&next_steps(&proposal));
    Ok(Report::new(
        text,
        json!({"proposal": proposal, "replaced": replaced, "note": without_index}),
    ))
}

/// `carrel restructure tags show [ID]`.
///
/// # Errors
///
/// [`Failure`] if there's no such proposal.
pub fn show_tags(archive: Option<&Path>, id: Option<&str>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let proposal = restructure::find(&archive, ProposalKind::Tags, id)?;
    let text = describe(&proposal) + &next_steps(&proposal);
    Ok(Report::new(text, json!({"proposal": proposal})))
}

/// `carrel restructure tags approve [ID] [--only N,…]`: applies all of the proposal, or the
/// changes `only` names, as one change in history.
///
/// # Errors
///
/// [`Failure`] if there's no such proposal or change, another writer holds the archive, or a
/// file can't be written.
pub fn approve_tags(
    archive: Option<&Path>,
    id: Option<&str>,
    only: &[String],
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let proposal = restructure::find(&archive, ProposalKind::Tags, id)?;
    let identity = Identity::load_or_create()?;
    let only = (!only.is_empty()).then_some(only);
    let approved = restructure::approve(&archive, &proposal, only, holder(&identity))?;
    let text = approved_text(&proposal, &approved);
    Ok(Report::new(
        text,
        json!({"proposal": proposal.id, "approved": approved}),
    ))
}

/// `carrel restructure tags reject [ID]`: drops the proposal, changing nothing else.
///
/// # Errors
///
/// [`Failure`] if there's no such proposal or the queue can't be written.
pub fn reject_tags(archive: Option<&Path>, id: Option<&str>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let proposal = restructure::find(&archive, ProposalKind::Tags, id)?;
    restructure::reject(&archive, &proposal)?;
    Ok(Report::new(
        format!(
            "Rejected tag proposal {}; nothing changed.\n",
            short_id(proposal.id)
        ),
        json!({"rejected": proposal.id}),
    ))
}

/// The proposal for people: its rationale, then each change with why and what it touches.
fn describe(proposal: &Proposal) -> String {
    let mut text = format!(
        "Tag proposal {} · {}\n{}\n",
        short_id(proposal.id),
        proposal.invasiveness.describe(),
        proposal.rationale
    );
    for change in &proposal.changes {
        text.push('\n');
        text.push_str(&describe_change(change));
    }
    text
}

fn describe_change(change: &ProposedChange) -> String {
    let mut text = format!(
        "{}. {}\n   {}\n",
        change.id,
        change.change.describe(),
        change.reason
    );
    if !change.items.is_empty() {
        writeln!(
            text,
            "   Changes {}: {}",
            count(change.items.len(), "item"),
            names(&change.items)
        )
        .unwrap();
    }
    for held in change.held.iter().take(LISTED) {
        writeln!(text, "   Leaves {} alone: {}", held.title, held.why).unwrap();
    }
    if change.held.len() > LISTED {
        writeln!(
            text,
            "   And leaves {} more alone",
            change.held.len() - LISTED
        )
        .unwrap();
    }
    if !change.documents.is_empty() {
        writeln!(text, "   Rewrites {}", change.documents.join(", ")).unwrap();
    }
    if !change.depends_on.is_empty() {
        writeln!(text, "   Needs {}", change.depends_on.join(", ")).unwrap();
    }
    text
}

fn names(items: &[Touched]) -> String {
    let mut listed: Vec<&str> = items
        .iter()
        .take(LISTED)
        .map(|t| t.title.as_str())
        .collect();
    let more = items.len().saturating_sub(LISTED);
    let more = format!("{more} more");
    if items.len() > LISTED {
        listed.push(&more);
    }
    listed.join(", ")
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn next_steps(proposal: &Proposal) -> String {
    let id = short_id(proposal.id);
    let example = if proposal.changes.len() > 1 {
        "1,2"
    } else {
        "1"
    };
    format!(
        "\nApprove it all with `carrel restructure tags approve {id}`, part of it with \
         `--only {example}`, or none with `carrel restructure tags reject {id}`.\n"
    )
}

fn approved_text(proposal: &Proposal, approved: &Approved) -> String {
    let mut text = if approved.commit.is_some() {
        format!("{}.\n", approved.summary)
    } else {
        "Nothing needed changing after all.\n".to_owned()
    };
    let titles: std::collections::BTreeMap<Id, &str> = proposal
        .changes
        .iter()
        .flat_map(|c| c.items.iter().map(|t| (t.id, t.title.as_str())))
        .collect();
    for outcome in &approved.outcomes {
        let Some(change) = proposal.change(&outcome.change) else {
            continue;
        };
        writeln!(
            text,
            "{}. {}: {}",
            change.id,
            change.change.describe(),
            count(outcome.items.len(), "item")
        )
        .unwrap();
        for (id, why) in &outcome.skipped {
            let title = titles.get(id).copied().unwrap_or("an item");
            writeln!(text, "   Left {title} alone: {why}").unwrap();
        }
        for tag in &outcome.kept {
            writeln!(
                text,
                "   Kept “{tag}” in the vocabulary, since items left alone still use it"
            )
            .unwrap();
        }
    }
    if !approved.selection.rejected.is_empty() {
        writeln!(
            text,
            "Left out: {}.",
            approved.selection.rejected.join(", ")
        )
        .unwrap();
    }
    for (id, missing) in &approved.selection.dropped {
        writeln!(
            text,
            "Dropped {id}, since it needs {missing}, which was left out."
        )
        .unwrap();
    }
    if approved.commit.is_some() {
        text.push_str("Undo it as one change from History or with `carrel undo`.\n");
    }
    text
}
