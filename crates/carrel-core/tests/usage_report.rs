//! `carrel report`'s numbers (#330), from a hand-made log (`data/usage-report/decisions.jsonl`)
//! whose every figure is worked out by hand below.

use std::path::Path;

use carrel_core::card::Id;
use carrel_core::decisions::report::{Sources, UsageReport};
use carrel_core::decisions::{self, Record};
use carrel_core::formats::Entry as Unread;
use carrel_core::history::Entry as Change;
use carrel_core::ingest::text::Unread as Why;

fn id(text: &str) -> Id {
    Id(text.parse().unwrap())
}

fn collection(n: u8) -> Id {
    id(&format!("01a18701-0500-7000-8000-0000000000c{n}"))
}

fn at(text: &str) -> jiff::Timestamp {
    text.parse().unwrap()
}

fn records() -> Vec<Record> {
    let dir = tempfile::tempdir().unwrap();
    let log = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/usage-report");
    std::fs::copy(
        log.join("decisions.jsonl"),
        dir.path().join("2026-10.jsonl"),
    )
    .unwrap();
    decisions::read(dir.path(), at("2026-01-01T00:00:00Z"))
}

fn change(n: u8, agent: Option<&str>, time: &str, reverts: Option<&str>) -> Change {
    Change {
        id: format!("c{n}"),
        summary: String::new(),
        operation: None,
        person: None,
        device: None,
        snapshot: None,
        items: Vec::new(),
        moves: Vec::new(),
        reverts: reverts.map(str::to_owned),
        agent: agent.map(str::to_owned),
        time: at(time).as_second(),
    }
}

