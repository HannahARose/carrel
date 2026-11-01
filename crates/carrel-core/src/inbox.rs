//! Inboxes (FORMAT.md §7.8): how a device that isn't the writer changes the archive. When
//! another device is the writer, every change made here (approving a proposal, answering a
//! question, an edit, a checkout or return, a desk change, a flag, an undo, or something new
//! to add) goes into this device's own inbox, `_System/inbox/<device-id>/`, as one JSON file
//! each, with a copy of any files it needs beside it. Only this device writes there, so
//! nothing conflicts, even offline. The writer applies every device's inbox in timestamp
//! order ([`apply`]), each entry recorded in history as the person's and device's that sent it
//! (`docs/design/sync-and-mobile.md`). An entry that no longer applies, such as a proposal
//! decided elsewhere meanwhile, is recorded as skipped, with why, in
//! `_System/inbox-skipped/<device-id>/`, for its device to show.

mod apply;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use apply::{Applied, Applying, apply};

use crate::actions::{ActionError, CardFields};
use crate::archive::Archive;
use crate::card::{Checksum, DateOrTime, Id};
use crate::compressed::plan::Choice;
use crate::coordinator::{Coordinator as _, DiskLock, Holder, LockInfo};
use crate::correct::Changes;
use crate::correct::prepare::Corrects;
use crate::history::Change;

/// The inboxes' folder, relative to the archive root.
pub const INBOX_DIR: &str = "_System/inbox";

/// Where the writer records the entries it skipped, by device, relative to the archive root.
pub const SKIPPED_DIR: &str = "_System/inbox-skipped";

/// How long a skipped entry is kept for its device to see, in days.
pub const SKIPPED_DAYS: i64 = 30;

/// One request from a device that isn't the writer: `_System/inbox/<device>/<name>.json`.
/// A request this version doesn't know isn't read, so it waits for a writer that does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxEntry {
    /// What it asks for: its `action` and that action's keys.
    #[serde(flatten)]
    pub request: Request,
    /// The person who asked.
    pub person: Id,
    /// The device it came from.
    pub device: Id,
    /// When it was asked (RFC 3339).
    pub at: String,
}

impl InboxEntry {
    /// Who asked, as the writer records it.
    #[must_use]
    pub fn holder(&self) -> Holder {
        Holder {
            person: self.person,
            device: self.device,
        }
    }
}

