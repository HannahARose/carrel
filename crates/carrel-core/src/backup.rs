//! Backups of item contents (`docs/design/backups.md`). History records metadata; snapshots
//! record everything, so undo, manifest checks and recovery can reach contents.
//!
//! [`Backups`] is what the rest of Carrel uses. [`Kopia`] implements it by running the `kopia`
//! program, found beside `carrel` or on `PATH`, with a configuration that belongs to this
//! device and archive. The repository's password never enters the archive: Kopia keeps it in
//! the operating system's keychain, or where there is none, in a file beside its configuration
//! that only the owner can read.

use std::path::{Path, PathBuf};
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::archive::Archive;
use crate::card::Id;

/// The prefix of a `Carrel-Snapshot` trailer naming a Kopia snapshot (FORMAT.md §8.2).
pub const KOPIA_PREFIX: &str = "kopia:";

/// A snapshot of the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The snapshot's identifier in the backup system.
    pub id: String,
    /// When it was taken (RFC 3339).
    pub time: String,
    /// Its description.
    pub description: Option<String>,
    /// Whether it's kept regardless of retention, as snapshots named in history are.
    pub pinned: bool,
    /// Bytes and files it holds, when known.
    pub size: Option<u64>,
    /// Files it holds, when known.
    pub files: Option<u64>,
}

impl Snapshot {
    /// The value of a `Carrel-Snapshot` trailer for it.
    #[must_use]
    pub fn trailer(&self) -> String {
        format!("{KOPIA_PREFIX}{}", self.id)
    }
}

/// Why a backup operation failed.
#[derive(Debug)]
pub enum BackupError {
    /// The `kopia` program isn't installed.
    NotInstalled,
    /// Backups aren't set up for this archive on this device.
    NotSetUp,
    /// The repository isn't reachable, as when its drive isn't plugged in.
    Unreachable(PathBuf),
    /// The password was wrong.
    WrongPassword,
    /// The repository can't go there.
    Refused(String),
    /// There's no snapshot to restore from, as asked.
    NoSnapshot(String),
    /// The backup program failed.
    Failed(String),
    /// A file or process operation failed.
    Io {
        /// What was being done.
        what: String,
        /// The underlying error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => f.write_str(
                "Kopia isn't installed; install it (brew install kopia, winget install Kopia.KopiaCLI, \
                 or https://kopia.io/docs/installation/) and try again",
            ),
            Self::NotSetUp => f.write_str(
                "backups aren't set up for this archive on this device; run `carrel backup setup PATH`",
            ),
            Self::Unreachable(path) => write!(
                f,
                "the backup repository at {} isn't reachable; is its drive connected?",
                path.display()
            ),
            Self::WrongPassword => f.write_str("the backup password is wrong"),
            Self::Refused(why) | Self::Failed(why) | Self::NoSnapshot(why) => f.write_str(why),
            Self::Io { what, source } => write!(f, "{what}: {source}"),
        }
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A backup system for one archive.
pub trait Backups {
    /// Where the backups are, for people.
    fn describe(&self) -> String;
    /// Snapshots the archive at `root`.
    ///
    /// # Errors
    ///
    /// [`BackupError`].
    fn snapshot(&self, root: &Path, description: &str) -> Result<Snapshot, BackupError>;
    /// The archive's snapshots, oldest first.
    ///
    /// # Errors
    ///
    /// [`BackupError`].
    fn list(&self, root: &Path) -> Result<Vec<Snapshot>, BackupError>;
    /// Checks the repository, reading back `percent` of the files' contents. Returns what
    /// the check found.
    ///
    /// # Errors
    ///
    /// [`BackupError::Failed`] when something doesn't check out.
    fn verify(&self, percent: u8) -> Result<String, BackupError>;
    /// Restores `path` (relative to the archive root, `/` separators) from `snapshot` into
    /// `to`, which must not exist yet.
    ///
    /// # Errors
    ///
    /// [`BackupError`].
    fn restore(&self, snapshot: &str, path: &str, to: &Path) -> Result<(), BackupError>;
    /// Deletes the snapshot `snapshot`, pinned or not. Its space is freed by the backup
    /// system's own maintenance, later.
    ///
    /// # Errors
    ///
    /// [`BackupError`].
    fn forget(&self, snapshot: &str) -> Result<(), BackupError>;
}

