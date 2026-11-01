//! Swift bindings for `carrel-core`, generated with `UniFFI`.
//!
//! This is the boundary the apps call through, so it speaks in plain records and strings
//! rather than the core's richer types. `scripts/build-xcframework.sh` builds it as
//! `CarrelFFI.xcframework` and generates the Swift side into the `CarrelKit` package under
//! `apple/`.
//!
//! The core owns the logic; each call here only translates: it finds what's named, takes the
//! writer lease and this device's identity where the CLI does, calls the core, and turns the
//! result into records (`docs/design/mac-app.md`). The operations are grouped as the app's
//! screens are:
//!
//! | Module | What |
//! | --- | --- |
//! | [`review`] | Review and Questions: proposals, batches, approving, holding, moving, rejecting; answering and dismissing questions |
//! | [`correct`] | Corrections worked out by a model, shown, then saved |
//! | [`edit`] | Editing a filed card's or a proposal's fields, with `carrel edit`'s checks |
//! | [`active`] | Checkout and return, the desk, and work sessions' pending entries |
//! | [`search`] | Search with filters and reasons, an item's card, history and related items |
//! | [`history`] | The history and undo, Status's summary and Recent |
//! | [`settings`] | The archive's settings and this device's model choices |
//! | [`models`] | The model catalog, providers, and downloads |
//! | [`intake`] | The intake queue |
//! | [`backups`](mod@backups) | Backups: setting up a repository, the snapshots, taking one, verifying, restoring |
//! | [`agent`] | The Mac app's agent: its claim on an archive, work sessions and scheduled snapshots |
//! | [`librarian`] | The librarian: a conversation, saving it to the Journal, and its own changes |
//! | [`discover`] | Discovery: on this day, wander, the timeline, the archive map, activity, resurfacing |
//! | [`quick_look`] | Quick Look: cards laid out for the preview extension, and the drawings the agent keeps for it |
//! | [`creating`] | Creating an archive with a chosen embedding model |
//!
//! [`TextModel`] and [`ModelProvider`](correct::ModelProvider) are implemented in Swift and called from Rust, as is
//! [`ProgressListener`], which long operations report to.

// UniFFI passes arguments from Swift as owned values.
#![allow(clippy::needless_pass_by_value)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use carrel_core::Access;
use carrel_core::archive::{Archive, InitOptions, ItemRef};
use carrel_core::card::Kind;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{
    Coordinator as _, CoordinatorError, DEFAULT_LEASE, DiskLock, Holder,
};
use carrel_core::history::History;
use carrel_core::local::{Identity, Prefs};
use carrel_core::providers::Registry;

pub mod active;
pub mod agent;
pub mod backups;
pub mod browse;
pub mod correct;
pub mod crawl;
pub mod creating;
pub mod discover;
pub mod edit;
pub mod graph;
pub mod history;
pub mod index;
pub mod intake;
pub mod librarian;
pub mod model_formats;
pub mod models;
pub mod previews;
pub mod questions;
pub mod quick_look;
pub mod records;
pub mod restructure;
pub mod review;
pub mod search;
pub mod settings;
pub mod usage_report;
pub mod working_copy;

uniffi::setup_scaffolding!();

/// The Carrel version and the archive format version, as shown in the apps' About window.
#[must_use]
#[uniffi::export]
pub fn version_summary() -> String {
    format!(
        "Carrel {} · archive format {}",
        carrel_core::CARREL_VERSION,
        carrel_core::FORMAT_VERSION
    )
}

/// Why a call failed. Swift sees this as a thrown error.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CarrelError {
    /// The folder isn't inside a Carrel archive.
    #[error("{message}")]
    NotAnArchive {
        /// What was looked for, in plain words.
        message: String,
    },
    /// Anything else, in plain words.
    #[error("{message}")]
    Failed {
        /// What went wrong.
        message: String,
    },
    /// A Swift-implemented callback failed.
    #[error("{message}")]
    Callback {
        /// The callback's own description of what went wrong.
        message: String,
    },
    /// Another writer holds the archive: the message says who, and what it's doing.
    #[error("{message}")]
    Busy {
        /// Who holds the writer lease, in words.
        message: String,
    },
    /// What was asked for doesn't make sense, such as an identifier that matches nothing.
    #[error("{message}")]
    Invalid {
        /// Why, in words.
        message: String,
    },
}

impl CarrelError {
    fn failed(error: impl std::fmt::Display) -> Self {
        Self::Failed {
            message: error.to_string(),
        }
    }

