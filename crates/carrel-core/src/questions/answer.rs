//! Answering, skipping and raising questions (`docs/design/librarian.md`, "Questions").
//!
//! - A collection, group or sender question is answered with a collection: the proposals
//!   waiting on it go there, and its filed items are refiled there, each an undoable move. A
//!   collection that doesn't exist yet is created only when the owner picked or asked for a new
//!   one.
//! - An item question is answered through the corrections path ([`crate::correct::prepare`]):
//!   [`item_words`] turns the answer into the owner's words for it, and the caller shows the
//!   change and saves it on the owner's yes, then calls [`settled`].
//! - A "no change" answer, such as "it still matters", settles the question and is remembered,
//!   so it isn't asked again.
//! - Nothing here adds a glossary fact or a routing rule by itself: [`add_rule`] is called only
//!   once the owner has said yes to the rule.

use std::fmt::Write as _;

use crate::actions::{ActionError, Moved, collection, waiting};
use crate::archive::{Archive, Collection};
use crate::card::Id;
use crate::card_file::CardFile;
use crate::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use crate::history::{Change, History, Operation};
use crate::ingest::route::{RULES_FILE, RoutingRules};
use crate::review::ReviewQueue;

use super::rank::{Context, Ranked, rank, same};
use super::{
    QuestionEntry, QuestionKind, QuestionQueue, Suggestion, clear_waiting, mark_waiting, now,
    redirect,
};

/// What answering a question with a collection did.
#[derive(Debug, Clone)]
pub struct Placed {
    /// The collection's path.
    pub collection: String,
    /// The history commit creating it, when it was created for the answer.
    pub created: Option<Option<String>>,
    /// The proposals pointed there.
    pub moved: Vec<Moved>,
    /// The filed items refiled there: each one's title and history commit.
    pub refiled: Vec<(String, Option<String>)>,
    /// Filed items that couldn't be refiled, with why.
    pub not_refiled: Vec<String>,
}

/// Answers a collection, group or sender `question` with the collection at `path`, as
/// `holder`: the proposals waiting on it are pointed there and its filed items are refiled
/// there, each as an undoable move. When there's no such collection, it's created if `create`
/// (the owner picked a new collection, or said to make one), and refused otherwise. The
/// question leaves the queue.
///
/// # Errors
///
/// [`ActionError::Invalid`] for an unknown collection without `create`, or one that can't be
/// created; a lease, history or queue error.
pub fn place(
    archive: &Archive,
    question: &QuestionEntry,
    path: &str,
    create: bool,
    holder: Holder,
) -> Result<Placed, ActionError> {
    let path = path.trim().trim_matches('/');
    let mut collections = archive.collections()?;
    let mut created = None;
    if collection(&collections, path).is_err() {
        if !create {
            let mut known = question.choices.clone();
            known.extend(
                question
                    .suggestions
                    .iter()
                    .filter(|s| !s.no_change)
                    .map(|s| s.answer.clone()),
            );
            known.dedup();
            return Err(ActionError::Invalid(format!(
                "no collection {path}{}; to create it, answer again with --create",
                if known.is_empty() {
                    String::new()
                } else {
                    format!(" (choose one of {})", known.join(", "))
                }
            )));
        }
        let lease = DiskLock::for_archive(archive).acquire(holder, "collections", DEFAULT_LEASE)?;
        let result = crate::collections::create(
            archive,
            path,
            &crate::collections::NewCollection {
                description: None,
                tags: Vec::new(),
            },
            &lease,
        );
        lease.release()?;
        created = Some(
            result
                .map_err(|e| ActionError::Invalid(e.to_string()))?
                .commit,
        );
        collections = archive.collections()?;
    }
    let target = collection(&collections, path)?.clone();

    let queue = ReviewQueue::of(archive);
    let mut moved = Vec::new();
    for mut entry in waiting(archive, question)? {
        clear_waiting(&mut entry, question);
        let dropped = redirect(
            archive,
            &collections,
            &mut entry,
            &target,
            format!("you said items about “{}” belong here", question.subject),
        );
        queue.save(&entry)?;
        moved.push(Moved {
            entry,
            to: target.path.clone(),
            dropped,
        });
    }
    let (refiled, not_refiled) = refile_all(archive, question, &target, holder)?;
    QuestionQueue::of(archive).remove(question.id)?;
    crate::decisions::question(
        archive,
        question,
        crate::decisions::QuestionOutcome::Answered,
        moved.len() + refiled.len(),
    );
    Ok(Placed {
        collection: target.path,
        created,
        moved,
        refiled,
        not_refiled,
    })
}

