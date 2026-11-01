//! `carrel restructure propose|list|show|approve|reject`: whole-section restructure proposals
//! (`docs/design/restructure.md`). Carrel proposes a better shape for a collection and
//! everything in it, or for the whole archive, as a list of changes grouped by how invasive
//! they are, approved all, in part or not at all. `show`, `approve` and `reject` take any
//! waiting proposal, tag proposals (`carrel restructure tags`) too. The logic is the core's
//! ([`carrel_core::restructure`]); this file only presents it.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use carrel_core::backup::Backups;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::discover::Catalog;
use carrel_core::graph::Graph;
use carrel_core::providers::{Capability, Registry};
use carrel_core::restructure::eagerness::{self, Eagerness, Source};
use carrel_core::restructure::grouping::card_vectors;
use carrel_core::restructure::section::{Assessment, SectionOptions, propose_section};
use carrel_core::restructure::{self, Invasiveness, Proposal, ProposalKind};
use carrel_core::review::short_id;
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::restructure::{approved_text, describe_change};
use crate::{Failure, Report};

/// `carrel restructure propose [COLLECTION]`: builds a proposal for the collection and
/// everything under it, or the whole archive, from the meaning index and the tag and link
/// graph, and queues it in place of any waiting one for the same section. With `no_model`, or
/// without a text model on the device, the names are plain and the findability test is left
/// for a device with a model.
///
/// # Errors
///
/// [`Failure`] if the collection isn't one, or the archive can't be read or the proposal saved.
pub fn propose(
    archive: Option<&Path>,
    collection: Option<&str>,
    no_model: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let section = collection.unwrap_or("");
    let index = carrel_core::index::related::open(&archive);
    let vectors = match &index {
        Ok(index) => card_vectors(index, &Graph::build(&Catalog::read(&archive)?), ""),
        Err(_) => HashMap::new(),
    };
    let registry = (!no_model)
        .then(|| crate::providers::registry().0)
        .filter(|r: &Registry| !r.ranked(Capability::TextModel).is_empty());
    let options = SectionOptions::for_archive(&archive);
    let proposal = propose_section(&archive, section, &vectors, registry.as_ref(), &options)?;
    let note = index.as_ref().err().map(|why| {
        format!(
            "Without the meaning index, items are grouped by their tags and links alone: {}",
            why.trim_start_matches("Only its links are listed: ")
        )
    });
    let mut text = String::new();
    if let Some(note) = &note {
        writeln!(text, "{note}\n").unwrap();
    }
    if proposal.changes.is_empty() {
        writeln!(text, "{}", proposal.rationale).unwrap();
        if let Some(assessment) = &proposal.assessment {
            text.push_str(&judged(assessment));
            text.push_str(&held_back(assessment));
        }
        return Ok(Report::new(
            text,
            json!({"proposal": null, "rationale": proposal.rationale, "assessment": proposal.assessment, "note": note}),
        ));
    }
    let replaced = restructure::queue(&archive, &proposal)?;
    text.push_str(&describe(&proposal));
    if replaced > 0 {
        writeln!(
            text,
            "\nIt replaces the proposal for the same section that was waiting."
        )
        .unwrap();
    }
    text.push_str(&next_steps(&proposal));
    Ok(Report::new(
        text,
        json!({"proposal": proposal, "replaced": replaced, "note": note}),
    ))
}

/// `carrel restructure list`: the waiting proposals, oldest first.
///
/// # Errors
///
/// [`Failure`] if the queue can't be read.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let waiting = restructure::waiting(&archive, None)?;
    let mut text = String::new();
    if waiting.is_empty() {
        text.push_str(
            "No restructure proposals are waiting. Make one with `carrel restructure propose [COLLECTION]`.\n",
        );
    }
    for proposal in &waiting {
        writeln!(
            text,
            "{}  {}  {}  {}  {}",
            short_id(proposal.id),
            proposal.created.0.get(..10).unwrap_or(&proposal.created.0),
            what(proposal),
            count(proposal.changes.len(), "change"),
            proposal.invasiveness.describe()
        )
        .unwrap();
    }
    let rows: Vec<serde_json::Value> = waiting
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "kind": p.kind,
                "section": p.section,
                "created": p.created,
                "changes": p.changes.len(),
                "invasiveness": p.invasiveness,
                "rationale": p.rationale,
            })
        })
        .collect();
    Ok(Report::new(text, json!({"proposals": rows})))
}

/// `carrel restructure show [ID]`.
///
/// # Errors
///
/// [`Failure`] if there's no such proposal, or several and no `id`.
pub fn show(archive: Option<&Path>, id: Option<&str>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let proposal = restructure::find_any(&archive, id)?;
    let text = describe(&proposal) + &next_steps(&proposal);
    Ok(Report::new(text, json!({"proposal": proposal})))
}

