//! Typed corrections (`docs/design/corrections.md`): `carrel review correct ID "…"` for a
//! proposal. The owner's words, narrow or broad, become field changes, shown before and after
//! and saved only on a yes.

use std::fmt::Write as _;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};

use carrel_core::actions;
use carrel_core::archive::Archive;
use carrel_core::card_file::CardFile;
use carrel_core::correct::{
    Correctable as _, Fact, Fields, Subject, Worked, level, notes, work_out,
};
use carrel_core::ingest::card::Vocabulary;
use carrel_core::review::{ReviewQueue, short_id};
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

/// The collections as the model sees them, by path with what each is for.
fn described(collections: &[carrel_core::archive::Collection]) -> Vec<(String, String)> {
    collections
        .iter()
        .map(|c| {
            let purpose = c
                .doc
                .description
                .clone()
                .unwrap_or_else(|| c.doc.title.clone());
            (c.path.clone(), purpose)
        })
        .collect()
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
    words: String,
    worked: Worked,
    changed: Vec<&'static str>,
    kept_sensitive: bool,
    to: Option<String>,
    pending: Pending,
}

/// What a prepared correction saves.
#[derive(Debug)]
enum Pending {
    /// A proposal, corrected.
    Proposal(Box<carrel_core::review::ReviewEntry>),
    /// A filed item's card, corrected, refiled when `to` is set.
    Filed {
        item: carrel_core::archive::ItemRef,
        card_path: PathBuf,
        file: Box<CardFile>,
    },
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
    /// Whether it changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// Everything about it, as JSON, for `--json`.
    fn json(&self) -> serde_json::Value {
        json!({
            "changed": self.changed,
            "collection": self.to,
            "kept_sensitive": self.kept_sensitive,
            "fact": self.fact,
            "model": self.worked.model,
            "changes": self.worked.changes,
        })
    }
}

/// The context a correction is worked out in: the archive's collections and tags, and its
/// text models.
struct Context {
    registry: carrel_core::providers::Registry,
    collections: Vec<carrel_core::archive::Collection>,
    described: Vec<(String, String)>,
    vocabulary: Vocabulary,
}

impl Context {
    fn of(archive: &Archive) -> Result<Self, Failure> {
        let (registry, _) = crate::providers::registry();
        let collections = archive.collections()?;
        Ok(Self {
            registry,
            vocabulary: Vocabulary::of(archive, &collections),
            described: described(&collections),
            collections,
        })
    }
}

/// Works out what `words` change in the proposal `id`, without saving anything.
///
/// # Errors
///
/// [`Failure`] if the proposal isn't found, or the correction can't be worked out.
pub fn prepare_review(archive: &Archive, id: &str, words: &str) -> Result<Prepared, Failure> {
    let entry = actions::proposals(archive, &[id.to_owned()])?
        .pop()
        .ok_or_else(|| Failure::new(format!("no proposal {id}")))?;
    let context = Context::of(archive)?;
    let before = entry.card.fields();
    let earlier: Vec<String> = notes(&before).into_iter().map(|n| n.said).collect();
    let text = item_text(archive, &context.registry, &entry.source.path, entry.kind);
    let subject = Subject {
        card: &before,
        collection: &entry.destination.path,
        text: &text,
        collections: &context.described,
        vocabulary: &context.vocabulary,
    };
    let worked = work_out(&context.registry, &subject, words, &earlier)?;
    let corrected = actions::correct_proposal(archive, &context.registry, &entry, &worked, words)?;
    let after = corrected.entry.card.fields();
    let moved = corrected
        .to
        .as_deref()
        .map(|to| (entry.destination.path.as_str(), to));
    let mut shown = describe(&before, &after, moved, &worked);
    if !shown.is_empty() {
        shown = notes_after(
            &shown,
            corrected.kept_sensitive,
            &corrected.dropped,
            worked.changes.fact.as_ref(),
        );
    }
    Ok(Prepared {
        title: entry.card.title.clone(),
        shown,
        fact: worked.changes.fact.clone(),
        words: words.to_owned(),
        changed: corrected.changed,
        kept_sensitive: corrected.kept_sensitive,
        to: corrected.to,
        worked,
        pending: Pending::Proposal(Box::new(corrected.entry)),
    })
}

