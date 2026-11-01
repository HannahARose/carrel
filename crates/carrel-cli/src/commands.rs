//! The commands. Each returns a [`Report`] and leaves printing to `main`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::Access;
use carrel_core::archive::{Archive, ArchiveError, InitOptions, ItemRef};
use carrel_core::card::{Card, Id};
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder, LockInfo};
use carrel_core::history::{Entry, History, HistoryError, Operation};
use carrel_core::status::{Happened, Looking, Recent};
use serde_json::{Value, json};

use crate::config::{Identity, Prefs};
use crate::{Failure, Report};

const SYNCED_WARNING: &str = "this archive is in a synced folder; until iCloud support \
     arrives, use it from one device at a time";

/// Finds the archive to work on: `--archive`, else the one containing the current folder,
/// else the one chosen with `carrel open`.
pub fn locate(archive: Option<&Path>) -> Result<Archive, Failure> {
    if let Some(path) = archive {
        return Ok(Archive::find(path)?);
    }
    let cwd = std::env::current_dir().map_err(|e| Failure::io(Path::new("."), &e))?;
    match Archive::find(&cwd) {
        Ok(found) => return Ok(found),
        Err(ArchiveError::NotAnArchive(_)) => {}
        Err(e) => return Err(e.into()),
    }
    match Prefs::load()?.archive {
        Some(path) => Archive::open(&path).map_err(|e| {
            Failure::new(format!(
                "the archive chosen with `carrel open` can't be opened: {e}"
            ))
        }),
        None => Err(Failure::new(
            "not inside an archive; use --archive, or choose one with `carrel open`",
        )),
    }
}

fn access_name(access: Access) -> &'static str {
    match access {
        Access::ReadWrite => "read-write",
        Access::ReadOnly => "read-only",
    }
}

/// The archive's root as an absolute path, for `--json` output whose paths are relative to it,
/// so an editor can open them.
pub(crate) fn root_of(archive: &Archive) -> PathBuf {
    std::fs::canonicalize(archive.root()).unwrap_or_else(|_| archive.root().to_path_buf())
}

fn archive_json(archive: &Archive) -> Value {
    json!({
        "name": archive.doc().name,
        "id": archive.info().id,
        "root": archive.root(),
        "format": archive.info().format.to_string(),
        "access": access_name(archive.access()),
        "created": archive.info().created,
    })
}

/// `carrel init`: creates an archive searched with `embedding_model`, or starts history for an
/// existing archive that has none (one made by hand or by another program).
pub fn init(
    path: Option<&Path>,
    name: Option<String>,
    allow_existing_files: bool,
    embedding_model: &str,
) -> Result<Report, Failure> {
    let embedding_choices = carrel_core::settings::Key::EmbeddingModel
        .choices()
        .unwrap_or_default();
    if !embedding_choices.contains(&embedding_model) {
        return Err(Failure::new(format!(
            "no embedding model {embedding_model}; choose one of {}",
            embedding_choices.join(", ")
        )));
    }
    let root = match path {
        Some(path) => path.to_owned(),
        None => std::env::current_dir().map_err(|e| Failure::io(Path::new("."), &e))?,
    };
    let identity = Identity::load_or_create()?;
    let existing = root.join("_System/archive.json").exists();
    let archive = if existing {
        let archive = Archive::open(&root)?;
        match History::open(&archive) {
            Ok(_) => {
                return Err(Failure::new(format!(
                    "{} is already an archive, and its history has started",
                    archive.root().display()
                )));
            }
            Err(HistoryError::NotInitialized) => {}
            Err(e) => return Err(e.into()),
        }
        if name.is_some() {
            return Err(Failure::new(format!(
                "this archive is already named “{}”; leave out --name to start its history",
                archive.doc().name
            )));
        }
        archive
    } else {
        let name = match name {
            Some(name) => name,
            None => std::path::absolute(&root)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .ok_or_else(|| Failure::new("give the archive a name with --name"))?,
        };
        let mut options = InitOptions::new(name);
        options.allow_existing_files = allow_existing_files;
        options.embedding_model = Some(embedding_model.to_owned());
        Archive::init(&root, &options)?
    };
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "create", DEFAULT_LEASE)?;
    if existing {
        History::start(&archive, &lease)?;
    } else {
        History::init(&archive, &lease)?;
    }
    lease.release()?;
    if let Ok(root) = std::fs::canonicalize(archive.root()) {
        let mut prefs = Prefs::load()?;
        prefs.note_archive(&root);
        prefs.save()?;
    }

    let mut warnings = Vec::new();
    if DiskLock::is_synced_folder(archive.root()) {
        warnings.push(SYNCED_WARNING);
    }
    let mut text = if existing {
        format!(
            "Started history for archive “{}” in {}\n",
            archive.doc().name,
            archive.root().display()
        )
    } else {
        format!(
            "Created archive “{}” in {}\n",
            archive.doc().name,
            archive.root().display()
        )
    };
    let model = archive.settings().embedding_model();
    if let Some(model) = model.filter(|_| !existing) {
        writeln!(
            text,
            "Every device searches it with {model}; `carrel models download {model}` gets it"
        )
        .unwrap();
    }
    for warning in &warnings {
        writeln!(text, "warning: {warning}").unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "archive": archive_json(&archive),
            "history": if existing { "started" } else { "created" },
            "embedding_model": model,
            "warnings": warnings,
        }),
    ))
}

