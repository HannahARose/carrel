//! `carrel report` (#330): a summary of this device's decision log for a period, with
//! recommendations, each naming the setting or command that acts on it. Like the log it holds
//! no titles, text, names or queries: collections are named by identifier, so it can be
//! shared, and [`UsageReport::text`] shows their paths only when asked, for the owner alone.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use serde::Serialize;

use super::record::{Decision, Event, QuestionOutcome, Record, Routed, SearchMode};
use crate::card::Id;
use crate::formats::Entry as Unread;
use crate::history::Entry as Change;
use crate::review::Confidence;

/// The report for one archive over a period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageReport {
    /// The period's start and end, RFC 3339.
    pub since: String,
    /// The period's end.
    pub until: String,
    /// How many records it's made from.
    pub records: usize,
    /// Each model's proposals and corrections, most used first.
    pub models: Vec<ModelUse>,
    /// Where items were moved out of and into, and rules that misfire.
    pub routing: Routing,
    /// Holding, low-confidence placements and the structure's signals.
    pub structure: Structure,
    /// Each kind of question, by how it ended.
    pub questions: Vec<Questions>,
    /// Each kind of search, by whether it found something that was opened.
    pub search: Vec<Searches>,
    /// How long ingest took.
    pub ingest: Ingest,
    /// Changes programs made on their own, such as the librarian's, and how many were undone.
    pub changes: Vec<Changes>,
    /// File types this device couldn't read (`unreadable.json`).
    pub unreadable: Vec<Unread>,
    /// What to tweak, most useful first.
    pub recommendations: Vec<Recommendation>,
}

/// How the owner took one model's proposals.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelUse {
    /// The model, as `provider/offer`, or `none` when no model drafted.
    pub model: String,
    /// Proposals decided.
    pub proposals: usize,
    /// Approved where they were going.
    pub accepted: usize,
    /// Moved to another collection, then approved or held.
    pub moved: usize,
    /// Sent to Holding without being moved.
    pub held: usize,
    /// Rejected.
    pub rejected: usize,
    /// `accepted` as a share of `proposals`.
    pub accepted_share: f64,
    /// Corrections of its fields, by field.
    pub corrected_fields: BTreeMap<String, usize>,
    /// Corrections of its cards, by the item's extension (or kind, for a folder).
    pub corrected_types: BTreeMap<String, usize>,
    /// Items it drafted.
    pub drafted: usize,
    /// The mean time it took to draft one, in seconds.
    pub seconds_per_item: f64,
}

/// Where routing went wrong.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Routing {
    /// Collections items were most often moved out of, by identifier, with how many.
    pub moved_out_of: Vec<Count>,
    /// Collections items were most often moved into.
    pub moved_into: Vec<Count>,
    /// Owner rules whose proposals were moved or rejected, by the collection they file into.
    pub rules: Vec<RuleUse>,
}

/// A collection and how many times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Count {
    /// The collection.
    pub collection: Id,
    /// How many.
    pub count: usize,
}

/// How one owner rule's proposals went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleUse {
    /// The collection the rule files into, which names it here.
    pub collection: Id,
    /// Proposals it chose that were decided.
    pub decided: usize,
    /// Those moved elsewhere.
    pub moved: usize,
    /// Those rejected.
    pub rejected: usize,
}

/// Holding and the structure's signals over the period.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Structure {
    /// Proposals decided whose routing was unsure.
    pub low_confidence: usize,
    /// Of them, those the owner moved.
    pub low_confidence_moved: usize,
    /// The signals as Status saw them, a day at a time.
    pub days: Vec<Day>,
}

/// The structure on one day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Day {
    /// The day, `YYYY-MM-DD` (UTC).
    pub day: String,
    /// Items in the archive.
    pub items: usize,
    /// Items in Holding.
    pub holding: usize,
    /// Items filed with low confidence, in collections that show it.
    pub low: usize,
    /// Crowded collections.
    pub crowded: usize,
    /// Collections with one item.
    pub single: usize,
    /// Tags spread over several top-level collections.
    pub spread_tags: usize,
}

