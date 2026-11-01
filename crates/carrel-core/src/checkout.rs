//! Checking items out and back in (`docs/design/checkout.md`): what an item looked like when
//! it went out, how its working copy differs now, and the lock that keeps the archive copy
//! from changing meanwhile.
//!
//! The lock is the files' own permissions: while an item is out, its archive copy is
//! read-only, so an editor opened on it by mistake can't save, and anything (Finder, `chmod`)
//! can undo it if something goes wrong. Carrel itself also refuses to change an item whose
//! card says it's checked out. On macOS the working copy also carries a "Checked Out" Finder
//! tag ([`crate::finder::CHECKED_OUT`]) while it's out, so it shows in Finder; it's a nicety,
//! so failing to set or clear it never stops a checkout or return.

use std::path::{Path, PathBuf};

use crate::archive::{Archive, ItemRef};
use crate::card::{Checkout, Checksum, DateOrTime, Extra, Id};
use crate::card_file::CardFile;
use crate::coordinator::WriterLease;
use crate::detect::Rules;
use crate::history::{Change, History, HistoryError, Operation};
use crate::manifest::{Diff, Entry, Manifest, ManifestError};

/// Why an item couldn't be compared or locked.
#[derive(Debug)]
pub enum CheckoutError {
    /// Reading the item failed.
    Manifest(ManifestError),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The working copy isn't there.
    Missing(PathBuf),
    /// A card-only item has nothing to check out.
    CardOnly(String),
    /// The item is already checked out, to this path.
    AlreadyOut(String),
    /// The card couldn't be read or written.
    Card(String),
    /// The copy didn't match the item, so nothing was checked out.
    CopyChanged(PathBuf),
    /// The history couldn't be written.
    History(HistoryError),
}

impl std::fmt::Display for CheckoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(e) => write!(f, "{e}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Missing(path) => write!(
                f,
                "the working copy isn't at {}; give its new place with --from",
                path.display()
            ),
            Self::CardOnly(card) => write!(f, "{card} has no contents to check out"),
            Self::AlreadyOut(to) => write!(f, "it's already checked out, to {to}"),
            Self::Card(why) => f.write_str(why),
            Self::CopyChanged(path) => write!(
                f,
                "the copy at {} doesn't match what it was copied from, so nothing changed",
                path.display()
            ),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CheckoutError {}

impl From<ManifestError> for CheckoutError {
    fn from(e: ManifestError) -> Self {
        Self::Manifest(e)
    }
}

impl From<HistoryError> for CheckoutError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

/// An item that's been checked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOut {
    /// The item.
    pub item: Id,
    /// Its title.
    pub title: String,
    /// Where the working copy is.
    pub working: PathBuf,
    /// The history commit that recorded it.
    pub commit: Option<String>,
}

/// Where the item as it was checked out is kept, to compare with on return. Manifests aren't
/// in the history (FORMAT.md §8.1), so this one isn't either.
#[must_use]
pub fn manifest_path(archive: &Archive, item: Id) -> PathBuf {
    archive
        .root()
        .join(format!("_System/manifests/{item}.checkout.tsv"))
}