/// What the archive's snapshots leave out, as Kopia ignore patterns: regenerable folders
/// (FORMAT.md §4.3), ignored files (§2.8), and the parts of `_System` that are rebuilt or
/// belong to the moment.
#[must_use]
pub fn ignore_patterns(archive: &Archive) -> Vec<String> {
    let rules = archive.rules().unwrap_or_default();
    let mut patterns: Vec<String> = rules
        .regenerable
        .iter()
        .map(|p| format!("{}/", p.0))
        .collect();
    patterns.extend(
        [
            ".DS_Store",
            "._*",
            "Icon\r",
            ".obsidian/",
            ".trash/",
            "/_System/cache.nosync/",
            "/_System/index/",
            "/_System/writer.lock*",
        ]
        .map(String::from),
    );
    patterns
}

/// Backups with Kopia. Not on iOS and the other platforms that can't start programs.
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
#[derive(Clone)]
pub struct Kopia {
    program: PathBuf,
    config: PathBuf,
    patterns: Vec<String>,
    /// The password, held only in memory, when Kopia isn't keeping it.
    password: Option<String>,
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
impl std::fmt::Debug for Kopia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kopia")
            .field("program", &self.program)
            .field("config", &self.config)
            .field("password", &self.password.as_ref().map(|_| "(hidden)"))
            .finish_non_exhaustive()
    }
}

/// What setting up backups did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    /// Whether a new repository was created, rather than an existing one connected.
    pub created: bool,
    /// The repository's identifier, recorded in `settings.json`.
    pub repository: String,
    /// Whether the repository is on the same disk as the archive.
    pub same_disk: bool,
}

#[derive(Deserialize)]
struct KopiaSnapshot {
    id: String,
    #[serde(rename = "startTime")]
    start_time: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    pins: Vec<String>,
    #[serde(rename = "rootEntry", default)]
    root_entry: Option<RootEntry>,
}

#[derive(Deserialize)]
struct RootEntry {
    #[serde(default)]
    summ: Option<Summary>,
}

#[derive(Deserialize)]
struct Summary {
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    files: Option<u64>,
}

impl From<KopiaSnapshot> for Snapshot {
    fn from(k: KopiaSnapshot) -> Self {
        let summary = k.root_entry.and_then(|r| r.summ);
        Self {
            id: k.id,
            time: k.start_time,
            description: k.description.filter(|d| !d.is_empty()),
            pinned: k.pins.iter().any(|p| p == PIN),
            size: summary.as_ref().and_then(|s| s.size),
            files: summary.and_then(|s| s.files),
        }
    }
}

/// The pin on snapshots Carrel takes, so retention never removes one history names.
const PIN: &str = "carrel";

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
impl Kopia {
    /// The `kopia` program, beside `carrel` or on `PATH`.
    #[must_use]
    pub fn program() -> Option<PathBuf> {
        crate::providers::process::find_program(
            "kopia",
            &crate::providers::process::default_folders(),
        )
    }

    /// Where this device keeps Kopia's configuration for `archive`.
    #[must_use]
    pub fn config_for(archive: &Archive) -> Option<PathBuf> {
        crate::local::config_dir().map(|dir| {
            dir.join("backups")
                .join(format!("{}.config", archive.info().id))
        })
    }

    /// Backups for `archive` on this device, once set up.
    ///
    /// # Errors
    ///
    /// [`BackupError::NotInstalled`] or [`BackupError::NotSetUp`].
    pub fn for_archive(archive: &Archive) -> Result<Self, BackupError> {
        let config = Self::config_for(archive).ok_or(BackupError::NotSetUp)?;
        Self::open(archive, config)
    }

