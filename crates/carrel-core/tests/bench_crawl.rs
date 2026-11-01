//! The crawler benchmark (`docs/design/restructure.md`, "Measuring it"): the muddled copy of
//! the test set's archive, scoring each judgment, and a whole run with stub models that know
//! the answers, or know nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::bench::Answers;
use carrel_core::bench::crawl::{
    CRAWL_BAR, Judged, Misplaced, Muddle, build, counts, score_item, summarize,
};
use carrel_core::card::Id;
use carrel_core::coordinator::Holder;
use carrel_core::crawl::Crawler;
use carrel_core::providers::device::AvailableModel;
use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, Registry,
};
use serde_json::{Value, json};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench")
}

fn answers() -> Answers {
    Answers::load(&bench().join("answers.yaml")).unwrap()
}

fn muddle() -> Muddle {
    Muddle::load(&bench().join("muddle.yaml")).unwrap()
}

#[test]
fn the_muddle_is_consistent_with_the_test_set() {
    let answers = answers();
    let muddle = muddle();
    let archive = Archive::open(&bench().join("archive")).unwrap();
    assert_eq!(muddle.check(&answers, &archive), Vec::<String>::new());
    let places = muddle.places(&answers);
    let mut kinds: BTreeMap<Misplaced, usize> = BTreeMap::new();
    for (_, kind) in places.values() {
        if let Some(kind) = kind {
            *kinds.entry(*kind).or_default() += 1;
        }
    }
    assert_eq!(
        kinds,
        BTreeMap::from([
            (Misplaced::Missing, 4),
            (Misplaced::Overfilled, 6),
            (Misplaced::Scattered, 8),
        ])
    );
    assert_eq!(places["sourdough loaf.md"].0, "Home");
    assert_eq!(places["kettle manual.zip"].0, "Home");
    assert_eq!(places["letter from grandma.txt"].0, "Finances");
    assert_eq!(places["tenancy agreement.md"], ("Home".into(), None));

    let broken: Muddle = serde_saphyr::from_str(
        "missing: {collection: Nowhere, into: Home}\noverfilled: {collection: Home, from: Manuals}\nscattered:\n  \"tenancy agreement.md\": Home\n",
    )
    .unwrap();
    let problems = broken.check(&answers, &archive);
    assert!(
        problems.iter().any(|p| p.contains("no collection Nowhere")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.contains("where it belongs")),
        "{problems:?}"
    );
}

#[test]
fn the_muddled_archive_is_built_without_its_missing_collection() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("archive");
    let answers = answers();
    let muddle = muddle();
    let archive = build(
        &bench().join("archive"),
        &bench().join("items"),
        &answers,
        &muddle,
        &work,
    )
    .unwrap();
    let collections: Vec<String> = archive
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert!(!collections.contains(&"Recipes".to_owned()));
    assert!(
        collections.contains(&"Manuals".to_owned()),
        "left there, empty"
    );
    assert!(work.join("Home/sourdough loaf.md.md").is_file());
    assert!(work.join("Home/kettle manual.zip.md").is_file());
    assert!(work.join("Reading/receipt desk chair.txt.md").is_file());
    assert!(work.join("Home/tenancy agreement.md.md").is_file());
    assert!(!work.join("Manuals/kettle manual.zip.md").exists());

    // Built again, each card keeps its identifier.
    let before: Vec<Id> = archive.items().unwrap().iter().map(|i| i.id).collect();
    let again = build(
        &bench().join("archive"),
        &bench().join("items"),
        &answers,
        &muddle,
        &work,
    )
    .unwrap();
    let after: Vec<Id> = again.items().unwrap().iter().map(|i| i.id).collect();
    assert_eq!(before, after);
}

fn judged(fits: Option<bool>, better: Option<&str>) -> Judged {
    Judged {
        fits,
        better: better.map(str::to_owned),
        confidence: 0.8,
        reason: String::new(),
        seconds: 2.0,
        error: None,
    }
}