    fn invalid(why: impl std::fmt::Display) -> Self {
        Self::Invalid {
            message: why.to_string(),
        }
    }
}

impl From<CoordinatorError> for CarrelError {
    fn from(error: CoordinatorError) -> Self {
        match error {
            CoordinatorError::Held(_) => Self::Busy {
                message: error.to_string(),
            },
            error => Self::failed(error),
        }
    }
}

impl From<carrel_core::actions::ActionError> for CarrelError {
    fn from(error: carrel_core::actions::ActionError) -> Self {
        use carrel_core::actions::ActionError;
        match error {
            ActionError::Lease(e) => e.into(),
            ActionError::Invalid(why) => Self::Invalid { message: why },
            error => Self::failed(error),
        }
    }
}

/// Core errors with nothing to tell apart for the apps: their messages are what's shown.
macro_rules! failed_from {
    ($($error:ty),* $(,)?) => {
        $(impl From<$error> for CarrelError {
            fn from(error: $error) -> Self {
                Self::failed(error)
            }
        })*
    };
}

failed_from!(
    carrel_core::archive::ArchiveError,
    carrel_core::review::ReviewError,
    carrel_core::history::HistoryError,
    carrel_core::local::LocalError,
    carrel_core::status::StatusError,
    carrel_core::checkout::CheckoutError,
    carrel_core::desk::DeskError,
    carrel_core::search::run::SearchError,
    carrel_core::models::ModelError,
    carrel_core::card_file::CardFileError,
    std::io::Error,
);

impl From<carrel_core::correct::prepare::PrepareError> for CarrelError {
    fn from(error: carrel_core::correct::prepare::PrepareError) -> Self {
        use carrel_core::correct::prepare::PrepareError;
        match error {
            PrepareError::Lease(e) => e.into(),
            PrepareError::Action(e) => e.into(),
            error => Self::failed(error),
        }
    }
}

impl From<carrel_core::sessions::SessionError> for CarrelError {
    fn from(error: carrel_core::sessions::SessionError) -> Self {
        match error {
            carrel_core::sessions::SessionError::Invalid(why) => Self::Invalid { message: why },
            error => Self::failed(error),
        }
    }
}

impl From<uniffi::UnexpectedUniFFICallbackError> for CarrelError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Callback {
            message: error.reason,
        }
    }
}

/// An item's kind (FORMAT.md §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ItemKind {
    /// A single file.
    File,
    /// An open folder.
    Folder,
    /// A sealed bundle, such as a code repository.
    Bundle,
    /// A physical object.
    Physical,
    /// A web page.
    Url,
    /// Something stored elsewhere.
    External,
    /// A removed item's record.
    Tombstone,
}

impl From<Kind> for ItemKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::File => Self::File,
            Kind::Folder => Self::Folder,
            Kind::Bundle => Self::Bundle,
            Kind::Physical => Self::Physical,
            Kind::Url => Self::Url,
            Kind::External => Self::External,
            Kind::Tombstone => Self::Tombstone,
        }
    }
}

/// An item, as listed in the apps.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ItemSummary {
    /// The item's identifier.
    pub id: String,
    /// Its card's title.
    pub title: String,
    /// Its kind.
    pub kind: ItemKind,
    /// The card's path relative to the archive root.
    pub card: String,
    /// The item's path relative to the archive root, if it has one.
    pub item: Option<String>,
}

/// A collection, as listed in the apps.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CollectionSummary {
    /// The collection's folder relative to the archive root.
    pub path: String,
    /// Its title.
    pub title: String,
    /// Its one-line description, if it has one.
    pub description: Option<String>,
}

/// A language model the platform provides, implemented in Swift.
#[uniffi::export(with_foreign)]
pub trait TextModel: Send + Sync {
    /// Answers `prompt` with plain text.
    ///
    /// # Errors
    ///
    /// Whatever the model reports, as [`CarrelError::Callback`].
    fn complete(&self, prompt: String) -> Result<String, CarrelError>;
}

/// Told how far a long operation has got, implemented in Swift. Called from the thread doing
/// the work, so the app hops to the main actor itself.
#[uniffi::export(with_foreign)]
pub trait ProgressListener: Send + Sync {
    /// `done` of `total` are done; `item` names the one being worked on, when there is one.
    fn update(&self, done: u64, total: u64, item: Option<String>);
}