/// The items refiled, each its title and history commit, and why others couldn't be.
type Refiles = (Vec<(String, Option<String>)>, Vec<String>);

/// Refiles `question`'s filed items into `target`, each as its own undoable move. Returns the
/// items refiled, with their commits, and why others couldn't be.
fn refile_all(
    archive: &Archive,
    question: &QuestionEntry,
    target: &Collection,
    holder: Holder,
) -> Result<Refiles, ActionError> {
    if question.about.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let items = archive.items()?;
    let mut refiled = Vec::new();
    let mut not = Vec::new();
    let lease = DiskLock::for_archive(archive).acquire(holder, "refile", DEFAULT_LEASE)?;
    for item in items.iter().filter(|i| question.about.contains(&i.id)) {
        let folder = item.card.rsplit_once('/').map_or("", |(f, _)| f);
        if folder == target.path {
            continue;
        }
        let path = archive.root().join(&item.card);
        let file = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| CardFile::parse(&t).map_err(|e| e.to_string()))
        {
            Ok(file) => file,
            Err(e) => {
                not.push(format!("{}: {e}", item.card));
                continue;
            }
        };
        let title = file.card().title.clone();
        let summary = format!(
            "Refile “{title}” to {}, as the owner answered about “{}”",
            target.path, question.subject
        );
        match crate::rename::refile(archive, item, &target.path, file, &summary, &lease) {
            Ok(done) => refiled.push((title, done.commit)),
            Err(e) => not.push(format!("“{title}”: {e}")),
        }
    }
    lease.release()?;
    Ok((refiled, not))
}

/// Settles `question` with an answer that changes nothing ("it still matters", "no, it doesn't
/// need its own collection"): its proposals stop waiting on it, and it's kept as declined, so
/// it's never asked again.
///
/// # Errors
///
/// A queue error.
pub fn decline(archive: &Archive, question: &QuestionEntry) -> Result<(), ActionError> {
    let queue = ReviewQueue::of(archive);
    for mut entry in waiting(archive, question)? {
        clear_waiting(&mut entry, question);
        queue.save(&entry)?;
    }
    let mut kept = question.clone();
    kept.declined = Some(now());
    QuestionQueue::of(archive).save(&kept)?;
    crate::decisions::question(
        archive,
        question,
        crate::decisions::QuestionOutcome::Declined,
        0,
    );
    Ok(())
}

/// Skips `question`: it rests out of the queue for the archive's `questions.skip_days`, and
/// its proposals keep waiting on it. Returns it as saved.
///
/// # Errors
///
/// A queue error.
pub fn skip(archive: &Archive, question: &QuestionEntry) -> Result<QuestionEntry, ActionError> {
    let mut skipped = question.clone();
    skipped.skipped = Some(now());
    skipped.skips += 1;
    QuestionQueue::of(archive).save(&skipped)?;
    crate::decisions::question(
        archive,
        question,
        crate::decisions::QuestionOutcome::Skipped,
        0,
    );
    Ok(skipped)
}

/// Takes `question` off the queue once it's been answered another way, such as an item
/// question answered by a correction: its proposals stop waiting on it.
///
/// # Errors
///
/// A queue error.
pub fn settled(archive: &Archive, question: &QuestionEntry) -> Result<(), ActionError> {
    crate::decisions::question(
        archive,
        question,
        crate::decisions::QuestionOutcome::Answered,
        question.items.len(),
    );
    crate::actions::dismiss(archive, question)
}

/// What answering a sender or project question does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderAnswer {
    /// No: it stays where it is, and the question isn't asked again ([`decline`]).
    Decline,
    /// Gather its items in the collection at `path`, creating it when `create`, then offer a
    /// routing rule for future ones ([`place`], [`add_rule`]).
    Place {
        /// The collection's path.
        path: String,
        /// Whether it's a new collection.
        create: bool,
    },
}

/// What `answer` to the sender `question` does, `picked` being the suggestion it picks, if
/// any: "no" declines; "yes" gathers its items in the collection the question proposes;
/// anything else is a collection's path, created when `create` or the suggestion is a new
/// collection.
///
/// # Errors
///
/// [`ActionError::Invalid`] for "yes" when the question proposes no collection.
pub fn sender_answer(
    question: &QuestionEntry,
    answer: &str,
    picked: Option<&Suggestion>,
    create: bool,
) -> Result<SenderAnswer, ActionError> {
    let lower = answer.trim().to_lowercase();
    if matches!(lower.as_str(), "no" | "n" | "no thanks") || picked.is_some_and(|s| s.no_change) {
        return Ok(SenderAnswer::Decline);
    }
    if matches!(lower.as_str(), "yes" | "y") {
        let path = question
            .suggestions
            .iter()
            .find(|s| s.new_collection)
            .map(|s| s.answer.clone())
            .or_else(|| question.choices.first().cloned())
            .ok_or_else(|| ActionError::Invalid("say which collection they go in".into()))?;
        return Ok(SenderAnswer::Place { path, create: true });
    }
    let path = picked.map_or_else(|| answer.trim().to_owned(), |s| s.answer.clone());
    Ok(SenderAnswer::Place {
        path,
        create: create || picked.is_some_and(|s| s.new_collection),
    })
}