/// What an inbox entry asks the writer to do (FORMAT.md §7.8's table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Request {
    /// Add the files kept beside the entry to the intake queue.
    Add {
        /// The files' names in the folder beside the entry, named as it is without `.json`.
        #[serde(default)]
        files: Vec<String>,
        /// Whether each folder's entries are added one by one.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        each: bool,
        /// The owner's note about them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// For a desk item being archived, its desk card's name beside the files, to keep.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        desk_card: Option<String>,
    },
    /// Add a web address to the intake queue, as a `url` item.
    AddUrl {
        /// The address.
        url: String,
        /// The title it came with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The owner's note about it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// File proposals where proposed, or in Holding.
    Approve {
        /// The proposals.
        proposals: Vec<Id>,
        /// Whether they go to Holding, for the owner to place later, rather than where
        /// proposed.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        holding: bool,
        /// How they're kept beside the archived items they duplicate, when they do.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<KeepAs>,
    },
    /// Reject proposals: their items stay where they are.
    Reject {
        /// The proposals.
        proposals: Vec<Id>,
    },
    /// Propose another collection for a proposal.
    MoveProposal {
        /// The proposal.
        proposal: Id,
        /// The collection's path.
        collection: String,
    },
    /// Say whether proposals' compressed files are expanded when approved.
    ChooseCompressed {
        /// The proposals.
        proposals: Vec<Id>,
        /// What to do with them.
        choice: Choice,
    },
    /// Answer a question.
    Answer {
        /// The question.
        question: Id,
        /// The answer.
        answer: Answer,
    },
    /// Add a routing rule filing items named for a subject into a collection.
    AddRule {
        /// The subject.
        subject: String,
        /// The collection's path.
        collection: String,
    },
    /// Add a fact to the glossary.
    Learn {
        /// The word or name it's about.
        about: String,
        /// What's true.
        fact: String,
    },
    /// A correction, worked out on the device that sent it, of a proposal or a filed item.
    Correct {
        /// What it corrects.
        corrects: Corrects,
        /// The owner's words.
        words: String,
        /// What they were worked out into.
        changes: Box<Changes>,
        /// The model that worked them out, as `provider/offer`, when one did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Who said the words, when not the owner, such as `Librarian`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
        /// The question it answers, when it answers one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answering: Option<Id>,
    },
    /// An edit of a filed card's fields, merged field by field with the card as it is then.
    EditFields {
        /// The item.
        item: Id,
        /// The fields as the editor opened them.
        base: Box<CardFields>,
        /// The fields as edited.
        fields: Box<CardFields>,
    },
    /// An edit of a filed card's body, merged with the body as it is then.
    EditBody {
        /// The item.
        item: Id,
        /// The body as the editor opened it.
        base: String,
        /// The body as edited.
        body: String,
    },
    /// A checkout this device made: the writer records it.
    CheckOut {
        /// The item.
        item: Id,
        /// Where the working copy is, on the device that sent it.
        to: String,
        /// The manifest checksum of the copy it took, which the item must still match.
        checksum: Checksum,
        /// When it's due back.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        due: Option<DateOrTime>,
        /// The commit a Git repository was at.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git_head: Option<String>,
    },
    /// Return a checked-out item from the copy of its working copy kept beside the entry.
    Return {
        /// The item.
        item: Id,
        /// The copy's name in the folder beside the entry.
        name: String,
    },
    /// Cancel a checkout: the archive copy is unlocked as it was.
    CancelCheckout {
        /// The item.
        item: Id,
    },
    /// Flag an item or collection as needing work.
    Flag {
        /// What's flagged.
        target: FlagTarget,
        /// What needs doing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Clear an item's or collection's flag.
    Unflag {
        /// What's unflagged.
        target: FlagTarget,
    },
    /// Expand a filed compressed file.
    Expand {
        /// The item.
        item: Id,
        /// Whether software and big expansions are expanded anyway.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        anyway: bool,
    },
    /// Undo a change in the history.
    Undo {
        /// The change's commit.
        change: String,
    },
    /// Record a change already made in this device's own files, such as its desk.
    Record {
        /// The operation (FORMAT.md §8.2).
        operation: String,
        /// The summary.
        summary: String,
        /// The items it concerns.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        items: Vec<Id>,
    },
}

impl Request {
    /// What it asks, in a few words, for people.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Add { files, .. } => match files.len() {
                1 => format!("add {}", files[0]),
                n => format!("add {n} items"),
            },
            Self::AddUrl { url, .. } => format!("add {url}"),
            Self::Approve {
                proposals, holding, ..
            } => format!(
                "{} {}",
                if *holding { "hold" } else { "approve" },
                count(proposals.len(), "proposal")
            ),
            Self::Reject { proposals } => format!("reject {}", count(proposals.len(), "proposal")),
            Self::MoveProposal { collection, .. } => format!("propose {collection}"),
            Self::ChooseCompressed { proposals, .. } => {
                format!("choose for {}", count(proposals.len(), "compressed file"))
            }
            Self::Answer { .. } => "answer a question".into(),
            Self::AddRule { subject, .. } => format!("add a rule for “{subject}”"),
            Self::Learn { about, .. } => format!("add “{about}” to the glossary"),
            Self::Correct { words, .. } => format!("correct: {words}"),
            Self::EditFields { .. } => "edit a card".into(),
            Self::EditBody { .. } => "edit a card's notes".into(),
            Self::CheckOut { .. } => "check out an item".into(),
            Self::Return { .. } => "return an item".into(),
            Self::CancelCheckout { .. } => "cancel a checkout".into(),
            Self::Flag { .. } => "flag as needing work".into(),
            Self::Unflag { .. } => "clear a flag".into(),
            Self::Expand { .. } => "expand a compressed file".into(),
            Self::Undo { .. } => "undo a change".into(),
            Self::Record { summary, .. } => summary.clone(),
        }
    }
}

fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// How approved proposals are kept beside the archived items they duplicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepAs {
    /// As a new version of it, or both kept, related.
    pub link: KeepLink,
    /// The archived item, when given; otherwise the one it duplicates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// How a duplicate is kept ([`crate::actions::Kept`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeepLink {
    /// As a new version.
    Version,
    /// Both kept, related.
    Both,
}

/// An answer to a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Answer {
    /// Nothing changes, and it isn't asked again.
    Decline,
    /// It rests, to be asked again later.
    Skip,
    /// The items stop waiting on it.
    Dismiss,
    /// It's answered by a change made another way.
    Settle,
    /// A fact for the glossary; the items waiting on it are drafted again.
    Teach {
        /// The fact.
        fact: String,
    },
    /// A collection for its items.
    Place {
        /// The collection's path.
        collection: String,
        /// Whether to create it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        create: bool,
    },
}

/// What a flag is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FlagTarget {
    /// An item.
    Item {
        /// Its identifier.
        id: Id,
    },
    /// A collection.
    Collection {
        /// Its folder, relative to the archive root.
        path: String,
    },
}

impl FlagTarget {
    /// The target of a flag on `what`.
    #[must_use]
    pub fn of(what: &crate::flags::Flaggable) -> Self {
        match what {
            crate::flags::Flaggable::Item { id, .. } => Self::Item { id: *id },
            crate::flags::Flaggable::Collection { path } => Self::Collection { path: path.clone() },
        }
    }
}

/// Why this device acts through its inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sending {
    /// Another device holds the writer lease.
    Writer(Box<LockInfo>),
    /// The archive is synced, and on this system Carrel can't take its writer lock, so it's
    /// never the writer here (`docs/design/sync-and-mobile.md`, decision 2).
    Unlocked,
}

impl Sending {
    /// Why, in a sentence, for people.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Writer(lock) => format!(
                "Another device ({}) is the writer now, so this went into this device's inbox, for it to apply.",
                lock.device
            ),
            Self::Unlocked => format!(
                "This archive is synced, and on {} Carrel can't take its writer lock, so this device acts through its inbox: it reads freely, and a Mac applies its changes. Full writes need an archive that isn't synced.",
                platform()
            ),
        }
    }

    /// The writer device, when another device is the writer.
    #[must_use]
    pub fn writer(&self) -> Option<Id> {
        match self {
            Self::Writer(lock) => Some(lock.device),
            Self::Unlocked => None,
        }
    }
}

fn platform() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "this system"
    }
}

/// Whether `device` acts through its inbox in `archive`, and why: when another device holds
/// the writer lease, or, off a Mac, iPad or iPhone, when the archive is synced.
#[must_use]
pub fn sending(archive: &Archive, device: Id) -> Option<Sending> {
    if !cfg!(target_vendor = "apple") && synced(archive.root()) {
        return Some(Sending::Unlocked);
    }
    writer_elsewhere(archive, device).map(|lock| Sending::Writer(Box::new(lock)))
}

/// Whether the archive at `root` is in a folder a sync service keeps, such as iCloud Drive,
/// as far as its path shows, or `CARREL_SYNCED` says it is (for a sync service Carrel doesn't
/// recognize, and for tests).
#[must_use]
pub fn synced(root: &Path) -> bool {
    if let Ok(said) = std::env::var("CARREL_SYNCED") {
        return matches!(said.trim(), "1" | "true" | "yes");
    }
    if DiskLock::is_synced_folder(root) {
        return true;
    }
    root.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_lowercase();
        [
            "icloud drive",
            "iclouddrive",
            "onedrive",
            "dropbox",
            "google drive",
        ]
        .iter()
        .any(|s| name == *s || name.starts_with(&format!("{s} -")))
    })
}

/// The writer's lease, when a device other than `device` holds it and it hasn't run out: then
/// what `device` does goes into its inbox.
#[must_use]
pub fn writer_elsewhere(archive: &Archive, device: Id) -> Option<LockInfo> {
    DiskLock::for_archive(archive)
        .current()
        .ok()
        .flatten()
        .filter(|lock| lock.device != device && !lock.is_expired())
}