#[test]
fn each_judgment_is_scored_on_finding_and_placing_and_added_up() {
    let answers = answers();
    let existing: Vec<String> = [
        "Home", "Health", "Letters", "Finances", "Reading", "Manuals",
    ]
    .map(String::from)
    .to_vec();
    let answer = |file: &str| &answers.items[file];
    let scattered = ("Letters", Some(Misplaced::Scattered));
    let missing = ("Home", Some(Misplaced::Missing));
    let fine = ("Home", None);

    let scores = [
        // Found and put right.
        score_item(
            "blood test results.txt",
            scattered,
            answer("blood test results.txt"),
            &judged(Some(false), Some("Health")),
            &existing,
        ),
        // Found, but put wrong.
        score_item(
            "letter from grandma.txt",
            ("Finances", Some(Misplaced::Scattered)),
            answer("letter from grandma.txt"),
            &judged(Some(false), Some("Reading")),
            &existing,
        ),
        // Missed.
        score_item(
            "kettle manual.zip",
            ("Home", Some(Misplaced::Overfilled)),
            answer("kettle manual.zip"),
            &judged(Some(true), None),
            &existing,
        ),
        // Found, its collection missing: no placement to score.
        score_item(
            "lentil dal.md",
            missing,
            answer("lentil dal.md"),
            &judged(Some(false), None),
            &existing,
        ),
        // A false alarm.
        score_item(
            "tenancy agreement.md",
            fine,
            answer("tenancy agreement.md"),
            &judged(Some(false), Some("Finances")),
            &existing,
        ),
        // Rightly left alone.
        score_item(
            "boiler repair note.txt",
            fine,
            answer("boiler repair note.txt"),
            &judged(Some(true), None),
            &existing,
        ),
        // Not usable.
        score_item(
            "internet contract.txt",
            fine,
            answer("internet contract.txt"),
            &judged(None, None),
            &existing,
        ),
    ];
    assert_eq!(scores[0].placed_right, Some(true));
    assert_eq!(scores[1].placed_right, Some(false));
    assert_eq!(
        scores[2].placed_right,
        Some(false),
        "it wouldn't move it at all"
    );
    assert_eq!(scores[3].placed_right, None);
    assert_eq!(scores[4].placed_right, None);

    let score = summarize(&scores);
    assert_eq!((score.items, score.misplaced, score.found), (7, 4, 3));
    assert!((score.recall - 0.75).abs() < 1e-9);
    assert_eq!(score.flagged, 4);
    assert!((score.precision - 0.75).abs() < 1e-9);
    assert_eq!((score.placeable, score.placed_right), (3, 1));
    assert!((score.placement - 1.0 / 3.0).abs() < 1e-9);
    assert_eq!(score.by_kind[&Misplaced::Scattered], (2, 2));
    assert_eq!(score.by_kind[&Misplaced::Overfilled], (0, 1));
    assert_eq!(score.unusable, 1);
    assert!((score.seconds_per_item - 2.0).abs() < 1e-9);
    let short = score.shortfalls(&CRAWL_BAR);
    assert_eq!(short, ["placement 33%"], "{short:?}");
    assert!(
        !counts(answer("misc notes.txt")),
        "Holding's own items don't count"
    );
}

/// Stub models judging by the test set's answers: `knows` knows where each item belongs, so it
/// finds every misplaced item and puts it right; `guesses` says every item fits.
struct Stub {
    offers: Vec<Offer>,
    answers: Answers,
}

impl Stub {
    fn new(answers: Answers) -> Self {
        let offer = |id: &str| Offer {
            capability: Capability::TextModel,
            id: id.into(),
            label: None,
            priority: 10,
            available: true,
            reason: None,
            media_types: Vec::new(),
            extra: serde_json::Map::new(),
        };
        Self {
            offers: vec![offer("knows"), offer("guesses")],
            answers,
        }
    }

