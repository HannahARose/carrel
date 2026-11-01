//! `carrel ingest`: proposing to file new files and folders (`docs/design/ingest.md`). Each
//! new item is examined, its text read, its card drafted and its destination chosen, and the
//! result saved as a review proposal. Nothing is moved until a proposal is approved.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::{Id, Kind};
use carrel_core::inbox;
use carrel_core::ingest::card::Draft;
use carrel_core::ingest::filing::Outcome;
use carrel_core::ingest::route::Route;
use carrel_core::ingest::run::{Run, name};
use carrel_core::ingest::sensitivity::Assessment;
use carrel_core::ingest::text::{ItemText, Unread};
use carrel_core::ingest::{Candidate, IntakeOptions};
use carrel_core::intake::{self, Queued};
use carrel_core::questions::QuestionEntry;
use carrel_core::review::short_id;
use serde_json::{Value, json};

use crate::commands::locate;
use crate::{Failure, Report};

/// What `carrel ingest` was asked to do besides adding paths.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// How to read folder arguments.
    pub intake: IntakeOptions,
    /// Show the plan only: queue nothing and run no model.
    pub dry_run: bool,
    /// Stay until the items are drafted, and report them.
    pub wait: bool,
    /// The owner's note about the items, for their drafts and cards.
    pub note: Option<String>,
}

/// Everything `carrel ingest` was given.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// The files and folders to add.
    pub paths: Vec<PathBuf>,
    /// A web address to add instead, with its title.
    pub url: Option<(String, Option<String>)>,
    /// How to add them.
    pub options: Options,
    /// Show the queue instead.
    pub status: bool,
    /// A change to one queued item instead, and its identifier.
    pub change: Option<(Change, String)>,
}

/// `carrel ingest`, whichever form it takes.
///
/// # Errors
///
/// As each form.
pub fn command(archive: Option<&Path>, request: &Request, json: bool) -> Result<Report, Failure> {
    if request.status {
        queue_status(archive)
    } else if let Some((change, id)) = &request.change {
        change_queued(archive, id, *change)
    } else if let Some((url, title)) = &request.url {
        ingest_url(archive, url, title.as_deref(), &request.options, json)
    } else {
        ingest(archive, &request.paths, &request.options, json)
    }
}

/// `carrel ingest PATH...`: adds the items to the intake queue and starts a worker to draft
/// them in the background (`docs/design/tui-ingest.md`). With `--dry-run`, shows what would be
/// added, at once; with `--wait`, drafts them before returning and reports each.
///
/// # Errors
///
/// [`Failure`] if the paths can't be planned or the queue can't be written.
pub fn ingest(
    archive: Option<&Path>,
    paths: &[PathBuf],
    options: &Options,
    json: bool,
) -> Result<Report, Failure> {
    if paths.is_empty() {
        return Err(Failure::new(
            "give the files and folders to ingest, or --status to see the queue",
        ));
    }
    let archive = locate(archive)?;
    let identity = crate::config::Identity::load_or_create()?;
    if options.dry_run {
        let planned = intake::preview(&archive, paths, options.intake, identity.device)?;
        return Ok(plan_report(
            &planned,
            "Nothing has been queued (--dry-run).",
        ));
    }
    if let Some(writer) = inbox::writer_elsewhere(&archive, identity.device) {
        let sent = inbox::send_paths(
            &archive,
            crate::commands::holder(&identity),
            paths,
            options.intake.each,
            options.note.as_deref(),
        )?;
        return Ok(inbox_report(&sent, &writer, paths.len()));
    }
    let queued = intake::add_with_note(
        &archive,
        paths,
        options.intake,
        identity.device,
        options.note.as_deref(),
    )?;
    if options.wait {
        return wait(&archive, &queued, json);
    }
    let started = crate::worker::start(&archive)?;
    let note = if queued.added.is_empty() {
        "Nothing new to queue.".to_owned()
    } else {
        queued_note(&archive, queued.added.len(), started)?
    };
    let mut report = plan_report(&queued, &note);
    report.json["worker_started"] = json!(started);
    Ok(report)
}