/// `device`'s inbox folder in `archive`.
#[must_use]
pub fn folder(archive: &Archive, device: Id) -> PathBuf {
    archive.root().join(INBOX_DIR).join(device.to_string())
}

/// A new entry's file name, without `.json`: `<timestamp>-<uuid>`, so names sort in the
/// order they were made.
fn new_name() -> String {
    format!(
        "{}-{}",
        jiff::Timestamp::now().strftime("%Y%m%dT%H%M%S%.6fZ"),
        Id::new_v7()
    )
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> ActionError + '_ {
    move |source| ActionError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Writes `value` as JSON to `path`, through a temporary file beside it.
fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or(path);
    std::fs::create_dir_all(folder)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = folder.join(format!(".{name}.new"));
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, text + "\n")?;
    std::fs::rename(&temporary, path)
}

/// Writes `entry` as `name` in its device's inbox.
fn save(archive: &Archive, name: &str, entry: &InboxEntry) -> Result<PathBuf, ActionError> {
    let path = folder(archive, entry.device).join(format!("{name}.json"));
    write_json(&path, entry).map_err(io_error(&path))?;
    Ok(path)
}

fn entry(request: Request, holder: Holder) -> InboxEntry {
    InboxEntry {
        request,
        person: holder.person,
        device: holder.device,
        at: jiff::Zoned::now().to_string(),
    }
}

fn trimmed(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// Puts `request` in `holder`'s inbox, for the writer to apply as theirs. Returns the entry's
/// file.
///
/// # Errors
///
/// [`ActionError::Io`] if the entry can't be written.
pub fn send(archive: &Archive, holder: Holder, request: Request) -> Result<PathBuf, ActionError> {
    save(archive, &new_name(), &entry(request, holder))
}

/// Puts `request` in `holder`'s inbox with copies of `paths` in a folder beside it, under free
/// names; `named` gets the names given, to say which is which. Returns the entry's file.
///
/// # Errors
///
/// [`ActionError::Io`] if a path can't be copied or the entry written; nothing is left then.
pub fn send_with_copies(
    archive: &Archive,
    holder: Holder,
    paths: &[PathBuf],
    named: impl FnOnce(Vec<String>) -> Request,
) -> Result<PathBuf, ActionError> {
    let name = new_name();
    let files = folder(archive, holder.device).join(&name);
    let copied = copy_in(&files, paths);
    let result = copied.and_then(|names| save(archive, &name, &entry(named(names), holder)));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&files);
    }
    result
}

/// Copies `paths` into `files`, each under a free name. Returns the names.
fn copy_in(files: &Path, paths: &[PathBuf]) -> Result<Vec<String>, ActionError> {
    std::fs::create_dir_all(files).map_err(io_error(files))?;
    let mut names = Vec::new();
    for path in paths {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| ActionError::Invalid(format!("{} has no name", path.display())))?;
        let taken = crate::ingest::filing::free_name(files, &file_name).map_err(io_error(files))?;
        copy(path, &files.join(&taken)).map_err(io_error(path))?;
        names.push(taken);
    }
    Ok(names)
}

/// Puts copies of `paths` in `holder`'s inbox, for the writer to add, with the owner's `note`.
/// Folders go in whole unless `each`. Returns the entry's file.
///
/// # Errors
///
/// [`ActionError::Io`] if a path can't be copied or the entry written.
pub fn send_paths(
    archive: &Archive,
    holder: Holder,
    paths: &[PathBuf],
    each: bool,
    note: Option<&str>,
) -> Result<PathBuf, ActionError> {
    send_with_copies(archive, holder, paths, |files| Request::Add {
        files,
        each,
        note: trimmed(note),
        desk_card: None,
    })
}

/// Puts the web address `url` in `holder`'s inbox, for the writer to add as a `url` item.
///
/// # Errors
///
/// [`ActionError::Io`] if the entry can't be written.
pub fn send_url(
    archive: &Archive,
    holder: Holder,
    url: &str,
    title: Option<&str>,
    note: Option<&str>,
) -> Result<PathBuf, ActionError> {
    let request = Request::AddUrl {
        url: url.trim().to_owned(),
        title: trimmed(title),
        note: trimmed(note),
    };
    send(archive, holder, request)
}

