//! `carrel ask`: the librarian at the terminal (`docs/design/librarian.md`). One question
//! gives one answer; without one, it's a conversation, a question per line. `--save` writes
//! the conversation to this month's Journal; `--json` gives the answers for programs. The
//! core runs the loop ([`carrel_core::librarian`]); this module chooses the model, words the
//! answers and asks the owner to confirm the changes the librarian suggests.
//!
//! Also here: the librarian's changes on `carrel status`, and `carrel undo --librarian`.

use std::fmt::Write as _;
use std::io::{IsTerminal as _, Write as _};
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::history::Entry;
use carrel_core::librarian::changes::{self, Outcome, Suggestion};
use carrel_core::librarian::{Answer, Conversation, Librarian, Stopped, journal, model};
use carrel_core::providers::Registry;
use carrel_core::providers::device::AvailableModel;
use carrel_core::questions::QuestionEntry;
use carrel_core::questions::moments::{self, Conversing};
use serde_json::{Value, json};

use crate::commands::{holder, locate, log_line, plural};
use crate::config::{Identity, Prefs};
use crate::{Failure, Report};

/// How far back `carrel status` lists the librarian's changes: a week.
const RECENT_DAYS: i64 = 7;

/// How much memory this device has, in gigabytes, when it can be found out.
pub fn memory_gb() -> Option<u64> {
    let bytes = if cfg!(target_os = "linux") {
        std::fs::read_to_string("/proc/meminfo")
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))?
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse::<u64>()
            .ok()?
            * 1024
    } else if cfg!(target_os = "macos") {
        let out = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()?
    } else {
        return None;
    };
    // Rounded, so a 16 GB machine reporting a little less still counts as 16.
    Some((bytes + (1 << 29)) >> 30)
}

/// The model the librarian runs on here: the `librarian.model` preference, else the strongest
/// this device has.
///
/// # Errors
///
/// [`Failure`] if the preference names a model that isn't available, or there's none, saying
/// what to do about it ([`what_to_do`]).
pub fn the_model(registry: &Registry) -> Result<AvailableModel, Failure> {
    explain_the_model(registry)?.map_err(|missing| Failure::new(what_to_do(&missing)))
}

/// The model the librarian runs on here, as [`the_model`] chooses it, or why there's none.
///
/// # Errors
///
/// [`Failure`] if the preferences can't be read.
pub fn explain_the_model(
    registry: &Registry,
) -> Result<Result<AvailableModel, Box<model::NoModel>>, Failure> {
    let prefs = Prefs::load()?;
    Ok(model::choose_or_explain(
        registry,
        prefs.librarian_model.as_deref(),
        memory_gb(),
        carrel_core::local::models_dir().as_deref(),
    ))
}

/// Why the librarian has no model, then the commands that act on it: clearing or changing
/// `librarian.model`, or downloading the model that suits this device, which asks first.
#[must_use]
pub fn what_to_do(missing: &model::NoModel) -> String {
    let mut text = missing.to_string();
    if let Some(instead) = &missing.instead {
        write!(
            text,
            " Clear the setting with `carrel settings set librarian.model default`, or choose \
             one with `carrel settings set librarian.model {}`.",
            instead.name()
        )
        .unwrap();
    }
    if let Some(download) = missing.download {
        write!(
            text,
            " Download it with `carrel models download {}`, which asks first.",
            download.name
        )
        .unwrap();
    }
    text
}

