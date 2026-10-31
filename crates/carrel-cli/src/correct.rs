//! Typed corrections (`docs/design/corrections.md`): `carrel review correct ID "…"` for a
//! proposal. The owner's words, narrow or broad, become field changes, shown before and after
//! and saved only on a yes. The core prepares and saves the correction
//! ([`carrel_core::correct::prepare`]); this module words it and asks on the terminal.

use std::fmt::Write as _;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::Path;

use carrel_core::actions;
use carrel_core::archive::Archive;
use carrel_core::card_file::CardFile;
use carrel_core::correct::prepare;
use carrel_core::correct::{Fact, Fields, Worked, level};
use carrel_core::review::short_id;
use serde_json::json;

use crate::commands::locate;
use crate::{Failure, Report};

/// The owner's words: those given, or asked for on the terminal under the card's `title`.
fn words(given: &[String], title: &str) -> Result<String, Failure> {
    let joined = given.join(" ");
    if !joined.trim().is_empty() {
        return Ok(joined);
    }
    if !std::io::stdin().is_terminal() {
        return Err(Failure::new(
            "say what's wrong, such as `carrel review correct ID \"it's from 2019\"`",
        ));
    }
    eprintln!("Correcting “{title}”");
    eprintln!(
        "Say what's wrong (\"it's from 2019\", \"it's a picture of A, not B\"), or write \
         `field: value`, such as `created: 2019-03-31`. Enter on its own cancels."
    );
    eprint!("\nWhat's wrong with it? ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(Failure::new("nothing to correct"));
    }
    Ok(line.trim().to_owned())
}

/// Says a model is working, on a terminal, when the words aren't `field: value` lines: it
/// takes several seconds.
fn working(words: &str) {
    let lines = words.lines().all(|l| {
        l.split_once(':').is_some_and(|(f, _)| {
            matches!(
                f.trim().to_lowercase().as_str(),
                "title" | "summary" | "created" | "date" | "tags" | "sensitivity" | "collection"
            )
        })
    });
    if !lines && std::io::stderr().is_terminal() {
        eprintln!("Working out what that changes…");
    }
}

/// Each changed field before and after, and where it goes, for people.
fn describe(
    before: &Fields,
    after: &Fields,
    collection: Option<(&str, &str)>,
    worked: &Worked,
) -> String {
    let mut text = String::new();
    let mark = |dictated: bool| if dictated { "  (yours)" } else { "" };
    let changes = &worked.changes;
    if let Some(f) = &changes.title {
        writeln!(
            text,
            "  title        {}  →  {}{}",
            before.title,
            after.title,
            mark(f.dictated)
        )
        .unwrap();
    }
    if let Some(f) = &changes.summary {
        writeln!(
            text,
            "  summary      {}\n           →  {}{}",
            before.summary.as_deref().unwrap_or("(none)"),
            after.summary.as_deref().unwrap_or("(none)"),
            mark(f.dictated)
        )
        .unwrap();
    }
    if let Some(f) = &changes.created {
        let was = before
            .dates
            .as_ref()
            .and_then(|d| d.created.as_ref())
            .map_or("(none)", |d| d.0.as_str());
        writeln!(
            text,
            "  created      {was}  →  {}{}",
            f.value.0,
            mark(f.dictated)
        )
        .unwrap();
    }
    if let Some(f) = &changes.tags {
        let mut parts: Vec<String> = f.value.add.iter().map(|t| format!("+ {}", t.0)).collect();
        parts.extend(f.value.new.iter().map(|t| format!("+ {} (new)", t.0)));
        parts.extend(f.value.remove.iter().map(|t| format!("− {}", t.0)));
        writeln!(
            text,
            "  tags         {}{}",
            parts.join("  "),
            mark(f.dictated)
        )
        .unwrap();
    }
    if before.sensitivity != after.sensitivity {
        let dictated = changes.sensitivity.as_ref().is_some_and(|f| f.dictated);
        writeln!(
            text,
            "  sensitivity  {}  →  {}{}",
            level(before.sensitivity),
            level(after.sensitivity),
            mark(dictated)
        )
        .unwrap();
    }
    if let Some((from, to)) = collection {
        writeln!(text, "  collection   {from}  →  {to}").unwrap();
    }
    text
}

/// The change, with what comes with it: a sensitivity kept by a rule, tags left off, and a
/// lasting fact Carrel noticed.
fn notes_after(shown: &str, kept: bool, dropped: &[String], fact: Option<&Fact>) -> String {
    let mut out = shown.to_owned();
    if kept {
        out.push_str("  (kept at sensitive: a rule found a restricted value in it)\n");
    }
    if !dropped.is_empty() {
        writeln!(
            out,
            "  Tags {} don't apply there, so they're left off",
            dropped.join(", ")
        )
        .unwrap();
    }
    if let Some(fact) = fact {
        writeln!(
            out,
            "  Something lasting, for the glossary: {}: {}",
            fact.about, fact.fact
        )
        .unwrap();
    }
    out
}