/// Says that `count` items were queued, and who's drafting them.
fn queued_note(archive: &Archive, count: usize, started: bool) -> Result<String, Failure> {
    Ok(format!(
        "Queued {}. {} drafting {} in the background; follow it with `carrel ingest --status`, and review proposals in `carrel` as they arrive.",
        plural(count, "item"),
        if started {
            "A worker is"
        } else if crate::worker::running(archive)? {
            "The running worker is"
        } else if crate::worker::agent(archive) {
            "The Carrel agent is"
        } else {
            "No worker is started (CARREL_WORKER is off); run `carrel worker` to start"
        },
        if count == 1 { "it" } else { "them" }
    ))
}

/// `carrel ingest --url URL`: queues a web address to be filed as a `url` item, drafted from
/// the address and `title` with nothing fetched, or puts it in this device's inbox while
/// another device is the writer.
///
/// # Errors
///
/// [`Failure`] if it isn't a web address or the queue can't be written.
pub fn ingest_url(
    archive: Option<&Path>,
    url: &str,
    title: Option<&str>,
    options: &Options,
    json: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let identity = crate::config::Identity::load_or_create()?;
    let note = options.note.as_deref();
    if let Some(writer) = inbox::writer_elsewhere(&archive, identity.device) {
        let sent = inbox::send_url(
            &archive,
            crate::commands::holder(&identity),
            url,
            title,
            note,
        )?;
        return Ok(inbox_report(&sent, &writer, 1));
    }
    let Some(entry) = intake::add_url(&archive, url, title, note, identity.device)? else {
        return Ok(Report::new(
            format!("{url} is already queued or waiting for review.\n"),
            json!({"saved": false, "queued": [], "skipped": [{"path": url, "reason": "already queued or waiting for review"}]}),
        ));
    };
    let queued = Queued {
        added: vec![entry],
        candidates: Vec::new(),
        skipped: Vec::new(),
    };
    if options.wait {
        return wait(&archive, &queued, json);
    }
    let started = crate::worker::start(&archive)?;
    let entry = &queued.added[0];
    let mut text = format!("{url}, to be filed as a web address\n");
    writeln!(text, "\n{}", queued_note(&archive, 1, started)?).unwrap();
    Ok(Report::new(
        text,
        json!({
            "saved": false,
            "queued": [{"id": entry.id, "path": entry.path, "url": entry.url}],
            "worker_started": started,
        }),
    ))
}

/// Says that `count` items went into this device's inbox at `sent`, for `writer` to add.
fn inbox_report(sent: &Path, writer: &carrel_core::coordinator::LockInfo, count: usize) -> Report {
    Report::new(
        format!(
            "Another device ({}) is the writer now, so {} went into this device's inbox, for it to add to its queue and draft.\n",
            writer.device,
            if count == 1 { "it" } else { "they" },
        ),
        json!({"saved": false, "queued": [], "inbox": sent, "writer": writer.device}),
    )
}

