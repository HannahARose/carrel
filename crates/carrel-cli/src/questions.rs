//! `carrel questions`: what Carrel needs the owner to explain, the most worth answering first
//! (`docs/design/librarian.md`, "Questions"). Each question offers likely answers to pick by
//! number, as well as a free answer, Skip and Dismiss. The core ranks, answers and remembers
//! ([`carrel_core::questions`]); this module words it and asks on the terminal.

use std::fmt::Write as _;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::Path;

use carrel_core::actions;
use carrel_core::archive::Archive;
use carrel_core::questions::answer::{self as core_answer, SenderAnswer, Target};
use carrel_core::questions::rank::Weighed;
use carrel_core::questions::subjects::{QuestionSubject, subjects};
use carrel_core::questions::{QuestionEntry, QuestionKind, QuestionQueue, Suggestion};
use carrel_core::review::short_id;
use serde_json::{Value, json};

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// How an answer is given.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // one per flag of `carrel questions answer`
pub struct Options {
    /// Save an item answer's correction without asking.
    pub yes: bool,
    /// Add what the answer teaches (a glossary fact from a correction, or a routing rule for a
    /// sender) without asking.
    pub learn: bool,
    /// Create the collection named, when it doesn't exist.
    pub create: bool,
    /// Whether to ask on the terminal, for what wasn't given.
    pub ask: bool,
}

/// "N items", in words.
fn items(n: usize) -> String {
    format!("{n} item{}", if n == 1 { "" } else { "s" })
}

/// A suggestion, for people: its answer, and why it's likely.
pub(crate) fn suggestion_line(n: usize, suggestion: &Suggestion) -> String {
    let mut line = format!("{n}  {}", suggestion.answer);
    let mut notes = Vec::new();
    if suggestion.new_collection {
        notes.push("a new collection".to_owned());
    }
    if let Some(why) = &suggestion.why {
        notes.push(why.clone());
    }
    if !notes.is_empty() {
        write!(line, "  ({})", notes.join("; ")).unwrap();
    }
    line
}

/// The most items a question lists under it; the rest are counted.
const SUBJECTS_SHOWN: usize = 5;

/// What `question` is about, as the list and JSON show it: each item's title and where its
/// file is, so it can be opened (#446).
fn subjects_of(archive: &Archive, question: &QuestionEntry) -> Vec<QuestionSubject> {
    subjects(archive, question).unwrap_or_default()
}

/// The lines naming what a question is about.
fn write_subjects(text: &mut String, found: &[QuestionSubject]) {
    for subject in found.iter().take(SUBJECTS_SHOWN) {
        let place = subject.path.as_deref().map_or_else(
            || "no file on this device".to_owned(),
            |p| p.display().to_string(),
        );
        let restricted = if subject.restricted {
            ", restricted"
        } else {
            ""
        };
        writeln!(
            text,
            "          about {}: {place}{restricted}",
            subject.title
        )
        .unwrap();
    }
    if found.len() > SUBJECTS_SHOWN {
        writeln!(
            text,
            "          and {} more",
            items(found.len() - SUBJECTS_SHOWN)
        )
        .unwrap();
    }
}

/// A question as the list shows it: its words, what an answer settles, what it's about, and
/// the suggestions.
fn describe(text: &mut String, archive: &Archive, weighed: &Weighed) {
    let question = &weighed.question;
    writeln!(text, "{}  {}", short_id(question.id), question.question).unwrap();
    let mut about = format!(
        "          {}, settles {}",
        question.kind.as_str(),
        items(weighed.settles)
    );
    if question.skips > 0 {
        write!(
            about,
            ", skipped {} time{}",
            question.skips,
            if question.skips == 1 { "" } else { "s" }
        )
        .unwrap();
    }
    writeln!(text, "{about}").unwrap();
    write_subjects(text, &subjects_of(archive, question));
    for (i, suggestion) in question.suggestions.iter().enumerate() {
        writeln!(text, "          {}", suggestion_line(i + 1, suggestion)).unwrap();
    }
}