/// `carrel ask`.
///
/// # Errors
///
/// [`Failure`] if there's no model, the archive can't be read, the model fails, or saving
/// fails.
pub fn ask(
    archive: Option<&Path>,
    question: &str,
    save: bool,
    json: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (registry, _) = crate::providers::registry();
    let model = the_model(&registry)?;
    let identity = Identity::load_or_create()?;
    let providers = || crate::providers::registry().0;
    let librarian =
        Librarian::new(&archive, &registry, model, &providers)?.with_changes_for(holder(&identity));
    let mut conversation = Conversation::default();
    let mut text = String::new();
    let mut asked: Vec<Option<QuestionEntry>> = Vec::new();
    let mut conversing = Conversing::default();
    if question.trim().is_empty() {
        converse(
            &archive,
            &registry,
            &librarian,
            (&mut conversation, &mut conversing, &mut asked),
            json,
        )?;
    } else {
        working(&librarian, json);
        let answer = librarian.ask(&mut conversation, question)?.clone();
        text = describe(&answer);
        let question = question_after(&archive, &mut conversing, &answer);
        if let Some(question) = &question {
            text.push_str(&question_later(question));
        }
        asked.push(question);
    }
    let saved = if save && !conversation.answers.is_empty() {
        let lease = DiskLock::for_archive(&archive).acquire(
            holder(&identity),
            "librarian",
            DEFAULT_LEASE,
        )?;
        let saved = journal::save(&archive, &conversation, &jiff::Zoned::now(), &lease);
        lease.release()?;
        let saved = saved?;
        writeln!(text, "\nSaved to {}", saved.path).unwrap();
        Some(saved)
    } else {
        None
    };
    let answers: Vec<Value> = conversation
        .answers
        .iter()
        .zip(asked.iter().chain(std::iter::repeat(&None)))
        .map(|(answer, question)| {
            let mut value = answer_json(answer);
            value["asks"] = question.as_ref().map_or(
                Value::Null,
                |q| json!({"id": q.id, "question": q.question, "subject": q.subject}),
            );
            value
        })
        .collect();
    let mut value = if conversation.answers.len() == 1 && !question.trim().is_empty() {
        answers.into_iter().next().unwrap_or(Value::Null)
    } else {
        json!({ "answers": answers })
    };
    value["saved"] = saved.map_or(Value::Null, |s| json!({"path": s.path, "commit": s.commit}));
    value["changes"] = json!(librarian.makes_changes());
    Ok(Report::new(text, value))
}

/// Says the librarian is working, on a terminal: an answer takes a while.
fn working(librarian: &Librarian<'_>, json: bool) {
    if !json && std::io::stderr().is_terminal() {
        eprintln!("Asking the librarian ({})…", librarian.model().label);
    }
}

/// A conversation: a question per line from standard input, until an empty line or the end,
/// each answer printed as it comes. On a terminal, suggestions left for the owner are offered
/// one by one.
fn converse(
    archive: &Archive,
    registry: &Registry,
    librarian: &Librarian<'_>,
    (conversation, conversing, asked): (
        &mut Conversation,
        &mut Conversing,
        &mut Vec<Option<QuestionEntry>>,
    ),
    json: bool,
) -> Result<(), Failure> {
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if terminal && !json {
        eprintln!(
            "Ask the librarian about your archive ({}). An empty line ends the conversation.",
            librarian.model().label
        );
    }
    // Read a line at a time, without holding standard input, so answering a question can
    // read from it too.
    let mut lines = std::iter::from_fn(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(Ok(line.trim_end_matches(['\n', '\r']).to_owned())),
            Err(e) => Some(Err(e)),
        }
    });
    loop {
        if terminal && !json {
            eprint!("\n> ");
            let _ = std::io::stderr().flush();
        }
        let Some(line) = lines.next() else { break };
        let line = line?;
        if line.trim().is_empty() || matches!(line.trim(), "exit" | "quit" | "bye") {
            break;
        }
        working(librarian, json);
        let answer = librarian.ask(conversation, line.trim())?.clone();
        let question = question_after(archive, conversing, &answer);
        asked.push(question.clone());
        if json {
            continue;
        }
        println!("\n{}", describe(&answer));
        if terminal {
            confirm(archive, registry, &answer, &mut lines)?;
            if let Some(question) = &question {
                answer_now(archive, question, &mut lines)?;
            }
        } else if let Some(question) = &question {
            print!("{}", question_later(question));
        }
    }
    Ok(())
}

