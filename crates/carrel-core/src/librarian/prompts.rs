//! Everything the librarian's model is told, in one place: its instructions, the JSON form it
//! answers in, and how each step's prompt is put together (`docs/design/librarian.md`).
//!
//! The model never calls tools itself. Each step, it answers in the form below, either asking
//! for a tool or giving its final answer, and the core runs the tool and asks again with the
//! result. So it works with every text model Carrel has, since all of them can answer to a
//! schema.

use std::fmt::Write as _;

use serde_json::{Value, json};

/// The tools the model may ask for, with what each takes, as the instructions list them.
pub const TOOLS: [(&str, &str); 6] = [
    (
        "search",
        "input: words to find, with filters if useful: collection:NAME, tag:NAME, kind:file|folder|bundle, after:YYYY[-MM[-DD]], before:YYYY[-MM[-DD]]. Finds items by their cards, their text and their meaning, with collections and glossary facts.",
    ),
    (
        "show",
        "input: an item (its id or card path). Its card: title, summary, tags, dates, collection, links.",
    ),
    (
        "related",
        "input: an item. The items it links to and those nearest it in meaning.",
    ),
    (
        "read",
        "input: an item. The passages of its text that best match the question. Restricted items can't be read.",
    ),
    (
        "history",
        "input: an item. Its changes and work sessions, newest first.",
    ),
    (
        "collection",
        "input: a collection's path. Its design document (what belongs in it) and its items.",
    ),
];

/// The librarian's instructions. `changes` says whether it may make changes on its own, which
/// only changes how sure it should say it is.
#[must_use]
pub fn system(changes: bool) -> String {
    let mut text = String::from(
        "You are the librarian of a personal archive: the owner's files, each described by a card, filed in collections. You answer the owner's questions about what's in it, using tools that read the archive. You can't see anything except through them.\n\n\
         How to work:\n\
         - Each step, answer with the JSON form: either an action naming one tool and its input, or the action \"answer\".\n\
         - Search first, then show, read or follow related items as the question needs. Don't guess: say what you found, and say plainly when you found nothing.\n\
         - Answer first, directly. Then be curious: in a sentence or two, offer what the owner might not have thought of: a related item they didn't ask about, a collection they may have forgotten, or a gap (\"you have the 2023 and 2025 statements but not 2024\"). Never instead of the answer.\n\
         - In \"items\", list the items your answer is about, by id or card path, as the tools gave them. In \"passages\", quote word for word the few passages you used (at most four), from what read or search returned, each with its item.\n\
         - Restricted items are found by their card only: never ask to read them, and never quote or guess their contents.\n\
         - Keep answers short and plain: a few sentences, or a short list.\n\n\
         Tools:\n",
    );
    for (name, what) in TOOLS {
        writeln!(text, "- {name}: {what}").unwrap();
    }
    text.push_str(
        "\nSuggestions: when you notice something wrong or missing in the catalog, add a suggestion with your final answer: refile an item to a better collection, correct a card's title, summary, created date or tags, rename an item, or create a collection for items that belong together, at most three. Give each one why, and how sure you are, from 0 to 1. Suggest only what the tools showed you. Never suggest deleting anything or changing a file's contents.\n",
    );
    if changes {
        text.push_str(
            "The owner lets you make the suggestions you're sure of yourself, each one undoable; the rest are shown to the owner to confirm. Only give a high confidence when the cards and text leave no real doubt.\n",
        );
    } else {
        text.push_str("Your suggestions are shown to the owner to confirm.\n");
    }
    text
}

/// The most items a final answer may list: more than any question needs, few enough that a
/// small model can't list them until it runs out of room.
pub const MAX_ITEMS: usize = 12;
/// The most passages a final answer may quote.
pub const MAX_PASSAGES: usize = 4;
/// The most changes a final answer may suggest.
pub const MAX_SUGGESTIONS: usize = 3;

/// What a step's prompt ends with when the model's reply to it was cut off, to ask for a
/// shorter one (#401).
pub const CUT_OFF: &str = "\nYour reply to this was cut off before it ended. Reply again, briefly: name a tool with a short input, or answer with the items first, then a few sentences, and no passages or suggestions.\n";

/// The actions the model may take: only `answer` when `must_answer` is set.
fn actions(must_answer: bool) -> Vec<&'static str> {
    if must_answer {
        vec!["answer"]
    } else {
        TOOLS
            .iter()
            .map(|(name, _)| *name)
            .chain(["answer"])
            .collect()
    }
}

