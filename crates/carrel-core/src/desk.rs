//! Desk items (FORMAT.md §9.4, `docs/design/checkout.md`): files and folders being worked on
//! where they are, tracked without being archived. Each gets a card beside it, and this
//! device's `desk.json` (§7.10) lists them. Nothing is written inside them, and nothing is
//! moved.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::{Card, CardMarker, DateOrTime, Dates, Extra, Id, Sensitivity};
use crate::card_file::CardFile;
use crate::coordinator::WriterLease;
use crate::history::{Change, History, HistoryError, Operation};
use crate::ingest::card::{Context, Draft, Glossary, Vocabulary};
use crate::providers::Registry;

/// The key, in an intake or review entry's extra fields, holding the path of the desk card an
/// item being archived from the desk keeps: its identifier, fields and body go with it.
pub const CARD_KEY: &str = "desk_card";

/// One tracked desk item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskEntry {
    /// Its card's identifier.
    pub id: Id,
    /// Where it is on this device.
    pub path: PathBuf,
    /// Where its card is: beside it, named after it with `.md` added.
    pub card: PathBuf,
    /// When it was added.
    pub added: DateOrTime,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A device's desk items: `_System/devices/<device-id>/desk.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Desk {
    /// The items, in the order they were added.
    #[serde(default)]
    pub items: Vec<DeskEntry>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// Why a desk item couldn't be added or removed.
#[derive(Debug)]
pub enum DeskError {
    /// Nothing is at the path.
    Missing(PathBuf),
    /// The path is inside the archive: it's an item already, or should be ingested.
    InArchive(PathBuf),
    /// It's already on the desk.
    Tracked(PathBuf),
    /// It isn't on the desk.
    NotTracked(PathBuf),
    /// Something already has the card's name.
    CardTaken(PathBuf),
    /// A file couldn't be read or written.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
    /// The history couldn't be written.
    History(HistoryError),
    /// It couldn't be queued for ingest.
    NotQueued {
        /// The item.
        path: PathBuf,
        /// Why.
        reason: String,
    },
}

impl std::fmt::Display for DeskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(p) => write!(f, "nothing is at {}", p.display()),
            Self::InArchive(p) => write!(
                f,
                "{} is inside the archive; ingest it instead",
                p.display()
            ),
            Self::Tracked(p) => write!(f, "{} is already on the desk", p.display()),
            Self::NotTracked(p) => write!(f, "{} isn't on the desk", p.display()),
            Self::CardTaken(p) => write!(
                f,
                "{} already exists, so its card can't go there",
                p.display()
            ),
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::History(e) => write!(f, "{e}"),
            Self::NotQueued { path, reason } => {
                write!(f, "{} can't be queued for ingest: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for DeskError {}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> DeskError + '_ {
    move |e| DeskError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    }
}

impl Desk {
    /// Where `device`'s desk is kept.
    #[must_use]
    pub fn path(archive: &Archive, device: Id) -> PathBuf {
        archive
            .root()
            .join(format!("_System/devices/{device}/desk.json"))
    }

    /// `device`'s desk, or an empty one.
    ///
    /// # Errors
    ///
    /// [`DeskError::Io`] if it's there but can't be read.
    pub fn load(archive: &Archive, device: Id) -> Result<Self, DeskError> {
        let path = Self::path(archive, device);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| DeskError::Io {
                path,
                message: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(io(&path)(e)),
        }
    }

    fn save(&self, archive: &Archive, device: Id) -> Result<(), DeskError> {
        let path = Self::path(archive, device);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| DeskError::Io {
            path: path.clone(),
            message: e.to_string(),
        })?;
        std::fs::write(&path, text + "\n").map_err(io(&path))
    }

    /// The entry for `path`, if it's on the desk.
    #[must_use]
    pub fn find(&self, path: &Path) -> Option<&DeskEntry> {
        self.items.iter().find(|e| e.path == path)
    }
}

/// The card's path for a desk item at `path`: beside it, with `.md` added.
#[must_use]
pub fn card_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".md");
    path.with_file_name(name)
}