/// Checks `item` out into the folder `destination`, holding `lease` (`docs/design/checkout.md`):
/// saves what it holds now, copies it (under a free name, checked against what was saved),
/// records `checked_out` on its card, with the commit a Git repository is at, makes the
/// archive copy read-only, commits it all as one `checkout`, and on macOS tags the working
/// copy "Checked Out" in Finder. If any step fails, what was done is undone.
///
/// # Errors
///
/// [`CheckoutError::CardOnly`] or [`CheckoutError::AlreadyOut`] when it can't come out, or a
/// failure reading, copying or recording it.
pub fn check_out(
    archive: &Archive,
    item: &ItemRef,
    destination: &Path,
    due: Option<DateOrTime>,
    lease: &WriterLease,
) -> Result<CheckedOut, CheckoutError> {
    let Some(contents) = &item.item else {
        return Err(CheckoutError::CardOnly(item.card.clone()));
    };
    let root = archive.root();
    let card_path = root.join(&item.card);
    let original = std::fs::read_to_string(&card_path).map_err(io(&card_path))?;
    let mut file = CardFile::parse(&original).map_err(|e| CheckoutError::Card(e.to_string()))?;
    if let Some(out) = &file.card().checked_out {
        return Err(CheckoutError::AlreadyOut(out.to.clone()));
    }
    let source = root.join(contents);
    let rules = archive
        .rules()
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    let manifest = snapshot(&source, item.id, &rules)?;
    let saved = manifest_path(archive, item.id);
    if let Some(parent) = saved.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    std::fs::write(&saved, manifest.to_tsv()).map_err(io(&saved))?;

    std::fs::create_dir_all(destination).map_err(io(destination))?;
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let working = destination
        .join(crate::ingest::filing::free_name(destination, &name).map_err(io(destination))?);
    let undo_copy = |working: &Path| {
        let _ = if working.is_dir() {
            std::fs::remove_dir_all(working)
        } else {
            std::fs::remove_file(working)
        };
    };
    crate::ingest::filing::copy(&source, &working)
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    if !compare(&manifest, &working, &rules)?.is_empty() {
        undo_copy(&working);
        return Err(CheckoutError::CopyChanged(working));
    }

    let title = file.card().title.clone();
    let mut extra = Extra::new();
    if let Some(head) = git_head(&source) {
        extra.insert("git_head".into(), serde_json::Value::String(head));
    }
    file.card_mut().checked_out = Some(Checkout {
        to: working.display().to_string(),
        device: lease.info().device,
        person: lease.info().person,
        since: DateOrTime(
            jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string(),
        ),
        due,
        manifest: Some(manifest.checksum()),
        extra,
    });
    let result = (|| -> Result<Option<String>, CheckoutError> {
        let text = file
            .to_text()
            .map_err(|e| CheckoutError::Card(e.to_string()))?;
        write_card(&card_path, &text)?;
        lock(&source)?;
        let mut change = Change::new(
            Operation::Checkout,
            format!("Check out “{title}”"),
            lease.info().person,
            lease.info().device,
        );
        change.items = vec![item.id];
        refresh_active(archive)?;
        Ok(History::open(archive)?.commit(archive, &change, lease)?)
    })();
    match result {
        Ok(commit) => {
            let _ = crate::finder::add_tag(&working, crate::finder::CHECKED_OUT);
            Ok(CheckedOut {
                item: item.id,
                title,
                working,
                commit,
            })
        }
        Err(e) => {
            let _ = unlock(&source);
            let _ = write_card(&card_path, &original);
            undo_copy(&working);
            Err(e)
        }
    }
}

/// A return, worked out but not yet done: what changed and the history entry it would add.
#[derive(Debug, Clone)]
pub struct ReturnPlan {
    /// The item.
    pub item: Id,
    /// Its card's path.
    pub card: String,
    /// Its title.
    pub title: String,
    /// Where the working copy is.
    pub working: PathBuf,
    /// How it differs from the item as it went out.
    pub diff: Diff,
    /// For a Git repository, the subjects of the commits since checkout, newest first.
    pub commits: Vec<String>,
    /// For a Git repository, whether there are changes not committed.
    pub uncommitted: bool,
    /// The history entry the return adds, approved with it.
    pub entry: crate::entries::HistoryEntry,
}

/// What returning did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Returned {
    /// The history commit.
    pub commit: Option<String>,
    /// The backup snapshot taken first, if there are backups.
    pub snapshot: Option<String>,
}