    /// Backups for `archive` using Kopia's configuration at `config`.
    ///
    /// # Errors
    ///
    /// [`BackupError::NotInstalled`] or [`BackupError::NotSetUp`].
    pub fn open(archive: &Archive, config: PathBuf) -> Result<Self, BackupError> {
        let program = Self::program().ok_or(BackupError::NotInstalled)?;
        if !config.is_file() {
            return Err(BackupError::NotSetUp);
        }
        Ok(Self {
            program,
            config,
            patterns: ignore_patterns(archive),
            password: None,
        })
    }

    /// The same backups, passing `password` to Kopia on every call instead of relying on the
    /// one it keeps. The password stays in memory only.
    #[must_use]
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Creates a repository at `repository`, or connects to the one there, for `archive` on
    /// this device. The password is passed to Kopia only for this. With `keep`, Kopia keeps it
    /// in the keychain; without, the caller supplies it in `KOPIA_PASSWORD` each time, as
    /// scripts and CI do.
    ///
    /// # Errors
    ///
    /// [`BackupError::Refused`] for a synced folder or a folder inside the archive,
    /// [`BackupError::WrongPassword`], or another [`BackupError`].
    pub fn setup(
        archive: &Archive,
        config: PathBuf,
        repository: &Path,
        password: &str,
        keep: bool,
        device: Id,
    ) -> Result<(Self, Setup), BackupError> {
        let program = Self::program().ok_or(BackupError::NotInstalled)?;
        let repository = std::path::absolute(repository).map_err(|source| BackupError::Io {
            what: format!("finding {}", repository.display()),
            source,
        })?;
        if repository.starts_with(archive.root()) {
            return Err(BackupError::Refused(
                "the backup repository can't be inside the archive it backs up".into(),
            ));
        }
        if crate::coordinator::DiskLock::is_synced_folder(&repository) {
            return Err(BackupError::Refused(
                "the backup repository can't be in a synced folder, which can corrupt it; \
                 choose an external drive or a local folder"
                    .into(),
            ));
        }
        if let Some(parent) = config.parent() {
            std::fs::create_dir_all(parent).map_err(|source| BackupError::Io {
                what: format!("creating {}", parent.display()),
                source,
            })?;
        }
        let kopia = Self {
            program,
            config,
            patterns: ignore_patterns(archive),
            password: (!keep).then(|| password.to_owned()),
        };
        let created = !is_repository(&repository);
        std::fs::create_dir_all(&repository).map_err(|source| BackupError::Io {
            what: format!("creating {}", repository.display()),
            source,
        })?;
        let path = format!("--path={}", repository.display());
        let description = format!("--description=Carrel: {}", archive.doc().name);
        let hostname = format!("--override-hostname={device}");
        let mut args = vec![
            "repository",
            if created { "create" } else { "connect" },
            "filesystem",
            &path,
            &description,
            &hostname,
            "--override-username=carrel",
        ];
        if !keep {
            args.push("--no-persist-credentials");
        } else if cfg!(target_os = "macos") {
            args.extend(["--persist-credentials", "--use-keychain"]);
        } else {
            args.push("--persist-credentials");
        }
        kopia.run(&args, Some(password))?;
        let status = kopia.run(&["repository", "status", "--json"], None)?;
        let repository_id = serde_json::from_str::<serde_json::Value>(&status)
            .ok()
            .and_then(|v| v["uniqueIDHex"].as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok((
            kopia,
            Setup {
                created,
                repository: repository_id,
                same_disk: same_disk(archive.root(), &repository),
            },
        ))
    }

    /// The repository's folder, from Kopia's configuration.
    #[must_use]
    pub fn repository_path(&self) -> Option<PathBuf> {
        let text = std::fs::read_to_string(&self.config).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        value["storage"]["config"]["path"]
            .as_str()
            .map(PathBuf::from)
    }

    fn reachable(&self) -> Result<(), BackupError> {
        match self.repository_path() {
            Some(path) if !is_repository(&path) => Err(BackupError::Unreachable(path)),
            _ => Ok(()),
        }
    }

    /// Runs Kopia with this configuration and returns its standard output.
    fn run(&self, args: &[&str], password: Option<&str>) -> Result<String, BackupError> {
        let mut command = Command::new(&self.program);
        command
            .arg(format!("--config-file={}", self.config.display()))
            .args(args)
            .env("KOPIA_CHECK_FOR_UPDATES", "false")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(password) = password.or(self.password.as_deref()) {
            command.env("KOPIA_PASSWORD", password);
        }
        let output = command.output().map_err(|source| BackupError::Io {
            what: format!("running {}", self.program.display()),
            source,
        })?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            return Ok(stdout);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("invalid repository password") {
            return Err(BackupError::WrongPassword);
        }
        let message = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Kopia failed")
            .trim()
            .to_owned();
        Err(BackupError::Failed(format!("kopia: {message}")))
    }

    /// Sets the archive's ignore policy when it differs from the one wanted. Kopia applies
    /// `--clear-ignore` after any additions in the same call, so clearing is a call of its own.
    fn set_policy(&self, root: &Path) -> Result<(), BackupError> {
        let root = root.display().to_string();
        let current: Vec<String> = self
            .run(&["policy", "show", root.as_str(), "--json"], None)
            .ok()
            .and_then(|out| serde_json::from_str::<serde_json::Value>(&out).ok())
            .and_then(|v| {
                v["files"]["ignore"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_str().map(str::to_owned))
                        .collect()
                })
            })
            .unwrap_or_default();
        let mut wanted = self.patterns.clone();
        wanted.sort();
        let mut have = current.clone();
        have.sort();
        if have == wanted {
            return Ok(());
        }
        if !current.is_empty() {
            self.run(&["policy", "set", root.as_str(), "--clear-ignore"], None)?;
        }
        let adds: Vec<String> = wanted.iter().map(|p| format!("--add-ignore={p}")).collect();
        let mut args = vec!["policy", "set", root.as_str()];
        args.extend(adds.iter().map(String::as_str));
        self.run(&args, None).map(|_| ())
    }
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
impl Backups for Kopia {
    fn describe(&self) -> String {
        self.repository_path().map_or_else(
            || "a Kopia repository".into(),
            |p| format!("the Kopia repository at {}", p.display()),
        )
    }