/// Offers a lasting fact from a correction for the glossary: added with `learn`, asked about
/// on a terminal, otherwise left with a hint. Never added silently. Returns what happened.
fn offer_fact(archive: &Archive, fact: Option<&Fact>, add: bool) -> Result<String, Failure> {
    let Some(fact) = fact else {
        return Ok(String::new());
    };
    let agreed = if add {
        true
    } else if std::io::stdin().is_terminal() {
        eprint!("Add “{}” to the glossary? [y/N] ", fact.about);
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer)?;
        matches!(answer.trim(), "y" | "Y" | "yes" | "Yes")
    } else {
        return Ok("Not added to the glossary; run again with --learn to add it.\n".into());
    };
    if !agreed {
        return Ok(String::new());
    }
    Ok(learn(archive, fact)? + "\n")
}

/// Asks whether to save, unless `yes`. `None` when there's no terminal to ask on.
fn agree(yes: bool) -> Result<Option<bool>, Failure> {
    if yes {
        return Ok(Some(true));
    }
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    eprint!("Save this correction? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(Some(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes")))
}

/// A correction worked out and described, waiting for the owner's yes. The interactive mode
/// shows it in a pop-up; the commands print it and ask on the terminal.
#[derive(Debug)]
pub struct Prepared {
    /// The card's title, for a heading.
    pub title: String,
    /// Each change, and what comes with it, one per line; empty when it changes nothing.
    pub shown: String,
    /// A lasting fact to offer for the glossary once it's saved.
    pub fact: Option<Fact>,
    inner: prepare::Prepared,
}

/// What saving a correction did.
#[derive(Debug, Clone)]
pub struct Saved {
    /// For people.
    pub message: String,
    /// The history commit, for a filed item.
    pub commit: Option<String>,
    /// The card's path after, for a filed item.
    pub card: Option<String>,
}

impl Prepared {
    /// `prepared`, described for people.
    fn of(prepared: prepare::Prepared) -> Self {
        let moved = prepared
            .moved
            .as_ref()
            .map(|(from, to)| (from.as_str(), to.as_str()));
        let mut shown = describe(&prepared.before, &prepared.after, moved, &prepared.worked);
        if !shown.is_empty() {
            shown = notes_after(
                &shown,
                prepared.kept_sensitive,
                &prepared.dropped,
                prepared.fact(),
            );
        }
        Self {
            title: prepared.title.clone(),
            shown,
            fact: prepared.fact().cloned(),
            inner: prepared,
        }
    }

    /// Whether it changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// Everything about it, as JSON, for `--json`.
    fn json(&self) -> serde_json::Value {
        let prepared = &self.inner;
        json!({
            "changed": prepared.changed,
            "collection": prepared.to(),
            "kept_sensitive": prepared.kept_sensitive,
            "fact": self.fact,
            "model": prepared.worked.model,
            "changes": prepared.worked.changes,
        })
    }
}

/// Works out what `words` change in the proposal `id`, without saving anything.
///
/// # Errors
///
/// [`Failure`] if the proposal isn't found, or the correction can't be worked out.
pub fn prepare_review(archive: &Archive, id: &str, words: &str) -> Result<Prepared, Failure> {
    let (registry, _) = crate::providers::registry();
    Ok(Prepared::of(prepare::prepare_review(
        archive, &registry, id, words,
    )?))
}

/// Works out what `words` change in the filed item `query`, without saving anything.
///
/// # Errors
///
/// [`Failure`] if the item isn't found, or the correction can't be worked out.
pub fn prepare_filed(archive: &Archive, query: &str, words: &str) -> Result<Prepared, Failure> {
    let items = archive.items()?;
    let item = crate::commands::find_item(archive, &items, query)?.clone();
    let (registry, _) = crate::providers::registry();
    Ok(Prepared::of(prepare::prepare_filed(
        archive, &registry, item, words,
    )?))
}

/// Saves a prepared correction: a proposal back to the queue, or a filed item's card as one
/// change, refiled when its collection changed.
///
/// # Errors
///
/// [`Failure`] if it can't be written or committed.
pub fn save(archive: &Archive, prepared: Prepared) -> Result<Saved, Failure> {
    let saved = prepare::save(archive, prepared.inner, || {
        crate::config::Identity::load_or_create().map(|i| crate::commands::holder(&i))
    })?;
    let message = if saved.filed {
        format!(
            "Saved the correction to “{}”; undo it from History or with `carrel undo`.",
            saved.title
        )
    } else {
        format!("Saved the correction to “{}”.", saved.title)
    };
    Ok(Saved {
        message,
        commit: saved.commit,
        card: saved.card,
    })
}