/// Offers each suggestion left for the owner, making those they agree to as their own changes.
fn confirm(
    archive: &Archive,
    registry: &Registry,
    answer: &Answer,
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
) -> Result<(), Failure> {
    let open: Vec<&Suggestion> = answer
        .suggestions
        .iter()
        .filter(|s| matches!(s.outcome, Outcome::Proposed { .. }))
        .collect();
    for suggestion in open {
        eprint!("{}? [y/N] ", capitalized(&suggestion.change.describe()));
        let _ = std::io::stderr().flush();
        let Some(line) = lines.next() else { break };
        if !matches!(line?.trim().to_lowercase().as_str(), "y" | "yes") {
            continue;
        }
        let identity = Identity::load_or_create()?;
        let lease = DiskLock::for_archive(archive).acquire(
            holder(&identity),
            "librarian",
            DEFAULT_LEASE,
        )?;
        let made = changes::apply(archive, registry, &answer.model, &suggestion.change, &lease);
        lease.release()?;
        match made {
            Ok(Some(commit)) => eprintln!(
                "Done; undo it with `carrel undo {}`",
                &commit[..commit.len().min(10)]
            ),
            Ok(None) => eprintln!("Nothing needed changing."),
            Err(why) => eprintln!("Couldn't: {why}"),
        }
    }
    Ok(())
}

/// The question the librarian asks after `answer`, now and then, about an item it came up
/// with ([`Conversing`]), added to the queue.
fn question_after(
    archive: &Archive,
    conversing: &mut Conversing,
    answer: &Answer,
) -> Option<QuestionEntry> {
    let items: Vec<_> = answer.items.iter().map(|i| i.id).collect();
    let question = conversing
        .after(archive, &items, crate::discover::today())
        .ok()
        .flatten()?;
    moments::ask(archive, &question).ok()
}

/// The librarian's question, for answering later with `carrel questions answer`.
fn question_later(question: &QuestionEntry) -> String {
    format!(
        "\nWhile we're here, about “{}”:\n{}Answer with `carrel questions answer {}`.\n",
        question.subject,
        crate::asking::question_text(question, "  "),
        carrel_core::review::short_id(question.id)
    )
}