/// A question, weighed, as JSON, with what it's about.
fn weighed_json(archive: &Archive, weighed: &Weighed) -> Value {
    let mut value = serde_json::to_value(&weighed.question).unwrap_or(Value::Null);
    value["subjects"] = subjects_of(archive, &weighed.question)
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "proposal": s.proposal,
                "title": s.title,
                "card": s.card,
                "path": s.path,
                "restricted": s.restricted,
            })
        })
        .collect();
    value["settles"] = json!(weighed.settles);
    value["value"] = json!(weighed.value);
    value
}

/// `carrel questions` or `carrel questions list [--all]`: the questions, the most worth
/// answering first, after dropping those the glossary or a rule already answers and merging
/// those that say the same. With `all`, the questions resting after a skip too.
///
/// # Errors
///
/// [`Failure`] if the queues can't be read or written.
pub fn list(archive: Option<&Path>, all: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let ranked = core_answer::tidy(&archive)?;
    let mut text = String::new();
    if ranked.open.is_empty() {
        text.push_str("No questions\n");
    }
    for weighed in &ranked.open {
        describe(&mut text, &archive, weighed);
    }
    if !ranked.resting.is_empty() {
        if all {
            let days = archive.settings().questions.skip_days;
            writeln!(text, "\nResting after a skip:").unwrap();
            for weighed in &ranked.resting {
                describe(&mut text, &archive, weighed);
                if let Some(until) = weighed.question.rests_until(days) {
                    writeln!(text, "          back on {}", until.strftime("%Y-%m-%d")).unwrap();
                }
            }
        } else {
            writeln!(
                text,
                "{} skipped question{} resting; `carrel questions list --all` shows {}",
                ranked.resting.len(),
                if ranked.resting.len() == 1 {
                    " is"
                } else {
                    "s are"
                },
                if ranked.resting.len() == 1 {
                    "it"
                } else {
                    "them"
                },
            )
            .unwrap();
        }
    }
    for (question, why) in &ranked.answered {
        writeln!(text, "Dropped “{}”: {why}", question.question).unwrap();
    }
    for (kept, folded) in &ranked.merged {
        writeln!(
            text,
            "Merged {} into {} ({}), which asks the same",
            folded.len(),
            short_id(kept.id),
            kept.question
        )
        .unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "questions": ranked.open.iter().map(|w| weighed_json(&archive, w)).collect::<Vec<_>>(),
            "resting": ranked.resting.iter().map(|w| weighed_json(&archive, w)).collect::<Vec<_>>(),
            "dropped": ranked.answered.iter().map(|(q, why)| json!({"id": q.id, "question": q.question, "why": why})).collect::<Vec<_>>(),
            "merged": ranked.merged.iter().map(|(k, f)| json!({"into": k.id, "merged": f.iter().map(|q| q.id).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        }),
    ))
}

/// What the owner says when asked on the terminal: a suggestion's number, words, `s` to skip,
/// or nothing to leave it.
enum Said {
    Answer(String),
    Skip,
    Nothing,
}

/// Shows `question` with its suggestions on the terminal and reads the answer.
fn ask_on_terminal(question: &QuestionEntry) -> Result<Said, Failure> {
    eprintln!("{}", question.question);
    for (i, suggestion) in question.suggestions.iter().enumerate() {
        eprintln!("  {}", suggestion_line(i + 1, suggestion));
    }
    if question.suggestions.is_empty() {
        eprint!("\nYour answer (s skips, Enter leaves it): ");
    } else {
        eprint!("\nA number, or your own answer (s skips, Enter leaves it): ");
    }
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(match line.trim() {
        "" => Said::Nothing,
        "s" | "S" | "skip" => Said::Skip,
        said => Said::Answer(said.to_owned()),
    })
}