/// The JSON Schema the model answers to, each step. When `must_answer` is set, `answer` is the
/// only action left. Every field is required, with an empty string or list for none: models
/// constrained by a grammar otherwise stop early.
///
/// The items come before the answer, and the lists are capped, so a model that goes on too
/// long has named what it found first, and can't list until it runs out of room (#401).
#[must_use]
pub fn schema(must_answer: bool) -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": actions(must_answer), "description": "A tool to run, or answer to give your final answer"},
            "input": {"type": "string", "description": "The tool's input; an empty string when answering"},
            "items": {"type": "array", "items": {"type": "string"}, "maxItems": MAX_ITEMS, "description": "The items the answer is about, by id or card path; empty when using a tool"},
            "answer": {"type": "string", "description": "Your final answer to the owner; an empty string when using a tool"},
            "passages": {
                "type": "array",
                "maxItems": MAX_PASSAGES,
                "description": "Passages quoted word for word from what the tools returned; empty when using a tool",
                "items": {
                    "type": "object",
                    "properties": {
                        "item": {"type": "string"},
                        "text": {"type": "string"}
                    },
                    "required": ["item", "text"]
                }
            },
            "suggestions": {
                "type": "array",
                "maxItems": MAX_SUGGESTIONS,
                "description": "Changes to the catalog you suggest; usually empty",
                "items": {
                    "type": "object",
                    "properties": {
                        "change": {"type": "string", "enum": ["refile", "correct", "rename", "collection"]},
                        "item": {"type": "string", "description": "The item, by id or card path; empty for a new collection"},
                        "field": {"type": "string", "description": "For correct: title, summary, created or tags; otherwise empty"},
                        "value": {"type": "string", "description": "refile: the collection's path; correct: the new value (tags as +add -remove); rename: the new name; collection: the new collection's path"},
                        "why": {"type": "string", "description": "Why, in a sentence; for a new collection, what belongs in it"},
                        "confidence": {"type": "number", "description": "How sure you are, from 0 to 1"}
                    },
                    "required": ["change", "item", "field", "value", "why", "confidence"]
                }
            }
        },
        "required": ["action", "input", "items", "answer", "passages", "suggestions"]
    })
}

/// The schema for a reply asked for again after it was cut off ([`CUT_OFF`]): as
/// [`schema`], without passages or suggestions.
#[must_use]
pub fn brief_schema(must_answer: bool) -> Value {
    let mut brief = schema(must_answer);
    for field in ["passages", "suggestions"] {
        if let Some(properties) = brief["properties"].as_object_mut() {
            properties.remove(field);
        }
    }
    brief["required"] = json!(["action", "input", "items", "answer"]);
    brief
}

/// What the model knows of the archive before it uses any tool.
#[derive(Debug, Clone, Default)]
pub struct Setting {
    /// Today's date.
    pub today: String,
    /// The archive's name.
    pub archive: String,
    /// How many items it has.
    pub items: usize,
    /// Its collections, by path, with what each is for.
    pub collections: Vec<(String, String)>,
}

/// How many collections the prompt lists.
const COLLECTIONS_LISTED: usize = 40;

/// The prompt for one step: the archive, the conversation so far, the question, what's been
/// done towards answering it, and how many steps are left.
#[must_use]
pub fn step(
    setting: &Setting,
    earlier: &[(String, String)],
    question: &str,
    done: &[String],
    left: usize,
) -> String {
    let mut text = format!(
        "Today is {}. The archive \"{}\" has {} items in {} collections",
        setting.today,
        setting.archive,
        setting.items,
        setting.collections.len()
    );
    text.push_str(":\n");
    for (path, purpose) in setting.collections.iter().take(COLLECTIONS_LISTED) {
        writeln!(text, "- {path}: {purpose}").unwrap();
    }
    if setting.collections.len() > COLLECTIONS_LISTED {
        writeln!(
            text,
            "- and {} more",
            setting.collections.len() - COLLECTIONS_LISTED
        )
        .unwrap();
    }
    if !earlier.is_empty() {
        text.push_str("\nThe conversation so far:\n");
        for (asked, answered) in earlier {
            writeln!(text, "Owner: {asked}\nLibrarian: {answered}").unwrap();
        }
    }
    writeln!(text, "\nThe owner asks: {question}").unwrap();
    if !done.is_empty() {
        text.push_str("\nWhat you've done so far for this answer:\n");
        for (n, step) in done.iter().enumerate() {
            writeln!(text, "\nStep {}: {step}", n + 1).unwrap();
        }
    }
    match left {
        0 | 1 => text.push_str("\nThis is your last step: answer now, with what you have.\n"),
        n => writeln!(
            text,
            "\nYou have {n} steps left, this one included. Use a tool, or answer."
        )
        .unwrap(),
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_step_can_only_answer() {
        assert_eq!(
            schema(true)["properties"]["action"]["enum"],
            json!(["answer"])
        );
        let all = schema(false);
        let actions = all["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), TOOLS.len() + 1);
        let prompt = step(&Setting::default(), &[], "what?", &[], 1);
        assert!(prompt.contains("last step"), "{prompt}");
    }

    #[test]
    fn the_instructions_ask_for_curiosity_after_the_answer() {
        let text = system(false);
        assert!(text.contains("Answer first"));
        assert!(text.contains("Never instead of the answer"));
        assert!(text.contains("Restricted items are found by their card only"));
        assert!(system(true).contains("make the suggestions you're sure of yourself"));
    }
}