/// Puts copies of the desk item at `path` and its desk card `card` in `holder`'s inbox, for
/// the writer to queue for ingest, keeping the card ([`crate::desk::archive_through_inbox`]).
///
/// # Errors
///
/// [`ActionError::Io`] if either can't be copied or the entry written.
pub fn send_desk_item(
    archive: &Archive,
    holder: Holder,
    path: &Path,
    card: &Path,
) -> Result<PathBuf, ActionError> {
    let paths = [path.to_owned(), card.to_owned()];
    send_with_copies(archive, holder, &paths, |mut names| {
        let card = names.pop();
        Request::Add {
            files: names,
            each: false,
            note: None,
            desk_card: card,
        }
    })
}

/// Puts `change`, already made in `holder`'s own files, in their inbox, for the writer to
/// record in history.
///
/// # Errors
///
/// [`ActionError::Io`] if the entry can't be written.
pub fn send_record(
    archive: &Archive,
    holder: Holder,
    change: &Change,
) -> Result<PathBuf, ActionError> {
    let request = Request::Record {
        operation: change.operation.as_str().to_owned(),
        summary: change.summary.clone(),
        items: change.items.clone(),
    };
    send(archive, holder, request)
}

/// Checks `item` out to `destination` as `holder`, when another device is the writer: the copy
/// is made here, as [`crate::checkout::check_out`] makes it, and the writer records it, as long
/// as the item hasn't changed meanwhile. Returns the working copy and the entry's file.
///
/// # Errors
///
/// [`crate::checkout::CheckoutError`] if it can't come out, or the entry can't be written.
pub fn send_checkout(
    archive: &Archive,
    holder: Holder,
    item: &crate::archive::ItemRef,
    destination: &Path,
    due: Option<DateOrTime>,
) -> Result<(PathBuf, PathBuf), crate::checkout::CheckoutError> {
    let (working, manifest, git_head) = crate::checkout::copy_out(archive, item, destination)?;
    let request = Request::CheckOut {
        item: item.id,
        to: working.display().to_string(),
        checksum: manifest.checksum(),
        due,
        git_head,
    };
    let sent = send(archive, holder, request)
        .map_err(|e| crate::checkout::CheckoutError::Card(e.to_string()))?;
    Ok((working, sent))
}

/// Returns `item` from its working copy at `working` as `holder`, when another device is the
/// writer: a copy of the working copy goes to their inbox, for the writer to swap in. The
/// working copy stays where it is.
///
/// # Errors
///
/// [`ActionError::Io`] if it can't be copied or the entry written.
pub fn send_return(
    archive: &Archive,
    holder: Holder,
    item: Id,
    working: &Path,
) -> Result<PathBuf, ActionError> {
    send_with_copies(archive, holder, &[working.to_owned()], |mut names| {
        Request::Return {
            item,
            name: names.pop().unwrap_or_default(),
        }
    })
}

/// Every device's waiting inbox entries, in the order they were made: each file and what it
/// says. Files that can't be read, or ask for what this version doesn't know, are left out,
/// for a newer writer or the owner.
#[must_use]
pub fn waiting(archive: &Archive) -> Vec<(PathBuf, InboxEntry)> {
    let mut found = Vec::new();
    let Ok(devices) = std::fs::read_dir(archive.root().join(INBOX_DIR)) else {
        return found;
    };
    for device in devices.flatten().filter(|d| d.path().is_dir()) {
        found.extend(entries_in(&device.path()));
    }
    found.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
    found
}

/// `device`'s own waiting entries, in the order they were made.
#[must_use]
pub fn waiting_from(archive: &Archive, device: Id) -> Vec<(PathBuf, InboxEntry)> {
    let mut found = entries_in(&folder(archive, device));
    found.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
    found
}

fn entries_in<T: serde::de::DeserializeOwned>(folder: &Path) -> Vec<(PathBuf, T)> {
    let Ok(files) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    files
        .flatten()
        .filter(|f| {
            f.path().extension().is_some_and(|e| e == "json")
                && !f.file_name().to_string_lossy().starts_with('.')
        })
        .filter_map(|f| {
            let path = f.path();
            let text = std::fs::read_to_string(&path).ok()?;
            Some((path, serde_json::from_str(&text).ok()?))
        })
        .collect()
}