/// `carrel questions answer ID [ANSWER...]`: answers with a suggestion's number or words, or
/// freely; asks on the terminal when no answer is given.
///
/// # Errors
///
/// [`Failure`] if the question isn't found, the answer doesn't fit it, or it can't be saved.
pub fn answer(
    archive: Option<&Path>,
    id: &str,
    words: &[String],
    options: Options,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let question = QuestionQueue::of(&archive).find(id)?;
    let mut given = words.join(" ").trim().to_owned();
    if given.is_empty() {
        if !options.ask {
            return Err(Failure::new(
                "give an answer, or a suggestion's number from `carrel questions`",
            ));
        }
        match ask_on_terminal(&question)? {
            Said::Answer(said) => given = said,
            Said::Skip => return skip_question(&archive, &question),
            Said::Nothing => {
                return Ok(Report::new(
                    "Not answered; the question still waits.\n",
                    json!({"question": question.id, "answered": false}),
                ));
            }
        }
    }
    let picked = question.picked(&given).cloned();
    if picked.as_ref().is_some_and(|s| s.no_change) {
        core_answer::decline(&archive, &question)?;
        return Ok(Report::new(
            format!(
                "Settled “{}”: nothing changes, and Carrel won't ask it again.\n",
                question.question
            ),
            json!({"question": question.id, "declined": true}),
        ));
    }
    let answer = picked
        .as_ref()
        .map_or_else(|| given.clone(), |s| s.answer.clone());
    match question.kind {
        QuestionKind::Term => teach(&archive, &question, &answer),
        QuestionKind::Collection | QuestionKind::Group => {
            let create = options.create || picked.as_ref().is_some_and(|s| s.new_collection);
            place(&archive, &question, &answer, create, None)
        }
        QuestionKind::Sender => sender(&archive, &question, &answer, picked.as_ref(), options),
        QuestionKind::Item => item(&archive, &question, &answer, options),
    }
}

/// Answers a sender question: no settles it for good; yes, or a collection, gathers its items
/// there, then offers a routing rule for future ones.
fn sender(
    archive: &Archive,
    question: &QuestionEntry,
    answer: &str,
    picked: Option<&Suggestion>,
    options: Options,
) -> Result<Report, Failure> {
    let (path, create) = match core_answer::sender_answer(question, answer, picked, options.create)?
    {
        SenderAnswer::Decline => {
            core_answer::decline(archive, question)?;
            return Ok(Report::new(
                format!(
                    "Settled: “{}” stays where it is, and Carrel won't ask again.\n",
                    question.subject
                ),
                json!({"question": question.id, "declined": true}),
            ));
        }
        SenderAnswer::Place { path, create } => (path, create),
    };
    place(archive, question, &path, create, Some(options))
}

/// Points the question's proposals at the collection at `path`, refiling its filed items
/// there, creating it if `create`. For a sender, `rule` says how to offer a routing rule.
fn place(
    archive: &Archive,
    question: &QuestionEntry,
    path: &str,
    create: bool,
    rule: Option<Options>,
) -> Result<Report, Failure> {
    let identity = Identity::load_or_create()?;
    let placed = core_answer::place(archive, question, path, create, holder(&identity))?;
    let mut text = String::new();
    if placed.created.is_some() {
        writeln!(text, "Created the collection {}", placed.collection).unwrap();
    }
    for m in &placed.moved {
        write!(text, "{} will go to {}", short_id(m.entry.id), m.to).unwrap();
        if !m.dropped.is_empty() {
            write!(text, " (tags {} left off)", m.dropped.join(", ")).unwrap();
        }
        text.push('\n');
    }
    for (title, _) in &placed.refiled {
        writeln!(text, "Refiled “{title}” to {}", placed.collection).unwrap();
    }
    for problem in &placed.not_refiled {
        writeln!(text, "Not refiled: {problem}").unwrap();
    }
    if !placed.refiled.is_empty() {
        text.push_str("Undo any of these from History or with `carrel undo`.\n");
    }
    let mut json = json!({
        "question": question.id,
        "collection": placed.collection,
        "created": placed.created.is_some(),
        "proposals": placed.moved.iter().map(|m| m.entry.id).collect::<Vec<_>>(),
        "refiled": placed.refiled.iter().map(|(t, c)| json!({"title": t, "commit": c})).collect::<Vec<_>>(),
    });
    if let Some(options) = rule {
        let (said, added) = offer_rule(archive, &question.subject, &placed.collection, options)?;
        text.push_str(&said);
        json["rule"] =
            json!({"subject": question.subject, "collection": placed.collection, "added": added});
    }
    Ok(Report::new(text, json))
}

