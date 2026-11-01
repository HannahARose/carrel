//! Ranking the question queue by what an answer would settle (`docs/design/librarian.md`,
//! "Questions"): how many items an answer would place or improve, times how unsure Carrel is
//! about them now. Questions a rule or the glossary already answers are set aside, questions
//! that say the same thing merge, and skipped questions rest for a while.

use std::collections::{HashMap, HashSet};

use crate::archive::{Archive, ArchiveError, Collection};
use crate::card::Id;
use crate::ingest::card::Glossary;
use crate::ingest::route::{RoutingRules, Rule};
use crate::review::{Confidence, ReviewEntry, ReviewQueue};

use super::{QuestionEntry, QuestionKind};

/// How unsure Carrel is about a proposal it's this confident about, from 0 to 1.
#[must_use]
pub fn unsureness(confidence: Confidence) -> f64 {
    match confidence {
        Confidence::Low => 1.0,
        Confidence::Medium => 0.6,
        Confidence::High => 0.25,
    }
}

/// How unsure Carrel is about a filed item: its card was approved, but may still be the
/// model's guess.
pub const FILED_UNSURENESS: f64 = 0.5;

/// What ranking needs to know about the archive.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The open proposals, by identifier, with how sure their destination is.
    pub proposals: HashMap<Id, Confidence>,
    /// The filed items' identifiers.
    pub filed: HashSet<Id>,
    /// The glossary's terms, lowercased.
    pub glossary: Vec<String>,
    /// The owner's routing rules.
    pub rules: Vec<Rule>,
    /// The collections.
    pub collections: Vec<Collection>,
}

impl Context {
    /// Reads what ranking needs from `archive`.
    ///
    /// # Errors
    ///
    /// [`ArchiveError`] if the collections, items or review queue can't be read.
    pub fn of(archive: &Archive) -> Result<Self, ArchiveError> {
        let (entries, _) = ReviewQueue::of(archive)
            .list()
            .map_err(|e| ArchiveError::Invalid {
                path: archive.root().join(crate::review::REVIEW_DIR),
                message: e.to_string(),
            })?;
        let collections = archive.collections()?;
        let items = archive.items()?;
        Ok(Self::new(
            archive,
            &entries,
            items.iter().map(|i| i.id).collect(),
            collections,
        ))
    }

    /// The context from what the caller already read: `entries` from the review queue (open
    /// ones count), the filed items' identifiers, and the collections.
    #[must_use]
    pub fn new(
        archive: &Archive,
        entries: &[ReviewEntry],
        filed: HashSet<Id>,
        collections: Vec<Collection>,
    ) -> Self {
        let rules = RoutingRules::of(archive, &collections).rules;
        Self {
            proposals: entries
                .iter()
                .filter(|e| e.state.is_open())
                .map(|e| (e.id, e.destination.confidence))
                .collect(),
            filed,
            glossary: Glossary::of(archive)
                .facts
                .iter()
                .map(|(t, _)| t.to_lowercase())
                .collect(),
            rules,
            collections,
        }
    }

    /// Why `question` needs no answer any more, if it doesn't: the glossary or a rule answers
    /// it, a collection of its own exists, or nothing it's about is still there.
    #[must_use]
    pub fn answered(&self, question: &QuestionEntry) -> Option<String> {
        let subject = question.subject.trim();
        let lower = subject.to_lowercase();
        match question.kind {
            QuestionKind::Term if self.glossary.contains(&lower) => {
                return Some(format!("the glossary explains “{subject}”"));
            }
            QuestionKind::Collection | QuestionKind::Group | QuestionKind::Sender => {
                if let Some(rule) = self.rules.iter().find(|r| rule_covers(r, subject)) {
                    return Some(format!("the routing rule “{}” files them", rule.name));
                }
            }
            _ => {}
        }
        if question.kind == QuestionKind::Sender
            && let Some(own) = self.collections.iter().find(|c| {
                c.doc.title.eq_ignore_ascii_case(subject)
                    || c.path
                        .rsplit('/')
                        .next()
                        .is_some_and(|last| last.eq_ignore_ascii_case(subject))
            })
        {
            return Some(format!("“{subject}” has its own collection, {}", own.path));
        }
        if self.settles(question).0 == 0 {
            return Some("nothing it's about is waiting any more".into());
        }
        None
    }

    /// How many items an answer to `question` would place or improve, and how much that's
    /// worth: each item counts by how unsure Carrel is about it.
    #[must_use]
    pub fn settles(&self, question: &QuestionEntry) -> (usize, f64) {
        let mut count = 0;
        let mut value = 0.0;
        for id in &question.items {
            if let Some(confidence) = self.proposals.get(id) {
                count += 1;
                value += unsureness(*confidence);
            }
        }
        for id in &question.about {
            if self.filed.contains(id) {
                count += 1;
                value += FILED_UNSURENESS;
            }
        }
        (count, value)
    }
}