/// How one kind of question ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Questions {
    /// The kind, such as `term`.
    pub kind: String,
    /// Answered, including "no change" answers.
    pub answered: usize,
    /// Skipped.
    pub skipped: usize,
    /// Dismissed.
    pub dismissed: usize,
    /// Each as a share of all three.
    pub answer_rate: f64,
    /// Skipped as a share.
    pub skip_rate: f64,
    /// Dismissed as a share.
    pub dismiss_rate: f64,
    /// Proposals and items answering moved or settled.
    pub items_improved: usize,
}

/// How one kind of search went.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Searches {
    /// `words`, `both` (words and meaning) or `filters`.
    pub mode: String,
    /// Searches.
    pub searches: usize,
    /// Those that found anything.
    pub found: usize,
    /// Those with a result opened.
    pub opened: usize,
    /// `opened` as a share of `searches`.
    pub opened_share: f64,
    /// Searches whose first opened result was the first result.
    pub first: usize,
    /// Within the first three.
    pub top_three: usize,
    /// The mean rank of the first result opened.
    pub mean_rank: f64,
}

/// How long ingest took.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Ingest {
    /// Items drafted.
    pub drafted: usize,
    /// The mean time an item waited to be drafted, in seconds.
    pub queued_seconds: f64,
    /// The mean time drafting took.
    pub draft_seconds: f64,
    /// The mean time from drafted to decided.
    pub review_seconds: f64,
}

/// The changes one program made on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Changes {
    /// The program, such as `librarian`.
    pub by: String,
    /// Changes it made.
    pub made: usize,
    /// Of them, those undone.
    pub undone: usize,
}

/// Something to tweak.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    /// What it's about: `model`, `collection`, `rule`, `file-type`, `questions`, `search`,
    /// `librarian`, `holding` or `speed`.
    pub about: String,
    /// What the evidence says.
    pub says: String,
    /// The setting or command that acts on it.
    pub act: String,
}

/// What the report is made from.
#[derive(Debug, Clone, Copy)]
pub struct Sources<'a> {
    /// The archive.
    pub archive: Id,
    /// The log's records, any archive's.
    pub records: &'a [Record],
    /// The archive's history, newest first, if it has one.
    pub history: &'a [Change],
    /// The device's unreadable types.
    pub unreadable: &'a [Unread],
    /// The period's start.
    pub since: jiff::Timestamp,
    /// Its end.
    pub until: jiff::Timestamp,
}

#[allow(clippy::cast_precision_loss)] // counts
fn share(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

#[allow(clippy::cast_precision_loss)] // counts and seconds
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

impl UsageReport {
    /// The report from `sources`.
    #[must_use]
    pub fn of(sources: &Sources<'_>) -> Self {
        let records: Vec<&Record> = sources
            .records
            .iter()
            .filter(|r| r.archive == sources.archive)
            .filter(|r| {
                r.time()
                    .is_some_and(|t| t >= sources.since && t <= sources.until)
            })
            .collect();
        let events: Vec<&Event> = records.iter().map(|r| &r.event).collect();
        let first_moves = first_moves(&events);
        let mut report = Self {
            since: sources.since.to_string(),
            until: sources.until.to_string(),
            records: records.len(),
            models: models(&events, &first_moves),
            routing: routing(&events, &first_moves),
            structure: structure(&records, &first_moves),
            questions: questions(&events),
            search: searches(&events),
            ingest: ingest(&events),
            changes: changes(sources),
            unreadable: sources.unreadable.to_vec(),
            recommendations: Vec::new(),
        };
        report.recommendations = super::advice::recommend(&report);
        report
    }
}

/// Each proposal's first move: where routing had sent it.
fn first_moves(events: &[&Event]) -> HashMap<Id, Routed> {
    let mut moves = HashMap::new();
    for event in events {
        if let Event::Moved { item, from, .. } = event {
            moves.entry(*item).or_insert(*from);
        }
    }
    moves
}

/// The last decision on each proposal, in order.
fn decisions<'a>(events: &[&'a Event]) -> Vec<&'a Event> {
    let mut last: HashMap<Id, usize> = HashMap::new();
    let decided: Vec<&Event> = events
        .iter()
        .copied()
        .filter(|e| matches!(e, Event::Decided { .. }))
        .collect();
    for (n, event) in decided.iter().enumerate() {
        if let Event::Decided { item, .. } = event {
            last.insert(*item, n);
        }
    }
    let kept: HashSet<usize> = last.into_values().collect();
    decided
        .into_iter()
        .enumerate()
        .filter(|(n, _)| kept.contains(n))
        .map(|(_, e)| e)
        .collect()
}

