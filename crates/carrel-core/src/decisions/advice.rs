//! The report's recommendations ([`super::report`]): what the evidence suggests tweaking,
//! each with the setting or command that acts on it. A recommendation is only made from
//! enough cases ([`ENOUGH`]) that it isn't chance.

use super::report::{Recommendation, UsageReport};

/// The fewest cases a recommendation is made from.
pub const ENOUGH: usize = 5;

fn says(about: &str, says: String, act: &str) -> Recommendation {
    Recommendation {
        about: about.to_owned(),
        says,
        act: act.to_owned(),
    }
}

fn percent(share: f64) -> String {
    format!("{:.0}%", share * 100.0)
}

/// What `report` recommends, most useful first.
#[must_use]
pub fn recommend(report: &UsageReport) -> Vec<Recommendation> {
    let mut found = Vec::new();
    models(report, &mut found);
    routing(report, &mut found);
    structure(report, &mut found);
    questions(report, &mut found);
    search(report, &mut found);
    changes(report, &mut found);
    unreadable(report, &mut found);
    found
}

fn models(report: &UsageReport, found: &mut Vec<Recommendation>) {
    let best = report
        .models
        .iter()
        .filter(|m| m.proposals >= ENOUGH && m.model != "none")
        .max_by(|a, b| a.accepted_share.total_cmp(&b.accepted_share));
    for m in report.models.iter().filter(|m| m.model != "none") {
        if m.proposals >= ENOUGH && m.accepted_share < 0.5 {
            let better = best
                .filter(|b| b.model != m.model && b.accepted_share > m.accepted_share)
                .map(|b| format!("; {} had {} accepted", b.model, percent(b.accepted_share)))
                .unwrap_or_default();
            found.push(says(
                "model",
                format!(
                    "Only {} of {}'s {} proposals were accepted as proposed{better}.",
                    percent(m.accepted_share),
                    m.model,
                    m.proposals
                ),
                "Switch the model that drafts cards: `carrel settings set model NAME` (`carrel models` lists them), or set `second_model` to ask another when the first isn't sure.",
            ));
        }
        let corrections: usize = m.corrected_fields.values().sum();
        if m.proposals >= ENOUGH && corrections * 2 >= m.proposals {
            let (field, n) = m
                .corrected_fields
                .iter()
                .max_by_key(|(_, n)| **n)
                .map_or(("", 0), |(f, n)| (f.as_str(), *n));
            found.push(says(
                "model",
                format!(
                    "{}'s cards needed {corrections} corrections over {} proposals, most often the {field} ({n}).",
                    m.model, m.proposals
                ),
                "Try a stronger model: `carrel settings set model NAME`; `carrel bench` compares them on your device.",
            ));
        }
        if m.drafted >= ENOUGH && m.seconds_per_item > 120.0 {
            found.push(says(
                "speed",
                format!(
                    "{} took {:.0} seconds an item to draft.",
                    m.model, m.seconds_per_item
                ),
                "A smaller model drafts faster: `carrel settings set model NAME` (`carrel models` shows their sizes).",
            ));
        }
    }
}

fn routing(report: &UsageReport, found: &mut Vec<Recommendation>) {
    for c in report
        .routing
        .moved_out_of
        .iter()
        .filter(|c| c.count >= ENOUGH)
    {
        found.push(says(
            "collection",
            format!(
                "{} items routed to collection {} were moved elsewhere.",
                c.count, c.collection
            ),
            "Say what belongs there and what doesn't in its design document (COLLECTION.md in its folder), or subdivide it with `carrel collection new PARENT/NAME`.",
        ));
    }
    for c in report
        .routing
        .moved_into
        .iter()
        .filter(|c| c.count >= ENOUGH)
    {
        found.push(says(
            "collection",
            format!(
                "{} items were moved into collection {} by hand.",
                c.count, c.collection
            ),
            "Add a routing rule for it in `_System/rules/routing.yaml`, or describe it better in its COLLECTION.md, so routing finds it.",
        ));
    }
    for rule in report
        .routing
        .rules
        .iter()
        .filter(|r| r.decided >= 3 && (r.moved + r.rejected) * 3 >= r.decided)
    {
        found.push(says(
            "rule",
            format!(
                "The routing rule filing into collection {} misfired on {} of {} items ({} moved, {} rejected).",
                rule.collection,
                rule.moved + rule.rejected,
                rule.decided,
                rule.moved,
                rule.rejected
            ),
            "Narrow the rule's `match` in `_System/rules/routing.yaml`.",
        ));
    }
}