/// Whether the routing rule `rule` already files items about `subject`: its name mentions the
/// subject, or its name pattern matches it.
fn rule_covers(rule: &Rule, subject: &str) -> bool {
    if subject.is_empty() {
        return false;
    }
    if key(&rule.name).contains(&key(subject)) {
        return true;
    }
    let Some(pattern) = &rule.matches.name else {
        return false;
    };
    let Ok(regex) = regex::Regex::new(pattern) else {
        return false;
    };
    [
        subject.to_owned(),
        subject.replace(' ', "-"),
        subject.replace(' ', "_"),
    ]
    .iter()
    .any(|s| regex.is_match(s))
}

/// A question, weighed.
#[derive(Debug, Clone)]
pub struct Weighed {
    /// The question.
    pub question: QuestionEntry,
    /// How many items an answer would place or improve.
    pub settles: usize,
    /// What an answer is worth: those items, each counted by how unsure Carrel is about it.
    pub value: f64,
}

/// The queue, ranked.
#[derive(Debug, Clone, Default)]
pub struct Ranked {
    /// The questions to ask, the most worth answering first.
    pub open: Vec<Weighed>,
    /// Questions resting after a skip, the soonest back first.
    pub resting: Vec<Weighed>,
    /// Questions needing no answer any more, with why.
    pub answered: Vec<(QuestionEntry, String)>,
    /// Questions merged into another that says the same: the one kept, and those folded in.
    pub merged: Vec<(QuestionEntry, Vec<QuestionEntry>)>,
    /// How many questions the owner said no to, which aren't asked again.
    pub declined: usize,
}

/// Ranks `questions` with what `context` knows, at `now`, for an archive whose skipped
/// questions rest `rest_days`.
#[must_use]
pub fn rank(
    questions: Vec<QuestionEntry>,
    context: &Context,
    now: jiff::Timestamp,
    rest_days: u32,
) -> Ranked {
    let (kept, merged) = merge_all(questions);
    let mut ranked = Ranked {
        merged,
        ..Ranked::default()
    };
    for question in kept {
        if question.declined.is_some() {
            ranked.declined += 1;
            continue;
        }
        if let Some(why) = context.answered(&question) {
            ranked.answered.push((question, why));
            continue;
        }
        let (settles, value) = context.settles(&question);
        let resting = question.is_resting(now, rest_days);
        let weighed = Weighed {
            question,
            settles,
            value,
        };
        if resting {
            ranked.resting.push(weighed);
        } else {
            ranked.open.push(weighed);
        }
    }
    ranked.open.sort_by(|a, b| {
        b.value
            .total_cmp(&a.value)
            .then(b.settles.cmp(&a.settles))
            .then(a.question.id.cmp(&b.question.id))
    });
    ranked
        .resting
        .sort_by_key(|w| (w.question.rests_until(rest_days), w.question.id));
    ranked
}

/// `questions` with near-duplicates merged into the oldest of each, and what was merged.
#[must_use]
pub fn merge_all(
    mut questions: Vec<QuestionEntry>,
) -> (Vec<QuestionEntry>, Vec<(QuestionEntry, Vec<QuestionEntry>)>) {
    questions.sort_by_key(|q| q.id);
    let mut kept: Vec<QuestionEntry> = Vec::new();
    let mut folded: Vec<Vec<QuestionEntry>> = Vec::new();
    for question in questions {
        if let Some(i) = kept.iter().position(|k| same(k, &question)) {
            merge(&mut kept[i], &question);
            folded[i].push(question);
        } else {
            kept.push(question);
            folded.push(Vec::new());
        }
    }
    let merged = kept
        .iter()
        .zip(folded)
        .filter(|(_, f)| !f.is_empty())
        .map(|(k, f)| (k.clone(), f))
        .collect();
    (kept, merged)
}

/// Kinds whose questions can say the same thing: "which collection for these" is asked by
/// collection and group questions alike.
fn family(kind: QuestionKind) -> u8 {
    match kind {
        QuestionKind::Term => 0,
        QuestionKind::Collection | QuestionKind::Group => 1,
        QuestionKind::Sender => 2,
        QuestionKind::Item => 3,
    }
}

/// `text` reduced to what tells subjects apart: lowercase letters and digits, without a
/// plural or possessive `s`.
fn key(text: &str) -> String {
    let text = text.trim_end_matches("'s").trim_end_matches("’s");
    let mut key: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if key.len() > 4 && key.ends_with('s') && !key.ends_with("ss") {
        key.pop();
    }
    key
}

/// How many single-character edits turn `a` into `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitute = previous + usize::from(ca != *cb);
            previous = row[j + 1];
            row[j + 1] = substitute.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