/// Drafts a card for the file or folder at `path` as ingest does: its text is read, and the
/// text models in `registry` are asked for a title, summary, tags and date. With no model to
/// answer, the draft has no provenance, and [`add`] falls back to the item's name. Nothing is
/// written; it can take a while, so do it before taking the writer lock.
///
/// # Errors
///
/// [`DeskError`] if it's missing or the archive's rules can't be read.
pub fn draft(
    archive: &Archive,
    registry: &Registry,
    path: &Path,
    device: Id,
) -> Result<Draft, DeskError> {
    let path = std::fs::canonicalize(path).map_err(|_| DeskError::Missing(path.to_owned()))?;
    let failed = |e: &dyn std::fmt::Display| DeskError::Io {
        path: path.clone(),
        message: e.to_string(),
    };
    let candidate = crate::ingest::examine(archive, &path).map_err(|e| failed(&e))?;
    let rules = archive.rules().map_err(|e| failed(&e))?;
    let collections = archive.collections().map_err(|e| failed(&e))?;
    let vocabulary = Vocabulary::of(archive, &collections);
    let glossary = Glossary::of(archive);
    let context = Context {
        archive_name: &archive.doc().name,
        vocabulary: &vocabulary,
        glossary: &glossary,
        device,
    };
    let text = crate::ingest::text::read(registry, &rules, &candidate);
    Ok(crate::ingest::card::draft(
        registry, &context, &candidate, &text,
    ))
}