    fn snapshot(&self, root: &Path, description: &str) -> Result<Snapshot, BackupError> {
        self.reachable()?;
        self.set_policy(root)?;
        let root = root.display().to_string();
        let description = format!("--description={description}");
        let pin = format!("--pin={PIN}");
        let out = self.run(
            &["snapshot", "create", &root, "--json", &description, &pin],
            None,
        )?;
        let snapshot: KopiaSnapshot = serde_json::from_str(&out)
            .map_err(|e| BackupError::Failed(format!("kopia's answer wasn't understood: {e}")))?;
        Ok(snapshot.into())
    }

    fn list(&self, root: &Path) -> Result<Vec<Snapshot>, BackupError> {
        self.reachable()?;
        let root = root.display().to_string();
        let out = self.run(&["snapshot", "list", &root, "--json"], None)?;
        let snapshots: Vec<KopiaSnapshot> = serde_json::from_str(&out)
            .map_err(|e| BackupError::Failed(format!("kopia's answer wasn't understood: {e}")))?;
        Ok(snapshots.into_iter().map(Snapshot::from).collect())
    }

    fn verify(&self, percent: u8) -> Result<String, BackupError> {
        self.reachable()?;
        let percent = format!("--verify-files-percent={}", percent.min(100));
        let mut command_output = self.run(&["snapshot", "verify", &percent], None)?;
        if command_output.trim().is_empty() {
            command_output = "every snapshot checks out".into();
        }
        Ok(command_output.trim().to_owned())
    }