/// Whether `a` and `b` are near-duplicates: the same kind of question about the same subject,
/// written alike (`scope-ds`, `Scope DS`, `scope-ds's`, or one letter apart in a long word),
/// or, for item questions, the same question about the same item.
#[must_use]
pub fn same(a: &QuestionEntry, b: &QuestionEntry) -> bool {
    if family(a.kind) != family(b.kind) {
        return false;
    }
    if a.kind == QuestionKind::Item {
        let target = |q: &QuestionEntry| q.items.first().or(q.about.first()).copied();
        return target(a) == target(b) && key(&a.question) == key(&b.question);
    }
    let (ka, kb) = (key(&a.subject), key(&b.subject));
    ka == kb || (ka.len().min(kb.len()) >= 6 && distance(&ka, &kb) <= 1)
}

/// Folds `other` into `into`: the items, choices and suggestions of both, and the later skip.
pub fn merge(into: &mut QuestionEntry, other: &QuestionEntry) {
    for id in &other.items {
        if !into.items.contains(id) {
            into.items.push(*id);
        }
    }
    for id in &other.about {
        if !into.about.contains(id) {
            into.about.push(*id);
        }
    }
    for choice in &other.choices {
        if !into.choices.contains(choice) {
            into.choices.push(choice.clone());
        }
    }
    for suggestion in &other.suggestions {
        if !into
            .suggestions
            .iter()
            .any(|s| s.answer.eq_ignore_ascii_case(&suggestion.answer))
        {
            into.suggestions.push(suggestion.clone());
        }
    }
    if other.skipped > into.skipped {
        into.skipped.clone_from(&other.skipped);
    }
    into.skips = into.skips.max(other.skips);
    if into.declined.is_none() {
        into.declined.clone_from(&other.declined);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(kind: QuestionKind, subject: &str) -> QuestionEntry {
        QuestionEntry::new(kind, subject, format!("About {subject}?"))
    }

    #[test]
    fn near_duplicates_are_the_same_kind_about_the_same_subject() {
        let a = question(QuestionKind::Term, "scope-ds");
        assert!(same(&a, &question(QuestionKind::Term, "Scope DS")));
        assert!(same(&a, &question(QuestionKind::Term, "scope-ds's")));
        assert!(!same(&a, &question(QuestionKind::Collection, "scope-ds")));
        assert!(same(
            &question(QuestionKind::Collection, "receipts"),
            &question(QuestionKind::Group, "Receipt")
        ));
        assert!(same(
            &question(QuestionKind::Sender, "Northbridge Bank"),
            &question(QuestionKind::Sender, "Northbrige Bank")
        ));
        assert!(!same(
            &question(QuestionKind::Term, "ABX"),
            &question(QuestionKind::Term, "ABY")
        ));
    }

    #[test]
    fn merging_keeps_the_oldest_with_everything_both_wait_on() {
        let mut a = question(QuestionKind::Term, "scope-ds");
        a.items = vec![Id::new_v7()];
        let mut b = question(QuestionKind::Term, "Scope DS");
        b.items = vec![a.items[0], Id::new_v7()];
        b.skips = 2;
        let (kept, merged) = merge_all(vec![b.clone(), a.clone()]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, a.id);
        assert_eq!(kept[0].items, [a.items[0], b.items[1]]);
        assert_eq!(kept[0].skips, 2);
        assert_eq!(merged[0].1[0].id, b.id);
    }

    #[test]
    fn the_most_worth_answering_comes_first_and_skipped_ones_rest() {
        let (low, high) = (Id::new_v7(), Id::new_v7());
        let mut context = Context::default();
        context.proposals.insert(low, Confidence::Low);
        context.proposals.insert(high, Confidence::High);
        let mut sure = question(QuestionKind::Term, "ABX");
        sure.items = vec![high];
        let mut unsure = question(QuestionKind::Term, "CellTrack");
        unsure.items = vec![low];
        let mut skipped = question(QuestionKind::Term, "Cog2");
        skipped.items = vec![low, high];
        skipped.skipped = Some(crate::card::DateOrTime("2026-10-30T09:00:00Z".into()));
        let mut gone = question(QuestionKind::Term, "scope-ds");
        gone.items = vec![Id::new_v7()];
        context.glossary.push("abx".into());
        let now: jiff::Timestamp = "2026-11-01T09:00:00Z".parse().unwrap();

        let ranked = rank(
            vec![sure.clone(), unsure.clone(), skipped.clone(), gone.clone()],
            &context,
            now,
            14,
        );
        let open: Vec<&str> = ranked
            .open
            .iter()
            .map(|w| w.question.subject.as_str())
            .collect();
        assert_eq!(open, ["CellTrack"]);
        assert_eq!(ranked.resting[0].question.id, skipped.id);
        assert_eq!(ranked.answered.len(), 2, "the glossary answers ABX");

        let later: jiff::Timestamp = "2026-11-20T09:00:00Z".parse().unwrap();
        let ranked = rank(vec![unsure, skipped.clone()], &context, later, 14);
        assert_eq!(
            ranked.open[0].question.id, skipped.id,
            "back after resting, and it settles more"
        );
    }
}