fn name_of(model: Option<&String>) -> String {
    model.cloned().unwrap_or_else(|| "none".into())
}

fn models(events: &[&Event], moves: &HashMap<Id, Routed>) -> Vec<ModelUse> {
    let mut by: BTreeMap<String, ModelUse> = BTreeMap::new();
    let mut draft_times: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for event in decisions(events) {
        let Event::Decided {
            item,
            decision,
            model,
            ..
        } = event
        else {
            continue;
        };
        let use_ = model_use(&mut by, name_of(model.as_ref()));
        use_.proposals += 1;
        match (decision, moves.contains_key(item)) {
            (Decision::Rejected, _) => use_.rejected += 1,
            (_, true) => use_.moved += 1,
            (Decision::Approved, false) => use_.accepted += 1,
            (Decision::Held, false) => use_.held += 1,
        }
    }
    for event in events {
        match event {
            Event::Corrected {
                model,
                fields,
                extension,
                kind,
                ..
            } => {
                let use_ = model_use(&mut by, name_of(model.as_ref()));
                for field in fields {
                    *use_.corrected_fields.entry(field.clone()).or_default() += 1;
                }
                *use_
                    .corrected_types
                    .entry(type_of(extension, *kind))
                    .or_default() += 1;
            }
            Event::Drafted {
                model, draft_ms, ..
            } => {
                let name = name_of(model.as_ref());
                model_use(&mut by, name.clone()).drafted += 1;
                #[allow(clippy::cast_precision_loss)] // milliseconds
                draft_times
                    .entry(name)
                    .or_default()
                    .push(*draft_ms as f64 / 1000.0);
            }
            _ => {}
        }
    }
    let mut models: Vec<ModelUse> = by
        .into_values()
        .map(|mut m| {
            m.accepted_share = share(m.accepted, m.proposals);
            m.seconds_per_item = mean(draft_times.get(&m.model).map_or(&[], Vec::as_slice));
            m
        })
        .collect();
    models.sort_by(|a, b| {
        (b.proposals + b.drafted)
            .cmp(&(a.proposals + a.drafted))
            .then(a.model.cmp(&b.model))
    });
    models
}

fn model_use(by: &mut BTreeMap<String, ModelUse>, model: String) -> &mut ModelUse {
    by.entry(model.clone()).or_insert_with(|| ModelUse {
        model,
        proposals: 0,
        accepted: 0,
        moved: 0,
        held: 0,
        rejected: 0,
        accepted_share: 0.0,
        corrected_fields: BTreeMap::new(),
        corrected_types: BTreeMap::new(),
        drafted: 0,
        seconds_per_item: 0.0,
    })
}

/// The kind of file an item is, for counting: its extension, or its kind when it has none.
fn type_of(extension: &str, kind: crate::card::Kind) -> String {
    if extension.is_empty() {
        serde_json::to_value(kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    } else {
        format!(".{extension}")
    }
}

fn counted(counts: HashMap<Id, usize>) -> Vec<Count> {
    let mut list: Vec<Count> = counts
        .into_iter()
        .map(|(collection, count)| Count { collection, count })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then(a.collection.cmp(&b.collection)));
    list
}

fn routing(events: &[&Event], moves: &HashMap<Id, Routed>) -> Routing {
    let mut out_of: HashMap<Id, usize> = HashMap::new();
    let mut into: HashMap<Id, usize> = HashMap::new();
    let mut last_to: HashMap<Id, Id> = HashMap::new();
    for event in events {
        if let Event::Moved { item, to, .. } = event {
            last_to.insert(*item, *to);
        }
    }
    for (item, from) in moves {
        if last_to.get(item) != Some(&from.collection) {
            *out_of.entry(from.collection).or_default() += 1;
            if let Some(to) = last_to.get(item) {
                *into.entry(*to).or_default() += 1;
            }
        }
    }
    let mut rules: BTreeMap<Id, RuleUse> = BTreeMap::new();
    for event in decisions(events) {
        let Event::Decided {
            item, decision, to, ..
        } = event
        else {
            continue;
        };
        let routed = moves.get(item).unwrap_or(to);
        if !routed.rule {
            continue;
        }
        let rule = rules.entry(routed.collection).or_insert(RuleUse {
            collection: routed.collection,
            decided: 0,
            moved: 0,
            rejected: 0,
        });
        rule.decided += 1;
        if *decision == Decision::Rejected {
            rule.rejected += 1;
        } else if moves.contains_key(item) {
            rule.moved += 1;
        }
    }
    let mut rules: Vec<RuleUse> = rules
        .into_values()
        .filter(|r| r.moved + r.rejected > 0)
        .collect();
    rules.sort_by_key(|r| std::cmp::Reverse(r.moved + r.rejected));
    Routing {
        moved_out_of: counted(out_of),
        moved_into: counted(into),
        rules,
    }
}