    fn restore(&self, snapshot: &str, path: &str, to: &Path) -> Result<(), BackupError> {
        self.reachable()?;
        if to.exists() {
            return Err(BackupError::Refused(format!(
                "{} already exists; restore never overwrites",
                to.display()
            )));
        }
        let source = if path.is_empty() {
            snapshot.to_owned()
        } else {
            format!("{snapshot}/{}", path.trim_matches('/'))
        };
        let to = to.display().to_string();
        self.run(
            &[
                "restore",
                &source,
                &to,
                "--no-overwrite-files",
                "--no-overwrite-directories",
                "--no-overwrite-symlinks",
            ],
            None,
        )
        .map(|_| ())
    }

    fn forget(&self, snapshot: &str) -> Result<(), BackupError> {
        self.reachable()?;
        self.run(&["snapshot", "delete", snapshot, "--delete"], None)
            .map(|_| ())
    }
}

/// Whether two paths are on the same disk, as far as can be told.
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
#[cfg(unix)]
fn same_disk(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

/// Whether two paths are on the same disk, as far as can be told: on Windows, the same drive.
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
#[cfg(not(unix))]
fn same_disk(a: &Path, b: &Path) -> bool {
    a.components().next() == b.components().next()
}

/// Why [`snapshot_now`] failed.
#[derive(Debug)]
pub enum SnapshotNowError {
    /// The snapshot couldn't be taken.
    Backup(BackupError),
    /// The snapshot was taken, but history couldn't record it.
    History(crate::history::HistoryError),
}

impl std::fmt::Display for SnapshotNowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backup(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SnapshotNowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backup(e) => Some(e),
            Self::History(e) => Some(e),
        }
    }
}

/// Snapshots the whole archive with `backups`, described as `description`, and records it in
/// history with a change of its own, holding `lease`: `carrel backup now` and the Mac app's
/// scheduled backups. History records a snapshot only with a change; with nothing changed,
/// the snapshot still exists in the repository. Returns the snapshot and the history commit.
///
/// # Errors
///
/// [`SnapshotNowError`] if the snapshot can't be taken or history can't record it.
pub fn snapshot_now(
    archive: &Archive,
    backups: &dyn Backups,
    lease: &crate::coordinator::WriterLease,
    description: &str,
) -> Result<(Snapshot, Option<String>), SnapshotNowError> {
    use crate::history::{Change, History, Operation};
    let snapshot = backups
        .snapshot(archive.root(), description)
        .map_err(SnapshotNowError::Backup)?;
    let mut change = Change::new(
        Operation::Edit,
        description,
        lease.info().person,
        lease.info().device,
    );
    change.snapshot = Some(snapshot.trailer());
    let commit = History::open(archive)
        .and_then(|history| history.commit(archive, &change, lease))
        .map_err(SnapshotNowError::History)?;
    Ok((snapshot, commit))
}

/// Records the repository in `settings.json`'s `backups` (FORMAT.md §7.2): its type, its
/// identifier and a description. Never the path, which is per device, or the password.
///
/// # Errors
///
/// A file error, or a `settings.json` that isn't JSON.
///
/// # Panics
///
/// Never: JSON values always serialize.
pub fn record(archive: &Archive, repository: &str, description: &str) -> std::io::Result<()> {
    let path = archive.root().join("_System/settings.json");
    let mut value: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e),
    };
    value["backups"] = serde_json::json!({
        "type": "kopia",
        "repository": repository,
        "description": description,
    });
    let text = serde_json::to_string_pretty(&value).expect("JSON always serializes") + "\n";
    let temp = path.with_extension(format!("tmp-{}", Id::new_v7()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)
}

/// Whether `folder` holds a Kopia repository already, so setting up connects to it rather than
/// creating one, and asks for its password once rather than twice.
#[must_use]
pub fn is_repository(folder: &Path) -> bool {
    folder.join("kopia.repository.f").exists()
}