/// How an archive is opened: where this device's identity and preferences are, and which
/// providers to start.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct OpenOptions {
    /// The configuration folder holding this device's identity and preferences; `None` for
    /// the device's own ([`carrel_core::local::config_dir`]). Tests give a temporary folder,
    /// so they never write the real configuration.
    pub config_dir: Option<String>,
    /// Folders to look in for provider programs before the usual ones, such as an app
    /// bundle's `Contents/Helpers`.
    pub provider_folders: Vec<String>,
    /// Whether to start provider programs; without them only the built-in providers read
    /// text, and corrections need a [`ModelProvider`](correct::ModelProvider).
    pub start_providers: bool,
    /// The folder standing for the person's home: where Status looks for active work (its
    /// Desktop and Code folders), where a checkout goes by default (its Desktop), and what
    /// `~` means in a checked-out path. `None` for the person's own
    /// ([`carrel_core::local::home_dir`]). Tests give a temporary folder, so they never look
    /// in the owner's.
    #[uniffi(default = None)]
    pub home: Option<String>,
}

/// An open archive.
#[derive(Debug, uniffi::Object)]
pub struct ArchiveHandle {
    archive: Archive,
    options: OpenOptions,
}

impl ArchiveHandle {
    /// The archive as it is now on disk, read again, since the CLI, the agent and other
    /// windows change it too.
    fn fresh(&self) -> Result<Archive, CarrelError> {
        Ok(Archive::open(self.archive.root())?)
    }

    /// The configuration folder this handle uses.
    fn config_dir(&self) -> Result<PathBuf, CarrelError> {
        match &self.options.config_dir {
            Some(dir) => Ok(PathBuf::from(dir)),
            None => Ok(carrel_core::local::config_folder()?),
        }
    }

    /// The person's home folder, or the one standing for it.
    fn home(&self) -> Option<PathBuf> {
        self.options
            .home
            .as_ref()
            .map(PathBuf::from)
            .or_else(carrel_core::local::home_dir)
    }

    /// This device's identity, created on first use.
    fn identity(&self) -> Result<Identity, CarrelError> {
        Ok(Identity::load_or_create_in(&self.config_dir()?)?)
    }

    /// Who takes the writer lease from here.
    fn holder(&self) -> Result<Holder, CarrelError> {
        Ok(self.identity()?.holder())
    }

    /// This device's preferences.
    fn prefs(&self) -> Result<Prefs, CarrelError> {
        Ok(Prefs::load_from(&self.config_dir()?)?)
    }

    /// The providers to use, as [`OpenOptions`] says.
    fn registry(&self) -> Registry {
        if !self.options.start_providers {
            return Registry::with_built_ins();
        }
        let folders: Vec<PathBuf> = self
            .options
            .provider_folders
            .iter()
            .map(PathBuf::from)
            .collect();
        let prefs = self.prefs().unwrap_or_default();
        carrel_core::providers::device::registry_with(&folders, &prefs).0
    }

    /// The item with the identifier `id`.
    fn item(archive: &Archive, id: &str) -> Result<ItemRef, CarrelError> {
        let wanted = id.trim().to_lowercase();
        archive
            .items()?
            .into_iter()
            .find(|i| i.id.to_string() == wanted)
            .ok_or_else(|| CarrelError::invalid(format!("no item has the identifier {id}")))
    }
}

/// The archive's backups, to snapshot before a change, when they're set up. Backups use the
/// installed Kopia, so they're only on a Mac.
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
fn backups(archive: &Archive) -> Option<Box<dyn carrel_core::backup::Backups>> {
    carrel_core::backup::Kopia::for_archive(archive)
        .ok()
        .map(|kopia| Box::new(kopia) as Box<dyn carrel_core::backup::Backups>)
}

/// The archive's backups: none away from a Mac.
#[cfg(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
fn backups(_archive: &Archive) -> Option<Box<dyn carrel_core::backup::Backups>> {
    None
}

/// A path from Swift.
fn path(text: &str) -> &Path {
    Path::new(text)
}