/// Offers a routing rule filing future items about `subject` into `path`: added with `learn`,
/// asked about on a terminal, otherwise left with a hint. Never added silently.
fn offer_rule(
    archive: &Archive,
    subject: &str,
    path: &str,
    options: Options,
) -> Result<(String, bool), Failure> {
    let agreed = if options.learn {
        true
    } else if options.ask {
        eprint!("File future items about “{subject}” in {path} with a routing rule? [y/N] ");
        let _ = std::io::stderr().flush();
        let mut said = String::new();
        std::io::stdin().lock().read_line(&mut said)?;
        matches!(said.trim(), "y" | "Y" | "yes" | "Yes")
    } else {
        return Ok((
            format!(
                "No routing rule added; answering with --learn adds one filing future items about “{subject}” in {path}.\n"
            ),
            false,
        ));
    };
    if !agreed {
        return Ok((String::new(), false));
    }
    add_rule(archive, subject, path)?;
    Ok((
        format!("Added a routing rule: items named for “{subject}” go to {path}.\n"),
        true,
    ))
}

/// Adds a routing rule filing items named for `subject` into the collection at `path`, as the
/// owner agreed.
///
/// # Errors
///
/// [`Failure`] if there's no such collection, or the rule can't be written or committed.
pub fn add_rule(archive: &Archive, subject: &str, path: &str) -> Result<Option<String>, Failure> {
    let identity = Identity::load_or_create()?;
    let collections = archive.collections()?;
    let target = actions::collection(&collections, path)?;
    Ok(core_answer::add_rule(
        archive,
        subject,
        target,
        holder(&identity),
    )?)
}

/// Answers an item question through the corrections path: the answer becomes a correction of
/// the item's card, shown before and after, and saved on the owner's yes.
fn item(
    archive: &Archive,
    question: &QuestionEntry,
    answer: &str,
    options: Options,
) -> Result<Report, Failure> {
    let words = core_answer::item_words(question, answer);
    if options.ask && std::io::stderr().is_terminal() {
        eprintln!("Working out what that changes…");
    }
    let mut prepared = match core_answer::target(question) {
        Some(Target::Proposal(id)) => {
            crate::correct::prepare_review(archive, &id.to_string(), &words)?
        }
        Some(Target::Filed(id)) => crate::correct::prepare_filed(archive, &id.to_string(), &words)?,
        None => return Err(Failure::new("the question isn't about an item")),
    };
    prepared.answering = Some(question.id);
    let heading = format!("{}\n", question.question);
    let mut report = crate::correct::finish(
        archive,
        heading,
        prepared,
        options.yes,
        options.learn,
        json!({"question": question.id}),
    )?;
    // An answer that changes nothing still answers the question.
    if report.json["saved"] == json!(false) && report.text.contains("It changes nothing") {
        core_answer::settled(archive, question)?;
        report.text.push_str("The question is answered.\n");
    }
    Ok(report)
}

/// `carrel questions skip ID`: the question rests for the archive's `questions.skip_days`.
///
/// # Errors
///
/// [`Failure`] if the question isn't found or can't be saved.
pub fn skip(archive: Option<&Path>, id: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let question = QuestionQueue::of(&archive).find(id)?;
    skip_question(&archive, &question)
}