/// Asks the librarian's question in a conversation at the terminal; an answer goes through
/// the corrections path, as `carrel questions answer` does, and an empty line leaves it for
/// later.
fn answer_now(
    archive: &Archive,
    question: &QuestionEntry,
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
) -> Result<(), Failure> {
    eprint!(
        "\nWhile we're here, about “{}”:\n{}Your answer (a number picks one; Enter leaves it for later): ",
        question.subject,
        crate::asking::question_text(question, "  ")
    );
    let _ = std::io::stderr().flush();
    let Some(line) = lines.next() else {
        return Ok(());
    };
    let line = line?;
    if line.trim().is_empty() {
        eprintln!("It waits in Questions.");
        return Ok(());
    }
    let options = crate::questions::Options {
        ask: true,
        ..crate::questions::Options::default()
    };
    match crate::questions::answer(
        Some(archive.root()),
        &question.id.to_string(),
        &[line.trim().to_owned()],
        options,
    ) {
        Ok(report) => eprint!("{}", report.text),
        Err(Failure(e)) => eprintln!("Couldn't answer it: {e}"),
    }
    Ok(())
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// An answer, for people: the answer, the items and passages, the suggestions and what became
/// of them, and the model that gave it.
pub fn describe(answer: &Answer) -> String {
    let mut text = format!("{}\n", answer.text.trim());
    if !answer.items.is_empty() {
        text.push_str("\nItems:\n");
        for item in &answer.items {
            writeln!(
                text,
                "  {}  {}{}",
                item.card,
                item.title,
                crate::related::mark(item.sensitivity)
            )
            .unwrap();
        }
    }
    if !answer.passages.is_empty() {
        text.push_str("\nPassages:\n");
        for quote in &answer.passages {
            writeln!(text, "  “{}” ({})", quote.text, quote.item.title).unwrap();
        }
    }
    if !answer.suggestions.is_empty() {
        text.push_str("\nSuggestions:\n");
        for suggestion in &answer.suggestions {
            text.push_str(&suggestion_lines(suggestion));
        }
    }
    let stopped = match answer.stopped {
        Some(Stopped::Steps) => "; out of steps",
        Some(Stopped::Tokens) => "; out of room",
        None => "",
    };
    writeln!(
        text,
        "\n({} · {}{stopped})",
        answer.model,
        plural(answer.steps.len(), "step")
    )
    .unwrap();
    text
}

fn suggestion_lines(suggestion: &Suggestion) -> String {
    let mut text = format!(
        "  {} ({:.0}% sure){}\n",
        capitalized(&suggestion.change.describe()),
        suggestion.confidence * 100.0,
        if suggestion.why.is_empty() {
            String::new()
        } else {
            format!(": {}", suggestion.why)
        }
    );
    match &suggestion.outcome {
        Outcome::Made { commit } => writeln!(
            text,
            "    The librarian made it{}",
            commit
                .as_deref()
                .map(|c| format!("; undo it with `carrel undo {}`", &c[..c.len().min(10)]))
                .unwrap_or_default()
        ),
        Outcome::Proposed { reason } => writeln!(
            text,
            "    For you to confirm ({reason}): {}",
            suggestion.change.command()
        ),
        Outcome::Refused { reason } => writeln!(text, "    Not made: {reason}"),
        Outcome::Failed { reason } => writeln!(text, "    It couldn't be made: {reason}"),
    }
    .unwrap();
    text
}

/// An answer, for programs.
pub fn answer_json(answer: &Answer) -> Value {
    let mut value = serde_json::to_value(answer).unwrap_or(Value::Null);
    if let (Some(suggestions), Value::Object(map)) = (Some(&answer.suggestions), &mut value) {
        map.insert(
            "suggestions".into(),
            Value::Array(
                suggestions
                    .iter()
                    .map(|s| {
                        let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
                        v["command"] = json!(s.change.command());
                        v
                    })
                    .collect(),
            ),
        );
    }
    value
}

/// The part of `carrel status` about the librarian's changes this week, for people and for
/// programs. Empty when it made none.
pub fn status_section(history: Option<&[Entry]>) -> (String, Value) {
    let since = jiff::Timestamp::now().as_second() - RECENT_DAYS * 86_400;
    let recent = history
        .map(|log| changes::recent(log, since))
        .unwrap_or_default();
    let value = json!({
        "since_days": RECENT_DAYS,
        "changes": recent.iter().map(crate::commands::entry_json).collect::<Vec<_>>(),
    });
    if recent.is_empty() {
        return (String::new(), value);
    }
    let mut text = format!(
        "\nThe librarian's changes this week ({}; undo them all with `carrel undo --librarian {RECENT_DAYS}d`):\n",
        plural(recent.len(), "change")
    );
    for entry in recent.iter().take(5) {
        writeln!(text, "  {}", log_line(entry)).unwrap();
    }
    if recent.len() > 5 {
        writeln!(
            text,
            "  and {} more: `carrel log --librarian`",
            recent.len() - 5
        )
        .unwrap();
    }
    (text, value)
}

/// `carrel undo --librarian SINCE`: undoes every change the librarian made on its own since
/// then, newest first.
///
/// # Errors
///
/// [`Failure`] if `since` can't be read, or the history can't be.
pub fn undo(archive: Option<&Path>, since: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let at = changes::parse_since(since, &jiff::Zoned::now()).ok_or_else(|| {
        Failure::new(format!(
            "{since} isn't a time; give a span such as 24h or 7d, `today`, or a date such as 2026-11-01"
        ))
    })?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "undo", DEFAULT_LEASE)?;
    let result = changes::undo_since(&archive, at, &lease);
    lease.release()?;
    let done = result?;
    let mut text = if done.undone.is_empty() && done.failed.is_empty() {
        "The librarian made no changes since then that are still there.\n".to_owned()
    } else {
        format!(
            "Undid {} the librarian made:\n",
            plural(done.undone.len(), "change")
        )
    };
    for (entry, _) in &done.undone {
        writeln!(text, "  {}", entry.summary).unwrap();
    }
    if !done.failed.is_empty() {
        writeln!(
            text,
            "Couldn't undo {}:",
            plural(done.failed.len(), "change")
        )
        .unwrap();
        for (entry, why) in &done.failed {
            writeln!(text, "  {}: {why}", entry.summary).unwrap();
        }
    }
    let value = json!({
        "undone": done.undone.iter().map(|(e, commit)| json!({"change": e.id, "summary": e.summary, "commit": commit})).collect::<Vec<_>>(),
        "failed": done.failed.iter().map(|(e, why)| json!({"change": e.id, "summary": e.summary, "error": why})).collect::<Vec<_>>(),
    });
    if !done.failed.is_empty() && done.undone.is_empty() {
        return Err(Failure::new(text.trim_end().to_owned()));
    }
    Ok(Report::new(text, value))
}