/// What a repository at `repository` is called when the owner gives no description: its
/// folder's name, such as "Backup drive".
#[must_use]
pub fn default_description(repository: &Path) -> String {
    repository
        .file_name()
        .map_or_else(|| "Backups".into(), |n| n.to_string_lossy().into_owned())
}

/// Records a repository just set up in `settings.json` ([`record`]) and in history, with a
/// change of its own, holding `lease`. Returns the history commit.
///
/// # Errors
///
/// [`crate::history::HistoryError`] if the settings or history can't be written.
pub fn record_setup(
    archive: &Archive,
    setup: &Setup,
    description: &str,
    lease: &crate::coordinator::WriterLease,
) -> Result<Option<String>, crate::history::HistoryError> {
    use crate::history::{Change, History, HistoryError, Operation};
    record(archive, &setup.repository, description).map_err(|source| HistoryError::Io {
        path: archive.root().join("_System/settings.json"),
        source,
    })?;
    let change = Change::new(
        Operation::Edit,
        format!("Back up to “{description}”"),
        lease.info().person,
        lease.info().device,
    );
    History::open(archive)?.commit(archive, &change, lease)
}

/// The history entries in `log` that took a Kopia snapshot, in `log`'s order (newest first
/// for [`crate::history::History::log`]). History records each snapshot with its change, so
/// this needs no backup drive.
pub fn snapshot_entries(
    log: &[crate::history::Entry],
) -> impl Iterator<Item = &crate::history::Entry> {
    log.iter().filter(|e| {
        e.snapshot
            .as_deref()
            .is_some_and(|s| s.starts_with(KOPIA_PREFIX))
    })
}

/// The snapshot to restore from, with the change it was taken with: the one taken with the
/// change `at` names (an identifier or its start), or the latest before it; without `at`, the
/// latest. `log` is newest first.
///
/// # Errors
///
/// [`BackupError::NoSnapshot`] when there's no such change, or no snapshot by then.
pub fn restore_point<'a>(
    log: &'a [crate::history::Entry],
    at: Option<&str>,
) -> Result<(&'a crate::history::Entry, &'a str), BackupError> {
    let entry = match at {
        Some(at) => {
            let position = log
                .iter()
                .position(|e| e.id.starts_with(at))
                .ok_or_else(|| BackupError::NoSnapshot(format!("no change {at} in the history")))?;
            snapshot_entries(&log[position..]).next().ok_or_else(|| {
                BackupError::NoSnapshot(format!("no snapshot was taken by change {at}"))
            })?
        }
        None => snapshot_entries(log)
            .next()
            .ok_or_else(|| BackupError::NoSnapshot("no snapshot has been taken yet".into()))?,
    };
    let snapshot = entry
        .snapshot
        .as_deref()
        .and_then(|s| s.strip_prefix(KOPIA_PREFIX))
        .unwrap_or_default();
    Ok((entry, snapshot))
}