    fn judge(&self, knows: bool, request: &CompletionRequest) -> Value {
        let line = |prefix: &str| {
            request
                .prompt
                .lines()
                .find_map(|l| l.strip_prefix(prefix))
                .map(|l| l.trim_end_matches('.').to_owned())
        };
        let file = line("File: ").unwrap_or_default();
        let place = line("The item is in the collection ").unwrap_or_default();
        let paths: Vec<&str> = request.schema.as_ref().unwrap()["properties"]["collection"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let destinations = self
            .answers
            .items
            .get(&file)
            .map(|a| a.destinations.clone())
            .unwrap_or_default();
        let fits = !knows || destinations.contains(&place);
        let better = destinations
            .iter()
            .find(|d| paths.contains(&d.as_str()))
            .filter(|_| !fits)
            .cloned()
            .unwrap_or_default();
        json!({"fits": fits, "collection": better, "confidence": 0.9, "reason": "", "title": "", "summary": "", "created": "", "tags": []})
    }
}

impl Provider for Stub {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn offers(&self) -> &[Offer] {
        &self.offers
    }
    fn complete(
        &self,
        offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        let answer = self.judge(offer == "knows", request);
        Ok(Completion {
            text: answer.to_string(),
            json: Some(answer),
            model: "stub/m".into(),
            usage: None,
        })
    }
}

/// Every counted item of the muddled archive, judged by `offer` of the stub, scored.
fn run(
    archive: &Archive,
    registry: &Registry,
    offer: &str,
    answers: &Answers,
) -> carrel_core::bench::crawl::CrawlScore {
    let crawler = Crawler {
        archive,
        registry,
        model: AvailableModel {
            provider: "stub".into(),
            offer: offer.into(),
            label: offer.into(),
        },
        holder: Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        },
    };
    let places = muddle().places(answers);
    let existing: Vec<String> = archive
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    let mut scores = Vec::new();
    for item in archive.items().unwrap() {
        let Some(file) = item.item.as_ref().and_then(|i| i.rsplit('/').next()) else {
            continue;
        };
        let Some(answer) = answers.items.get(file).filter(|a| counts(a)) else {
            continue;
        };
        let judgment = crawler.judge_item(&item).unwrap().unwrap();
        let gave = Judged {
            fits: Some(judgment.fits),
            better: judgment.better,
            confidence: judgment.confidence,
            reason: judgment.reason,
            seconds: 0.1,
            error: None,
        };
        let (place, kind) = &places[file];
        scores.push(score_item(file, (place, *kind), answer, &gave, &existing));
    }
    summarize(&scores)
}

#[test]
fn a_model_that_knows_the_answers_finds_and_places_everything_and_one_that_doesnt_finds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let answers = answers();
    let archive = build(
        &bench().join("archive"),
        &bench().join("items"),
        &answers,
        &muddle(),
        &dir.path().join("archive"),
    )
    .unwrap();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Stub::new(answers.clone())));

    let knows = run(&archive, &registry, "knows", &answers);
    assert_eq!(knows.misplaced, 18);
    assert_eq!(knows.found, 18);
    assert!((knows.recall - 1.0).abs() < 1e-9);
    assert!((knows.precision - 1.0).abs() < 1e-9);
    assert_eq!(
        knows.placeable, 14,
        "the missing collection's 4 can't be placed"
    );
    assert!((knows.placement - 1.0).abs() < 1e-9);
    assert_eq!(knows.shortfalls(&CRAWL_BAR), Vec::<String>::new());

    let guesses = run(&archive, &registry, "guesses", &answers);
    assert_eq!((guesses.found, guesses.flagged), (0, 0));
    assert_eq!(guesses.recall, 0.0);
    assert_eq!(guesses.placed_right, 0);
    assert!(
        guesses
            .shortfalls(&CRAWL_BAR)
            .contains(&"recall 0%".to_owned())
    );
}
