//! Asking the librarian about what the owner is looking at (#460), with a scripted model: the
//! item, collection, search or Discover view is loaded up front and named in the prompt, so
//! "this" means it, and every interface finds it the same way.

mod common;

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::flags::Flaggable;
use carrel_core::librarian::{About, Conversation, Librarian, journal};
use carrel_core::providers::device::AvailableModel;
use carrel_core::providers::{CompletionRequest, ProviderError, Registry};
use common::Scripted;
use serde_json::{Value, json};

const PARTS: &str = "01a18701-0500-776a-9867-080d24773181";
const PARTS_CARD: &str = "Projects/Hardware/parts-list.csv.md";

type Asked = std::sync::Arc<std::sync::Mutex<Vec<CompletionRequest>>>;

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

fn sample() -> (tempfile::TempDir, Archive) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    let archive = Archive::open(&root).unwrap();
    (dir, archive)
}

#[allow(clippy::unnecessary_wraps)] // the scripted model's answers are results
fn reply(action: &str, input: &str, answer: &str, items: &[&str]) -> Result<Value, ProviderError> {
    Ok(
        json!({"action": action, "input": input, "answer": answer, "items": items,
              "passages": [], "suggestions": []}),
    )
}

fn scripted(answers: Vec<Result<Value, ProviderError>>) -> (Registry, Asked) {
    let model = Scripted::new("scripted", 10, 8_000, answers);
    let asked = model.asked.clone();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(model));
    (registry, asked)
}

fn the_model() -> AvailableModel {
    AvailableModel {
        provider: "scripted".into(),
        offer: "m".into(),
        label: "scripted/m".into(),
    }
}

fn parts() -> About {
    About::Item(Id(uuid::Uuid::parse_str(PARTS).unwrap()))
}

#[test]
fn asking_about_an_item_loads_its_card_and_collection_so_this_means_it() {
    let (_dir, archive) = sample();
    let (registry, asked) = scripted(vec![
        reply("show", PARTS, "", &[]),
        reply(
            "answer",
            "",
            "Yes: it's a build's parts list, and Hardware is for physical builds.",
            &[],
        ),
    ]);
    let providers = Registry::with_built_ins;
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    let answer = librarian
        .ask_about(
            &mut conversation,
            "Is this in the right place?",
            Some(&parts()),
        )
        .unwrap()
        .clone();

    let asked = asked.lock().unwrap();
    let prompt = &asked[0].prompt;
    assert!(
        prompt.contains(
            "The owner is looking at the item “Desk lamp parts list” (Projects/Hardware/parts-list.csv.md) as they ask, so \"this\" and \"here\" mean it."
        ),
        "{prompt}"
    );
    assert!(
        prompt.contains("summary: Bill of materials for the desk lamp build."),
        "its card is loaded up front: {prompt}"
    );
    assert!(
        prompt.contains(
            "Its collection: The collection Projects/Hardware (Hardware): Physical builds"
        ),
        "and its collection's design document: {prompt}"
    );
    assert!(
        prompt.find("looking at") < prompt.find("The owner asks:"),
        "before the question"
    );
    assert!(
        asked[1]
            .prompt
            .contains("Already done, so it wasn't run again"),
        "asking for the card again is answered from what was loaded: {}",
        asked[1].prompt
    );
    assert_eq!(
        answer.about.as_deref(),
        Some("the item “Desk lamp parts list” (Projects/Hardware/parts-list.csv.md)")
    );
    assert!(
        answer.steps.is_empty(),
        "loading isn't a step, nor is a repeat"
    );

    // The Journal says what it was about.
    let text = journal::markdown(&conversation, "Journal/2026", &jiff::Zoned::now());
    assert!(
        text.contains("**You asked,** about the item “Desk lamp parts list” (Projects/Hardware/parts-list.csv.md): Is this in the right place?"),
        "{text}"
    );
}

#[test]
fn asking_about_a_collection_a_search_or_a_discover_view_says_so() {
    let (_dir, archive) = sample();
    let (registry, asked) = scripted(vec![
        reply("answer", "", "It's tidy.", &[]),
        reply("answer", "", "Two.", &[]),
        reply("answer", "", "Today's.", &[]),
        reply("answer", "", "Nothing in view.", &[]),
    ]);
    let providers = Registry::with_built_ins;
    let librarian = Librarian::new(&archive, &registry, the_model(), &providers).unwrap();
    let mut conversation = Conversation::default();
    let hardware = About::Collection("Projects/Hardware".into());
    librarian
        .ask_about(&mut conversation, "Anything missing here?", Some(&hardware))
        .unwrap();
    let search = About::Search("lamp".into());
    librarian
        .ask_about(&mut conversation, "How many is this?", Some(&search))
        .unwrap();
    let today = About::Discover("on this day".into());
    librarian
        .ask_about(&mut conversation, "Why these?", Some(&today))
        .unwrap();
    librarian.ask(&mut conversation, "And now?").unwrap();

    let asked = asked.lock().unwrap();
    assert!(
        asked[0]
            .prompt
            .contains("looking at the collection “Hardware” (Projects/Hardware) as they ask"),
        "{}",
        asked[0].prompt
    );
    assert!(asked[0].prompt.contains("Its design document:\n# Hardware"));
    assert!(
        asked[0].prompt.contains("Desk lamp parts list (id"),
        "its items"
    );
    assert!(
        asked[1]
            .prompt
            .contains("looking at what a search for “lamp” found"),
        "{}",
        asked[1].prompt
    );
    assert!(asked[1].prompt.contains("matched"), "{}", asked[1].prompt);
    assert!(
        asked[2]
            .prompt
            .contains("looking at Discover's on this day view")
    );
    assert!(
        !asked[3].prompt.contains("looking at"),
        "a question without it says nothing"
    );
    let about: Vec<Option<&str>> = conversation
        .answers
        .iter()
        .map(|a| a.about.as_deref())
        .collect();
    assert_eq!(about[3], None);
}

#[test]
fn every_interface_finds_what_its_about_the_same_way() {
    let (_dir, archive) = sample();
    assert_eq!(About::find(&archive, PARTS).unwrap(), parts());
    assert_eq!(About::find(&archive, PARTS_CARD).unwrap(), parts());
    assert_eq!(
        About::find(&archive, "desk lamp parts list").unwrap(),
        parts()
    );
    assert_eq!(
        About::find(&archive, "Projects/Hardware/").unwrap(),
        About::Collection("Projects/Hardware".into())
    );
    assert_eq!(
        About::find(&archive, "hardware").unwrap(),
        About::Collection("Projects/Hardware".into()),
        "a collection by its title"
    );
    assert!(About::find(&archive, "nowhere at all").is_err());
    assert!(About::find(&archive, " ").is_err());

    assert_eq!(parts().title(&archive), "Desk lamp parts list");
    assert_eq!(
        About::Collection("Projects/Hardware".into()).title(&archive),
        "Hardware"
    );

    // A flag hands the librarian what was flagged (#454).
    let flagged = Flaggable::Collection {
        path: "Projects/Hardware".into(),
    };
    assert_eq!(
        About::from(&flagged),
        About::Collection("Projects/Hardware".into())
    );
    assert_eq!(
        serde_json::to_value(parts()).unwrap(),
        json!({"item": PARTS}),
        "for programs"
    );
}
