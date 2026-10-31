//! The commands. Each returns a [`Report`] and leaves printing to `main`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::Access;
use carrel_core::archive::{Archive, ArchiveError, InitOptions, ItemRef};
use carrel_core::card::{Card, Id};
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder, LockInfo};
use carrel_core::entries::HistoryEntry;
use carrel_core::history::{Entry, History, HistoryError, Operation};
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
    Holder {
        person: identity.person,
        device: identity.device,
    }
}

/// `carrel open`.
pub fn open(path: Option<&Path>) -> Result<Report, Failure> {
    let mut prefs = Prefs::load()?;
    let Some(path) = path else {
        return Ok(match prefs.archive {
            Some(chosen) => Report::new(
                format!("Commands outside an archive use {}\n", chosen.display()),
                json!({ "archive": chosen }),
            ),
            None => Report::new(
                "No archive chosen; commands only work inside one\n",
                json!({ "archive": null }),
            ),
        });
    };
    let archive = Archive::find(path)?;
    let root =
        std::fs::canonicalize(archive.root()).map_err(|e| Failure::io(archive.root(), &e))?;
    prefs.archive = Some(root.clone());
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

/// A card read from disk along with where it is.
struct LoadedCard {
    item: ItemRef,
    card: Card,
}

fn load_card(archive: &Archive, item: &ItemRef) -> Result<LoadedCard, Failure> {
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

/// How many lines `carrel status` gives Recent.
const RECENT: usize = 10;

/// Something filed, changed or worked on lately, for Recent.
struct Recent {
    /// When, as `YYYY-MM-DD HH:MM` for a change and `YYYY-MM-DD` for work, so they sort
    /// together.
    when: String,
    /// What happened, for people.
    what: String,
    /// The same for programs.
    json: Value,
}

/// A work session's approved history entry, as Recent shows it.
fn worked_on(item: Id, title: &str, entry: &HistoryEntry) -> Option<Recent> {
    let duration = entry.duration.as_ref()?;
    let first = entry.lines.iter().map(|l| l.trim()).find(|l| !l.is_empty());
    let mut what = format!("Worked on “{title}” for {duration}");
    if let Some(first) = first {
        write!(what, ": {first}").unwrap();
    }
    Some(Recent {
        when: entry.date.clone(),
        json: json!({
            "kind": "work",
            "date": entry.date,
            "item": item,
            "title": title,
            "duration": duration,
            "summary": first,
        }),
        what,
    })
}

/// The latest changes and work sessions, newest first. Recording or approving a session is
/// left out: the session itself is shown, on the day it happened.
fn recent(history: Option<&[Entry]>, mut work: Vec<Recent>) -> Vec<Recent> {
    let changes = history
        .unwrap_or_default()
        .iter()
        .filter(|e| e.operation != Some(Operation::Session))
        .take(RECENT)
        .map(|e| Recent {
            when: local_time(e.time),
            what: e.summary.clone(),
            json: json!({ "kind": "change", "change": entry_json(e) }),
        });
    work.extend(changes);
    work.sort_by(|a, b| b.when.cmp(&a.when));
    work.truncate(RECENT);
    work
}

/// The part of `carrel status` about what was filed, changed or worked on lately.
fn recent_section(recent: &[Recent]) -> String {
    let mut text = String::new();
    if recent.is_empty() {
        return text;
    }
    text.push_str("\nRecent:\n");
    for r in recent {
        writeln!(text, "  {:<16}  {}", r.when, r.what).unwrap();
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
    let collections: Vec<String> = archive.collections()?.into_iter().map(|c| c.path).collect();
    let items = archive.items()?;
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut problems = Vec::new();
    let mut work = Vec::new();
    for item in &items {
        *by_kind.entry(kind_name(item)).or_default() += 1;
        let file = load_file(&archive, item)?;
        let found = file.card().problems();
        if !found.is_empty() {
            problems.push((item.card.clone(), found));
        }
        // A card whose history section can't be read shows its problem in `carrel show`.
        if let Ok(log) = file.history() {
            work.extend(
                log.entries
                    .iter()
                    .filter(|e| !e.pending)
                    .filter_map(|e| worked_on(item.id, &file.card().title, e)),
            );
        }
    }
    let history = match History::open(&archive) {
        Ok(history) => Some(history.log()?),
        Err(HistoryError::NotInitialized) => None,
        Err(e) => return Err(e.into()),
    };
    let lock = DiskLock::for_archive(&archive).current()?;
    let synced = DiskLock::is_synced_folder(archive.root());
    let this_device = Identity::load()?.map(|i| i.device);

    let mut text = archive_header(&archive);
    let kinds: Vec<String> = by_kind.iter().map(|(k, n)| format!("{n} {k}")).collect();
    writeln!(
        text,
        "{} in {}{}",
        plural(items.len(), "item"),
        plural(collections.len(), "collection"),
        if kinds.is_empty() {
            String::new()
        } else {
            format!(" ({})", kinds.join(", "))
        }
    )
    .unwrap();
    text.push_str(&history_line(history.as_deref()));
    text.push_str(&crate::backup::status_line(&archive, history.as_deref()));
    text.push_str(&writer_line(lock.as_ref(), this_device));
    let recent = recent(history.as_deref(), work);
    text.push_str(&recent_section(&recent));
    if !problems.is_empty() {
        writeln!(text, "\n{} with problems:", plural(problems.len(), "card")).unwrap();
        for (card, found) in &problems {
            writeln!(text, "  {card}").unwrap();
            for problem in found {
                writeln!(text, "    {problem}").unwrap();
            }
        }
    }
    let names = carrel_core::names::check(&archive)?;
    text.push_str(&names_section(&names));
    let signals = carrel_core::signals::signals(&archive)?;
    text.push_str(&signals_section(&signals));
    let (desk_text, desk) = crate::desk::suggestions_section(&archive, this_device)?;
    text.push_str(&desk_text);
    let mut warnings = Vec::new();
    if synced {
        warnings.push(SYNCED_WARNING);
        writeln!(text, "\nwarning: {SYNCED_WARNING}").unwrap();
    }

    let json = json!({
        "archive": archive_json(&archive),
        "collections": collections.len(),
        "items": { "total": items.len(), "by_kind": by_kind },
        "problems": problems.iter().map(|(card, found)| json!({
            "card": card,
            "problems": found.iter().map(|p| json!({ "field": p.field, "message": p.message })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "history": history.as_ref().map(|log| json!({
            "changes": log.len(),
            "last": log.first().map(entry_json),
        })),
        "writer": writer_json(lock.as_ref()),
        "recent": recent.iter().map(|r| &r.json).collect::<Vec<_>>(),
        "signals": signals,
        "desk": desk,
        "names": names.iter().map(|f| json!({
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
    jiff::Timestamp::from_second(seconds).map_or_else(
        |_| seconds.to_string(),
        |t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
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
    })
}

/// `carrel show`.
pub fn show(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let Shown {
        id,
        mut text,
        mut json,
        links,
    } = describe(&archive, query)?;
    if !links.is_empty() {
        writeln!(text, "\n{}:", plural(links.len(), "link")).unwrap();
        for link in &links {
            let target = link.card.clone().unwrap_or_else(|| link.id.to_string());
            writeln!(text, "  {} {target}", link.link_type).unwrap();
        }
    }
    let history = match History::open(&archive) {
        Ok(history) => Some(history.item_log(id)?),
        Err(HistoryError::NotInitialized) => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(entries) = &history {
        text.push_str(&item_history_section(entries));
    }
    json["history"] = history.map_or(Value::Null, |entries| {
        Value::Array(entries.iter().map(entry_json).collect())
    });
    Ok(Report::new(text, json))
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