/// A free folder beside the archive for an item restored as of `entry`, from the item's path
/// `path`: `NAME (restored 2026-10-29)`, then `NAME (restored 2026-10-29, 2)` and so on.
#[must_use]
pub fn restore_beside(archive: &Archive, path: &str, entry: &crate::history::Entry) -> PathBuf {
    let parent = archive
        .root()
        .parent()
        .map_or_else(|| archive.root().to_owned(), Path::to_owned);
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path);
    let date = jiff::Timestamp::from_second(entry.time)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default();
    let mut target = parent.join(format!("{name} (restored {date})"));
    let mut n = 2;
    while target.exists() {
        target = parent.join(format!("{name} (restored {date}, {n})"));
        n += 1;
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backups that take every snapshot they're asked for.
    struct Taken;

    impl Backups for Taken {
        fn describe(&self) -> String {
            "taken".into()
        }
        fn snapshot(&self, _: &Path, description: &str) -> Result<Snapshot, BackupError> {
            Ok(Snapshot {
                id: "k1".into(),
                time: "2026-10-31T09:00:00Z".into(),
                description: Some(description.to_owned()),
                pinned: false,
                size: None,
                files: None,
            })
        }
        fn list(&self, _: &Path) -> Result<Vec<Snapshot>, BackupError> {
            Ok(Vec::new())
        }
        fn verify(&self, _: u8) -> Result<String, BackupError> {
            Ok(String::new())
        }
        fn restore(&self, _: &str, _: &str, _: &Path) -> Result<(), BackupError> {
            Ok(())
        }
        fn forget(&self, _: &str) -> Result<(), BackupError> {
            Ok(())
        }
    }

    #[test]
    fn a_snapshot_now_is_taken_even_with_nothing_for_history_to_record() {
        use crate::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
        use crate::history::History;
        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::init(
            &dir.path().join("Archive"),
            &crate::archive::InitOptions::new("Test"),
        )
        .unwrap();
        let holder = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        let lease = DiskLock::for_archive(&archive)
            .acquire(holder, "backup", DEFAULT_LEASE)
            .unwrap();
        History::start(&archive, &lease).unwrap();
        let (snapshot, commit) =
            snapshot_now(&archive, &Taken, &lease, "Scheduled snapshot").unwrap();
        lease.release().unwrap();
        assert_eq!(snapshot.id, "k1");
        assert_eq!(
            commit, None,
            "nothing changed, so history has nothing to record"
        );
        assert_eq!(History::open(&archive).unwrap().log().unwrap().len(), 1);
    }

    fn entry(id: &str, snapshot: Option<&str>) -> crate::history::Entry {
        crate::history::Entry {
            id: id.into(),
            summary: format!("Change {id}"),
            operation: None,
            person: None,
            device: None,
            snapshot: snapshot.map(str::to_owned),
            items: Vec::new(),
            moves: Vec::new(),
            reverts: None,
            time: 0,
        }
    }

    #[test]
    fn restoring_finds_the_snapshot_taken_by_then() {
        let log = [
            entry("c3", None),
            entry("c2", Some("kopia:k2")),
            entry("c1", None),
            entry("c0", Some("kopia:k0")),
        ];
        let at = |at| restore_point(&log, at).map(|(e, s)| (e.id.clone(), s.to_owned()));
        assert_eq!(at(None).unwrap(), ("c2".into(), "k2".into()));
        assert_eq!(at(Some("c1")).unwrap(), ("c0".into(), "k0".into()));
        assert_eq!(at(Some("c2")).unwrap(), ("c2".into(), "k2".into()));
        assert!(matches!(at(Some("c9")), Err(BackupError::NoSnapshot(_))));
        assert!(matches!(
            restore_point(&log[2..3], None),
            Err(BackupError::NoSnapshot(_))
        ));
        assert_eq!(snapshot_entries(&log).count(), 2);
    }

    #[test]
    fn kopia_snapshots_are_read_from_its_json() {
        let json = r#"{"id":"27afc1af","source":{"host":"h","userName":"carrel","path":"/a"},
            "description":"File “lamp”","startTime":"2026-10-30T01:07:19Z","pins":["carrel"],
            "rootEntry":{"obj":"k6c9","summ":{"size":5,"files":2,"dirs":2}}}"#;
        let snapshot: Snapshot = serde_json::from_str::<KopiaSnapshot>(json).unwrap().into();
        assert_eq!(snapshot.id, "27afc1af");
        assert_eq!(snapshot.trailer(), "kopia:27afc1af");
        assert!(snapshot.pinned);
        assert_eq!((snapshot.size, snapshot.files), (Some(5), Some(2)));
        let bare: Snapshot =
            serde_json::from_str::<KopiaSnapshot>(r#"{"id":"x","startTime":"t","description":""}"#)
                .unwrap()
                .into();
        assert_eq!(
            (bare.description, bare.pinned, bare.size),
            (None, false, None)
        );
    }
}