/// Works out what `words` change in the filed item `query`, without saving anything.
///
/// # Errors
///
/// [`Failure`] if the item isn't found, or the correction can't be worked out.
pub fn prepare_filed(archive: &Archive, query: &str, words: &str) -> Result<Prepared, Failure> {
    let items = archive.items()?;
    let item = crate::commands::find_item(archive, &items, query)?.clone();
    let card_path = archive.root().join(&item.card);
    let mut file = CardFile::parse(&std::fs::read_to_string(&card_path)?)?;
    let context = Context::of(archive)?;
    let before = file.card().fields();
    let earlier: Vec<String> = notes(&before).into_iter().map(|n| n.said).collect();
    let here = item.card.rsplit_once('/').map_or("", |(f, _)| f).to_owned();
    let contents = item.item.as_ref().map(|p| archive.root().join(p));
    let text = contents
        .as_deref()
        .map(|p| item_text(archive, &context.registry, p, item.kind))
        .unwrap_or_default();
    let subject = Subject {
        card: &before,
        collection: &here,
        text: &text,
        collections: &context.described,
        vocabulary: &context.vocabulary,
    };
    let worked = work_out(&context.registry, &subject, words, &earlier)?;
    let (after, changed, kept_sensitive) =
        corrected(&before, &worked, words, contents.as_deref(), &text);
    let to = target_of(
        archive,
        &context.registry,
        &context.collections,
        &item,
        contents.as_deref(),
        &after,
        &worked,
    )
    .filter(|t| *t != here);
    let mut shown = describe(
        &before,
        &after,
        to.as_deref().map(|t| (here.as_str(), t)),
        &worked,
    );
    if !shown.is_empty() {
        shown = notes_after(&shown, kept_sensitive, &[], worked.changes.fact.as_ref());
    }
    file.card_mut().set_fields(after);
    Ok(Prepared {
        title: before.title.clone(),
        shown,
        fact: worked.changes.fact.clone(),
        words: words.to_owned(),
        changed,
        kept_sensitive,
        to,
        worked,
        pending: Pending::Filed {
            item,
            card_path,
            file: Box::new(file),
        },
    })
}