/// What an item question is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A proposal in Review.
    Proposal(Id),
    /// A filed item.
    Filed(Id),
}

/// What the item `question` is about, if it's an item question.
#[must_use]
pub fn target(question: &QuestionEntry) -> Option<Target> {
    if question.kind != QuestionKind::Item {
        return None;
    }
    question
        .items
        .first()
        .map(|id| Target::Proposal(*id))
        .or_else(|| question.about.first().map(|id| Target::Filed(*id)))
}

/// The owner's words for a correction answering the item `question` with `answer`: the
/// question and the answer together, so the model knows what the answer is about. `field:
/// value` lines are taken as they are, applied without a model, and a suggested reading of a
/// proposal picks it ([`crate::ingest::readings`]).
#[must_use]
pub fn item_words(question: &QuestionEntry, answer: &str) -> String {
    if let Some(words) = crate::ingest::readings::answer_words(question, answer) {
        return words;
    }
    let answer = answer.trim();
    let lines = answer.lines().all(|l| {
        l.split_once(':').is_some_and(|(f, _)| {
            matches!(
                f.trim().to_lowercase().as_str(),
                "title" | "summary" | "created" | "date" | "tags" | "sensitivity" | "collection"
            )
        })
    });
    if lines {
        answer.to_owned()
    } else {
        format!("Asked “{}”, the owner said: {answer}", question.question)
    }
}

/// `subject` as a pattern on item names: its words in order, any case, separated by spaces,
/// dashes, underscores or dots.
#[must_use]
pub fn name_pattern(subject: &str) -> String {
    let words: Vec<String> = subject.split_whitespace().map(regex::escape).collect();
    format!("(?i){}", words.join("[ _.-]*"))
}

/// `text` as a YAML double-quoted string.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Adds a routing rule filing items whose names mention `subject` into `target`, at the end of
/// `_System/rules/routing.yaml`, as one history commit made as `holder`. Call it only once
/// the owner has said yes to the rule. Returns the commit.
///
/// # Errors
///
/// [`ActionError::Invalid`] if the rule wouldn't be read back; a lease, file or history error.
pub fn add_rule(
    archive: &Archive,
    subject: &str,
    target: &Collection,
    holder: Holder,
) -> Result<Option<String>, ActionError> {
    let path = archive.root().join(RULES_FILE);
    let io = |source| ActionError::Io {
        path: path.clone(),
        source,
    };
    let lease = DiskLock::for_archive(archive).acquire(holder, "rules", DEFAULT_LEASE)?;
    let result = (|| -> Result<Option<String>, ActionError> {
        let before = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(io(e)),
        };
        let collections = archive.collections()?;
        let had = RoutingRules::of(archive, &collections);
        let mut text = before.trim_end().to_owned();
        if text.is_empty() || text == "rules: []" {
            text = "rules:".into();
        }
        let name = format!("{subject} → {}", target.path);
        write!(
            text,
            "\n  - name: {}\n    match: {{ name: {} }}\n    collection: {}\n",
            quoted(&name),
            quoted(&name_pattern(subject)),
            target.doc.id
        )
        .expect("writing to a string");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&path, &text).map_err(io)?;
        let now = RoutingRules::of(archive, &collections);
        if now.rules.len() != had.rules.len() + 1 || now.problems.len() > had.problems.len() {
            std::fs::write(&path, &before).map_err(io)?;
            return Err(ActionError::Invalid(format!(
                "the rule for “{subject}” couldn't be read back ({}); {RULES_FILE} is as it was",
                now.problems.join("; ")
            )));
        }
        let change = Change::new(
            Operation::Edit,
            format!("Add the routing rule “{name}”"),
            holder.person,
            holder.device,
        );
        Ok(History::open(archive)?.commit(archive, &change, &lease)?)
    })();
    lease.release()?;
    result
}