/// Puts the file or folder at `path` on this device's desk (the lease's), holding `lease`:
/// writes a card beside it, lists it in `desk.json`, and commits that as one change.
///
/// The card's title and summary are `title` and `summary` when given, else the model's from
/// `drafted` (see [`draft`]); its tags and date come from `drafted` too. Without a model's
/// draft, the title is the item's name.
///
/// # Errors
///
/// [`DeskError`] if it's missing, inside the archive, already on the desk, or its card's
/// name is taken.
pub fn add(
    archive: &Archive,
    path: &Path,
    title: Option<&str>,
    summary: Option<&str>,
    drafted: Option<&Draft>,
    lease: &WriterLease,
) -> Result<DeskEntry, DeskError> {
    let path = std::fs::canonicalize(path).map_err(|_| DeskError::Missing(path.to_owned()))?;
    let root = std::fs::canonicalize(archive.root()).map_err(io(archive.root()))?;
    if path.starts_with(&root) {
        return Err(DeskError::InArchive(path));
    }
    let device = lease.info().device;
    let mut desk = Desk::load(archive, device)?;
    if desk.find(&path).is_some() {
        return Err(DeskError::Tracked(path));
    }
    let card = card_path(&path);
    if card.exists() {
        return Err(DeskError::CardTaken(card));
    }
    let detected = archive.detect(&path).map_err(|e| DeskError::Io {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let id = Id::new_v7();
    let card_value = new_card(id, &detected, name, (title, summary), drafted);
    let file = CardFile::new(card_value, "").map_err(|e| DeskError::Io {
        path: card.clone(),
        message: e.to_string(),
    })?;
    let text = file.to_text().map_err(|e| DeskError::Io {
        path: card.clone(),
        message: e.to_string(),
    })?;
    std::fs::write(&card, text).map_err(io(&card))?;
    let entry = DeskEntry {
        id,
        path: path.clone(),
        card: card.clone(),
        added: DateOrTime(
            jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string(),
        ),
        extra: Extra::new(),
    };
    desk.items.push(entry.clone());
    let result = desk.save(archive, device).and_then(|()| {
        let mut change = Change::new(
            Operation::Edit,
            format!("Put “{}” on the desk", file.card().title),
            lease.info().person,
            device,
        );
        change.items = vec![id];
        commit(archive, &change, lease)
    });
    if let Err(e) = result {
        let _ = std::fs::remove_file(&card);
        desk.items.pop();
        let _ = desk.save(archive, device);
        return Err(e);
    }
    Ok(entry)
}

/// A new desk card for an item named `name`: the owner's title and summary in `given` if
/// set, else a model's from `drafted`, else the name.
fn new_card(
    id: Id,
    detected: &crate::detect::Detected,
    name: String,
    given: (Option<&str>, Option<&str>),
    drafted: Option<&Draft>,
) -> Card {
    let (title, summary) = given;
    let today = DateOrTime(jiff::Zoned::now().date().to_string());
    // Only a model's draft is used for the title, summary and tags; without one, the
    // fallback title is the file's stem, and the item's name says more.
    let modelled = drafted.filter(|d| d.generated.is_some());
    Card {
        carrel: CardMarker::Card,
        format: crate::FORMAT_VERSION,
        id,
        kind: detected.kind,
        title: title
            .map(str::to_owned)
            .or_else(|| modelled.map(|d| d.title.clone()))
            .unwrap_or_else(|| name.clone()),
        item: Some(name),
        sealed: detected.sealed,
        allow_readme: None,
        summary: summary
            .map(str::to_owned)
            .or_else(|| modelled.and_then(|d| d.summary.clone())),
        tags: modelled.map(|d| d.tags.clone()).unwrap_or_default(),
        links: Vec::new(),
        dates: Some(Dates {
            created: drafted.and_then(|d| d.created.clone()),
            added: Some(today.clone()),
            ..Dates::default()
        }),
        sensitivity: Sensitivity::Personal,
        checksum: None,
        size: None,
        manifest: None,
        location: None,
        url: None,
        checked_out: None,
        snapshots: None,
        generated: modelled.and_then(|d| provenance(d, title.is_some(), summary.is_some())),
        placement: None,
        removed: None,
        expanded_from: None,
        needs_work: None,
        extra: Extra::new(),
    }
}

/// Rebuilds `Active/`, the desk mirrors included, and commits `change` with it.
fn commit(archive: &Archive, change: &Change, lease: &WriterLease) -> Result<(), DeskError> {
    crate::active::regenerate(archive).map_err(|e| DeskError::Io {
        path: archive.root().join("Active"),
        message: e.to_string(),
    })?;
    History::open(archive)
        .and_then(|h| h.commit(archive, change, lease))
        .map(|_| ())
        .map_err(DeskError::History)
}

/// What a model's draft produced, leaving out the fields the owner gave.
fn provenance(
    drafted: &Draft,
    title_given: bool,
    summary_given: bool,
) -> Option<crate::card::Provenance> {
    let mut generated = drafted.generated.clone()?;
    generated
        .fields
        .retain(|f| !(title_given && f == "title" || summary_given && f == "summary"));
    (!generated.fields.is_empty()).then_some(generated)
}

/// Takes `path` off this device's desk, holding `lease`. Its card stays beside it unless
/// `delete_card`; the item itself is never touched.
///
/// # Errors
///
/// [`DeskError::NotTracked`] if it isn't on the desk, or an error writing.
pub fn remove(
    archive: &Archive,
    path: &Path,
    delete_card: bool,
    lease: &WriterLease,
) -> Result<DeskEntry, DeskError> {
    let wanted = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let device = lease.info().device;
    let mut desk = Desk::load(archive, device)?;
    let Some(at) = desk.items.iter().position(|e| e.path == wanted) else {
        return Err(DeskError::NotTracked(wanted));
    };
    let entry = desk.items.remove(at);
    desk.save(archive, device)?;
    if delete_card {
        let _ = std::fs::remove_file(&entry.card);
    }
    let mut change = Change::new(
        Operation::Edit,
        format!(
            "Take {} off the desk",
            entry.path.file_name().unwrap_or_default().to_string_lossy()
        ),
        lease.info().person,
        device,
    );
    change.items = vec![entry.id];
    commit(archive, &change, lease)?;
    Ok(entry)
}

/// Archives the desk item at `path`, holding `lease`: queues it for ingest, its desk card to be
/// kept (its identifier, fields and history), and takes it off this device's desk. The item
/// and its card stay where they are until the proposal is approved; filing then moves the card
/// into the archive with it.
///
/// # Errors
///
/// [`DeskError::NotTracked`] if it isn't on the desk, [`DeskError::NotQueued`] if ingest
/// leaves it out (already queued, say), or an error writing.
pub fn archive(
    archive: &Archive,
    path: &Path,
    lease: &WriterLease,
) -> Result<(DeskEntry, crate::intake::IntakeEntry), DeskError> {
    let wanted = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let device = lease.info().device;
    let mut desk = Desk::load(archive, device)?;
    let Some(at) = desk.items.iter().position(|e| e.path == wanted) else {
        return Err(DeskError::NotTracked(wanted));
    };
    if !wanted.exists() {
        return Err(DeskError::Missing(wanted));
    }
    let not_queued = |reason: String| DeskError::NotQueued {
        path: wanted.clone(),
        reason,
    };
    let whole = crate::ingest::IntakeOptions {
        each: false,
        whole: true,
    };
    let mut queued = crate::intake::preview(archive, std::slice::from_ref(&wanted), whole, device)
        .map_err(|e| not_queued(e.to_string()))?;
    let Some(mut intake) = queued.added.pop() else {
        let reason = queued
            .skipped
            .first()
            .map_or_else(|| "nothing to ingest".to_owned(), |s| s.reason.clone());
        return Err(not_queued(reason));
    };
    let entry = desk.items.remove(at);
    intake.extra.insert(
        CARD_KEY.to_owned(),
        serde_json::Value::String(entry.card.to_string_lossy().into_owned()),
    );
    crate::intake::IntakeQueue::of(archive)
        .save(&intake)
        .map_err(|e| not_queued(e.to_string()))?;
    desk.save(archive, device)?;
    let title = std::fs::read_to_string(&entry.card)
        .ok()
        .and_then(|t| CardFile::parse(&t).ok())
        .map_or_else(
            || {
                entry
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            },
            |f| f.card().title.clone(),
        );
    let mut change = Change::new(
        Operation::Edit,
        format!("Archive “{title}” from the desk"),
        lease.info().person,
        device,
    );
    change.items = vec![entry.id];
    commit(archive, &change, lease)?;
    Ok((entry, intake))
}

/// The desk card a queued or proposed item keeps, from its entry's extra fields.
#[must_use]
pub fn carried_card(extra: &Extra) -> Option<PathBuf> {
    extra
        .get(CARD_KEY)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

/// Gives the proposal `entry` the desk card at `card`'s fields, in place of the ones ingest
/// drafted, and records the card so filing keeps its identifier and body. The sensitivity is
/// the stricter of the two. A card that's gone or doesn't parse leaves the proposal as drafted.
pub fn carry(entry: &mut crate::review::ReviewEntry, card: &Path) {
    let Some(file) = std::fs::read_to_string(card)
        .ok()
        .and_then(|t| CardFile::parse(&t).ok())
    else {
        return;
    };
    let desk = file.card();
    let draft = &mut entry.card;
    draft.title.clone_from(&desk.title);
    draft.summary.clone_from(&desk.summary);
    draft.tags.clone_from(&desk.tags);
    draft.sensitivity = draft.sensitivity.max(desk.sensitivity);
    draft.generated.clone_from(&desk.generated);
    let mut dates = desk.dates.clone().unwrap_or_default();
    if dates.created.is_none() {
        dates.created = draft.dates.as_ref().and_then(|d| d.created.clone());
    }
    draft.dates = Some(dates);
    entry.extra.insert(
        CARD_KEY.to_owned(),
        serde_json::Value::String(card.to_string_lossy().into_owned()),
    );
}

/// How recently a folder must have changed to look like active work, in days.
pub const ACTIVE_DAYS: u32 = 14;

/// How long a desk item must have gone unchanged to be suggested for archiving, in days.
pub const IDLE_DAYS: u32 = 60;

/// The most entries looked at inside one folder when finding when it last changed, so a huge
/// folder doesn't hold up `carrel status`.
const MAX_ENTRIES: usize = 20_000;

/// A folder that looks like active work, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToTrack {
    /// The folder.
    pub path: PathBuf,
    /// Whether it's a Git repository.
    pub git: bool,
    /// When anything in it last changed, as a date.
    pub changed: Option<String>,
}

/// A desk item that hasn't changed for [`IDLE_DAYS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToArchive {
    /// Its desk entry.
    pub entry: DeskEntry,
    /// When it last changed, as a date.
    pub changed: String,
}

/// What `carrel status` suggests about the desk. Nothing is tracked or moved without the owner.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Suggestions {
    /// Folders that look like active work, to put on the desk.
    pub to_track: Vec<ToTrack>,
    /// Desk items unchanged for [`IDLE_DAYS`], to archive.
    pub to_archive: Vec<ToArchive>,
}

