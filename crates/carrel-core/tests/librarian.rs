//! The librarian (`docs/design/librarian.md`), driven by a scripted model: which tools it
//! uses, its limits, restricted items, saving to the Journal, and its changes on and off.

mod common;

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::{Checksum, Id, Sensitivity};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::history::History;
use carrel_core::librarian::changes::{self, Change, Outcome};
use carrel_core::librarian::{Conversation, Librarian, Limits, Stopped, journal, model};
use carrel_core::providers::Registry;
use carrel_core::providers::device::AvailableModel;
use carrel_core::search::texts::TextCache;
use common::Scripted;
use serde_json::{Value, json};

const NOTE: &str = "01a18701-0500-776a-9867-0805d715b667";
const NOTE_CARD: &str = "Reading/Coupled oscillators note.pdf.md";
const PARTS: &str = "01a18701-0500-776a-9867-080d24773181";
const PARTS_CARD: &str = "Projects/Hardware/parts-list.csv.md";
const RFC: &str = "01a18701-0500-776a-9867-0807eff2807e";
const NOTE_TEXT: &str = "Two coupled oscillators lock their phases when the coupling is stronger than the difference in their natural frequencies. This is the locking condition.";

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn holder() -> Holder {
    Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    }
}

/// The sample archive with its history started, and the note's text read.
fn sample() -> (tempfile::TempDir, Archive, Holder) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    let archive = Archive::open(&root).unwrap();
    let holder = holder();
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::start(&archive, &lease).unwrap();
    lease.release().unwrap();
    TextCache::of(&archive)
        .put(
            &Checksum(
                "sha256:93de2dbee3f53bc4fcdd31247d40eb1bd4e994ec55009c9b70a10dc8db4d2ded".into(),
            ),
            NOTE_TEXT,
        )
        .unwrap();
    (dir, archive, holder)
}

#[allow(clippy::unnecessary_wraps)] // the scripted model's answers are results
fn tool(action: &str, input: &str) -> Result<Value, carrel_core::providers::ProviderError> {
    Ok(
        json!({"action": action, "input": input, "answer": "", "items": [], "passages": [], "suggestions": []}),
    )
}

#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)] // as for `tool`
fn answer(
    text: &str,
    items: &[&str],
    passages: Value,
    suggestions: Value,
) -> Result<Value, carrel_core::providers::ProviderError> {
    Ok(
        json!({"action": "answer", "input": "", "answer": text, "items": items, "passages": passages, "suggestions": suggestions}),
    )
}

fn the_model() -> AvailableModel {
    AvailableModel {
        provider: "scripted".into(),
        offer: "m".into(),
        label: "scripted/m".into(),
    }
}

/// A registry with a scripted model giving `answers`, and what it's asked.
fn scripted(
    answers: Vec<Result<Value, carrel_core::providers::ProviderError>>,
) -> (
    Registry,
    std::sync::Arc<std::sync::Mutex<Vec<carrel_core::providers::CompletionRequest>>>,
) {
    let model = Scripted::new("scripted", 10, 8_000, answers);
    let asked = model.asked.clone();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(model));
    (registry, asked)
}

fn providers() -> Registry {
    Registry::with_built_ins()
}