/// `carrel restructure approve [ID] [--only N,…]`: applies all of the proposal, or the changes
/// `only` names, as one change in history, after a backup snapshot when it moves files and
/// backups are set up.
///
/// # Errors
///
/// [`Failure`] if there's no such proposal or change, another writer holds the archive, or a
/// file can't be written.
pub fn approve(
    archive: Option<&Path>,
    id: Option<&str>,
    only: &[String],
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let proposal = restructure::find_any(&archive, id)?;
    let identity = Identity::load_or_create()?;
    let only = (!only.is_empty()).then_some(only);
    let (backups, note) = if proposal.kind == ProposalKind::Section {
        crate::backup::for_batch(&archive)
    } else {
        (None, None)
    };
    let approved = restructure::approve_with_snapshot(
        &archive,
        &proposal,
        only,
        holder(&identity),
        backups.as_ref().map(|b| b as &dyn Backups),
    )?;
    let mut text = approved_text(&proposal, &approved);
    for outcome in &approved.outcomes {
        if let Some(why) = &outcome.not_applied {
            writeln!(text, "Change {} wasn't made: {why}.", outcome.change).unwrap();
        }
    }
    match (&approved.snapshot, &note) {
        (Some(snapshot), _) => writeln!(text, "Snapshot {snapshot} taken first.").unwrap(),
        (None, Some(note)) if approved.commit.is_some() => writeln!(text, "{note}").unwrap(),
        _ => {}
    }
    Ok(Report::new(
        text,
        json!({"proposal": proposal.id, "approved": approved, "note": note}),
    ))
}

/// `carrel restructure reject [ID]`: drops the proposal, changing nothing else.
///
/// # Errors
///
/// [`Failure`] if there's no such proposal or the queue can't be written.
pub fn reject(archive: Option<&Path>, id: Option<&str>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let proposal = restructure::find_any(&archive, id)?;
    restructure::reject(&archive, &proposal)?;
    Ok(Report::new(
        format!(
            "Rejected restructure proposal {}; nothing changed.\n",
            short_id(proposal.id)
        ),
        json!({"rejected": proposal.id}),
    ))
}

/// `carrel restructure eagerness [COLLECTION] [LEVEL]`: how eager Carrel is to rearrange,
/// archive-wide or at a collection, and the numbers that stand for it; with a level, sets the
/// collection's own in its design document (`default` clears it, so the archive's applies).
///
/// # Errors
///
/// [`Failure`] if the collection or level isn't one, or the design document can't be written.
pub fn eagerness(
    archive: Option<&Path>,
    collection: Option<&str>,
    level: Option<&str>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    if let (Some(collection), Some(level)) = (collection, level) {
        let wanted = if level.trim().eq_ignore_ascii_case("default") {
            None
        } else {
            Some(Eagerness::parse(level).ok_or_else(|| {
                Failure::new(format!(
                    "“{level}” isn't a level; use settled, balanced, eager or default"
                ))
            })?)
        };
        let identity = Identity::load_or_create()?;
        let lease = DiskLock::for_archive(&archive).acquire(
            holder(&identity),
            "restructure",
            DEFAULT_LEASE,
        )?;
        let result = eagerness::set_for_collection(&archive, collection, wanted, &lease);
        lease.release()?;
        let commit = result?;
        let archive = carrel_core::archive::Archive::open(archive.root())?;
        let mut report = eagerness_at(&archive, collection)?;
        report["commit"] = json!(commit);
        let text = if commit.is_some() {
            format!(
                "{collection} is now {}; undo it from History if you change your mind\n",
                wanted.map_or("as eager as the archive", Eagerness::as_str)
            )
        } else {
            format!("{collection} was already so\n")
        };
        return Ok(Report::new(text, report));
    }
    let report = eagerness_at(&archive, collection.unwrap_or(""))?;
    let t = &report["thresholds"];
    let mut text = format!(
        "{}: {} ({})\n",
        collection.unwrap_or("The archive"),
        report["eagerness"].as_str().unwrap_or_default(),
        report["from"].as_str().unwrap_or_default()
    );
    writeln!(
        text,
        "  margin {}, scaled by how invasive a change is; stability penalty {} for {} days; move budget {}",
        t["margin"],
        t["stability_penalty"],
        t["stability_days"],
        t["move_budget"].as_u64().map_or("none".to_owned(), |n| n.to_string())
    )
    .unwrap();
    for (path, level) in report["collections"].as_object().into_iter().flatten() {
        writeln!(
            text,
            "  {path} sets its own: {}",
            level.as_str().unwrap_or_default()
        )
        .unwrap();
    }
    text.push_str(
        "Set a collection's with `carrel restructure eagerness COLLECTION settled|balanced|eager|default`, \
         the archive's with `carrel settings set restructure.eagerness LEVEL`.\n",
    );
    Ok(Report::new(text, report))
}