fn structure(records: &[&Record], moves: &HashMap<Id, Routed>) -> Structure {
    let events: Vec<&Event> = records.iter().map(|r| &r.event).collect();
    let mut structure = Structure::default();
    for event in decisions(&events) {
        if let Event::Decided { item, to, .. } = event {
            let routed = moves.get(item).unwrap_or(to);
            if routed.confidence == Confidence::Low {
                structure.low_confidence += 1;
                if moves.contains_key(item) {
                    structure.low_confidence_moved += 1;
                }
            }
        }
    }
    let mut days: BTreeMap<String, Day> = BTreeMap::new();
    for record in records {
        if let Event::Snapshot {
            items,
            holding,
            low,
            crowded,
            single,
            spread_tags,
            ..
        } = &record.event
        {
            let day = record.at.get(..10).unwrap_or(&record.at).to_owned();
            days.insert(
                day.clone(),
                Day {
                    day,
                    items: *items,
                    holding: *holding,
                    low: *low,
                    crowded: *crowded,
                    single: *single,
                    spread_tags: *spread_tags,
                },
            );
        }
    }
    structure.days = days.into_values().collect();
    structure
}

fn questions(events: &[&Event]) -> Vec<Questions> {
    // (answered, skipped, dismissed, items) by kind. A question settled by another answer,
    // such as a correction, is dismissed as it's settled: the answer is what counts.
    let mut by: BTreeMap<&'static str, (usize, usize, usize, usize)> = BTreeMap::new();
    let mut ended: BTreeMap<Id, (&'static str, QuestionOutcome, usize)> = BTreeMap::new();
    for event in events {
        let Event::Question {
            question,
            kind,
            outcome,
            items,
        } = event
        else {
            continue;
        };
        if *outcome == QuestionOutcome::Skipped {
            by.entry(kind.as_str()).or_default().1 += 1;
            continue;
        }
        let end = ended
            .entry(*question)
            .or_insert((kind.as_str(), *outcome, 0));
        if end.1 == QuestionOutcome::Dismissed {
            end.1 = *outcome;
        }
        end.2 += items;
    }
    for (kind, outcome, items) in ended.into_values() {
        let counts = by.entry(kind).or_default();
        if outcome == QuestionOutcome::Dismissed {
            counts.2 += 1;
        } else {
            counts.0 += 1;
        }
        counts.3 += items;
    }
    by.into_iter()
        .map(|(kind, (answered, skipped, dismissed, items))| {
            let all = answered + skipped + dismissed;
            Questions {
                kind: kind.to_owned(),
                answered,
                skipped,
                dismissed,
                answer_rate: share(answered, all),
                skip_rate: share(skipped, all),
                dismiss_rate: share(dismissed, all),
                items_improved: items,
            }
        })
        .collect()
}

fn searches(events: &[&Event]) -> Vec<Searches> {
    let mut first_open: HashMap<Id, usize> = HashMap::new();
    for event in events {
        if let Event::Opened { search, rank } = event {
            first_open.entry(*search).or_insert(*rank);
        }
    }
    let mut by: BTreeMap<SearchMode, (usize, usize, Vec<usize>)> = BTreeMap::new();
    for event in events {
        if let Event::Searched {
            search,
            mode,
            results,
        } = event
        {
            let counts = by.entry(*mode).or_default();
            counts.0 += 1;
            if *results > 0 {
                counts.1 += 1;
            }
            if let Some(rank) = first_open.get(search) {
                counts.2.push(*rank);
            }
        }
    }
    by.into_iter()
        .map(|(mode, (searches, found, ranks))| {
            #[allow(clippy::cast_precision_loss)] // ranks
            let as_f64: Vec<f64> = ranks.iter().map(|r| *r as f64).collect();
            Searches {
                mode: mode.as_str().to_owned(),
                searches,
                found,
                opened: ranks.len(),
                opened_share: share(ranks.len(), searches),
                first: ranks.iter().filter(|r| **r == 1).count(),
                top_three: ranks.iter().filter(|r| **r <= 3).count(),
                mean_rank: mean(&as_f64),
            }
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)] // seconds
fn ingest(events: &[&Event]) -> Ingest {
    let mut queued = Vec::new();
    let mut drafting = Vec::new();
    for event in events {
        if let Event::Drafted {
            queued_secs,
            draft_ms,
            ..
        } = event
        {
            queued.push(*queued_secs as f64);
            drafting.push(*draft_ms as f64 / 1000.0);
        }
    }
    let review: Vec<f64> = decisions(events)
        .into_iter()
        .filter_map(|e| match e {
            Event::Decided { waited_secs, .. } => Some(*waited_secs as f64),
            _ => None,
        })
        .collect();
    Ingest {
        drafted: queued.len(),
        queued_seconds: mean(&queued),
        draft_seconds: mean(&drafting),
        review_seconds: mean(&review),
    }
}

fn changes(sources: &Sources<'_>) -> Vec<Changes> {
    let undone: HashSet<&str> = sources
        .history
        .iter()
        .filter_map(|e| e.reverts.as_deref())
        .collect();
    let (since, until) = (sources.since.as_second(), sources.until.as_second());
    let mut by: BTreeMap<&str, Changes> = BTreeMap::new();
    for entry in sources.history {
        let Some(agent) = entry.agent.as_deref() else {
            continue;
        };
        if entry.time < since || entry.time > until {
            continue;
        }
        let changes = by.entry(agent).or_insert_with(|| Changes {
            by: agent.to_owned(),
            made: 0,
            undone: 0,
        });
        changes.made += 1;
        if undone.contains(entry.id.as_str()) {
            changes.undone += 1;
        }
    }
    by.into_values().collect()
}

/// `seconds` for people: `40s`, `3m`, `2h`.
fn duration(seconds: f64) -> String {
    if seconds < 90.0 {
        format!("{seconds:.0}s")
    } else if seconds < 90.0 * 60.0 {
        format!("{:.0}m", seconds / 60.0)
    } else {
        format!("{:.1}h", seconds / 3600.0)
    }
}

fn percent(share: f64) -> String {
    format!("{:.0}%", share * 100.0)
}

impl UsageReport {
    /// Whether there's nothing in it yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records == 0 && self.changes.is_empty() && self.unreadable.is_empty()
    }

    /// The report for people. Collections are named by identifier, unless `names` gives
    /// their paths, which only the owner should see.
    #[must_use]
    pub fn text(&self, names: Option<&HashMap<Id, String>>) -> String {
        let collection = |id: &Id| -> String {
            names
                .and_then(|n| n.get(id).cloned())
                .unwrap_or_else(|| id.to_string())
        };
        let mut text = String::new();
        let days = |t: &str| t.get(..10).unwrap_or(t).to_owned();
        writeln!(
            text,
            "From {} to {}, {} decisions recorded on this device.",
            days(&self.since),
            days(&self.until),
            self.records
        )
        .unwrap();
        self.models_text(&mut text);
        self.routing_text(&mut text, &collection);
        self.structure_text(&mut text);
        self.questions_text(&mut text);
        self.search_text(&mut text);
        self.changes_text(&mut text);
        if !self.recommendations.is_empty() {
            text.push_str("\nRecommendations:\n");
            for r in &self.recommendations {
                let says = names.map_or_else(
                    || r.says.clone(),
                    |n| {
                        n.iter().fold(r.says.clone(), |s, (id, path)| {
                            s.replace(&id.to_string(), path)
                        })
                    },
                );
                writeln!(text, "  - {says}\n    {}", r.act).unwrap();
            }
        }
        text
    }

    fn models_text(&self, text: &mut String) {
        if self.models.is_empty() {
            return;
        }
        text.push_str("\nModels:\n");
        for m in &self.models {
            writeln!(
                text,
                "  {}: {} proposals, {} accepted as proposed, {} moved, {} held, {} rejected; {} drafted, {} each",
                m.model,
                m.proposals,
                percent(m.accepted_share),
                m.moved,
                m.held,
                m.rejected,
                m.drafted,
                duration(m.seconds_per_item)
            )
            .unwrap();
            if !m.corrected_fields.is_empty() {
                let fields: Vec<String> = m
                    .corrected_fields
                    .iter()
                    .map(|(f, n)| format!("{f} {n}"))
                    .collect();
                let types: Vec<String> = m
                    .corrected_types
                    .iter()
                    .map(|(t, n)| format!("{t} {n}"))
                    .collect();
                writeln!(
                    text,
                    "    corrected: {}; by type: {}",
                    fields.join(", "),
                    types.join(", ")
                )
                .unwrap();
            }
        }
    }

    fn routing_text(&self, text: &mut String, collection: &dyn Fn(&Id) -> String) {
        let r = &self.routing;
        if r.moved_out_of.is_empty() && r.rules.is_empty() {
            return;
        }
        text.push_str("\nRouting:\n");
        for c in r.moved_out_of.iter().take(5) {
            writeln!(
                text,
                "  moved out of {}: {}",
                collection(&c.collection),
                c.count
            )
            .unwrap();
        }
        for c in r.moved_into.iter().take(5) {
            writeln!(
                text,
                "  moved into {}: {}",
                collection(&c.collection),
                c.count
            )
            .unwrap();
        }
        for rule in &r.rules {
            writeln!(
                text,
                "  the rule filing into {}: {} decided, {} moved, {} rejected",
                collection(&rule.collection),
                rule.decided,
                rule.moved,
                rule.rejected
            )
            .unwrap();
        }
    }

    fn structure_text(&self, text: &mut String) {
        let s = &self.structure;
        if s.low_confidence == 0 && s.days.is_empty() {
            return;
        }
        text.push_str("\nStructure:\n");
        writeln!(
            text,
            "  {} proposals routed unsure, {} of them moved",
            s.low_confidence, s.low_confidence_moved
        )
        .unwrap();
        if let (Some(first), Some(last)) = (s.days.first(), s.days.last()) {
            writeln!(
                text,
                "  Holding {} → {}, low-confidence {} → {}, crowded collections {} → {} ({} to {})",
                first.holding,
                last.holding,
                first.low,
                last.low,
                first.crowded,
                last.crowded,
                first.day,
                last.day
            )
            .unwrap();
        }
    }

    fn questions_text(&self, text: &mut String) {
        if self.questions.is_empty() {
            return;
        }
        text.push_str("\nQuestions:\n");
        for q in &self.questions {
            writeln!(
                text,
                "  {}: {} answered, {} skipped, {} dismissed; answers settled {} items",
                q.kind,
                percent(q.answer_rate),
                percent(q.skip_rate),
                percent(q.dismiss_rate),
                q.items_improved
            )
            .unwrap();
        }
    }

    fn search_text(&self, text: &mut String) {
        if self.search.is_empty() {
            return;
        }
        text.push_str("\nSearch:\n");
        for s in &self.search {
            writeln!(
                text,
                "  {}: {} searches, {} found something, {} opened a result ({} first, {} in the top three)",
                s.mode,
                s.searches,
                s.found,
                percent(s.opened_share),
                s.first,
                s.top_three
            )
            .unwrap();
        }
    }

    fn changes_text(&self, text: &mut String) {
        let i = &self.ingest;
        if i.drafted > 0 {
            writeln!(
                text,
                "\nIngest: {} drafted; each waited {}, took {} to draft and {} to be decided",
                i.drafted,
                duration(i.queued_seconds),
                duration(i.draft_seconds),
                duration(i.review_seconds)
            )
            .unwrap();
        }
        for c in &self.changes {
            writeln!(
                text,
                "\nThe {}'s changes: {} made, {} undone",
                c.by, c.made, c.undone
            )
            .unwrap();
        }
        if !self.unreadable.is_empty() {
            text.push_str("\nFile types not read:\n");
            for u in &self.unreadable {
                writeln!(
                    text,
                    "  .{} ({}): {}, {}",
                    u.extension,
                    u.media_type,
                    u.count,
                    u.why.describe()
                )
                .unwrap();
            }
        }
    }
}