#[test]
fn it_answers_with_the_tools_the_model_chooses() {
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![
        tool("search", "oscillators"),
        tool("show", NOTE),
        tool("read", NOTE_CARD),
        answer(
            "You have a note on coupled oscillators, which gives the locking condition. You might also look at the oscillator simulation in Projects.",
            &[NOTE, "Projects/nowhere.md"],
            json!([
                {"item": NOTE, "text": "lock their phases when the coupling is stronger"},
                {"item": NOTE, "text": "an invented sentence the note doesn't have"}
            ]),
            json!([]),
        ),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "What do I have about oscillators?")
        .unwrap()
        .clone();

    let tools: Vec<&str> = answer.steps.iter().map(|s| s.tool.as_str()).collect();
    assert_eq!(tools, ["search", "show", "read"]);
    assert!(answer.text.starts_with("You have a note"));
    assert_eq!(answer.model, "scripted/m", "each answer names its model");
    assert_eq!(answer.stopped, None);
    let items: Vec<String> = answer.items.iter().map(|i| i.id.to_string()).collect();
    assert_eq!(items, [NOTE], "an item that isn't there is left out");
    assert_eq!(answer.items[0].card, NOTE_CARD);
    assert_eq!(answer.passages.len(), 1, "only passages the tools returned");
    assert!(answer.passages[0].text.contains("lock their phases"));

    let asked = asked.lock().unwrap();
    assert_eq!(asked.len(), 4);
    let system = asked[0].system.as_deref().unwrap();
    assert!(
        system.contains("Answer first"),
        "the instructions ask for curiosity"
    );
    assert!(asked[0].schema.is_some(), "it answers to a schema");
    assert!(
        asked[1].prompt.contains("A note on coupled oscillators"),
        "search's result is handed back"
    );
    assert!(
        asked[2]
            .prompt
            .contains("summary: An invented two-page note"),
        "and the card"
    );
    assert!(
        asked[3].prompt.contains("locking condition"),
        "and the passages read"
    );
    assert_eq!(conversation.answers.len(), 1);
}

#[test]
fn a_conversation_remembers_the_earlier_questions() {
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![
        answer("Two items.", &[], json!([]), json!([])),
        answer("The note.", &[], json!([]), json!([])),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    librarian.ask(&mut conversation, "How many notes?").unwrap();
    librarian
        .ask(&mut conversation, "Which is newest?")
        .unwrap();
    let asked = asked.lock().unwrap();
    assert!(
        asked[1]
            .prompt
            .contains("Owner: How many notes?\nLibrarian: Two items.")
    );
    assert_eq!(conversation.answers.len(), 2);
}

#[test]
fn it_says_how_far_it_has_got_as_each_step_starts() {
    let (_dir, archive, _) = sample();
    let (registry, _) = scripted(vec![
        tool("search", "oscillators"),
        answer("The note.", &[NOTE], json!([]), json!([])),
    ]);
    let seen = std::sync::Mutex::new(Vec::new());
    let progress = |p: carrel_core::librarian::Progress<'_>| {
        seen.lock()
            .unwrap()
            .push((p.step, p.steps, p.did.map(str::to_owned)));
    };
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_progress(&progress);
    librarian
        .ask(&mut Conversation::default(), "Oscillators?")
        .unwrap();
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!((seen[0].0, seen[0].1, seen[0].2.as_deref()), (1, 8, None));
    assert_eq!(seen[1].0, 2);
    assert!(
        seen[1].2.is_some(),
        "the second step says what the search did"
    );
}

#[test]
fn it_stops_at_its_limit_of_steps_and_must_answer_on_the_last() {
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![
        tool("search", "lamp"),
        tool("related", PARTS),
        tool("history", PARTS),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_limits(Limits {
            steps: 3,
            ..Limits::default()
        });
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "Tell me about the lamp")
        .unwrap();
    assert_eq!(answer.stopped, Some(Stopped::Steps));
    assert!(answer.text.contains("couldn't finish"), "{}", answer.text);
    assert!(
        answer.items.iter().any(|i| i.id.to_string() == PARTS),
        "it lists what it came across"
    );
    let asked = asked.lock().unwrap();
    assert_eq!(asked.len(), 3);
    assert_eq!(
        asked[0].schema.as_ref().unwrap()["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        asked[2].schema.as_ref().unwrap()["properties"]["action"]["enum"],
        json!(["answer"])
    );
    assert!(asked[2].prompt.contains("last step"));
}

#[test]
fn it_stops_at_its_limit_of_tokens() {
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![tool("search", "lamp"), tool("search", "lamp")]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_limits(Limits {
            tokens: 2_000,
            ..Limits::default()
        });
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "Tell me about the lamp")
        .unwrap();
    assert_eq!(answer.stopped, Some(Stopped::Tokens));
    assert!(answer.tokens <= 2_000);
    let asked = asked.lock().unwrap();
    assert_eq!(asked.len(), 1, "the second step wouldn't fit");
    assert_eq!(
        asked[0].schema.as_ref().unwrap()["properties"]["action"]["enum"],
        json!(["answer"]),
        "with room for one step, it's asked to answer"
    );
}