/// A path for Swift.
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[uniffi::export]
impl ArchiveHandle {
    /// Opens the archive containing `path`.
    ///
    /// # Errors
    ///
    /// [`CarrelError::NotAnArchive`] if no folder from `path` up is an archive, or
    /// [`CarrelError::Failed`] if it can't be read.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, CarrelError> {
        Self::open_with(
            path,
            OpenOptions {
                start_providers: true,
                ..OpenOptions::default()
            },
        )
    }

    /// Opens the archive containing `path` as `options` say.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::open`].
    #[uniffi::constructor]
    pub fn open_with(path: String, options: OpenOptions) -> Result<Arc<Self>, CarrelError> {
        match Archive::find(Path::new(&path)) {
            Ok(archive) => Ok(Arc::new(Self { archive, options })),
            Err(e @ carrel_core::archive::ArchiveError::NotAnArchive(_)) => {
                Err(CarrelError::NotAnArchive {
                    message: e.to_string(),
                })
            }
            Err(e) => Err(CarrelError::failed(e)),
        }
    }

    /// Creates an archive named `name` in the empty folder `path` (creating it if needed),
    /// with its history started, as `carrel init` does, and opens it.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if `path` is already an archive or has files in it, or if
    /// anything can't be written.
    #[uniffi::constructor]
    pub fn create(path: String, name: String) -> Result<Arc<Self>, CarrelError> {
        create_archive(
            Path::new(&path),
            &name,
            None,
            &carrel_core::local::config_folder()?,
        )
    }

    /// The archive's name.
    #[must_use]
    pub fn name(&self) -> String {
        self.archive.doc().name.clone()
    }

    /// The archive's identifier.
    #[must_use]
    pub fn id(&self) -> String {
        self.archive.info().id.to_string()
    }

    /// The archive's root folder.
    #[must_use]
    pub fn root(&self) -> String {
        self.archive.root().to_string_lossy().into_owned()
    }

    /// Whether this version can only read the archive (it uses a newer format).
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.archive.access() == Access::ReadOnly
    }

    /// The archive's collections, nested ones included, by path.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn collections(&self) -> Result<Vec<CollectionSummary>, CarrelError> {
        Ok(self
            .archive
            .collections()
            .map_err(CarrelError::failed)?
            .into_iter()
            .map(|c| CollectionSummary {
                path: c.path,
                title: c.doc.title,
                description: c.doc.description,
            })
            .collect())
    }

    /// Every cataloged item, by card path.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive or a card can't be read.
    pub fn items(&self) -> Result<Vec<ItemSummary>, CarrelError> {
        let root = self.archive.root();
        self.archive
            .items()
            .map_err(CarrelError::failed)?
            .into_iter()
            .map(|item| {
                let path = root.join(&item.card);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| CarrelError::failed(format!("{}: {e}", path.display())))?;
                let file = CardFile::parse(&text)
                    .map_err(|e| CarrelError::failed(format!("{}: {e}", item.card)))?;
                Ok(ItemSummary {
                    id: item.id.to_string(),
                    title: file.card().title.clone(),
                    kind: item.kind.into(),
                    card: item.card,
                    item: item.item,
                })
            })
            .collect()
    }

    /// Asks `model` for a one-paragraph description of the archive from its collections and
    /// item titles. Nothing but titles and descriptions is sent to the model.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read, or the model's error.
    pub fn describe(&self, model: Arc<dyn TextModel>) -> Result<String, CarrelError> {
        let mut prompt = format!(
            "Describe this personal archive in one short paragraph.\n\nArchive: {}\n\nCollections:\n",
            self.name()
        );
        for collection in self.collections()? {
            let description = collection
                .description
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            writeln!(prompt, "- {}{description}", collection.title).unwrap();
        }
        prompt.push_str("\nItems:\n");
        for item in self.items()? {
            writeln!(prompt, "- {}", item.title).unwrap();
        }
        Ok(model.complete(prompt)?.trim().to_owned())
    }
}