/// Adds `fact` to the glossary, as the owner agreed.
///
/// # Errors
///
/// [`Failure`] if the glossary can't be written or committed.
pub fn learn(archive: &Archive, fact: &Fact) -> Result<String, Failure> {
    let identity = crate::config::Identity::load_or_create()?;
    actions::learn(
        archive,
        &fact.about,
        &fact.fact,
        crate::commands::holder(&identity),
    )?;
    Ok(format!("Added “{}” to the glossary.", fact.about))
}

/// Shows a prepared correction, asks for the yes (unless `yes`), saves it, and offers its fact.
fn finish(
    archive: &Archive,
    heading: String,
    prepared: Prepared,
    yes: bool,
    learn_fact: bool,
    mut json: serde_json::Value,
) -> Result<Report, Failure> {
    let mut out = heading;
    if prepared.is_empty() {
        out.push_str("It changes nothing.\n");
        json["saved"] = json!(false);
        return Ok(Report::new(out, json));
    }
    out.push_str(&prepared.shown);
    if let serde_json::Value::Object(map) = prepared.json() {
        for (k, v) in map {
            json[k] = v;
        }
    }
    // When asking, the change is shown first, on the terminal; otherwise it's in the report.
    let asking = !yes && std::io::stdin().is_terminal();
    if asking {
        eprint!("{out}");
        out.clear();
    }
    let fact = prepared.fact.clone();
    match agree(yes)? {
        Some(true) => {
            if let Some(entry) = prepared.inner.proposal() {
                json["entry"] = serde_json::to_value(entry).unwrap_or_default();
            }
            let saved = save(archive, prepared)?;
            json["saved"] = json!(true);
            json["commit"] = json!(saved.commit);
            if let Some(card) = &saved.card {
                json["card"] = json!(card);
            }
            writeln!(out, "{}", saved.message).unwrap();
            out.push_str(&offer_fact(archive, fact.as_ref(), learn_fact)?);
        }
        Some(false) => {
            json["saved"] = json!(false);
            out.push_str("Not saved.\n");
        }
        None => {
            json["saved"] = json!(false);
            out.push_str("Not saved: there's no terminal to ask on; add --yes to save it.\n");
        }
    }
    Ok(Report::new(out, json))
}

/// `carrel review correct ID [WORDS...] [--yes] [--learn]`.
///
/// # Errors
///
/// [`Failure`] if the proposal isn't found, or the correction can't be worked out or saved.
pub fn review(
    archive: Option<&Path>,
    id: &str,
    given: &[String],
    yes: bool,
    learn_fact: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let entry = actions::proposals(&archive, &[id.to_owned()])?
        .pop()
        .ok_or_else(|| Failure::new(format!("no proposal {id}")))?;
    let words = words(given, &entry.card.title)?;
    working(&words);
    let prepared = prepare_review(&archive, id, &words)?;
    let heading = format!("Correcting {} ({})\n", short_id(entry.id), entry.card.title);
    finish(
        &archive,
        heading,
        prepared,
        yes,
        learn_fact,
        json!({"proposal": entry.id}),
    )
}

/// The item's text, as drafting read it, for the model's context; empty when it can't be read.
pub(crate) fn item_text(
    archive: &Archive,
    registry: &carrel_core::providers::Registry,
    path: &Path,
    kind: carrel_core::card::Kind,
) -> String {
    prepare::item_text(archive, registry, path, kind)
}

/// `carrel correct ITEM [WORDS...] [--yes] [--learn]`: corrects a filed item's card, and
/// refiles it when the collection changes, as one undoable change.
///
/// # Errors
///
/// [`Failure`] if the item isn't found, or the correction can't be worked out or saved.
pub fn filed(
    archive: Option<&Path>,
    query: &str,
    given: &[String],
    yes: bool,
    learn_fact: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = crate::commands::find_item(&archive, &items, query)?.clone();
    let title = CardFile::parse(&std::fs::read_to_string(archive.root().join(&item.card))?)?
        .card()
        .title
        .clone();
    let words = words(given, &title)?;
    working(&words);
    let prepared = prepare_filed(&archive, query, &words)?;
    let json = json!({"item": item.id, "card": item.card});
    finish(
        &archive,
        format!("Correcting {title}\n"),
        prepared,
        yes,
        learn_fact,
        json,
    )
}