#[test]
fn an_unusable_reply_is_said_and_costs_a_step() {
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![
        Ok(json!({"something": "else"})),
        answer("Nothing about that.", &[], json!([]), json!([])),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "Anything on boats?")
        .unwrap();
    assert_eq!(answer.text, "Nothing about that.");
    assert!(asked.lock().unwrap()[1].prompt.contains("wasn't usable"));
}

#[test]
fn restricted_items_are_found_by_their_card_and_never_read_or_quoted() {
    let (_dir, archive, _) = sample();
    let card = archive.root().join(PARTS_CARD);
    let text = std::fs::read_to_string(&card).unwrap();
    std::fs::write(
        &card,
        text.replace("sensitivity: personal", "sensitivity: restricted"),
    )
    .unwrap();
    let (registry, asked) = scripted(vec![
        tool("show", PARTS),
        tool("read", PARTS),
        answer(
            "It's the lamp's parts list.",
            &[PARTS],
            json!([{"item": PARTS, "text": "Bill of materials for the desk lamp build"}]),
            json!([]),
        ),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "What's in the parts list?")
        .unwrap();
    assert!(
        answer.steps[1].did.contains("restricted"),
        "{}",
        answer.steps[1].did
    );
    assert_eq!(
        answer.items[0].sensitivity,
        Sensitivity::Restricted,
        "found by its card"
    );
    assert!(
        answer.passages.is_empty(),
        "never quoted, even from its card"
    );
    let asked = asked.lock().unwrap();
    assert!(asked[2].prompt.contains("only its card can be seen"));
    assert!(
        !asked[2].prompt.contains("LED"),
        "its text never reaches the model"
    );
}

/// An answer suggesting every kind of change, about the note (whose summary a model wrote
/// and whose title the owner did), the parts list and the RFC.
fn suggesting() -> Result<Value, carrel_core::providers::ProviderError> {
    answer(
        "Here's what I found.",
        &[NOTE],
        json!([]),
        json!([
            {"change": "correct", "item": NOTE, "field": "summary", "value": "A two-page note on how coupled oscillators lock their phases.", "why": "The summary should say what it shows.", "confidence": 0.95},
            {"change": "correct", "item": NOTE, "field": "title", "value": "Coupled oscillators", "why": "Shorter.", "confidence": 0.99},
            {"change": "refile", "item": PARTS_CARD, "field": "", "value": "Reading", "why": "It's reference.", "confidence": 0.95},
            {"change": "collection", "item": "", "field": "", "value": "Reading/Standards", "why": "Standards documents", "confidence": 0.92},
            {"change": "rename", "item": RFC, "field": "", "value": "rfc9562.md", "why": "Tidier.", "confidence": 0.5},
            {"change": "delete", "item": RFC, "field": "", "value": "x", "why": "No.", "confidence": 1.0}
        ]),
    )
}

#[test]
fn with_changes_off_every_suggestion_is_left_for_the_owner() {
    let (_dir, archive, holder) = sample();
    let before = History::open(&archive).unwrap().log().unwrap().len();
    let (registry, _) = scripted(vec![suggesting()]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_changes_for(holder);
    assert!(
        !librarian.makes_changes(),
        "librarian.changes is off by default"
    );
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "Anything to tidy?")
        .unwrap();
    assert_eq!(
        answer.suggestions.len(),
        5,
        "a deletion isn't a suggestion it can make"
    );
    for suggestion in &answer.suggestions {
        assert!(
            matches!(suggestion.outcome, Outcome::Proposed { .. }),
            "{suggestion:?}"
        );
    }
    assert!(
        answer.suggestions[0]
            .change
            .command()
            .starts_with("carrel correct")
    );
    assert_eq!(
        History::open(&archive).unwrap().log().unwrap().len(),
        before,
        "nothing changed"
    );
    assert!(archive.root().join(PARTS_CARD).exists());

    // The owner confirms one: it's their change, through the same path.
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    let commit = changes::apply(
        &archive,
        &registry,
        "scripted/m",
        &answer.suggestions[2].change,
        &lease,
    )
    .unwrap();
    lease.release().unwrap();
    assert!(commit.is_some());
    assert!(archive.root().join("Reading/parts-list.csv").exists());
    let log = History::open(&archive).unwrap().log().unwrap();
    assert_eq!(log[0].agent, None, "the owner's change");
    assert!(log[0].summary.contains("Owner:"), "{}", log[0].summary);
}

fn turn_changes_on(archive: &Archive, holder: Holder) -> Archive {
    let lease = DiskLock::for_archive(archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    carrel_core::settings::change(
        archive,
        carrel_core::settings::Key::LibrarianChanges,
        "on",
        &lease,
    )
    .unwrap();
    lease.release().unwrap();
    Archive::open(archive.root()).unwrap()
}

/// [`suggesting`], with a correction to the preprint, made restricted, as well.
fn suggesting_for_a_restricted_item_too(archive: &Archive) -> Value {
    let card = archive
        .root()
        .join(NOTE_CARD.replace("note.pdf.md", "note (preprint).pdf.md"));
    let preprint = std::fs::read_to_string(&card).unwrap();
    std::fs::write(
        &card,
        preprint.replace("sensitivity: public", "sensitivity: restricted"),
    )
    .unwrap();
    let restricted =
        carrel_core::card_file::CardFile::parse(&std::fs::read_to_string(&card).unwrap())
            .unwrap()
            .card()
            .id
            .to_string();
    let mut reply = suggesting().unwrap();
    reply["suggestions"].as_array_mut().unwrap().push(json!(
        {"change": "correct", "item": restricted, "field": "summary", "value": "Secret.", "why": "x", "confidence": 1.0}
    ));
    reply
}

#[test]
fn with_changes_on_it_makes_those_it_is_sure_of_and_they_undo_together() {
    let (_dir, archive, holder) = sample();
    let archive = turn_changes_on(&archive, holder);
    let reply = suggesting_for_a_restricted_item_too(&archive);
    let (registry, _) = scripted(vec![Ok(reply)]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_changes_for(holder);
    assert!(librarian.makes_changes());
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "Anything to tidy?")
        .unwrap()
        .clone();
    let outcomes: Vec<&str> = answer
        .suggestions
        .iter()
        .map(|s| match s.outcome {
            Outcome::Made { .. } => "made",
            Outcome::Proposed { .. } => "proposed",
            Outcome::Refused { .. } => "refused",
            Outcome::Failed { .. } => "failed",
        })
        .collect();
    assert_eq!(
        outcomes,
        ["made", "proposed", "made", "made", "proposed", "refused"],
        "{:#?}",
        answer.suggestions
    );
    let Outcome::Proposed { reason } = &answer.suggestions[1].outcome else {
        unreachable!()
    };
    assert!(reason.contains("you wrote its title"), "{reason}");
    let Outcome::Proposed { reason } = &answer.suggestions[4].outcome else {
        unreachable!()
    };
    assert!(reason.contains("50%"), "{reason}");

    let note = std::fs::read_to_string(archive.root().join(NOTE_CARD)).unwrap();
    assert!(note.contains("lock their phases"), "{note}");
    assert!(
        note.contains("A note on coupled oscillators"),
        "the owner's title stays"
    );
    assert!(!note.contains("title: Coupled oscillators"), "{note}");
    assert!(
        archive.root().join("Reading/parts-list.csv").exists(),
        "refiled"
    );
    assert!(
        archive
            .root()
            .join("Reading/Standards/_collection.md")
            .exists()
    );

    let history = History::open(&archive).unwrap();
    let log = history.log().unwrap();
    let mine = changes::recent(&log, 0);
    assert_eq!(
        mine.len(),
        3,
        "each an ordinary change marked as the librarian's"
    );
    assert!(
        log[..3]
            .iter()
            .all(|e| e.agent.as_deref() == Some("librarian"))
    );
    assert!(log.iter().any(|e| e.summary.contains("Librarian:")));

    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "undo", DEFAULT_LEASE)
        .unwrap();
    let undone = changes::undo_since(&archive, 0, &lease).unwrap();
    lease.release().unwrap();
    assert_eq!(undone.undone.len(), 3, "{:?}", undone.failed);
    assert_eq!(undone.failed.len(), 0);
    let note = std::fs::read_to_string(archive.root().join(NOTE_CARD)).unwrap();
    assert!(note.contains("An invented two-page note"), "{note}");
    assert!(
        archive
            .root()
            .join("Projects/Hardware/parts-list.csv")
            .exists(),
        "back where it was"
    );
    assert!(
        !archive
            .root()
            .join("Reading/Standards")
            .join("_collection.md")
            .exists()
    );
    let left = changes::recent(&History::open(&archive).unwrap().log().unwrap(), 0);
    assert_eq!(left.len(), 0);
}

#[test]
fn a_saved_conversation_is_kept_in_the_months_journal() {
    let (_dir, archive, holder) = sample();
    let (registry, _) = scripted(vec![
        tool("read", NOTE),
        answer(
            "The note gives the locking condition.",
            &[NOTE],
            json!([{"item": NOTE, "text": "This is the locking condition."}]),
            json!([]),
        ),
        answer("Only the one note.", &[], json!([]), json!([])),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    librarian
        .ask(&mut conversation, "What's the locking condition?")
        .unwrap();
    let journal_file = archive.root().join("Journal/2026/2026-10.md");
    let before = std::fs::read_to_string(&journal_file).unwrap();

    let at: jiff::Zoned = "2026-10-31T09:30:00-07:00[America/Los_Angeles]"
        .parse()
        .unwrap();
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    let saved = journal::save(&archive, &conversation, &at, &lease).unwrap();
    assert_eq!(saved.path, "Journal/2026/2026-10.md");
    assert!(saved.commit.is_some());
    let text = std::fs::read_to_string(&journal_file).unwrap();
    assert!(
        text.starts_with(before.trim_end()),
        "the sessions stay first"
    );
    assert!(text.contains("## Conversations with the librarian"));
    assert!(text.contains("### 2026-10-31 09:30 · What's the locking condition?"));
    assert!(text.contains("**You asked:** What's the locking condition?"));
    assert!(text.contains("**The librarian:** The note gives the locking condition."));
    assert!(
        text.contains(
            "- [A note on coupled oscillators](../../Reading/Coupled%20oscillators%20note.pdf.md)"
        ),
        "{text}"
    );
    assert!(text.contains("> This is the locking condition."));
    assert!(text.contains("scripted/m"), "it names the model");

    // A second conversation goes first, and regenerating the Journal keeps both.
    let mut second = Conversation::default();
    librarian.ask(&mut second, "Any other notes?").unwrap();
    let later: jiff::Zoned = "2026-10-31T10:00:00-07:00[America/Los_Angeles]"
        .parse()
        .unwrap();
    journal::save(&archive, &second, &later, &lease).unwrap();
    lease.release().unwrap();
    carrel_core::active::regenerate(&archive).unwrap();
    let text = std::fs::read_to_string(&journal_file).unwrap();
    let newer = text.find("Any other notes?").unwrap();
    let older = text.find("What's the locking condition?").unwrap();
    assert!(newer < older, "newest first:\n{text}");
    assert_eq!(
        text.matches("## Conversations with the librarian").count(),
        1
    );

    // A month with no sessions keeps its conversations too.
    let november: jiff::Zoned = "2026-11-02T08:00:00-08:00[America/Los_Angeles]"
        .parse()
        .unwrap();
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    journal::save(&archive, &second, &november, &lease).unwrap();
    lease.release().unwrap();
    carrel_core::active::regenerate(&archive).unwrap();
    let text = std::fs::read_to_string(archive.root().join("Journal/2026/2026-11.md")).unwrap();
    assert!(text.starts_with("# November 2026"), "{text}");
    assert!(text.contains("Any other notes?"));
    assert!(matches!(
        journal::save(
            &archive,
            &Conversation::default(),
            &november,
            &DiskLock::for_archive(&archive)
                .acquire(holder, "test", DEFAULT_LEASE)
                .unwrap()
        ),
        Err(journal::SaveError::Empty)
    ));
}

#[test]
fn the_model_is_the_chosen_one_else_the_largest_that_fits_else_apples() {
    let model = |provider: &'static str, offer: &str| {
        let mut scripted = Scripted::new(provider, 10, 8_000, Vec::new());
        scripted.offers[0].id = offer.into();
        Box::new(scripted)
    };
    let mut registry = Registry::new();
    registry.add(model("apple", "afm"));
    registry.add(model("llama", "qwen3.5-4b"));
    registry.add(model("llama2", "qwen3.5-9b"));
    let chosen = |setting, memory| {
        model::choose(&registry, setting, memory)
            .unwrap()
            .unwrap()
            .name()
    };
    assert_eq!(
        chosen(None, Some(32)),
        "llama2/qwen3.5-9b",
        "the largest that fits"
    );
    assert_eq!(
        chosen(None, Some(8)),
        "llama/qwen3.5-4b",
        "a 16 GB model doesn't fit in 8"
    );
    assert_eq!(chosen(None, None), "llama2/qwen3.5-9b");
    assert_eq!(chosen(None, Some(4)), "apple/afm", "else Apple's model");
    assert_eq!(
        chosen(Some("llama/qwen3.5-4b"), Some(32)),
        "llama/qwen3.5-4b",
        "the setting wins"
    );
    assert!(model::choose(&registry, Some("nope"), None).is_err());
    assert!(
        model::choose(&Registry::new(), None, None)
            .unwrap()
            .is_none()
    );
    let collection = Change::Collection {
        path: "A".into(),
        description: String::new(),
    };
    assert_eq!(collection.item(), None);
}

/// A registry with a scripted model whose context is `context` tokens.
fn scripted_with_context(
    context: u64,
    answers: Vec<Result<Value, carrel_core::providers::ProviderError>>,
) -> (
    Registry,
    std::sync::Arc<std::sync::Mutex<Vec<carrel_core::providers::CompletionRequest>>>,
) {
    let model = Scripted::new("scripted", 10, context, answers);
    let asked = model.asked.clone();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(model));
    (registry, asked)
}

/// The final answer gets more room than a step naming a tool, and a step whose reply was cut
/// off is asked again once with that room, rather than failing the question (#356).
#[test]
fn a_cut_off_reply_is_asked_again_with_room_for_a_whole_answer() {
    use carrel_core::providers::ProviderError;
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![
        tool("search", "lamp"),
        Err(ProviderError::Failed(
            "m's answer was cut off at 900 tokens before the JSON ended".into(),
        )),
        answer("The parts list.", &[PARTS], json!([]), json!([])),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "What's the lamp made of?")
        .unwrap();
    assert_eq!(answer.text, "The parts list.");
    let asked = asked.lock().unwrap();
    let room: Vec<_> = asked.iter().map(|r| r.max_tokens).collect();
    assert_eq!(room, [Some(900), Some(900), Some(2_400)]);
    assert_eq!(
        asked[1].prompt, asked[2].prompt,
        "the same step, asked again"
    );

    let (registry, asked) = scripted(vec![tool("search", "lamp"), tool("show", PARTS)]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_limits(Limits {
            steps: 2,
            ..Limits::default()
        });
    librarian
        .ask(&mut Conversation::default(), "What's the lamp made of?")
        .unwrap();
    assert_eq!(
        asked.lock().unwrap()[1].max_tokens,
        Some(2_400),
        "the last step, which must answer, has room for it"
    );
}

/// The same tool call again isn't run or counted as a step: the model is told it's already
/// done, with the earlier result (#356).
#[test]
fn a_repeated_call_is_answered_from_before_without_a_step() {
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![
        tool("search", "lamp parts"),
        tool("search", "  Lamp   PARTS "),
        tool("search", "lamp parts"),
        answer("The parts list.", &[PARTS], json!([]), json!([])),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_limits(Limits {
            steps: 2,
            ..Limits::default()
        });
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "What's the lamp made of?")
        .unwrap();
    assert_eq!(answer.text, "The parts list.", "two repeats cost no steps");
    assert_eq!(answer.steps.len(), 1, "the search ran once");
    let asked = asked.lock().unwrap();
    assert_eq!(asked.len(), 4);
    assert!(
        asked[2].prompt.contains("Already done"),
        "{}",
        asked[2].prompt
    );
    let again = asked[2].prompt.split("Already done").nth(1).unwrap();
    assert!(
        again.contains("Desk lamp parts list"),
        "with the earlier result: {again}"
    );

    // Repeats past as many as it has steps cost a step each, so it can't go round for ever.
    let (registry, asked) = scripted(vec![tool("search", "lamp"); 10]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_limits(Limits {
            steps: 2,
            ..Limits::default()
        });
    let mut conversation = Conversation::default();
    let answer = librarian.ask(&mut conversation, "Lamp?").unwrap();
    assert_eq!(answer.stopped, Some(Stopped::Steps));
    assert_eq!(asked.lock().unwrap().len(), 4);
}

/// Each step's prompt fits the model's context: older results are left out, then the newest
/// is cut short (#356). A model whose context overflows anyway is asked again once with what
/// was done shortened further.
#[test]
fn tool_results_are_trimmed_to_fit_the_models_context() {
    use carrel_core::providers::{ProviderError, estimate_tokens};
    let (_dir, archive, _) = sample();
    let context = 2_600;
    let (registry, asked) = scripted_with_context(
        context,
        vec![
            tool("collection", "Reading"),
            tool("collection", "Projects/Hardware"),
            tool("search", "oscillators"),
            tool("collection", "Projects"),
            answer("Two collections.", &[], json!([]), json!([])),
        ],
    );
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    librarian
        .ask(
            &mut Conversation::default(),
            "What's in Reading and Projects?",
        )
        .unwrap();
    let asked = asked.lock().unwrap();
    assert_eq!(asked.len(), 5);
    for request in asked.iter() {
        let used = estimate_tokens(request.system.as_deref().unwrap())
            + estimate_tokens(&request.prompt)
            + u64::from(request.max_tokens.unwrap());
        assert!(used <= context, "{used} tokens: {}", request.prompt);
    }
    assert!(
        asked[4].prompt.contains("left out, to fit"),
        "{}",
        asked[4].prompt
    );

    let (registry, asked) = scripted_with_context(
        context,
        vec![
            tool("collection", "Projects"),
            Err(ProviderError::Unsupported(
                "too long for Apple's on-device model: Exceeded model context window size".into(),
            )),
            answer("Reading holds papers.", &[], json!([]), json!([])),
        ],
    );
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask(&mut conversation, "What's in Reading?")
        .unwrap();
    assert_eq!(answer.text, "Reading holds papers.");
    let asked = asked.lock().unwrap();
    assert!(
        asked[2].prompt.len() < asked[1].prompt.len(),
        "shortened further"
    );
}

#[test]
fn the_journal_escapes_titles_and_shortens_a_long_question_in_its_heading() {
    use carrel_core::card::Sensitivity;
    use carrel_core::librarian::{Answer, ItemLink};
    let question = "Which of the draft notes about the coupled oscillators mention the locking condition, and when?";
    let conversation = Conversation {
        answers: vec![Answer {
            question: question.into(),
            text: "Only one.".into(),
            items: vec![ItemLink {
                id: Id::new_v7(),
                title: "[draft] notes | v2".into(),
                card: "Reading/Draft notes.md".into(),
                collection: "Reading".into(),
                sensitivity: Sensitivity::Personal,
            }],
            passages: Vec::new(),
            suggestions: Vec::new(),
            steps: Vec::new(),
            model: "scripted/m".into(),
            tokens: 0,
            stopped: None,
        }],
    };
    let at: jiff::Zoned = "2026-10-31T09:30:00-07:00[America/Los_Angeles]"
        .parse()
        .unwrap();
    let text = journal::markdown(&conversation, "Journal/2026", &at);
    let heading = text.lines().next().unwrap();
    assert!(
        heading.starts_with("### 2026-10-31 09:30 · Which of the draft notes"),
        "{heading}"
    );
    assert!(
        heading.ends_with('…'),
        "a long question is cut short: {heading}"
    );
    assert_eq!(
        heading.chars().count(),
        "### 2026-10-31 09:30 · ".chars().count() + 81
    );
    assert!(
        text.contains(&format!("**You asked:** {question}")),
        "in full below"
    );
    assert!(
        text.contains("- [\\[draft\\] notes \\| v2](../../Reading/Draft%20notes.md)"),
        "brackets and bars can't break the link: {text}"
    );
}

/// Review of #361: a call that differs from an earlier one only in case is a different call
/// when the tool tells them apart. Card paths resolve case-sensitively, so the lowercased path
/// fails and the right one must still run.
#[test]
fn a_call_differing_in_case_that_resolves_differently_is_run() {
    let (_dir, archive, _) = sample();
    let (registry, asked) = scripted(vec![
        tool("show", &NOTE_CARD.to_lowercase()),
        tool("show", NOTE_CARD),
        answer("The note.", &[NOTE], json!([]), json!([])),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let answer = librarian
        .ask(&mut Conversation::default(), "What's the oscillators note?")
        .unwrap()
        .clone();
    let asked = asked.lock().unwrap();
    assert!(
        !asked[2].prompt.contains("Already done"),
        "the right path was refused as a repeat: {}",
        asked[2].prompt
    );
    assert_eq!(answer.steps.len(), 2, "{:?}", answer.steps);
}

/// Review of #361: each step's prompt fits the model's context even when the conversation so
/// far is long. Only what was done is trimmed, never the earlier answers.
#[test]
fn a_long_conversation_still_fits_the_models_context() {
    use carrel_core::providers::estimate_tokens;
    let (_dir, archive, _) = sample();
    let context = 4_096;
    let long = "The lamp is made of brass and glass, with a cotton cord. ".repeat(50);
    let mut script: Vec<_> = (0..4)
        .map(|_| answer(&long, &[], json!([]), json!([])))
        .collect();
    script.push(tool("search", "lamp"));
    script.push(answer("Brass.", &[], json!([]), json!([])));
    let (registry, asked) = scripted_with_context(context, script);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    for _ in 0..5 {
        librarian
            .ask(&mut conversation, "What's the lamp made of?")
            .unwrap();
    }
    let asked = asked.lock().unwrap();
    for request in asked.iter().skip(4) {
        let used = estimate_tokens(request.system.as_deref().unwrap())
            + estimate_tokens(&request.prompt)
            + u64::from(request.max_tokens.unwrap());
        assert!(
            used <= context,
            "{used} tokens in a {context}-token context"
        );
    }
}

/// Review of #361: a final answer cut off on the last step doesn't fail the whole question
/// and lose what was found; it ends as running out does, with the items come across.
#[test]
fn a_cut_off_answer_on_the_last_step_keeps_what_was_found() {
    use carrel_core::providers::ProviderError;
    let (_dir, archive, _) = sample();
    let (registry, _) = scripted(vec![
        tool("search", "lamp"),
        Err(ProviderError::Failed(
            "m's answer was cut off at 2400 tokens before the JSON ended".into(),
        )),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_limits(Limits {
            steps: 2,
            ..Limits::default()
        });
    let mut conversation = Conversation::default();
    let answer = librarian.ask(&mut conversation, "What's the lamp made of?");
    let answer = answer.expect("the question failed outright");
    assert_ne!(answer.items, []);
}

#[test]
fn a_model_that_fails_on_the_last_step_fails_the_question_to_ask_again() {
    use carrel_core::providers::ProviderError;
    let (_dir, archive, _) = sample();
    let (registry, _) = scripted(vec![
        tool("search", "lamp"),
        Err(ProviderError::Failed("the model crashed".into())),
    ]);
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers)
        .unwrap()
        .with_limits(Limits {
            steps: 2,
            ..Limits::default()
        });
    let mut conversation = Conversation::default();
    let asked = librarian.ask(&mut conversation, "What's the lamp made of?");
    assert!(asked.is_err(), "only a cut-off answer ends as out of room");
    assert_eq!(conversation.answers, []);
}