pub(crate) fn create_archive(
    root: &Path,
    name: &str,
    embedding_model: Option<&str>,
    config: &Path,
) -> Result<Arc<ArchiveHandle>, CarrelError> {
    let holder = Identity::load_or_create_in(config)?.holder();
    let mut init = InitOptions::new(name);
    if let Some(model) = embedding_model {
        init.embedding_model = Some(model.to_owned());
    }
    let archive = Archive::init(root, &init)?;
    let lease = DiskLock::for_archive(&archive).acquire(holder, "create", DEFAULT_LEASE)?;
    let started = History::init(&archive, &lease);
    lease.release()?;
    started?;
    Ok(Arc::new(ArchiveHandle {
        archive,
        options: OpenOptions {
            config_dir: Some(text(config)),
            provider_folders: Vec::new(),
            start_providers: true,
            home: None,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::EditOutcome;

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

    /// A copy of the sample archive with its history started, opened with a configuration
    /// folder of its own and only the built-in providers, so tests write nothing outside it.
    pub(crate) struct Sample {
        pub dir: tempfile::TempDir,
        pub handle: Arc<ArchiveHandle>,
    }

    impl Sample {
        pub fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("archive");
            copy_dir(Path::new(&sample_archive()), &root);
            let handle = ArchiveHandle::open_with(
                text(&root),
                OpenOptions {
                    config_dir: Some(text(&dir.path().join("config"))),
                    provider_folders: Vec::new(),
                    start_providers: false,
                    home: Some(text(&dir.path().join("home"))),
                },
            )
            .unwrap();
            handle.start_history().unwrap();
            Self { dir, handle }
        }

        pub fn root(&self) -> PathBuf {
            self.dir.path().join("archive")
        }

        pub fn first_proposal(&self) -> String {
            self.handle.review().unwrap().batches[0].proposals[0]
                .id
                .clone()
        }

        pub fn item_titled(&self, title: &str) -> String {
            self.handle
                .items()
                .unwrap()
                .into_iter()
                .find(|i| i.title == title)
                .unwrap()
                .id
        }
    }

    fn sample_archive() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/sample-archive")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn summary_names_both_versions() {
        let s = version_summary();
        assert!(s.contains(carrel_core::CARREL_VERSION));
        assert!(s.contains("archive format 1.0"));
    }

    #[test]
    fn lists_the_sample_archive() {
        let archive = ArchiveHandle::open(sample_archive()).unwrap();
        assert_eq!(archive.name(), "Sample archive");
        assert!(!archive.is_read_only());
        let collections = archive.collections().unwrap();
        assert!(collections.iter().any(|c| c.path == "Projects/Hardware"));
        let items = archive.items().unwrap();
        assert_eq!(items.len(), 12);
        let book = items
            .iter()
            .find(|i| i.card == "Reading/On the Origin of Species.md")
            .unwrap();
        assert_eq!(book.kind, ItemKind::Physical);
        assert_eq!(book.item, None);
    }

    #[test]
    fn folders_outside_archives_are_reported_as_such() {
        let dir = std::env::temp_dir();
        assert!(matches!(
            ArchiveHandle::open(dir.to_string_lossy().into_owned()),
            Err(CarrelError::NotAnArchive { .. })
        ));
    }

    #[test]
    fn creates_an_archive_with_its_history() {
        let dir =
            std::env::temp_dir().join(format!("carrel-ffi-{}", carrel_core::card::Id::new_v7()));
        let config = dir.join("config");
        let root = dir.join("New archive");
        let archive = create_archive(&root, "Letters", None, &config).unwrap();
        assert_eq!(archive.name(), "Letters");
        assert!(
            archive
                .collections()
                .unwrap()
                .iter()
                .any(|c| c.path == "Holding")
        );
        assert!(History::open(&archive.archive).is_ok());
        let first = std::fs::read_to_string(config.join("identity.json")).unwrap();
        assert!(create_archive(&root, "Again", None, &config).is_err());
        let other = dir.join("Second");
        create_archive(&other, "Second", None, &config).unwrap();
        let second = std::fs::read_to_string(config.join("identity.json")).unwrap();
        assert_eq!(first, second, "the identity is kept, not replaced");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    struct Echo;

    impl TextModel for Echo {
        fn complete(&self, prompt: String) -> Result<String, CarrelError> {
            Ok(format!("  {} lines\n", prompt.lines().count()))
        }
    }

    struct Unavailable;

    impl TextModel for Unavailable {
        fn complete(&self, _: String) -> Result<String, CarrelError> {
            Err(CarrelError::Callback {
                message: "model unavailable".into(),
            })
        }
    }

    #[test]
    fn describe_asks_the_model_and_passes_its_errors_on() {
        let archive = ArchiveHandle::open(sample_archive()).unwrap();
        let answer = archive.describe(Arc::new(Echo)).unwrap();
        assert!(answer.ends_with(" lines"), "{answer}");
        assert!(matches!(
            archive.describe(Arc::new(Unavailable)),
            Err(CarrelError::Callback { message }) if message == "model unavailable"
        ));
    }

    // Corrections

    /// A model that answers every correction with a new title, as a Swift model would.
    struct Retitles;

    impl correct::ModelProvider for Retitles {
        fn complete(
            &self,
            request: correct::ModelRequest,
        ) -> Result<correct::ModelAnswer, CarrelError> {
            assert!(request.schema.is_some(), "corrections ask for JSON");
            Ok(correct::ModelAnswer {
                text: r#"{"title": "Lamp diffuser, frosted"}"#.into(),
                model: "test/retitles".into(),
            })
        }
    }

    struct Offline;

    impl correct::ModelProvider for Offline {
        fn complete(&self, _: correct::ModelRequest) -> Result<correct::ModelAnswer, CarrelError> {
            Err(CarrelError::Callback {
                message: "model offline".into(),
            })
        }
    }

    #[test]
    fn a_swift_model_corrects_a_proposal_saved_once() {
        let sample = Sample::new();
        let id = sample.first_proposal();
        let correction = sample
            .handle
            .prepare_proposal_correction(
                id.clone(),
                "call it a frosted lamp diffuser".into(),
                Some(Arc::new(Retitles)),
            )
            .unwrap();
        let changes = correction.changes();
        assert_eq!(changes.after.title, "Lamp diffuser, frosted");
        assert_eq!(changes.model.as_deref(), Some("app/model"));
        assert!(!changes.is_empty);
        assert_eq!(
            sample.handle.proposal(id.clone()).unwrap().fields.title,
            changes.before.title
        );
        let saved = sample.handle.save_correction(correction.clone()).unwrap();
        assert!(!saved.filed);
        assert_eq!(
            sample.handle.proposal(id).unwrap().fields.title,
            "Lamp diffuser, frosted"
        );
        assert!(matches!(
            sample.handle.save_correction(correction),
            Err(CarrelError::Invalid { .. })
        ));
    }

    #[test]
    fn a_models_error_is_the_corrections() {
        let sample = Sample::new();
        let error = sample
            .handle
            .prepare_proposal_correction(
                sample.first_proposal(),
                "it's a lamp part".into(),
                Some(Arc::new(Offline)),
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("model offline") || error.to_string().contains("model"),
            "{error}"
        );
    }

    #[test]
    fn a_filed_items_correction_is_one_change_in_the_history() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let correction = sample
            .handle
            .prepare_item_correction(item.clone(), "title: Lamp parts list".into(), None)
            .unwrap();
        assert_eq!(correction.changes().changed, vec!["title"]);
        let saved = sample.handle.save_correction(correction).unwrap();
        assert!(saved.filed && saved.commit.is_some());
        assert_eq!(
            sample
                .handle
                .item_detail(item.clone())
                .unwrap()
                .fields
                .title,
            "Lamp parts list"
        );
        assert_eq!(sample.handle.item_history(item).unwrap().len(), 1);
    }

    // Editing

    #[test]
    fn editing_a_card_is_checked_saved_and_undone() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let editable = sample.handle.editable_card(item.clone()).unwrap();
        let mut fields = editable.fields.clone();
        fields.title = "  ".into();
        assert!(matches!(
            sample
                .handle
                .edit_card(item.clone(), editable.fields.clone(), fields)
                .unwrap(),
            EditOutcome::Problems { problems } if problems.iter().any(|p| p.contains("title"))
        ));
        assert_eq!(
            sample
                .handle
                .edit_card(
                    item.clone(),
                    editable.fields.clone(),
                    editable.fields.clone()
                )
                .unwrap(),
            EditOutcome::Unchanged
        );
        let mut fields = editable.fields.clone();
        fields.summary = Some("Every part, with where it came from.".into());
        let EditOutcome::Saved {
            fields: changed,
            commit,
        } = sample
            .handle
            .edit_card(item.clone(), editable.fields.clone(), fields)
            .unwrap()
        else {
            panic!("not saved");
        };
        assert_eq!(changed, vec!["summary"]);
        let commit = commit.unwrap();
        assert_eq!(sample.handle.history(None).unwrap()[0].id, commit);
        sample.handle.undo(commit[..8].to_owned()).unwrap();
        assert_eq!(
            sample.handle.item_detail(item).unwrap().fields.summary,
            editable.fields.summary
        );
    }

    #[test]
    fn a_proposals_card_takes_one_new_tag() {
        let sample = Sample::new();
        let id = sample.first_proposal();
        let editable = sample.handle.editable_proposal(id.clone()).unwrap();
        let mut fields = editable.fields.clone();
        fields
            .tags
            .extend(["one-new".to_owned(), "two-new".to_owned()]);
        assert!(matches!(
            sample.handle.edit_proposal(id.clone(), fields).unwrap(),
            EditOutcome::Problems { .. }
        ));
        let mut fields = editable.fields;
        fields.title = "Diffuser for the desk lamp".into();
        assert!(matches!(
            sample.handle.edit_proposal(id.clone(), fields).unwrap(),
            EditOutcome::Saved { fields, .. } if fields == ["title"]
        ));
        assert_eq!(
            sample.handle.proposal(id).unwrap().fields.title,
            "Diffuser for the desk lamp"
        );
    }

    #[test]
    fn a_change_while_another_writer_holds_the_archive_says_who() {
        use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let archive = Archive::open(&sample.root()).unwrap();
        let other = Holder {
            person: carrel_core::card::Id::new_v7(),
            device: carrel_core::card::Id::new_v7(),
        };
        let lease = DiskLock::for_archive(&archive)
            .acquire(other, "ingest", DEFAULT_LEASE)
            .unwrap();
        let base = sample.handle.editable_card(item.clone()).unwrap().fields;
        let mut fields = base.clone();
        fields.title = "Parts".into();
        assert!(matches!(
            sample.handle.edit_card(item, base, fields),
            Err(CarrelError::Busy { message }) if message.contains("ingest")
        ));
        lease.release().unwrap();
    }

    // Active

    #[test]
    fn checking_out_and_returning_an_item() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let to = sample.dir.path().join("Desktop");
        std::fs::create_dir_all(&to).unwrap();
        let out = sample
            .handle
            .check_out(item.clone(), Some(text(&to)), Some("2026-12-01".into()))
            .unwrap();
        assert!(out.commit.is_some());
        let active = sample.handle.active().unwrap();
        let listed = active.iter().find(|a| a.id == item).unwrap();
        assert_eq!(listed.kind, active::ActiveKind::CheckedOut);
        assert_eq!(listed.due.as_deref(), Some("2026-12-01"));
        assert_eq!(listed.state, "unchanged");
        std::fs::write(&out.working, "part,source\nLED strip,shop\n").unwrap();
        let preview = sample.handle.return_preview(item.clone(), None).unwrap();
        assert_eq!(preview.title, "Desk lamp parts list");
        let returned = sample
            .handle
            .return_item(item.clone(), None, false)
            .unwrap();
        assert!(returned.commit.is_some());
        assert!(!sample.handle.active().unwrap().iter().any(|a| a.id == item));
        assert!(matches!(
            sample.handle.check_out(item, None, Some("soon".into())),
            Err(CarrelError::Invalid { .. })
        ));
    }

    #[test]
    fn the_desk_takes_items_and_their_entries_are_decided() {
        let sample = Sample::new();
        let notes = sample.dir.path().join("plans.txt");
        std::fs::write(&notes, "plans").unwrap();
        let desk = sample
            .handle
            .add_to_desk(
                text(&notes),
                Some("Plans".into()),
                Some("What's next.".into()),
            )
            .unwrap();
        let active = sample.handle.active().unwrap();
        let listed = active.iter().find(|a| a.id == desk.id).unwrap();
        assert_eq!(listed.kind, active::ActiveKind::Desk);
        assert!(matches!(
            sample.handle.decide_entries(desk.id.clone(), vec![], active::EntryDecision::Approve),
            Err(CarrelError::Invalid { message }) if message.contains("no pending entries")
        ));
        sample.handle.remove_from_desk(text(&notes), true).unwrap();
        assert!(
            !sample
                .handle
                .active()
                .unwrap()
                .iter()
                .any(|a| a.id == desk.id)
        );
    }

    // Search, history and status

    #[test]
    fn search_finds_by_words_with_reasons_and_filters() {
        let sample = Sample::new();
        let results = sample.handle.search("lamp".into(), 10, None).unwrap();
        let hit = results
            .hits
            .iter()
            .find(|h| h.title == "Desk lamp parts list")
            .unwrap();
        assert_ne!(hit.why.len(), 0);
        let filtered = sample
            .handle
            .search("kind:physical".into(), 10, None)
            .unwrap();
        assert!(
            filtered
                .hits
                .iter()
                .all(|h| h.kind == Some(ItemKind::Physical))
        );
        assert_ne!(filtered.hits.len(), 0);
        assert!(matches!(
            sample.handle.search("  ".into(), 10, None),
            Err(CarrelError::Invalid { .. })
        ));
    }

    #[test]
    fn search_by_words_alone_finds_what_search_does_once_the_text_is_read() {
        let sample = Sample::new();
        let before = sample.handle.search_words("lamp".into(), 10).unwrap();
        assert_ne!(
            before.hits.len(),
            0,
            "found by their cards before the text is read"
        );
        let full = sample.handle.search("lamp".into(), 10, None).unwrap();
        let words = sample.handle.search_words("lamp".into(), 10).unwrap();
        assert_eq!(words.hits, full.hits);
        assert!(!words.by_meaning);
        assert_eq!(words.words_only, None);
        let filtered = sample
            .handle
            .search_words("collection:Reading".into(), 10)
            .unwrap();
        assert!(
            filtered
                .hits
                .iter()
                .all(|h| h.collection.as_deref() == Some("Reading"))
        );
        assert!(matches!(
            sample.handle.search_words("  ".into(), 10),
            Err(CarrelError::Invalid { .. })
        ));
    }

    #[test]
    fn an_items_detail_and_related_say_why_when_there_is_no_index() {
        let sample = Sample::new();
        let item = sample.item_titled("Desk lamp parts list");
        let detail = sample.handle.item_detail(item.clone()).unwrap();
        assert_eq!(detail.collection, "Projects/Hardware");
        let related = sample.handle.related(item).unwrap();
        assert_eq!(related.items.len(), 0);
        assert!(related.why_none.is_some());
    }

    #[test]
    fn status_looks_for_active_work_in_the_home_it_was_given() {
        let sample = Sample::new();
        let project = sample.dir.path().join("home/Code/lamp-firmware");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("main.c"), "int main(void) { return 0; }").unwrap();
        let status = sample.handle.status().unwrap();
        let tracked: Vec<&str> = status.to_track.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(tracked.len(), 1, "{tracked:?}");
        assert!(
            Path::new(tracked[0]).ends_with(Path::new("home").join("Code").join("lamp-firmware")),
            "{tracked:?}"
        );
    }

    #[test]
    fn status_counts_what_waits_and_recent_follows_changes() {
        let sample = Sample::new();
        let status = sample.handle.status().unwrap();
        assert_eq!(status.items, 12);
        assert!(status.has_history);
        assert_eq!(status.review, 1);
        assert!(status.writer.is_none());
        sample
            .handle
            .set_setting("ingest.originals".into(), "keep".into())
            .unwrap();
        let status = sample.handle.status().unwrap();
        assert!(
            status.recent[0].title.contains("ingest.originals"),
            "{:?}",
            status.recent
        );
    }

    // Settings and models

    #[test]
    fn settings_are_listed_and_changed_as_one_change() {
        let sample = Sample::new();
        let settings = sample.handle.settings().unwrap();
        let originals = settings
            .iter()
            .find(|s| s.key == "ingest.originals")
            .unwrap();
        assert_eq!(originals.choices, ["move", "keep"]);
        let changed = sample
            .handle
            .set_setting("ingest.originals".into(), "keep".into())
            .unwrap();
        assert_eq!(
            (changed.from.as_str(), changed.to.as_str()),
            ("move", "keep")
        );
        assert!(changed.commit.is_some());
        assert!(matches!(
            sample
                .handle
                .set_setting("ingest.originals".into(), "shred".into()),
            Err(CarrelError::Invalid { .. })
        ));
        assert!(matches!(
            sample.handle.set_setting("no.such".into(), "1".into()),
            Err(CarrelError::Invalid { .. })
        ));
    }

    #[test]
    fn models_are_chosen_from_those_available() {
        let sample = Sample::new();
        let choices = sample.handle.model_choices().unwrap();
        assert!(choices.available.is_empty(), "only built-in providers");
        assert!(matches!(
            sample
                .handle
                .choose_models(Some("nope/nothing".into()), None),
            Err(CarrelError::Invalid { .. })
        ));
        let off = sample
            .handle
            .choose_models(None, Some("off".into()))
            .unwrap();
        assert_eq!(off.second_model.as_deref(), Some("off"));
        assert_ne!(sample.handle.model_catalog().len(), 0);
        assert!(
            sample
                .handle
                .providers()
                .iter()
                .any(|p| p.name == "built-in")
        );
    }

    // The intake queue

    #[test]
    fn queued_files_are_drafted_into_proposals_and_filed() {
        let sample = Sample::new();
        let file = sample.dir.path().join("Inbox").join("shopping list.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "LED strip, diffuser, screws").unwrap();
        let added = sample
            .handle
            .queue(vec![text(&file)], false, false)
            .unwrap();
        assert_eq!(added.added.len(), 1);
        assert_eq!(sample.handle.intake().unwrap().waiting.len(), 1);
        let drafted = sample.handle.draft_queued(None).unwrap();
        assert_eq!(drafted.drafted, 1, "{drafted:?}");
        assert_eq!(sample.handle.intake().unwrap().waiting.len(), 0);
        let proposal = sample
            .handle
            .review()
            .unwrap()
            .batches
            .iter()
            .flat_map(|b| b.proposals.clone())
            .find(|p| p.source.ends_with("shopping list.txt"))
            .unwrap();
        let outcome = sample.handle.hold(vec![proposal.id]).unwrap();
        assert_eq!(outcome.filed.len(), 1, "{outcome:?}");
        assert!(outcome.commit.is_some());
        assert!(outcome.filed[0].path.starts_with("Holding/"));
    }
}