fn skip_question(archive: &Archive, question: &QuestionEntry) -> Result<Report, Failure> {
    let skipped = core_answer::skip(archive, question)?;
    let days = archive.settings().questions.skip_days;
    Ok(Report::new(
        format!(
            "Skipped {}; it's asked again after {days} day{}.\n",
            short_id(question.id),
            if days == 1 { "" } else { "s" }
        ),
        json!({"skipped": question.id, "skips": skipped.skips, "until": skipped.rests_until(days).map(|t| t.to_string())}),
    ))
}

/// `carrel questions dismiss ID`: the items stop waiting on the question.
///
/// # Errors
///
/// [`Failure`] if the question isn't found or the queues can't be written.
pub fn dismiss(archive: Option<&Path>, id: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let question = QuestionQueue::of(&archive).find(id)?;
    actions::dismiss(&archive, &question)?;
    Ok(Report::new(
        format!(
            "Dismissed {}: {}\n",
            short_id(question.id),
            question.question
        ),
        json!({"dismissed": question.id}),
    ))
}

/// `carrel questions about ITEM`: the questions Carrel has about a filed item, added to the
/// queue, with likely answers from a text model when there is one.
///
/// # Errors
///
/// [`Failure`] if the item isn't found, or the queue can't be written.
pub fn about(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = crate::commands::find_item(&archive, &items, query)?.clone();
    let file = carrel_core::card_file::CardFile::parse(&std::fs::read_to_string(
        archive.root().join(&item.card),
    )?)?;
    let card = file.card();
    let glossary = carrel_core::ingest::card::Glossary::of(&archive);
    let today = jiff::Zoned::now().date();
    let mut asked = carrel_core::questions::about::item(&item, card, &glossary, today);
    if !asked.is_empty() {
        let (registry, _) = crate::providers::registry();
        let context = format!(
            "Title: {}\nSummary: {}\nTags: {}\nIn: {}",
            card.title,
            card.summary.as_deref().unwrap_or("(none)"),
            card.tags
                .iter()
                .map(|t| t.0.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            item.card
        );
        for question in asked.iter_mut().filter(|q| q.suggestions.is_empty()) {
            question.suggestions =
                carrel_core::questions::suggest::with_model(&registry, question, &context);
        }
    }
    let raised = core_answer::raise(&archive, asked)?;
    let (all, _) = QuestionQueue::of(&archive).list()?;
    let about: Vec<&QuestionEntry> = all
        .iter()
        .filter(|q| q.about.contains(&item.id) && q.declined.is_none())
        .collect();
    let mut text = String::new();
    if about.is_empty() {
        writeln!(text, "No questions about “{}”", card.title).unwrap();
    }
    for question in &about {
        writeln!(text, "{}  {}", short_id(question.id), question.question).unwrap();
        for (i, suggestion) in question.suggestions.iter().enumerate() {
            writeln!(text, "          {}", suggestion_line(i + 1, suggestion)).unwrap();
        }
    }
    if !about.is_empty() {
        text.push_str("Answer with `carrel questions answer ID`.\n");
    }
    Ok(Report::new(
        text,
        json!({
            "item": item.id,
            "questions": about,
            "raised": raised.iter().map(|q| q.id).collect::<Vec<_>>(),
        }),
    ))
}

/// Adds the answer to the glossary, then drafts the items waiting on it again.
fn teach(archive: &Archive, question: &QuestionEntry, fact: &str) -> Result<Report, Failure> {
    let identity = Identity::load_or_create()?;
    let commit = actions::teach(archive, question, fact, holder(&identity))?;
    let mut text = format!("Added “{}” to the glossary\n", question.subject);
    let queued = actions::requeue_waiting(archive, question, identity.device)?;
    if !queued.is_empty() {
        let started = crate::worker::start(archive)?;
        writeln!(
            text,
            "Queued {} to be drafted again with it; {}",
            items(queued.len()),
            if started || crate::worker::running(archive)? {
                "the worker drafts them in the background"
            } else {
                "run `carrel worker` to draft them"
            }
        )
        .unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "question": question.id,
            "glossary": question.subject,
            "commit": commit,
            "requeued": queued.iter().map(|e| e.id).collect::<Vec<_>>(),
        }),
    ))
}