/// Drafts the queued items in the foreground, or follows the worker already drafting them,
/// then reports them as one batch.
fn wait(archive: &Archive, queued: &Queued, json: bool) -> Result<Report, Failure> {
    let ours: Vec<Id> = queued.added.iter().map(|e| e.id).collect();
    let mut display = crate::progress::Display::new("", json);
    let Some(lock) = crate::worker::try_lock(archive)? else {
        // Another worker has the queue; follow it until our items are gone from it.
        let total = ours.len();
        loop {
            let remaining = intake::remaining(archive, &ours)?;
            let current = crate::worker::status(archive).and_then(|s| s.item);
            display.count("items", total - remaining, total, current.as_deref());
            if remaining == 0 || !crate::worker::running(archive)? {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        display.finish();
        return Ok(plan_report(
            queued,
            "The running worker drafted them; review them with `carrel review` or in `carrel`.",
        ));
    };
    let mut runs = Vec::new();
    let mut not_drafted = Vec::new();
    let mut filed = Outcome::default();
    let mut questions: Vec<QuestionEntry> = Vec::new();
    crate::worker::work(
        archive,
        lock,
        |done, waiting, item| display.count("items", done, done + waiting, Some(item)),
        |processed| {
            for q in &processed.questions {
                questions.retain(|e| e.id != q.id);
                questions.push(q.clone());
            }
            if let Some(outcome) = processed.filed {
                filed.filed.extend(outcome.filed);
                filed.commit = outcome.commit;
            }
            match processed.run {
                Ok(run) => runs.push(run),
                Err(e) => not_drafted.push((processed.entry.path, e)),
            }
        },
    )?;
    display.finish();
    let mut run = merge(runs, queued);
    run.questions = questions;
    let mut report = report(&run, (!filed.filed.is_empty()).then_some(&filed), false);
    for (path, error) in &not_drafted {
        writeln!(report.text, "Not drafted {}: {error}", path.display()).unwrap();
    }
    report.json["failed"] = json!(
        not_drafted
            .iter()
            .map(|(path, error)| json!({ "path": path, "error": error }))
            .collect::<Vec<_>>()
    );
    Ok(report)
}

/// Runs of one item each, as one, with what queueing left out.
fn merge(runs: Vec<Run>, queued: &Queued) -> Run {
    let mut merged = Run {
        batch: runs.first().map_or_else(Id::new_v7, |r| r.batch),
        candidates: Vec::new(),
        skipped: queued.skipped.clone(),
        texts: Vec::new(),
        drafts: Vec::new(),
        routes: Vec::new(),
        assessments: Vec::new(),
        entries: Vec::new(),
        questions: Vec::new(),
        routing_problems: Vec::new(),
    };
    for run in runs {
        merged.candidates.extend(run.candidates);
        merged.texts.extend(run.texts);
        merged.drafts.extend(run.drafts);
        merged.routes.extend(run.routes);
        merged.assessments.extend(run.assessments);
        merged.entries.extend(run.entries);
        for problem in run.routing_problems {
            if !merged.routing_problems.contains(&problem) {
                merged.routing_problems.push(problem);
            }
        }
    }
    merged
}

/// What adding to the queue found: each item, what was left out and why, and `note`.
fn plan_report(queued: &Queued, note: &str) -> Report {
    let mut text = match queued.added.len() {
        0 => "Nothing to queue\n".to_owned(),
        1 => "1 item:\n".to_owned(),
        n => format!("{n} items:\n"),
    };
    let added: Vec<&Candidate> = queued
        .candidates
        .iter()
        .filter(|c| queued.added.iter().any(|e| e.path == c.path))
        .collect();
    let width = added
        .iter()
        .map(|c| name(c).chars().count())
        .max()
        .unwrap_or(0);
    for candidate in &added {
        let duplicate = candidate
            .duplicate_of
            .as_ref()
            .map(|d| format!(", a duplicate of {}", d.card))
            .or_else(|| {
                candidate
                    .likely
                    .first()
                    .map(|l| format!(", likely {}", likely_note(l)))
            })
            .unwrap_or_default();
        writeln!(
            text,
            "  {:width$}  {}, {}{duplicate}",
            name(candidate),
            describe_kind(candidate),
            human_size(candidate.size)
        )
        .unwrap();
    }
    if !queued.skipped.is_empty() {
        text.push_str("Left out:\n");
        for skipped in &queued.skipped {
            writeln!(text, "  {}: {}", skipped.path.display(), skipped.reason).unwrap();
        }
    }
    writeln!(text, "\n{note}").unwrap();
    Report::new(
        text,
        json!({
            "saved": false,
            "queued": queued.added.iter().map(|e| json!({"id": e.id, "path": e.path})).collect::<Vec<_>>(),
            "items": added.iter().map(|c| json!({
                "path": c.path,
                "kind": c.kind,
                "size": c.size,
                "duplicate_of": c.duplicate_of.as_ref().map(|d| json!({"id": d.id, "card": d.card})),
                "likely_duplicates": c.likely,
            })).collect::<Vec<_>>(),
            "skipped": queued.skipped.iter().map(|s| json!({"path": s.path, "reason": s.reason})).collect::<Vec<_>>(),
        }),
    )
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// `carrel ingest --status`: the worker and this device's queue.
///
/// # Errors
///
/// [`Failure`] if the queue can't be read.
pub fn queue_status(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let identity = crate::config::Identity::load_or_create()?;
    let intake::Listing {
        waiting,
        failed: stuck,
    } = intake::listing(&archive, identity.device)?;
    let worker = crate::worker::status(&archive);
    let running = crate::worker::running(&archive)?;
    let mut text = String::new();
    match (&worker, running) {
        (Some(status), _) => writeln!(
            text,
            "Worker: drafting {}{}, {} done so far",
            status.item.as_deref().unwrap_or("…"),
            status
                .stage
                .as_deref()
                .map(|s| format!(" ({s})"))
                .unwrap_or_default(),
            status.done
        )
        .unwrap(),
        (None, true) => text.push_str("Worker: starting\n"),
        (None, false) if crate::worker::agent(&archive) => {
            text.push_str("Worker: the Carrel agent, waiting for work\n");
        }
        (None, false) => text.push_str("Worker: not running\n"),
    }
    if waiting.is_empty() && stuck.is_empty() {
        text.push_str("The queue is empty.\n");
    }
    if !waiting.is_empty() {
        writeln!(text, "Waiting ({}):", waiting.len()).unwrap();
        for entry in &waiting {
            writeln!(text, "  {}  {}", short_id(entry.id), entry.path.display()).unwrap();
        }
    }
    if !stuck.is_empty() {
        writeln!(
            text,
            "Failed ({}; retry with `carrel ingest --retry ID`):",
            stuck.len()
        )
        .unwrap();
        for entry in &stuck {
            writeln!(
                text,
                "  {}  {}: {}",
                short_id(entry.id),
                entry.path.display(),
                entry.failure.as_deref().unwrap_or("")
            )
            .unwrap();
        }
    }
    if !waiting.is_empty() && !running {
        text.push_str("Nothing is drafting them; run `carrel worker` to start.\n");
    }
    Ok(Report::new(
        text,
        json!({
            "worker": worker,
            "running": running,
            "agent": crate::worker::agent(&archive),
            "waiting": waiting,
            "failed": stuck,
        }),
    ))
}

/// What to do to one queued item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Take it off the queue.
    Remove,
    /// Move it to the front.
    First,
    /// Try a failed one again.
    Retry,
}

/// `carrel ingest --remove`, `--first` and `--retry`.
///
/// # Errors
///
/// [`Failure`] if the item isn't queued or the queue can't be written.
pub fn change_queued(archive: Option<&Path>, id: &str, change: Change) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let identity = crate::config::Identity::load_or_create()?;
    let entry = match change {
        Change::Remove => intake::remove(&archive, id)?,
        Change::First => intake::move_to_front(&archive, id, identity.device)?,
        Change::Retry => {
            let entry = intake::retry(&archive, id)?;
            crate::worker::start(&archive)?;
            entry
        }
    };
    let done = match change {
        Change::Remove => "Removed",
        Change::First => "Moved to the front:",
        Change::Retry => "Will try again:",
    };
    Ok(Report::new(
        format!("{done} {}\n", entry.path.display()),
        json!({ "id": entry.id, "path": entry.path }),
    ))
}

/// An ingest run as a report: the plan for people, and everything as JSON.
pub fn report(run: &Run, filed: Option<&Outcome>, dry_run: bool) -> Report {
    let mut out = render(
        &run.candidates,
        &run.texts,
        &run.drafts,
        &run.routes,
        &run.assessments,
        &run.skipped,
    );
    for problem in &run.routing_problems {
        writeln!(out, "warning: {problem}").unwrap();
    }
    unreadable_line(&mut out, run);
    let saved = run.entries.iter().flatten().count();
    closing(&mut out, filed, &run.questions, saved, dry_run, run.batch);

    let items = items_json(&run.candidates, &run.texts, &run.drafts);
    Report::new(
        out,
        json!({
            "batch": run.batch,
            "saved": !dry_run,
            "items": items,
            "skipped": run.skipped.iter().map(|s| json!({"path": s.path, "reason": s.reason})).collect::<Vec<_>>(),
            "proposals": run.entries.iter().flatten().collect::<Vec<_>>(),
            "filed": filed.map(|o| o.filed.iter().map(|f| json!({"proposal": f.entry, "item": f.item, "path": f.path, "card": f.card})).collect::<Vec<_>>()).unwrap_or_default(),
        }),
    )
}

/// How many items are of a type nothing on this device reads, and which extensions.
fn unreadable_line(out: &mut String, run: &Run) {
    let mut extensions: Vec<String> = Vec::new();
    let mut count = 0;
    for (candidate, text) in run.candidates.iter().zip(&run.texts) {
        if text.as_ref().and_then(|t| t.unread) == Some(Unread::NoReader) {
            count += 1;
            let extension = carrel_core::formats::extension(&candidate.path);
            let shown = if extension.is_empty() {
                "no extension".to_owned()
            } else {
                format!(".{extension}")
            };
            if !extensions.contains(&shown) {
                extensions.push(shown);
            }
        }
    }
    if count > 0 {
        writeln!(
            out,
            "\n{} of a type nothing on this device reads ({}); see `carrel formats`.",
            plural(count, "item"),
            extensions.join(", ")
        )
        .unwrap();
    }
}

/// What was filed by rule, what was asked, and what was saved for review.
fn closing(
    out: &mut String,
    auto_filed: Option<&Outcome>,
    questions: &[QuestionEntry],
    proposed: usize,
    dry_run: bool,
    batch: Id,
) {
    if !questions.is_empty() {
        out.push_str("\nQuestions (answer them with `carrel questions answer`):\n");
        for question in questions {
            writeln!(
                out,
                "  {}  {} ({} item{})",
                short_id(question.id),
                question.question,
                question.items.len(),
                if question.items.len() == 1 { "" } else { "s" }
            )
            .unwrap();
        }
    }
    closing_lines(out, auto_filed, proposed, dry_run, batch);
}

/// What was filed by rule, and how many proposals were saved.
fn closing_lines(
    out: &mut String,
    auto_filed: Option<&Outcome>,
    proposed: usize,
    dry_run: bool,
    batch: Id,
) {
    if let Some(outcome) = auto_filed {
        for filed in &outcome.filed {
            writeln!(
                out,
                "Filed {} in {} (a routing rule files it without review)",
                filed.title, filed.path
            )
            .unwrap();
        }
        for (entry, error) in &outcome.failed {
            writeln!(out, "Not filed {}: {error}", entry.card.title).unwrap();
        }
    }
    let saved = proposed - auto_filed.map_or(0, |o| o.filed.len());
    if dry_run {
        out.push_str("\nNothing has been saved (--dry-run).\n");
    } else if saved > 0 {
        writeln!(
            out,
            "\nSaved {saved} proposal{} (batch {}). Review them with `carrel review`; nothing moves until you approve.",
            if saved == 1 { "" } else { "s" },
            short_id(batch)
        )
        .unwrap();
    }
}

/// Each item as JSON.
fn items_json(
    candidates: &[Candidate],
    texts: &[Option<ItemText>],
    drafts: &[Option<Draft>],
) -> Vec<Value> {
    candidates
        .iter()
        .zip(texts)
        .zip(drafts)
        .map(|((c, t), d)| {
            json!({
                "path": c.path,
                "kind": c.kind,
                "sealed": c.sealed,
                "markers": c.markers,
                "checksum": c.checksum,
                "size": c.size,
                "duplicate_of": c.duplicate_of.as_ref().map(|d| json!({"id": d.id, "card": d.card})),
                "likely_duplicates": c.likely,
                "same_as": c.same_as.map(|i| &candidates[i].path),
                "compressed": c.inside.as_ref().map(inside_json),
                "text": t.as_ref().map(text_json),
                "card": d.as_ref().map(draft_json),
            })
        })
        .collect()
}

/// The plan as a report for people.
fn render(
    candidates: &[Candidate],
    texts: &[Option<ItemText>],
    drafts: &[Option<Draft>],
    routes: &[Option<Route>],
    assessments: &[Option<Assessment>],
    skipped: &[carrel_core::ingest::Skipped],
) -> String {
    let mut out = String::new();
    match candidates.len() {
        0 => out.push_str("Nothing to ingest\n"),
        1 => out.push_str("1 item to ingest:\n"),
        n => writeln!(out, "{n} items to ingest:").unwrap(),
    }
    let width = candidates
        .iter()
        .map(|c| name(c).chars().count())
        .max()
        .unwrap_or(0);
    for ((((candidate, item_text), card), route), assessment) in candidates
        .iter()
        .zip(texts)
        .zip(drafts)
        .zip(routes)
        .zip(assessments)
    {
        let mut notes = Vec::new();
        if let Some(duplicate) = &candidate.duplicate_of {
            notes.push(format!("duplicate of {}", duplicate.card));
        }
        for likely in &candidate.likely {
            notes.push(format!("likely {}", likely_note(likely)));
        }
        if let Some(earlier) = candidate.same_as {
            notes.push(format!(
                "same as {} in this batch",
                name(&candidates[earlier])
            ));
        }
        writeln!(
            out,
            "  {:<width$}  {}, {}{}",
            name(candidate),
            describe_kind(candidate),
            human_size(candidate.size),
            if notes.is_empty() {
                String::new()
            } else {
                format!("  ({})", notes.join("; "))
            }
        )
        .unwrap();
        if let Some(item_text) = item_text {
            writeln!(out, "  {:<width$}  {}", "", describe_text(item_text)).unwrap();
        }
        if let Some(card) = card {
            for line in describe_draft(card) {
                writeln!(out, "  {:<width$}  {line}", "").unwrap();
            }
        }
        if let Some(route) = route {
            let why = route
                .reasons
                .first()
                .map(|r| format!(": {r}"))
                .unwrap_or_default();
            writeln!(
                out,
                "  {:<width$}  to: {} ({} confidence){why}",
                "",
                route.path,
                format!("{:?}", route.confidence).to_lowercase()
            )
            .unwrap();
        }
        if let Some(assessment) = assessment {
            let level = serde_json::to_value(assessment.level)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let why = if assessment.reasons.is_empty() {
                String::new()
            } else {
                format!(": {}", assessment.reasons.join("; "))
            };
            writeln!(out, "  {:<width$}  sensitivity: {level}{why}", "").unwrap();
        }
    }
    if !skipped.is_empty() {
        out.push_str("Skipped:\n");
        for skip in skipped {
            let name = skip.path.file_name().map_or_else(
                || skip.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            writeln!(out, "  {name}: {}", skip.reason).unwrap();
        }
    }
    out
}

/// The drafted card, a few lines for people.
fn describe_draft(draft: &Draft) -> Vec<String> {
    let mut lines = vec![format!("card: \u{201c}{}\u{201d}", draft.title)];
    if let Some(summary) = &draft.summary {
        lines.push(format!("      {summary}"));
    }
    let mut tags: Vec<String> = draft.tags.iter().map(|t| t.0.clone()).collect();
    if let Some(new_tag) = &draft.new_tag {
        tags.push(format!("{} (new)", new_tag.0));
    }
    if !tags.is_empty() {
        lines.push(format!("      tags: {}", tags.join(", ")));
    }
    if let Some(model) = draft.generated.as_ref().and_then(|g| g.model.as_ref()) {
        lines.push(format!("      drafted by {model}"));
    }
    for problem in &draft.problems {
        lines.push(format!("      to settle: {problem}"));
    }
    lines
}

fn draft_json(draft: &Draft) -> Value {
    json!({
        "title": draft.title,
        "summary": draft.summary,
        "tags": draft.tags,
        "new_tag": draft.new_tag,
        "created": draft.created,
        "generated": draft.generated,
        "problems": draft.problems,
        "attempts": draft.attempts,
    })
}

/// The provider and offer that read the text, if one did.
fn reader(text: &ItemText) -> Option<String> {
    text.attempts
        .iter()
        .rfind(|a| a.result == "ok")
        .map(|a| format!("{}/{}", a.provider, a.offer))
}

/// `text: 3200 characters, embedded, from apple/pdfkit (12 pages)`.
fn describe_text(text: &ItemText) -> String {
    if text.method == "none" {
        return format!(
            "no text: {}",
            text.note.as_deref().unwrap_or("nothing could read it")
        );
    }
    let mut line = format!(
        "text: {} characters, {}",
        text.text.chars().count(),
        text.method
    );
    if let Some(reader) = reader(text) {
        write!(line, ", from {reader}").unwrap();
    }
    if let Some(pages) = text.pages {
        write!(line, " ({pages} page{})", if pages == 1 { "" } else { "s" }).unwrap();
    }
    line
}

fn text_json(text: &ItemText) -> Value {
    json!({
        "method": text.method,
        "media_type": text.media_type,
        "characters": text.text.chars().count(),
        "pages": text.pages,
        "read_by": reader(text),
        "note": text.note,
    })
}

/// What a compressed file holds, for `--json`.
fn inside_json(inside: &carrel_core::compressed::Inside) -> Value {
    json!({
        "format": inside.format,
        "holds": inside.describe(),
        "files": inside.listing.as_ref().map(carrel_core::compressed::Listing::file_count),
        "size": inside.listing.as_ref().map(carrel_core::compressed::Listing::total_size),
        "unread": inside.unread,
    })
}

fn describe_kind(candidate: &Candidate) -> String {
    if let Some(inside) = &candidate.inside {
        return format!(
            "compressed {} file holding {}",
            inside.format.name(),
            inside.describe()
        );
    }
    match candidate.kind {
        Kind::Bundle if !candidate.markers.is_empty() => {
            format!("bundle (sealed by {})", candidate.markers.join(", "))
        }
        Kind::Bundle => "bundle".into(),
        Kind::Folder => "folder".into(),
        _ => "file".into(),
    }
}

pub(crate) use carrel_core::human_size;

/// `duplicate of Finance/report.pdf.md (same file name)`.
fn likely_note(likely: &carrel_core::ingest::likely::Likely) -> String {
    let why: Vec<String> = likely.why.iter().map(ToString::to_string).collect();
    format!("duplicate of {} ({})", likely.card, why.join(", "))
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_read_naturally() {
        assert_eq!(super::human_size(0), "0 B");
        assert_eq!(super::human_size(999), "999 B");
        assert_eq!(super::human_size(1294), "1.3 KB");
        assert_eq!(super::human_size(482_113), "482 KB");
        assert_eq!(super::human_size(2_100_000_000), "2.1 GB");
    }
}