pub(crate) fn holder(identity: &Identity) -> Holder {
    identity.holder()
}

/// `carrel open`: with a path, makes that archive the one commands outside an archive use;
/// without, says which that is and lists the archives opened or created on this device.
pub fn open(path: Option<&Path>) -> Result<Report, Failure> {
    let mut prefs = Prefs::load()?;
    let Some(path) = path else {
        let known = prefs.known_archives();
        let mut text = match &prefs.archive {
            Some(chosen) => format!("Commands outside an archive use {}\n", chosen.display()),
            None => "No archive chosen; commands only work inside one\n".to_owned(),
        };
        let others: Vec<&PathBuf> = known
            .iter()
            .filter(|r| Some(*r) != prefs.archive.as_ref())
            .collect();
        if !others.is_empty() {
            text.push_str("\nRecent archives:\n");
            for root in &others {
                writeln!(text, "  {}", root.display()).unwrap();
            }
        }
        return Ok(Report::new(
            text,
            json!({ "archive": prefs.archive, "recent": known }),
        ));
    };
    let (archive, root) = open_archive(path)?;
    prefs.archive = Some(root.clone());
    prefs.note_archive(&root);
    prefs.save()?;
    let mut text = format!(
        "Commands outside an archive now use “{}” ({})\n",
        archive.doc().name,
        root.display()
    );
    if archive.access() == Access::ReadOnly {
        text.push_str("It uses a newer format, so this version can only read it.\n");
    }
    Ok(Report::new(
        text,
        json!({ "archive": archive_json(&archive) }),
    ))
}

/// The archive `path` is in, or is, and its root with links resolved, as `carrel open` and
/// the interactive mode's archive switcher find it.
///
/// # Errors
///
/// [`Failure`] if `path` isn't in an archive.
pub fn open_archive(path: &Path) -> Result<(Archive, PathBuf), Failure> {
    let archive = Archive::find(path)?;
    let root =
        std::fs::canonicalize(archive.root()).map_err(|e| Failure::io(archive.root(), &e))?;
    Ok((archive, root))
}

/// A card read from disk along with where it is.
pub(crate) struct LoadedCard {
    pub item: ItemRef,
    pub card: Card,
}

pub(crate) fn load_card(archive: &Archive, item: &ItemRef) -> Result<LoadedCard, Failure> {
    Ok(LoadedCard {
        item: item.clone(),
        card: load_file(archive, item)?.card().clone(),
    })
}