fn structure(report: &UsageReport, found: &mut Vec<Recommendation>) {
    let s = &report.structure;
    if s.low_confidence >= ENOUGH && s.low_confidence_moved * 2 >= s.low_confidence {
        found.push(says(
            "collection",
            format!(
                "{} of {} proposals routing was unsure about were moved.",
                s.low_confidence_moved, s.low_confidence
            ),
            "Answer the collection and group questions (`carrel questions`): each answer teaches routing.",
        ));
    }
    if let (Some(first), Some(last)) = (s.days.first(), s.days.last()) {
        if last.holding >= 10 && last.holding > first.holding {
            found.push(says(
                "holding",
                format!(
                    "Holding grew from {} to {} items.",
                    first.holding, last.holding
                ),
                "Give them homes: `carrel questions` asks where groups of them go, and `carrel collection new PATH` makes one.",
            ));
        }
        if last.crowded > 0 {
            found.push(says(
                "collection",
                format!("{} collections are crowded with loose items.", last.crowded),
                "Subdivide them: `carrel status` names them, and `carrel collection new PARENT/NAME` makes a subcollection; `signals.crowded_items` sets how many is crowded.",
            ));
        }
    }
}

fn questions(report: &UsageReport, found: &mut Vec<Recommendation>) {
    for q in &report.questions {
        let all = q.answered + q.skipped + q.dismissed;
        if all >= ENOUGH && q.skip_rate + q.dismiss_rate > 0.6 {
            found.push(says(
                "questions",
                format!(
                    "{} of {} {} questions were skipped or dismissed.",
                    q.skipped + q.dismissed,
                    all,
                    q.kind
                ),
                "Let skipped questions rest longer: `carrel settings set questions.skip_days DAYS`.",
            ));
        }
    }
}

fn search(report: &UsageReport, found: &mut Vec<Recommendation>) {
    for s in &report.search {
        if s.searches < ENOUGH {
            continue;
        }
        if s.mode == "words" && s.opened_share < 0.5 {
            found.push(says(
                "search",
                format!(
                    "{} of {} searches by words alone opened nothing.",
                    s.searches - s.opened,
                    s.searches
                ),
                "Search by meaning too: `carrel index status` says what's missing, and `carrel index rebuild` builds the index.",
            ));
        } else if s.opened > 0 && s.top_three * 2 < s.opened {
            found.push(says(
                "search",
                format!(
                    "Of {} searches whose results were opened, only {} opened one in the top three.",
                    s.opened, s.top_three
                ),
                "Try another embedding model: `carrel index model NAME`.",
            ));
        }
    }
}

fn changes(report: &UsageReport, found: &mut Vec<Recommendation>) {
    for c in &report.changes {
        if c.made >= ENOUGH && c.undone * 4 >= c.made {
            found.push(says(
                "librarian",
                format!(
                    "{} of the {}'s {} changes were undone.",
                    c.undone, c.by, c.made
                ),
                "Ask it to be surer: `carrel settings set librarian.confidence 0.95`, or `carrel settings set librarian.changes off` to have it only suggest.",
            ));
        }
    }
}

fn unreadable(report: &UsageReport, found: &mut Vec<Recommendation>) {
    for u in report.unreadable.iter().filter(|u| u.count >= 3) {
        let how = crate::formats::hint(&u.media_type).map_or_else(
            || "`carrel formats` lists what reads what".to_owned(),
            |h| format!("{h}; `carrel formats` lists what reads what"),
        );
        found.push(says(
            "file-type",
            format!(
                "{} .{} files ({}) couldn't be read.",
                u.count, u.extension, u.media_type
            ),
            &format!("Read the type: {how}."),
        ));
    }
}