/// Works out returning `item`: compares the working copy (at `from`, or where it was checked
/// out to) with the item as it went out, reads a Git repository's commits since then, and
/// drafts the history entry. Changes nothing.
///
/// # Errors
///
/// [`CheckoutError::Card`] if the item isn't checked out, [`CheckoutError::Missing`] if the
/// working copy isn't there, or an error reading it.
pub fn plan_return(
    archive: &Archive,
    item: &ItemRef,
    from: Option<&Path>,
) -> Result<ReturnPlan, CheckoutError> {
    let card_path = archive.root().join(&item.card);
    let text = std::fs::read_to_string(&card_path).map_err(io(&card_path))?;
    let file = CardFile::parse(&text).map_err(|e| CheckoutError::Card(e.to_string()))?;
    let card = file.card();
    let Some(out) = &card.checked_out else {
        return Err(CheckoutError::Card(format!(
            "{} isn't checked out",
            card.title
        )));
    };
    let working = from.map_or_else(|| PathBuf::from(&out.to), Path::to_owned);
    let saved = manifest_path(archive, item.id);
    let then = Manifest::parse(&std::fs::read_to_string(&saved).map_err(io(&saved))?)?;
    let rules = archive
        .rules()
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    let diff = compare(&then, &working, &rules)?;
    let head = out.extra.get("git_head").and_then(|v| v.as_str());
    let (commits, uncommitted) = head.map_or((Vec::new(), false), |h| git_since(&working, h));
    let entry = draft_entry(&diff, &commits);
    Ok(ReturnPlan {
        item: item.id,
        card: item.card.clone(),
        title: card.title.clone(),
        working,
        diff,
        commits,
        uncommitted,
        entry,
    })
}

/// The history entry for a return: what changed, in a line or two.
fn draft_entry(diff: &Diff, commits: &[String]) -> crate::entries::HistoryEntry {
    let mut lines = Vec::new();
    if diff.is_empty() {
        lines.push("Returned unchanged.".to_owned());
    } else {
        let mut parts = Vec::new();
        for (n, what) in [
            (diff.changed.len(), "changed"),
            (diff.added.len(), "added"),
            (diff.removed.len(), "removed"),
            (diff.renamed.len(), "renamed"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {what}"));
            }
        }
        lines.push(format!("Returned: {} file(s).", parts.join(", ")));
        let names: Vec<&str> = diff
            .changed
            .iter()
            .chain(&diff.added)
            .map(String::as_str)
            .take(8)
            .collect();
        if !names.is_empty() {
            lines.push(format!("Files: {}", names.join(", ")));
        }
    }
    if !commits.is_empty() {
        let quoted: Vec<String> = commits.iter().take(8).map(|c| format!("\"{c}\"")).collect();
        lines.push(format!("Commits: {}", quoted.join(", ")));
    }
    let today = jiff::Zoned::now().date().to_string();
    crate::entries::HistoryEntry::pending(&today, None, lines)
}

/// The subjects of a repository's commits since `head`, newest first, and whether it has
/// uncommitted changes.
fn git_since(path: &Path, head: &str) -> (Vec<String>, bool) {
    let Ok(repo) = git2::Repository::open(path) else {
        return (Vec::new(), false);
    };
    let mut subjects = Vec::new();
    if let (Ok(mut walk), Ok(since)) = (repo.revwalk(), git2::Oid::from_str(head))
        && walk.push_head().is_ok()
        && walk.hide(since).is_ok()
    {
        for oid in walk.flatten() {
            if let Ok(commit) = repo.find_commit(oid) {
                subjects.push(commit.summary().ok().flatten().unwrap_or("").to_owned());
            }
        }
    }
    let uncommitted = repo
        .statuses(None)
        .is_ok_and(|s| s.iter().any(|e| !e.status().is_ignored()));
    (subjects, uncommitted)
}