fn load_file(archive: &Archive, item: &ItemRef) -> Result<CardFile, Failure> {
    let path = archive.root().join(&item.card);
    let text = std::fs::read_to_string(&path).map_err(|e| Failure::io(&path, &e))?;
    CardFile::parse(&text).map_err(|e| Failure::new(format!("{}: {e}", item.card)))
}

/// What a [`Recent`] says, for people and for programs.
fn recent_line(recent: &Recent) -> (String, Value) {
    match &recent.what {
        Happened::Change(e) => (
            e.summary.clone(),
            json!({ "kind": "change", "change": entry_json(e) }),
        ),
        Happened::Work {
            item,
            title,
            date,
            duration,
            summary,
        } => {
            let mut what = format!("Worked on “{title}” for {duration}");
            if let Some(first) = summary {
                write!(what, ": {first}").unwrap();
            }
            let json = json!({
                "kind": "work",
                "date": date,
                "item": item,
                "title": title,
                "duration": duration,
                "summary": summary,
            });
            (what, json)
        }
    }
}

/// The part of `carrel status` about what was filed, changed or worked on lately.
fn recent_section(recent: &[Recent]) -> String {
    let mut text = String::new();
    if recent.is_empty() {
        return text;
    }
    text.push_str("\nRecent:\n");
    for r in recent {
        writeln!(text, "  {:<16}  {}", r.when, recent_line(r).0).unwrap();
    }
    text
}

/// The collection a card belongs to: the deepest collection folder containing it.
fn collection_of<'a>(collections: &'a [String], card_path: &str) -> Option<&'a str> {
    collections
        .iter()
        .filter(|c| card_path.starts_with(&format!("{c}/")))
        .max_by_key(|c| c.len())
        .map(String::as_str)
}

fn writer_json(lock: Option<&LockInfo>) -> Value {
    lock.map_or(Value::Null, |info| {
        json!({
            "person": info.person,
            "device": info.device,
            "purpose": info.purpose,
            "program": info.program,
            "acquired": info.acquired,
            "expires": info.expires,
            "expired": info.is_expired(),
        })
    })
}

/// The start of `carrel status`: the archive's name, where it is, and what it is.
fn archive_header(archive: &Archive) -> String {
    let mut text = String::new();
    let info = archive.info();
    writeln!(text, "{}", archive.doc().name).unwrap();
    writeln!(text, "  location  {}", archive.root().display()).unwrap();
    writeln!(text, "  id        {}", info.id).unwrap();
    writeln!(
        text,
        "  format    {} ({})",
        info.format,
        access_name(archive.access())
    )
    .unwrap();
    writeln!(text, "  created   {}", info.created.0).unwrap();
    text.push('\n');
    text
}

