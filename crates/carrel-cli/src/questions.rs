//! `carrel questions`: what ingest needs the owner to explain, most items first.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::actions::{self, Ingested};
use carrel_core::archive::Archive;
use carrel_core::backup::Backups;
use carrel_core::questions::{QuestionEntry, QuestionKind, QuestionQueue};
use carrel_core::review::short_id;
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
    let question = QuestionQueue::of(&archive).find(id)?;
    let answer = words.join(" ");
    let answer = answer.trim();
    if answer.is_empty() {
        return Err(Failure::new("give an answer"));
    }
    match question.kind {
        QuestionKind::Term => teach(&archive, &question, answer),
        QuestionKind::Collection => place(&archive, &question, answer),
    }
}

/// `carrel questions dismiss ID`: the items stop waiting on the question.
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

/// Adds the answer to the glossary, then drafts the items waiting on it again.
fn teach(archive: &Archive, question: &QuestionEntry, fact: &str) -> Result<Report, Failure> {
    let identity = Identity::load_or_create()?;
    let commit = actions::teach(archive, question, fact, holder(&identity))?;
    let mut text = format!("Added “{}” to the glossary\n", question.subject);
    let (registry, _) = crate::providers::registry();
    let (backups, _) = crate::backup::for_batch(archive);
    if let Some(Ingested { run, filed }) = actions::redraft(
        archive,
        &registry,
        question,
        holder(&identity),
        backups.as_ref().map(|b| b as &dyn Backups),
    )? {
        let report = crate::ingest::report(&run, filed.as_ref(), false);
        write!(text, "\nDrafted again:\n{}", report.text).unwrap();
    }
    Ok(Report::new(
        text,
        json!({"question": question.id, "glossary": question.subject, "commit": commit}),
    ))
}

/// Points every waiting proposal at the collection the owner named.
fn place(archive: &Archive, question: &QuestionEntry, path: &str) -> Result<Report, Failure> {
    let moved = actions::place(archive, question, path)?;
    let mut text = String::new();
    for m in &moved {
        write!(text, "{} will go to {}", short_id(m.entry.id), m.to).unwrap();
        if !m.dropped.is_empty() {
            write!(text, " (tags {} left off)", m.dropped.join(", ")).unwrap();
        }
        text.push('\n');
    }
    let to = moved
        .first()
        .map_or_else(|| path.trim_matches('/').to_owned(), |m| m.to.clone());
    Ok(Report::new(
        text,
        json!({
            "question": question.id,
            "collection": to,
            "proposals": moved.iter().map(|m| m.entry.id).collect::<Vec<_>>(),
        }),
    ))
}