/// An inbox entry the writer skipped because it no longer applied:
/// `_System/inbox-skipped/<device>/<name>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skipped {
    /// The entry as it was sent.
    pub entry: InboxEntry,
    /// Why it was skipped, for people.
    pub reason: String,
    /// The writer device that skipped it.
    pub by: Id,
    /// When (RFC 3339).
    pub skipped: String,
}

/// The entries `device` sent that the writer skipped, newest first, from the last
/// [`SKIPPED_DAYS`] days.
#[must_use]
pub fn skipped(archive: &Archive, device: Id) -> Vec<Skipped> {
    let folder = archive.root().join(SKIPPED_DIR).join(device.to_string());
    let mut found: Vec<(PathBuf, Skipped)> = entries_in(&folder);
    found.sort_by(|a, b| b.0.file_name().cmp(&a.0.file_name()));
    found.into_iter().map(|(_, s)| s).collect()
}

/// What Status says about this device's inbox: how many of its entries wait for the writer,
/// and which were skipped, and why.
#[must_use]
pub fn status_lines(archive: &Archive, device: Id) -> Vec<String> {
    let mut lines = Vec::new();
    let waiting = waiting_from(archive, device).len();
    if waiting > 0 {
        lines.push(format!(
            "{} in this device's inbox, waiting for the writer to apply {}",
            count(waiting, "change"),
            if waiting == 1 { "it" } else { "them" }
        ));
    }
    for skip in skipped(archive, device) {
        lines.push(format!(
            "Skipped “{}” ({}): {}",
            skip.entry.request.describe(),
            skip.entry.at.get(..10).unwrap_or_default(),
            skip.reason
        ));
    }
    lines
}

/// Copies the file or folder at `from` to `to`.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(drop)
    }
}

/// Takes the writer lease as `holder` for `purpose`.
fn lease_as(
    archive: &Archive,
    holder: Holder,
    purpose: &str,
) -> Result<crate::coordinator::WriterLease, crate::coordinator::CoordinatorError> {
    DiskLock::for_archive(archive).acquire(holder, purpose, crate::coordinator::DEFAULT_LEASE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_reads_back_as_it_was_written() {
        let holder = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        let written = entry(
            Request::Answer {
                question: Id::new_v7(),
                answer: Answer::Place {
                    collection: "Work".into(),
                    create: true,
                },
            },
            holder,
        );
        let text = serde_json::to_string(&written).unwrap();
        assert!(text.contains(r#""action":"answer""#), "{text}");
        let read: InboxEntry = serde_json::from_str(&text).unwrap();
        assert_eq!(read, written);
    }

    #[test]
    fn an_older_add_entry_still_reads() {
        let text = r#"{"action":"add","files":["a.txt"],"note":"hi","person":"01928f39-eeee-7fff-8000-000000000005","device":"01928f3a-0000-7000-8000-00000000d002","at":"2026-10-29T18:01:00-07:00"}"#;
        let read: InboxEntry = serde_json::from_str(text).unwrap();
        assert!(matches!(read.request, Request::Add { ref files, .. } if files == &["a.txt"]));
    }

    #[test]
    fn an_action_this_version_doesnt_know_isnt_read() {
        let text = r#"{"action":"teleport","person":"01928f39-eeee-7fff-8000-000000000005","device":"01928f3a-0000-7000-8000-00000000d002","at":"2026-10-29T18:01:00-07:00"}"#;
        assert!(serde_json::from_str::<InboxEntry>(text).is_err());
    }

    #[test]
    fn synced_folders_are_recognized_by_name() {
        assert!(synced(Path::new("/home/me/Dropbox/Archive")));
        assert!(synced(Path::new("C:/Users/me/iCloudDrive/Archive")));
        assert!(synced(Path::new("/home/me/OneDrive - Work/Archive")));
        assert!(!synced(Path::new("/home/me/Archive")) || std::env::var("CARREL_SYNCED").is_ok());
    }
}