/// Suggestions for `desk`, as of `now`: the folders directly inside `places` (the Desktop and
/// `~/Code`, say) that look like active work, changed in the last [`ACTIVE_DAYS`] or a Git
/// repository, and the desk items unchanged for [`IDLE_DAYS`]. Folders already on the desk,
/// checked out to, or inside the archive are left out, as are hidden ones. A folder checked
/// out to a path written with `~` is in `home`.
///
/// # Errors
///
/// [`DeskError::Io`] if the archive's rules or items can't be read.
pub fn suggest(
    archive: &Archive,
    desk: &Desk,
    places: &[PathBuf],
    home: Option<&Path>,
    now: std::time::SystemTime,
) -> Result<Suggestions, DeskError> {
    let failed = |e: &dyn std::fmt::Display| DeskError::Io {
        path: archive.root().to_owned(),
        message: e.to_string(),
    };
    let rules = archive.rules().map_err(|e| failed(&e))?;
    let root = std::fs::canonicalize(archive.root()).map_err(io(archive.root()))?;
    let mut taken: Vec<PathBuf> = desk.items.iter().map(|e| e.path.clone()).collect();
    for item in archive.items().map_err(|e| failed(&e))? {
        let card = std::fs::read_to_string(archive.root().join(&item.card))
            .ok()
            .and_then(|t| CardFile::parse(&t).ok());
        if let Some(out) = card.and_then(|f| f.card().checked_out.clone()) {
            let to = match out.to.strip_prefix("~/") {
                Some(rest) => home.map_or_else(|| out.to.clone().into(), |h| h.join(rest)),
                None => PathBuf::from(&out.to),
            };
            taken.push(std::fs::canonicalize(&to).unwrap_or(to));
        }
    }
    let day = std::time::Duration::from_hours(24);
    let mut suggestions = Suggestions::default();
    for place in places {
        let Ok(entries) = std::fs::read_dir(place) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| std::fs::canonicalize(e.path()).ok())
            .filter(|p| !p.starts_with(&root) && !taken.contains(p))
            .collect();
        found.sort();
        for path in found {
            let git = path.join(".git").exists();
            let changed = last_changed(&path, &rules);
            let recent = changed
                .is_some_and(|t| now.duration_since(t).unwrap_or_default() < day * ACTIVE_DAYS);
            if git || recent {
                suggestions.to_track.push(ToTrack {
                    path,
                    git,
                    changed: changed.map(date),
                });
            }
        }
    }
    for entry in &desk.items {
        if let Some(changed) = last_changed(&entry.path, &rules)
            && now.duration_since(changed).unwrap_or_default() >= day * IDLE_DAYS
        {
            suggestions.to_archive.push(ToArchive {
                entry: entry.clone(),
                changed: date(changed),
            });
        }
    }
    Ok(suggestions)
}

/// The day `time` falls on, in this device's time zone.
fn date(time: std::time::SystemTime) -> String {
    jiff::Timestamp::try_from(time)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).date().to_string())
        .unwrap_or_default()
}

/// When anything in `path` last changed: a file's modification time, or the newest of a
/// folder's files, leaving out regenerable folders and Git's own. `None` if it's gone.
#[must_use]
pub fn last_changed(path: &Path, rules: &crate::detect::Rules) -> Option<std::time::SystemTime> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_dir() {
        return meta.modified().ok();
    }
    let mut newest = None;
    let mut waiting = vec![path.to_owned()];
    let mut seen = 0;
    while let Some(dir) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            seen += 1;
            if seen > MAX_ENTRIES {
                return newest;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if name != ".git" && !rules.is_regenerable(&name) {
                    waiting.push(entry.path());
                }
            } else if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                newest = newest.max(Some(modified));
            }
        }
    }
    newest
}