/// The eagerness at `path`, where it comes from, its numbers, and every collection that sets
/// its own, as JSON.
fn eagerness_at(
    archive: &carrel_core::archive::Archive,
    path: &str,
) -> Result<serde_json::Value, Failure> {
    let collections = archive.collections()?;
    let path = path.trim().trim_matches('/');
    if !path.is_empty()
        && !collections
            .iter()
            .any(|c| c.path.eq_ignore_ascii_case(path))
    {
        return Err(Failure::new(format!("there's no collection {path}")));
    }
    let (level, source) = eagerness::at(archive, &collections, path);
    let from = match &source {
        Source::Archive => "the archive's setting".to_owned(),
        Source::Collection(c) => format!("set by {c}"),
    };
    let own: serde_json::Map<String, serde_json::Value> = collections
        .iter()
        .filter_map(|c| eagerness::of_collection(c).map(|l| (c.path.clone(), json!(l))))
        .collect();
    Ok(json!({
        "collection": path,
        "eagerness": level,
        "from": from,
        "source": source,
        "thresholds": eagerness::thresholds_at(archive, &collections, path),
        "collections": own,
    }))
}

fn what(proposal: &Proposal) -> String {
    match proposal.kind {
        ProposalKind::Tags => "tags".to_owned(),
        ProposalKind::Section if proposal.section.is_empty() => "the whole archive".to_owned(),
        ProposalKind::Section => proposal.section.clone(),
    }
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The proposal for people: what it covers, its rationale and how it was judged, then its
/// changes grouped by how invasive they are, least first, and what it held back.
fn describe(proposal: &Proposal) -> String {
    let mut text = format!(
        "Restructure proposal {} for {} · {}\n{}\n",
        short_id(proposal.id),
        what(proposal),
        proposal.invasiveness.describe(),
        proposal.rationale
    );
    if let Some(assessment) = &proposal.assessment {
        text.push_str(&judged(assessment));
    }
    for kind in Invasiveness::ALL {
        let changes: Vec<_> = proposal
            .changes
            .iter()
            .filter(|c| c.invasiveness == kind)
            .collect();
        if changes.is_empty() {
            continue;
        }
        let heading = kind.describe();
        let mut heading = heading.chars();
        let first = heading.next().map(|c| c.to_uppercase().to_string());
        writeln!(text, "\n{}{}", first.unwrap_or_default(), heading.as_str()).unwrap();
        for change in changes {
            text.push_str(&describe_change(change));
        }
    }
    if let Some(assessment) = &proposal.assessment {
        text.push_str(&held_back(assessment));
    }
    text
}

/// What the proposal left out, and why.
fn held_back(assessment: &Assessment) -> String {
    let mut text = String::new();
    if !assessment.held_back.is_empty() {
        text.push_str("\nHeld back\n");
        for held in &assessment.held_back {
            writeln!(text, "- {}: {}", held.what, held.why).unwrap();
        }
    }
    text
}

/// How a section proposal was judged, in a few lines.
fn judged(assessment: &Assessment) -> String {
    let from = match &assessment.eagerness_from {
        Source::Archive => "the archive's setting".to_owned(),
        Source::Collection(c) => format!("set by {c}"),
    };
    let mut text = format!(
        "Eagerness: {} ({from}) · benefit {:.2}, cost {:.2}\n",
        assessment.eagerness, assessment.benefit, assessment.cost
    );
    match (&assessment.findability, &assessment.untested) {
        (Some(f), _) => writeln!(
            text,
            "Findability: {} of {} held-out cards found now, {} with the new shape{}",
            f.found_now,
            f.tried,
            f.found_after,
            f.model
                .as_deref()
                .map(|m| format!(" ({m})"))
                .unwrap_or_default()
        )
        .unwrap(),
        (None, Some(why)) => writeln!(text, "Findability: untested; {why}").unwrap(),
        (None, None) => {}
    }
    if assessment.recently_restructured {
        text.push_str("Restructured recently, so changes here cost more.\n");
    }
    text
}

fn next_steps(proposal: &Proposal) -> String {
    let id = short_id(proposal.id);
    let example = if proposal.changes.len() > 1 {
        "1,2"
    } else {
        "1"
    };
    format!(
        "\nApprove it all with `carrel restructure approve {id}`, part of it with \
         `--only {example}` (changes that need one left out are dropped, and said so), or none \
         with `carrel restructure reject {id}`. Undo it as one change with `carrel undo`.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_grouped_by_how_invasive_they_are() {
        let mut proposal = Proposal::new(ProposalKind::Section, "Reading");
        proposal.rationale = "Tidy Reading.".into();
        proposal.changes = vec![
            serde_json::from_value(json!({
                "id": "1", "change": {"kind": "merge-collection", "from": "A", "into": "B"},
                "reason": "A is small.", "invasiveness": "top-level",
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "id": "2", "change": {"kind": "create-collection", "path": "Reading/Birds", "description": "Birds"},
                "reason": "A home.", "invasiveness": "subdividing",
            }))
            .unwrap(),
        ];
        let text = describe(&proposal);
        let subdividing = text.find("Subdividing a collection").unwrap();
        let top = text
            .find("Moving or merging top-level collections")
            .unwrap();
        assert!(subdividing < top, "{text}");
        assert!(text.contains("for Reading"));
    }
}