/// Returns an item as `plan` worked out, holding `lease`: a backup snapshot first when there
/// are backups, then the working copy written beside the archive copy, checked, and swapped
/// in; the card's `checked_out` removed, its checksum or manifest updated and the entry
/// approved; the lock released; and one `return` commit. The working copy is left where it
/// is, without its "Checked Out" Finder tag: deleting it is the owner's choice, made
/// afterwards. The owner's other tags come back with it.
///
/// # Errors
///
/// An error at any step. Until the swap, the archive copy is untouched; a failure after it
/// leaves the new copy in place and reports what's left to do.
pub fn finish_return(
    archive: &Archive,
    plan: &ReturnPlan,
    lease: &WriterLease,
    backups: Option<&dyn crate::backup::Backups>,
) -> Result<Returned, CheckoutError> {
    let root = archive.root();
    let card_path = root.join(&plan.card);
    let text = std::fs::read_to_string(&card_path).map_err(io(&card_path))?;
    let mut file = CardFile::parse(&text).map_err(|e| CheckoutError::Card(e.to_string()))?;
    let contents = file
        .card()
        .item
        .clone()
        .ok_or_else(|| CheckoutError::CardOnly(plan.card.clone()))?;
    let folder = Path::new(&plan.card)
        .parent()
        .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
    let archived = root.join(&folder).join(&contents);
    let rules = archive
        .rules()
        .map_err(|e| CheckoutError::Card(e.to_string()))?;

    let backup = match backups {
        Some(backups) => Some(
            backups
                .snapshot(root, &format!("Before returning “{}”", plan.title))
                .map_err(|e| CheckoutError::Card(e.to_string()))?
                .trailer(),
        ),
        None => None,
    };

    // Write the new copy beside the old, check it, then swap the two.
    let incoming = archived.with_file_name(format!(".carrel-return-{}", plan.item));
    let outgoing = archived.with_file_name(format!(".carrel-old-{}", plan.item));
    let _ = crate::finder::remove_tag(&plan.working, crate::finder::CHECKED_OUT);
    crate::ingest::filing::copy(&plan.working, &incoming)
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    let _ = crate::finder::remove_tag(&incoming, crate::finder::CHECKED_OUT);
    let same = if plan.working.is_dir() {
        snapshot(&plan.working, plan.item, &rules)?
            .diff(&snapshot(&incoming, plan.item, &rules)?)
            .is_empty()
    } else {
        Checksum::of_file(&plan.working).map_err(io(&plan.working))?
            == Checksum::of_file(&incoming).map_err(io(&incoming))?
    };
    if !same {
        remove_any(&incoming);
        return Err(CheckoutError::CopyChanged(incoming));
    }
    unlock(&archived)?;
    std::fs::rename(&archived, &outgoing).map_err(io(&archived))?;
    if let Err(e) = std::fs::rename(&incoming, &archived) {
        let _ = std::fs::rename(&outgoing, &archived);
        return Err(io(&archived)(e));
    }
    remove_any(&outgoing);

    let card = file.card_mut();
    card.checked_out = None;
    if archived.is_dir() {
        let manifest = Manifest::build(&archived, plan.item, &rules, None)?;
        let path = format!("_System/manifests/{}.tsv", plan.item);
        std::fs::write(root.join(&path), manifest.to_tsv()).map_err(io(&root.join(&path)))?;
        card.size = Some(manifest.total_size());
        if let Some(reference) = card.manifest.as_mut() {
            reference.checksum = manifest.checksum();
        }
    } else {
        card.checksum = Some(Checksum::of_file(&archived).map_err(io(&archived))?);
        card.size = Some(std::fs::metadata(&archived).map_err(io(&archived))?.len());
    }
    let mut log = file
        .history()
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    let mut entry = plan.entry.clone();
    entry.pending = false;
    log.add(entry);
    file.set_history(&log)
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    let text = file
        .to_text()
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    write_card(&card_path, &text)?;
    let _ = std::fs::remove_file(manifest_path(archive, plan.item));

    let mut change = Change::new(
        Operation::Return,
        format!("Return “{}”", plan.title),
        lease.info().person,
        lease.info().device,
    );
    change.items = vec![plan.item];
    change.snapshot.clone_from(&backup);
    refresh_active(archive)?;
    let commit = History::open(archive)?.commit(archive, &change, lease)?;
    Ok(Returned {
        commit,
        snapshot: backup,
    })
}

