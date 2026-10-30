//! `carrel questions`: what ingest needs the owner to explain, most items first.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock};
use carrel_core::history::{Change, History, Operation};
use carrel_core::ingest::IntakeOptions;
use carrel_core::questions::{
    QuestionEntry, QuestionKind, QuestionQueue, add_fact, clear_waiting, redirect,
};
use carrel_core::review::{ReviewEntry, ReviewQueue, short_id};
use serde_json::{Value, json};

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// `carrel questions` or `carrel questions list`.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (mut questions, _) = QuestionQueue::of(&archive).list()?;
    questions.sort_by_key(|q| std::cmp::Reverse(q.items.len()));
    let mut text = String::new();
    if questions.is_empty() {
        text.push_str("No questions\n");
    }
    for question in &questions {
        writeln!(
            text,
            "{}  {} ({} item{})",
            short_id(question.id),
            question.question,
            question.items.len(),
            if question.items.len() == 1 { "" } else { "s" }
        )
        .unwrap();
        if !question.choices.is_empty() {
            writeln!(text, "          choices: {}", question.choices.join(", ")).unwrap();
        }
    }
    Ok(Report::new(
        text,
        serde_json::to_value(&questions).unwrap_or(Value::Null),
    ))
}

/// `carrel questions answer ID ANSWER...`. A term's answer becomes a glossary fact and the
/// items waiting on it are drafted again; a collection's answer redirects its items there.
pub fn answer(archive: Option<&Path>, id: &str, words: &[String]) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let queue = QuestionQueue::of(&archive);
    let question = queue.find(id)?;
    let answer = words.join(" ");
    let answer = answer.trim();
    if answer.is_empty() {
        return Err(Failure::new("give an answer"));
    }
    let (mut text, json) = match question.kind {
        QuestionKind::Term => teach(&archive, &question, answer)?,
        QuestionKind::Collection => place(&archive, &question, answer)?,
    };
    queue.remove(question.id)?;
    if question.kind == QuestionKind::Term {
        text.push_str(&redraft(&archive, &question)?);
    }
    Ok(Report::new(text, json))
}

/// `carrel questions dismiss ID`: the items stop waiting on the question.
pub fn dismiss(archive: Option<&Path>, id: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let queue = QuestionQueue::of(&archive);
    let question = queue.find(id)?;
    for mut entry in waiting(&archive, &question)? {
        clear_waiting(&mut entry, &question);
        ReviewQueue::of(&archive).save(&entry)?;
    }
    queue.remove(question.id)?;
    Ok(Report::new(
        format!(
            "Dismissed {}: {}\n",
            short_id(question.id),
            question.question
        ),
        json!({"dismissed": question.id}),
    ))
}

/// The proposals still waiting on `question`.
fn waiting(archive: &Archive, question: &QuestionEntry) -> Result<Vec<ReviewEntry>, Failure> {
    let (entries, _) = ReviewQueue::of(archive).list()?;
    Ok(entries
        .into_iter()
        .filter(|e| question.items.contains(&e.id) && e.state.is_open())
        .collect())
}

/// Adds the answer to the glossary, as one history commit.
fn teach(
    archive: &Archive,
    question: &QuestionEntry,
    fact: &str,
) -> Result<(String, Value), Failure> {
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(archive).acquire(holder(&identity), "glossary", DEFAULT_LEASE)?;
    let result = (|| -> Result<Option<String>, Failure> {
        add_fact(archive, &question.subject, fact)
            .map_err(|e| Failure::io(&archive.root().join(carrel_core::questions::GLOSSARY), &e))?;
        let change = Change::new(
            Operation::Edit,
            format!("Add “{}” to the glossary", question.subject),
            identity.person,
            identity.device,
        );
        Ok(History::open(archive)?.commit(archive, &change, &lease)?)
    })();
    lease.release()?;
    let commit = result?;
    Ok((
        format!("Added “{}” to the glossary\n", question.subject),
        json!({"question": question.id, "glossary": question.subject, "commit": commit}),
    ))
}

/// Points every waiting proposal at the collection the owner named.
fn place(
    archive: &Archive,
    question: &QuestionEntry,
    path: &str,
) -> Result<(String, Value), Failure> {
    let collections = archive.collections()?;
    let wanted = path.trim_matches('/');
    let target = collections
        .iter()
        .find(|c| c.path.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            Failure::new(format!(
                "no collection {wanted}; choose one of {}",
                question.choices.join(", ")
            ))
        })?;
    let mut text = String::new();
    let mut moved = Vec::new();
    for mut entry in waiting(archive, question)? {
        clear_waiting(&mut entry, question);
        let dropped = redirect(
            archive,
            &collections,
            &mut entry,
            target,
            format!("you said items about “{}” belong here", question.subject),
        );
        ReviewQueue::of(archive).save(&entry)?;
        write!(text, "{} will go to {}", short_id(entry.id), target.path).unwrap();
        if !dropped.is_empty() {
            write!(text, " (tags {} left off)", dropped.join(", ")).unwrap();
        }
        text.push('\n');
        moved.push(entry.id);
    }
    Ok((
        text,
        json!({"question": question.id, "collection": target.path, "proposals": moved}),
    ))
}

/// Drafts the items waiting on a term question again, now that the glossary explains it.
/// Items on other devices, or no longer where they were, just stop waiting.
fn redraft(archive: &Archive, question: &QuestionEntry) -> Result<String, Failure> {
    let identity = Identity::load_or_create()?;
    let queue = ReviewQueue::of(archive);
    let mut paths: Vec<PathBuf> = Vec::new();
    for mut entry in waiting(archive, question)? {
        if entry.source.device == identity.device && entry.source.path.exists() {
            queue.remove(entry.id)?;
            paths.push(entry.source.path.clone());
        } else {
            clear_waiting(&mut entry, question);
            queue.save(&entry)?;
        }
    }
    if paths.is_empty() {
        return Ok(String::new());
    }
    let options = IntakeOptions {
        each: false,
        whole: true,
    };
    let report = crate::ingest::ingest(Some(archive.root()), &paths, options, false)?;
    Ok(format!("\nDrafted again:\n{}", report.text))
}