/// `carrel status`.
pub fn status(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let this_device = Identity::load()?.map(|i| i.device);
    let summary = carrel_core::status::summary(&archive, &Looking::here(this_device))?;
    let history = summary.history.as_deref();

    let mut text = archive_header(&archive);
    let kinds: Vec<String> = summary
        .by_kind
        .iter()
        .map(|(k, n)| format!("{n} {k}"))
        .collect();
    writeln!(
        text,
        "{} in {}{}",
        plural(summary.items, "item"),
        plural(summary.collections, "collection"),
        if kinds.is_empty() {
            String::new()
        } else {
            format!(" ({})", kinds.join(", "))
        }
    )
    .unwrap();
    text.push_str(&history_line(history));
    text.push_str(&crate::backup::status_line(&archive, history));
    text.push_str(&writer_line(summary.writer.as_ref(), this_device));
    text.push_str(&recent_section(&summary.recent));
    if !summary.problems.is_empty() {
        writeln!(
            text,
            "\n{} with problems:",
            plural(summary.problems.len(), "card")
        )
        .unwrap();
        for found in &summary.problems {
            writeln!(text, "  {}", found.card).unwrap();
            for problem in &found.problems {
                writeln!(text, "    {problem}").unwrap();
            }
        }
    }
    text.push_str(&names_section(&summary.names));
    text.push_str(&signals_section(&summary.signals));
    let (desk_text, desk) = crate::desk::suggestions_section(&summary.desk);
    text.push_str(&desk_text);
    let (resurfaced_text, resurfaced) = crate::discover::resurfaced(&archive);
    text.push_str(&resurfaced_text);
    let mut warnings = Vec::new();
    if summary.synced {
        warnings.push(SYNCED_WARNING);
        writeln!(text, "\nwarning: {SYNCED_WARNING}").unwrap();
    }

    let json = json!({
        "archive": archive_json(&archive),
        "collections": summary.collections,
        "items": { "total": summary.items, "by_kind": summary.by_kind },
        "problems": summary.problems.iter().map(|found| json!({
            "card": found.card,
            "problems": found.problems.iter().map(|p| json!({ "field": p.field, "message": p.message })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "history": history.map(|log| json!({
            "changes": log.len(),
            "last": log.first().map(entry_json),
        })),
        "writer": writer_json(summary.writer.as_ref()),
        "recent": summary.recent.iter().map(|r| recent_line(r).1).collect::<Vec<_>>(),
        "signals": summary.signals,
        "desk": desk,
        "resurfaced": resurfaced,
        "names": summary.names.iter().map(|f| json!({
            "path": f.path,
            "problems": f.problems,
            "suggestion": f.suggestion,
        })).collect::<Vec<_>>(),
        "warnings": warnings,
    });
    Ok(Report::new(text, json))
}

/// The part of `carrel status` about the structure: where it shows signs of needing a
/// restructure (`docs/design/signals.md`).
fn signals_section(signals: &[carrel_core::signals::Signal]) -> String {
    let mut text = String::new();
    if signals.is_empty() {
        return text;
    }
    text.push_str("\nStructure (signs a restructure may help later):\n");
    for signal in signals {
        writeln!(text, "  {}", signal.describe()).unwrap();
    }
    text
}

/// The part of `carrel status` about names that won't work on every system.
fn names_section(names: &[carrel_core::names::Finding]) -> String {
    let mut text = String::new();
    if names.is_empty() {
        return text;
    }
    writeln!(
        text,
        "\n{} won't work on every system:",
        plural(names.len(), "name")
    )
    .unwrap();
    for finding in names {
        writeln!(text, "  {}: {}", finding.path, finding.problems.join("; ")).unwrap();
        if let Some(suggestion) = &finding.suggestion {
            writeln!(
                text,
                "    rename it with `carrel rename \"{}\" \"{suggestion}\"`",
                finding.path
            )
            .unwrap();
        }
    }
    text
}

fn history_line(history: Option<&[Entry]>) -> String {
    match history {
        None => "No history yet (run `carrel init` to start it)\n".to_owned(),
        Some([]) => "No changes recorded\n".to_owned(),
        Some(log) => format!(
            "{} recorded; last: {} ({})\n",
            plural(log.len(), "change"),
            log[0].summary,
            local_time(log[0].time)
        ),
    }
}

fn writer_line(lock: Option<&LockInfo>, this_device: Option<Id>) -> String {
    let Some(info) = lock else {
        return "Nobody is writing\n".to_owned();
    };
    let who = if Some(info.device) == this_device {
        "this device".to_owned()
    } else {
        format!("device {}", info.device)
    };
    if info.is_expired() {
        format!("A lock from {who} ({}) has expired\n", info.purpose)
    } else {
        format!(
            "Being changed by {who} ({}) until {}\n",
            info.purpose, info.expires
        )
    }
}

fn kind_name(item: &ItemRef) -> String {
    serde_json::to_value(item.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub(crate) fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

pub(crate) fn local_time(seconds: i64) -> String {
    carrel_core::status::local_time(seconds)
}

/// Finds the item `query` names: an identifier, or the path of the item or its card
/// (relative to the current folder, or to the archive root).
pub(crate) fn find_item<'a>(
    archive: &Archive,
    items: &'a [ItemRef],
    query: &str,
) -> Result<&'a ItemRef, Failure> {
    if let Ok(uuid) = uuid::Uuid::parse_str(query) {
        let id = Id(uuid);
        return items
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| Failure::new(format!("no item has the identifier {query}")));
    }
    let root =
        std::fs::canonicalize(archive.root()).map_err(|e| Failure::io(archive.root(), &e))?;
    let candidates = [PathBuf::from(query), archive.root().join(query)];
    let relative = candidates
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .find_map(|p| {
            p.strip_prefix(&root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        })
        .ok_or_else(|| Failure::new(format!("{query} isn't an item or card in this archive")))?;
    items
        .iter()
        .find(|item| item.card == relative || item.item.as_deref() == Some(relative.as_str()))
        .ok_or_else(|| Failure::new(format!("{query} has no card in this archive")))
}

/// An item another links to, as the item's view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Linked {
    /// How it's linked: `version-of`, `related`, `part-of`, and so on.
    pub link_type: String,
    /// The linked item.
    pub id: Id,
    /// Its card's path, if it's in the archive.
    pub card: Option<String>,
    /// Its title, if its card could be read.
    pub title: Option<String>,
}

/// A filed item as `carrel show` describes it, before its links and history.
pub(crate) struct Shown {
    /// The item's identifier.
    pub id: Id,
    /// The card's fields, summary and problems, for people.
    pub text: String,
    /// The same for programs.
    pub json: Value,
    /// The items it links to.
    pub links: Vec<Linked>,
    /// Its card.
    pub card: Card,
    /// Its card's path.
    pub card_path: String,
    /// Every item in the archive, as it was listed to find it.
    pub items: Vec<ItemRef>,
}

/// Describes the item `query` names: its card, and what it links to.
pub(crate) fn describe(archive: &Archive, query: &str) -> Result<Shown, Failure> {
    let items = archive.items()?;
    let item = find_item(archive, &items, query)?;
    let LoadedCard { item, card } = load_card(archive, item)?;
    let collections: Vec<String> = archive.collections()?.into_iter().map(|c| c.path).collect();
    let collection = collection_of(&collections, &item.card);
    let problems = card.problems();

    let mut text = String::new();
    writeln!(text, "{}", card.title).unwrap();
    let mut row = |label: &str, value: &str| writeln!(text, "  {label:<13}{value}").unwrap();
    row("kind", &kind_name(&item));
    row("id", &card.id.to_string());
    if let Some(path) = &item.item {
        row("item", path);
    }
    row("card", &item.card);
    if let Some(collection) = collection {
        row("collection", collection);
    }
    row(
        "sensitivity",
        &serde_json::to_value(card.sensitivity)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
    );
    if !card.tags.is_empty() {
        let tags: Vec<&str> = card.tags.iter().map(|t| t.0.as_str()).collect();
        row("tags", &tags.join(", "));
    }
    if let Some(location) = &card.location {
        row("location", location);
    }
    if let Some(url) = &card.url {
        row("url", url);
    }
    if let Some(size) = card.size {
        row("size", &format!("{size} bytes"));
    }
    if let Some(checkout) = &card.checked_out {
        row(
            "checked out",
            &format!("to {} since {}", checkout.to, checkout.since.0),
        );
    }
    if let Some(summary) = &card.summary {
        writeln!(text, "\n{}", summary.trim_end()).unwrap();
    }
    if !problems.is_empty() {
        writeln!(text, "\nProblems:").unwrap();
        for problem in &problems {
            writeln!(text, "  {problem}").unwrap();
        }
    }
    let links = card
        .links
        .iter()
        .map(|link| {
            let target = items.iter().find(|i| i.id == link.id);
            Linked {
                link_type: serde_json::to_value(&link.link_type)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                id: link.id,
                card: target.map(|i| i.card.clone()),
                title: target
                    .and_then(|i| load_card(archive, i).ok())
                    .map(|loaded| loaded.card.title),
            }
        })
        .collect();

    let json = json!({
        "root": root_of(archive),
        "card": item.card,
        "item": item.item,
        "collection": collection,
        "fields": card,
        "problems": problems.iter().map(|p| json!({ "field": p.field, "message": p.message })).collect::<Vec<_>>(),
    });
    Ok(Shown {
        id: card.id,
        text,
        json,
        links,
        card,
        card_path: item.card,
        items,
    })
}

/// `carrel show`.
pub fn show(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let mut shown = describe(&archive, query)?;
    if let Some((collection, _)) = shown.card_path.rsplit_once('/') {
        crate::discover::opened(&archive, collection);
    }
    let near = carrel_core::index::related::open(&archive)
        .and_then(|index| crate::related::nearest(&archive, &index, &shown));
    let mut text = std::mem::take(&mut shown.text);
    text.push_str(&related_section(&shown.links, &near));
    let history = match History::open(&archive) {
        Ok(history) => Some(history.item_log(shown.id)?),
        Err(HistoryError::NotInitialized) => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(entries) = &history {
        text.push_str(&item_history_section(entries));
    }
    let mut json = shown.json;
    json["history"] = history.map_or(Value::Null, |entries| {
        Value::Array(entries.iter().map(entry_json).collect())
    });
    json["near"] = Value::Array(
        near.as_ref()
            .map(|n| n.iter().map(crate::related::json).collect())
            .unwrap_or_default(),
    );
    json["links_only"] = near.err().map_or(Value::Null, Value::String);
    Ok(Report::new(text, json))
}

/// The part of `carrel show` about the items related to it: those it links to, then those
/// nearest it by meaning, or why there are none of those.
fn related_section(
    links: &[Linked],
    near: &Result<Vec<crate::related::Neighbour>, String>,
) -> String {
    let mut text = String::new();
    let near_ones = near.as_deref().unwrap_or_default();
    if links.is_empty() && near_ones.is_empty() && near.is_ok() {
        return text;
    }
    writeln!(text, "\nRelated:").unwrap();
    for link in links {
        let target = link.card.clone().unwrap_or_else(|| link.id.to_string());
        writeln!(text, "  {} {target}", link.link_type).unwrap();
    }
    for neighbour in near_ones {
        writeln!(
            text,
            "  near {}  {}{}",
            neighbour.card,
            neighbour.title,
            crate::related::mark(neighbour.sensitivity)
        )
        .unwrap();
    }
    if let Err(why) = near {
        writeln!(text, "  {why}").unwrap();
    }
    text
}

/// The part of `carrel show` about every change that touched the item, newest first.
fn item_history_section(entries: &[Entry]) -> String {
    let mut text = String::new();
    if entries.is_empty() {
        return text;
    }
    writeln!(
        text,
        "\nHistory ({}; undo one with `carrel undo CHANGE`):",
        plural(entries.len(), "change")
    )
    .unwrap();
    for entry in entries {
        writeln!(text, "  {}", log_line(entry)).unwrap();
    }
    text
}

/// A change on one line: its short identifier, when, the operation and the summary.
pub(crate) fn log_line(entry: &Entry) -> String {
    format!(
        "{}  {}  {:<11} {}",
        &entry.id[..entry.id.len().min(10)],
        local_time(entry.time),
        entry.operation.map_or("", |o| o.as_str()),
        entry.summary
    )
}

fn entry_json(entry: &Entry) -> Value {
    json!({
        "id": entry.id,
        "summary": entry.summary,
        "operation": entry.operation.map(Operation::as_str),
        "person": entry.person,
        "device": entry.device,
        "snapshot": entry.snapshot,
        "items": entry.items,
        "moves": entry.moves.iter().map(|m| json!({ "item": m.item, "from": m.from, "to": m.to })).collect::<Vec<_>>(),
        "reverts": entry.reverts,
        "time": jiff::Timestamp::from_second(entry.time).map(|t| t.to_string()).unwrap_or_default(),
    })
}

/// `carrel log`.
pub fn log(archive: Option<&Path>, max_count: Option<usize>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let history = match History::open(&archive) {
        Ok(history) => history,
        Err(HistoryError::NotInitialized) => {
            return Err(Failure::new(
                "this archive has no history yet; run `carrel init` in it to start one",
            ));
        }
        Err(e) => return Err(e.into()),
    };
    let mut entries = history.log()?;
    if let Some(max) = max_count {
        entries.truncate(max);
    }
    let mut text = String::new();
    for entry in &entries {
        writeln!(text, "{}", log_line(entry)).unwrap();
    }
    Ok(Report::new(
        text,
        Value::Array(entries.iter().map(entry_json).collect()),
    ))
}

/// `carrel undo CHANGE`: undoes a recorded change with a new change that reverses it. Items it
/// moved go back, and the metadata it changed returns to how it was.
pub fn undo(archive: Option<&Path>, change: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let history = History::open(&archive)?;
    let entries = history.log()?;
    let matching: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.id.starts_with(change))
        .collect();
    let entry = match matching.as_slice() {
        [entry] => *entry,
        [] => return Err(Failure::new(format!("no change {change} in the history"))),
        _ => {
            return Err(Failure::new(format!(
                "several changes start with {change}; give more of it"
            )));
        }
    };
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "undo", DEFAULT_LEASE)?;
    let result = history.undo(&archive, &entry.id, &lease);
    lease.release()?;
    let commit = result?;
    Ok(Report::new(
        format!(
            "Undid “{}” as change {}\n",
            entry.summary,
            &commit[..commit.len().min(10)]
        ),
        json!({"undone": entry.id, "commit": commit}),
    ))
}

/// `carrel collection new PATH`: a new collection with a design document to fill in.
///
/// # Errors
///
/// [`Failure`] if the path can't be used, a tag isn't `name=meaning`, or writing fails.
pub fn collection_new(
    archive: Option<&Path>,
    path: &str,
    description: Option<String>,
    tags: &[String],
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let tags = tags
        .iter()
        .map(|t| {
            t.split_once('=')
                .map(|(name, meaning)| (name.trim().to_owned(), meaning.trim().to_owned()))
                .filter(|(name, meaning)| !name.is_empty() && !meaning.is_empty())
                .ok_or_else(|| Failure::new(format!("give a tag as name=meaning, not “{t}”")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "collection", DEFAULT_LEASE)?;
    let new = carrel_core::collections::NewCollection { description, tags };
    let result = carrel_core::collections::create(&archive, path, &new, &lease);
    lease.release()?;
    let created = result?;
    let text = format!(
        "Created the collection {}.\nDescribe what belongs there, and what doesn't, in {}: models read it before filing anything.\n",
        created.path,
        archive.root().join(&created.design_document).display()
    );
    Ok(Report::new(
        text,
        json!({
            "path": created.path,
            "id": created.id,
            "design_document": created.design_document,
            "commit": created.commit,
        }),
    ))
}

/// `carrel rename ITEM [NEW_NAME]`: renames an item and its card together, to a portable name
/// suggested for it unless one is given.
pub fn rename(
    archive: Option<&Path>,
    query: &str,
    new_name: Option<&str>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    let current = item.item.as_deref().unwrap_or(&item.card);
    let current = current.rsplit('/').next().unwrap_or(current);
    let new_name = if let Some(name) = new_name {
        name.to_owned()
    } else {
        let suggestion = carrel_core::names::portable(current);
        if suggestion == current {
            return Err(Failure::new(format!(
                "“{current}” already works on every system; give a new name to rename it"
            )));
        }
        suggestion
    };
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "rename", DEFAULT_LEASE)?;
    let result = carrel_core::rename::rename(&archive, item, &new_name, &lease);
    lease.release()?;
    let renamed = result?;
    let mut text = format!("Renamed {} to {}\n", renamed.from, renamed.to);
    if !renamed.relinked.is_empty() {
        writeln!(
            text,
            "Updated links in {}",
            plural(renamed.relinked.len(), "card")
        )
        .unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "from": renamed.from,
            "to": renamed.to,
            "card": renamed.card,
            "relinked": renamed.relinked,
            "commit": renamed.commit,
        }),
    ))
}