/// Cancels `item`'s checkout, holding `lease`: the archive copy is unlocked as it was, the
/// card's `checked_out` removed, and the working copy left where it is, with its changes not
/// brought back and its "Checked Out" Finder tag removed. One `return` commit.
///
/// # Errors
///
/// [`CheckoutError::Card`] if it isn't checked out, or an error writing.
pub fn cancel_checkout(
    archive: &Archive,
    item: &ItemRef,
    lease: &WriterLease,
) -> Result<Option<String>, CheckoutError> {
    let root = archive.root();
    let card_path = root.join(&item.card);
    let text = std::fs::read_to_string(&card_path).map_err(io(&card_path))?;
    let mut file = CardFile::parse(&text).map_err(|e| CheckoutError::Card(e.to_string()))?;
    let title = file.card().title.clone();
    let Some(out) = &file.card().checked_out else {
        return Err(CheckoutError::Card(format!("{title} isn't checked out")));
    };
    let working = PathBuf::from(&out.to);
    if let Some(contents) = &item.item {
        unlock(&root.join(contents))?;
    }
    file.card_mut().checked_out = None;
    let text = file
        .to_text()
        .map_err(|e| CheckoutError::Card(e.to_string()))?;
    write_card(&card_path, &text)?;
    let _ = std::fs::remove_file(manifest_path(archive, item.id));
    let mut change = Change::new(
        Operation::Return,
        format!("Cancel the checkout of “{title}”"),
        lease.info().person,
        lease.info().device,
    );
    change.items = vec![item.id];
    refresh_active(archive)?;
    let commit = History::open(archive)?.commit(archive, &change, lease)?;
    let _ = crate::finder::remove_tag(&working, crate::finder::CHECKED_OUT);
    Ok(commit)
}

/// Rebuilds `Active/` and the Journal, so the change that follows records them too.
fn refresh_active(archive: &Archive) -> Result<(), CheckoutError> {
    crate::active::regenerate(archive)
        .map(|_| ())
        .map_err(|e| CheckoutError::Card(e.to_string()))
}

fn remove_any(path: &Path) {
    let _ = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}

/// The commit a Git repository at `path` is at, if it is one.
fn git_head(path: &Path) -> Option<String> {
    if !path.join(".git").exists() {
        return None;
    }
    let repo = git2::Repository::open(path).ok()?;
    let head = repo.head().ok()?.peel_to_commit().ok()?;
    Some(head.id().to_string())
}

/// Writes a card in full, through a temporary file beside it.
fn write_card(path: &Path, text: &str) -> Result<(), CheckoutError> {
    let temp = path.with_extension("md.tmp-checkout");
    std::fs::write(&temp, text).map_err(io(&temp))?;
    std::fs::rename(&temp, path).map_err(io(path))
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> CheckoutError + '_ {
    move |source| CheckoutError::Io {
        path: path.to_owned(),
        source,
    }
}

/// What `path` holds now, as a manifest: every file in a folder or bundle, leaving out what
/// `rules` call regenerable, or a single file as one entry named after it.
///
/// # Errors
///
/// [`CheckoutError::Missing`] if nothing is at `path`, or an error reading it.
pub fn snapshot(path: &Path, item: Id, rules: &Rules) -> Result<Manifest, CheckoutError> {
    let built = std::time::SystemTime::now();
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CheckoutError::Missing(path.to_owned()));
        }
        Err(e) => return Err(io(path)(e)),
    };
    if meta.is_dir() {
        return Ok(Manifest::build(path, item, rules, None)?);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified = meta.modified().map_err(io(path))?;
    Ok(Manifest {
        item,
        entries: vec![Entry {
            path: name,
            checksum: Checksum::of_file(path).map_err(io(path))?,
            size: meta.len(),
            modified: crate::manifest::timestamp(modified),
        }],
        built: Some(built),
    })
}