/// Saves a prepared correction: a proposal back to the queue, or a filed item's card as one
/// change, refiled when its collection changed.
///
/// # Errors
///
/// [`Failure`] if it can't be written or committed.
pub fn save(archive: &Archive, prepared: Prepared) -> Result<Saved, Failure> {
    match prepared.pending {
        Pending::Proposal(entry) => {
            ReviewQueue::of(archive).save(&entry)?;
            Ok(Saved {
                message: format!("Saved the correction to “{}”.", entry.card.title),
                commit: None,
                card: None,
            })
        }
        Pending::Filed {
            item,
            card_path,
            file,
        } => {
            let title = file.card().title.clone();
            let (card, commit) = save_filed(
                archive,
                &item,
                &card_path,
                *file,
                prepared.to.as_deref(),
                &prepared.words,
                &prepared.changed,
            )?;
            Ok(Saved {
                message: format!(
                    "Saved the correction to “{title}”; undo it from History or with `carrel undo`."
                ),
                commit,
                card: Some(card),
            })
        }
    }
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
            if let Pending::Proposal(entry) = &prepared.pending {
                json["entry"] = serde_json::to_value(entry.as_ref()).unwrap_or_default();
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
    let Ok(rules) = archive.rules() else {
        return String::new();
    };
    let candidate = carrel_core::ingest::Candidate {
        path: path.to_owned(),
        kind,
        sealed: None,
        markers: Vec::new(),
        checksum: carrel_core::card::Checksum(String::new()),
        size: 0,
        duplicate_of: None,
        likely: Vec::new(),
        same_as: None,
        inside: None,
    };
    carrel_core::ingest::text::read(registry, &rules, &candidate).text
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

/// `before` with the correction applied, the fields changed, and whether a lower sensitivity
/// was kept at sensitive because a rule finds a restricted value in the item at `contents`,
/// read as `text`.
fn corrected(
    before: &Fields,
    worked: &Worked,
    words: &str,
    contents: Option<&Path>,
    text: &str,
) -> (Fields, Vec<&'static str>, bool) {
    use carrel_core::card::Sensitivity;
    use carrel_core::ingest::sensitivity::{floor, restricted_by_rule};

    let mut after = before.clone();
    let at = carrel_core::card::DateOrTime(
        jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    );
    let changed = carrel_core::correct::apply(&mut after, worked, words, &at);
    let name = contents
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kept = after.sensitivity < Sensitivity::Sensitive
        && floor(&name, text, Sensitivity::Public)
            .reasons
            .iter()
            .any(|r| restricted_by_rule(r));
    if kept {
        after.sensitivity = Sensitivity::Sensitive;
    }
    (after, changed, kept)
}

/// Writes a filed item's corrected card, refiling it into `target` when there is one, as one
/// change described with the owner's words. Returns the card's path and the commit.
fn save_filed(
    archive: &Archive,
    item: &carrel_core::archive::ItemRef,
    card_path: &Path,
    file: CardFile,
    target: Option<&str>,
    words: &str,
    changed: &[&str],
) -> Result<(String, Option<String>), Failure> {
    use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
    use carrel_core::history::{Change, History, Operation};

    let mut said: String = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if said.chars().count() > 200 {
        said = said.chars().take(200).collect::<String>() + "…";
    }
    let summary = format!(
        "Correct “{}”: {}. Owner: {said}",
        file.card().title,
        changed.join(", ")
    );
    let identity = crate::config::Identity::load_or_create()?;
    let lease = DiskLock::for_archive(archive).acquire(
        crate::commands::holder(&identity),
        "correct",
        DEFAULT_LEASE,
    )?;
    let result = (|| -> Result<(String, Option<String>), Failure> {
        if let Some(to) = target {
            let moved = carrel_core::rename::refile(archive, item, to, file, &summary, &lease)
                .map_err(|e| Failure::new(e.to_string()))?;
            return Ok((moved.card, moved.commit));
        }
        let temp = card_path.with_extension("md.tmp-correct");
        std::fs::write(&temp, file.to_text()?)?;
        std::fs::rename(&temp, card_path)?;
        let mut change = Change::new(
            Operation::Edit,
            summary,
            lease.info().person,
            lease.info().device,
        );
        change.items = vec![item.id];
        Ok((
            item.card.clone(),
            History::open(archive)?.commit(archive, &change, &lease)?,
        ))
    })();
    lease.release()?;
    result
}

/// The collection a correction sends a filed item to: one the owner named, or, when what it is
/// changed, wherever it's routed now.
fn target_of(
    archive: &Archive,
    registry: &carrel_core::providers::Registry,
    collections: &[carrel_core::archive::Collection],
    item: &carrel_core::archive::ItemRef,
    contents: Option<&Path>,
    fields: &Fields,
    worked: &Worked,
) -> Option<String> {
    if let Some(named) = &worked.changes.collection {
        return Some(named.value.clone());
    }
    worked
        .changes
        .reroute
        .then(|| reroute(archive, registry, collections, item, contents, fields))
        .flatten()
}

/// Where a corrected item belongs now, routed again from its corrected card.
fn reroute(
    archive: &Archive,
    registry: &carrel_core::providers::Registry,
    collections: &[carrel_core::archive::Collection],
    item: &carrel_core::archive::ItemRef,
    contents: Option<&Path>,
    fields: &Fields,
) -> Option<String> {
    use carrel_core::ingest::route::{Router, RoutingRules};
    let rules = RoutingRules::of(archive, collections);
    let router = Router::new(archive, collections, &rules);
    let path = contents.map_or_else(|| archive.root().join(&item.card), Path::to_owned);
    let candidate = carrel_core::ingest::Candidate {
        path: path.clone(),
        kind: item.kind,
        sealed: None,
        markers: Vec::new(),
        checksum: carrel_core::card::Checksum(String::new()),
        size: 0,
        duplicate_of: None,
        likely: Vec::new(),
        same_as: None,
        inside: None,
    };
    let draft = carrel_core::ingest::card::Draft {
        title: fields.title.clone(),
        summary: fields.summary.clone(),
        tags: fields.tags.clone(),
        new_tag: None,
        created: fields.dates.as_ref().and_then(|d| d.created.clone()),
        generated: None,
        attempts: Vec::new(),
        problems: Vec::new(),
    };
    let source = path.parent().unwrap_or(&path).to_owned();
    let route = router.route(registry, &candidate, &source, &draft);
    // Only somewhere it fits: an unsure answer, or Holding, leaves it where it is.
    (route.collection.is_some()
        && route.confidence != carrel_core::review::Confidence::Low
        && route.path != "Holding")
        .then_some(route.path)
}