/// Adds `asked` to the queue: each merges into a question already there that says the same,
/// or is saved as new, and proposals it newly names are marked as waiting on it. A question
/// the owner declined isn't asked again. Returns the questions saved, merged ones as merged.
///
/// # Errors
///
/// A queue error.
pub fn raise(
    archive: &Archive,
    asked: Vec<QuestionEntry>,
) -> Result<Vec<QuestionEntry>, ActionError> {
    let questions = QuestionQueue::of(archive);
    let reviews = ReviewQueue::of(archive);
    let (mut existing, _) = questions.list()?;
    let (mut open, _) = reviews.list()?;
    open.retain(|e| e.state.is_open());
    let mut saved: Vec<QuestionEntry> = Vec::new();
    for question in asked {
        let (merged, newly) = match existing.iter_mut().find(|e| same(e, &question)) {
            Some(earlier) if earlier.declined.is_some() => continue,
            Some(earlier) => {
                let newly: Vec<Id> = question
                    .items
                    .iter()
                    .filter(|id| !earlier.items.contains(id))
                    .copied()
                    .collect();
                let before = earlier.clone();
                super::rank::merge(earlier, &question);
                if *earlier == before {
                    continue;
                }
                (earlier.clone(), newly)
            }
            None => {
                existing.push(question.clone());
                let newly = question.items.clone();
                (question, newly)
            }
        };
        questions.save(&merged)?;
        // In place, so a proposal named by two questions keeps both marks.
        for entry in open.iter_mut().filter(|e| newly.contains(&e.id)) {
            mark_waiting(entry, std::slice::from_ref(&merged));
            reviews.save(entry)?;
        }
        saved.retain(|s| s.id != merged.id);
        saved.push(merged);
    }
    Ok(saved)
}

/// The queue ranked, after tidying it: questions the glossary or a rule already answers, or
/// that nothing waits on any more, leave it (their proposals stop waiting on them), and
/// near-duplicates are merged into one on disk. Skipped questions rest for the archive's
/// `questions.skip_days`.
///
/// # Errors
///
/// [`ActionError`] if a queue or the archive can't be read or written.
pub fn tidy(archive: &Archive) -> Result<Ranked, ActionError> {
    let questions = QuestionQueue::of(archive);
    let (asked, _) = questions.list()?;
    let context = Context::of(archive)?;
    let ranked = rank(
        asked,
        &context,
        jiff::Timestamp::now(),
        archive.settings().questions.skip_days,
    );
    let reviews = ReviewQueue::of(archive);
    let (mut open, _) = reviews.list()?;
    open.retain(|e| e.state.is_open());
    for (kept, folded) in &ranked.merged {
        questions.save(kept)?;
        for other in folded {
            questions.remove(other.id)?;
            for entry in open.iter_mut().filter(|e| other.items.contains(&e.id)) {
                clear_waiting(entry, other);
                if kept.declined.is_none() {
                    mark_waiting(entry, std::slice::from_ref(kept));
                }
                reviews.save(entry)?;
            }
        }
    }
    for (question, _) in &ranked.answered {
        for entry in open.iter_mut().filter(|e| question.items.contains(&e.id)) {
            clear_waiting(entry, question);
            reviews.save(entry)?;
        }
        questions.remove(question.id)?;
    }
    Ok(ranked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::questions::suggest;

    fn sender() -> QuestionEntry {
        let mut question = QuestionEntry::new(
            QuestionKind::Sender,
            "Northbridge Bank",
            "Should Northbridge Bank have its own collection?",
        );
        question.suggestions = vec![
            suggest::new_collection("Finances/Northbridge", "12 statements"),
            suggest::no_change("no", None),
        ];
        question.choices = vec!["Finances".into()];
        question
    }

    #[test]
    fn a_sender_answer_declines_or_names_where_its_items_go() {
        let question = sender();
        let answer = |said: &str, picked| sender_answer(&question, said, picked, false).unwrap();
        assert_eq!(answer("No", None), SenderAnswer::Decline);
        assert_eq!(
            answer("no", question.suggestions.get(1)),
            SenderAnswer::Decline
        );
        let gathered = SenderAnswer::Place {
            path: "Finances/Northbridge".into(),
            create: true,
        };
        assert_eq!(
            answer("yes", None),
            gathered,
            "yes takes the proposed collection"
        );
        assert_eq!(
            answer("Finances/Northbridge", question.suggestions.first()),
            gathered
        );
        assert_eq!(
            answer("Finances", None),
            SenderAnswer::Place {
                path: "Finances".into(),
                create: false
            }
        );
        let mut bare = sender();
        bare.suggestions.clear();
        bare.choices.clear();
        assert!(sender_answer(&bare, "yes", None, false).is_err());
    }
}