/// How the working copy at `working` differs from the item as it was checked out
/// (`at_checkout`). For a single file, a different name counts as the same file changed.
///
/// # Errors
///
/// As [`snapshot`].
pub fn compare(
    at_checkout: &Manifest,
    working: &Path,
    rules: &Rules,
) -> Result<Diff, CheckoutError> {
    let mut now = snapshot(working, at_checkout.item, rules)?;
    if let ([then], [entry]) = (at_checkout.entries.as_slice(), now.entries.as_mut_slice()) {
        // A file checked out and renamed on the desk is still the same item.
        if !working.is_dir() {
            entry.path.clone_from(&then.path);
        }
    }
    Ok(at_checkout.diff(&now))
}

/// Makes the item at `path` read-only: every file, and for a folder every folder inside it
/// too, so nothing can be added, changed or removed. Symbolic links are left as they are.
///
/// # Errors
///
/// An error reading the item or changing a permission.
pub fn lock(path: &Path) -> Result<(), CheckoutError> {
    // Files first, then folders from the deepest up, so each step can still reach the next.
    let (dirs, files) = walk(path)?;
    for file in &files {
        set_writable(file, false)?;
    }
    for dir in dirs.iter().rev() {
        set_writable(dir, false)?;
    }
    Ok(())
}

/// Undoes [`lock`]: the owner can write to the item again.
///
/// # Errors
///
/// An error reading the item or changing a permission.
pub fn unlock(path: &Path) -> Result<(), CheckoutError> {
    // Folders first, from the top down, so their contents can be listed and changed.
    if path.is_dir() {
        set_writable(path, true)?;
    }
    let (dirs, files) = walk(path)?;
    for dir in &dirs {
        set_writable(dir, true)?;
    }
    for file in &files {
        set_writable(file, true)?;
    }
    Ok(())
}

/// Whether the item at `path` is locked: its top file or folder isn't writable.
///
/// # Errors
///
/// An error reading its metadata.
pub fn is_locked(path: &Path) -> Result<bool, CheckoutError> {
    let meta = std::fs::metadata(path).map_err(io(path))?;
    Ok(meta.permissions().readonly())
}