fn report() -> UsageReport {
    let records = records();
    assert_eq!(records.len(), 37, "every line is read");
    let history = [
        change(6, None, "2026-10-24T12:00:00Z", Some("c3")),
        change(5, Some("librarian"), "2026-10-23T12:00:00Z", None),
        change(4, Some("librarian"), "2026-10-22T12:00:00Z", None),
        change(3, Some("librarian"), "2026-10-21T12:00:00Z", None),
        change(2, None, "2026-10-21T11:00:00Z", None),
        change(1, Some("librarian"), "2026-09-01T12:00:00Z", None),
    ];
    let unreadable = [Unread {
        media_type: "model/stl".into(),
        extension: "stl".into(),
        why: Why::NoReader,
        count: 4,
        first_seen: "2026-10-20".into(),
        last_seen: "2026-10-24".into(),
    }];
    UsageReport::of(&Sources {
        archive: id("01a18701-0500-7000-8000-00000000000a"),
        records: &records,
        history: &history,
        unreadable: &unreadable,
        since: at("2026-10-01T00:00:00Z"),
        until: at("2026-10-31T00:00:00Z"),
    })
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn each_models_and_routings_numbers_match_the_hand_worked_fixture() {
    let r = report();
    // 35 lines are this archive's in the period; one is another archive's, one is older.
    assert_eq!(r.records, 35);

    // llama/small: p1, p2 approved where proposed; p3 and p6 moved; p4 held, then approved
    // (the last decision counts); p5 rejected. 3 of 6 accepted. It drafted p1–p4 in 2, 4, 6
    // and 8 seconds: 5 on average. Its cards were corrected twice: title and summary of a
    // .pdf, then the title of a folder.
    let small = &r.models[0];
    assert_eq!(small.model, "llama/small");
    assert_eq!(
        (
            small.proposals,
            small.accepted,
            small.moved,
            small.held,
            small.rejected
        ),
        (6, 3, 2, 0, 1)
    );
    assert!(close(small.accepted_share, 0.5));
    assert_eq!((small.drafted, small.seconds_per_item), (4, 5.0));
    assert_eq!(small.corrections, 2);
    assert_eq!(small.corrected_fields["title"], 2);
    assert_eq!(small.corrected_fields["summary"], 1);
    assert_eq!(small.corrected_types[".pdf"], 1);
    assert_eq!(small.corrected_types["folder"], 1);
    // apple/fm: p7 approved, p8 rejected.
    let fm = &r.models[1];
    assert_eq!(fm.model, "apple/fm");
    assert_eq!((fm.proposals, fm.accepted, fm.rejected), (2, 1, 1));
    assert!(close(fm.accepted_share, 0.5));

    // p3 left C1 and p6 left C3, both for C2. The rule filing into C3 chose p6 (moved), p7
    // (approved) and p8 (rejected).
    let out: Vec<(Id, usize)> = r
        .routing
        .moved_out_of
        .iter()
        .map(|c| (c.collection, c.count))
        .collect();
    assert_eq!(out, [(collection(1), 1), (collection(3), 1)]);
    assert_eq!(r.routing.moved_into[0].collection, collection(2));
    assert_eq!(r.routing.moved_into[0].count, 2);
    let rule = &r.routing.rules[0];
    assert_eq!(
        (rule.collection, rule.decided, rule.moved, rule.rejected),
        (collection(3), 3, 1, 1)
    );
}

#[test]
fn the_structure_questions_search_and_changes_match_the_fixture() {
    let r = report();
    // Routing was unsure of p1 and p3; p3 was moved. Two days of signals.
    assert_eq!(r.structure.low_confidence, 2);
    assert_eq!(r.structure.low_confidence_moved, 1);
    let days: Vec<(&str, usize, usize)> = r
        .structure
        .days
        .iter()
        .map(|d| (d.day.as_str(), d.holding, d.crowded))
        .collect();
    assert_eq!(days, [("2026-10-20", 4, 1), ("2026-10-25", 12, 2)]);

    // Term questions: q1 skipped then answered (3 items), q2 dismissed. Collection
    // questions: q3 answered (2 items) then dismissed as it settled, so answered; q4 and q5
    // skipped.
    let term = &r.questions[1];
    assert_eq!(term.kind, "term");
    assert_eq!(
        (
            term.answered,
            term.skipped,
            term.dismissed,
            term.items_improved
        ),
        (1, 1, 1, 3)
    );
    assert!(close(term.answer_rate, 1.0 / 3.0));
    let place = &r.questions[0];
    assert_eq!(place.kind, "collection");
    assert_eq!(
        (
            place.answered,
            place.skipped,
            place.dismissed,
            place.items_improved
        ),
        (1, 2, 0, 2)
    );
    assert!(close(place.skip_rate, 2.0 / 3.0));

    // Searches: by words s1 (5 found, opened at 2 then 1: the first open counts) and s2
    // (none); by both s3 (opened at 1) and s4; by filters s5 (opened at 4).
    let modes: Vec<(&str, usize, usize, usize, usize, usize)> = r
        .search
        .iter()
        .map(|s| {
            (
                s.mode.as_str(),
                s.searches,
                s.found,
                s.opened,
                s.first,
                s.top_three,
            )
        })
        .collect();
    assert_eq!(
        modes,
        [
            ("words", 2, 1, 1, 0, 1),
            ("both", 2, 2, 1, 1, 1),
            ("filters", 1, 1, 1, 0, 0)
        ]
    );
    assert!(close(r.search[0].opened_share, 0.5));
    assert!(close(r.search[0].mean_rank, 2.0));
    assert!(close(r.search[2].mean_rank, 4.0));

    // Queued 10, 30, 20 and 60 seconds: 30. Decided after 100, 300, 200, 900, 0, 50, 10 and
    // 40 seconds (p4's last decision): 200.
    assert_eq!(r.ingest.drafted, 4);
    assert!(close(r.ingest.queued_seconds, 30.0));
    assert!(close(r.ingest.draft_seconds, 5.0));
    assert!(close(r.ingest.review_seconds, 200.0));

    // The librarian made c3, c4 and c5 in the period (c1 is older); c3 was undone.
    assert_eq!(r.changes.len(), 1);
    assert_eq!((r.changes[0].made, r.changes[0].undone), (3, 1));

    assert_eq!(r.unreadable[0].extension, "stl");
}

#[test]
fn each_recommendation_names_what_acts_on_it() {
    let r = report();
    let about: Vec<&str> = r.recommendations.iter().map(|r| r.about.as_str()).collect();
    // The rule into C3 misfired on 2 of 3; Holding grew from 4 to 12; 2 collections are
    // crowded; 4 .stl files weren't read. Everything else has too few cases.
    assert_eq!(
        about,
        ["rule", "holding", "collection", "file-type"],
        "{r:#?}"
    );
    for recommendation in &r.recommendations {
        assert!(
            recommendation.act.contains('`'),
            "names a command or setting: {recommendation:?}"
        );
    }
    assert!(r.recommendations[0].act.contains("routing.yaml"));
    assert!(r.recommendations[2].act.contains("carrel collection new"));
}