/// Every folder (top first, parents before children) and file under `path`, or `path` alone
/// if it's a file. Symbolic links aren't followed or included.
fn walk(path: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), CheckoutError> {
    let meta = std::fs::symlink_metadata(path).map_err(io(path))?;
    if meta.file_type().is_symlink() {
        return Ok((Vec::new(), Vec::new()));
    }
    if !meta.is_dir() {
        return Ok((Vec::new(), vec![path.to_owned()]));
    }
    let mut dirs = vec![path.to_owned()];
    let mut files = Vec::new();
    let mut i = 0;
    while let Some(dir) = dirs.get(i).cloned() {
        i += 1;
        for entry in std::fs::read_dir(&dir).map_err(io(&dir))? {
            let entry = entry.map_err(io(&dir))?;
            let kind = entry.file_type().map_err(io(&dir))?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                dirs.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    Ok((dirs, files))
}

/// Gives or takes away write permission. On Unix only the write bits change, and giving it
/// back gives it to the owner alone; on Windows, the read-only attribute.
fn set_writable(path: &Path, writable: bool) -> Result<(), CheckoutError> {
    let mut permissions = std::fs::metadata(path).map_err(io(path))?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = permissions.mode();
        permissions.set_mode(if writable {
            mode | 0o200
        } else {
            mode & !0o222
        });
    }
    #[cfg(not(unix))]
    {
        // Folders on Windows ignore the attribute; their files carry the lock.
        #[allow(clippy::permissions_set_readonly_false)] // Windows has no world-writable
        permissions.set_readonly(!writable);
    }
    std::fs::set_permissions(path, permissions).map_err(io(path))
}

/// How the working copy at `working` compares with `item` as it went out: `unchanged`,
/// `changed`, `missing` or not compared, with what changed when it could tell.
#[must_use]
pub fn compared(
    archive: &Archive,
    rules: &Rules,
    item: Id,
    working: &Path,
) -> (&'static str, Option<Diff>) {
    let Some(then) = std::fs::read_to_string(manifest_path(archive, item))
        .ok()
        .and_then(|t| Manifest::parse(&t).ok())
    else {
        return ("not compared (no saved manifest)", None);
    };
    match compare(&then, working, rules) {
        Ok(diff) if diff.is_empty() => ("unchanged", Some(diff)),
        Ok(diff) => ("changed", Some(diff)),
        Err(CheckoutError::Missing(_)) => ("missing", None),
        Err(_) => ("not compared", None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules::default()
    }

    #[test]
    fn a_folder_is_locked_inside_and_unlocked_again() {
        let dir = tempfile::tempdir().unwrap();
        let item = dir.path().join("project");
        std::fs::create_dir_all(item.join("src")).unwrap();
        std::fs::write(item.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(item.join("README.md"), "hi").unwrap();

        lock(&item).unwrap();
        assert!(is_locked(&item).unwrap());
        assert!(std::fs::write(item.join("README.md"), "changed").is_err());
        #[cfg(unix)]
        assert!(
            std::fs::write(item.join("src/new.rs"), "").is_err(),
            "nothing can be added either"
        );

        unlock(&item).unwrap();
        assert!(!is_locked(&item).unwrap());
        std::fs::write(item.join("README.md"), "changed").unwrap();
        std::fs::write(item.join("src/new.rs"), "").unwrap();
    }

    #[test]
    fn a_single_file_locks_too() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "x").unwrap();
        lock(&file).unwrap();
        assert!(std::fs::write(&file, "y").is_err());
        unlock(&file).unwrap();
        std::fs::write(&file, "y").unwrap();
    }

    #[test]
    fn the_working_copy_is_compared_with_the_item_as_it_went_out() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("project");
        std::fs::create_dir_all(out.join("data")).unwrap();
        std::fs::write(out.join("a.txt"), "same").unwrap();
        std::fs::write(out.join("b.txt"), "before").unwrap();
        std::fs::write(out.join("c.txt"), "moving").unwrap();
        std::fs::write(out.join("gone.txt"), "bye").unwrap();
        let then = snapshot(&out, Id::new_v7(), &rules()).unwrap();

        std::fs::write(out.join("b.txt"), "after").unwrap();
        std::fs::rename(out.join("c.txt"), out.join("data/c.txt")).unwrap();
        std::fs::remove_file(out.join("gone.txt")).unwrap();
        std::fs::write(out.join("new.txt"), "hello").unwrap();
        let diff = compare(&then, &out, &rules()).unwrap();
        assert_eq!(diff.changed, ["b.txt"]);
        assert_eq!(
            diff.renamed,
            [("c.txt".to_owned(), "data/c.txt".to_owned())]
        );
        assert_eq!(diff.removed, ["gone.txt"]);
        assert_eq!(diff.added, ["new.txt"]);

        assert!(matches!(
            compare(&then, &dir.path().join("elsewhere"), &rules()),
            Err(CheckoutError::Missing(_))
        ));
    }

    #[test]
    fn a_renamed_single_file_is_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("draft.md");
        std::fs::write(&file, "one").unwrap();
        let then = snapshot(&file, Id::new_v7(), &rules()).unwrap();
        let moved = dir.path().join("draft (2).md");
        std::fs::rename(&file, &moved).unwrap();
        assert!(compare(&then, &moved, &rules()).unwrap().is_empty());
        std::fs::write(&moved, "two").unwrap();
        assert_eq!(
            compare(&then, &moved, &rules()).unwrap().changed,
            ["draft.md"]
        );
    }
}
