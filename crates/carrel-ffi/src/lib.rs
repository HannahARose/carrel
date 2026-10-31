//! Swift bindings for `carrel-core`, generated with `UniFFI`.
//!
//! This is the boundary the apps call through, so it speaks in plain records and strings
//! rather than the core's richer types. `scripts/build-xcframework.sh` builds it as
//! `CarrelFFI.xcframework` and generates the Swift side into the `CarrelKit` package under
//! `apple/`.
//!
//! [`TextModel`] is implemented in Swift and called from Rust. Model and text-extraction
//! connectors (Foundation Models, Vision, `PDFKit`) will plug in the same way: the core
//! decides what to ask, and the platform answers.

// UniFFI passes arguments from Swift as owned values.
#![allow(clippy::needless_pass_by_value)]

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use carrel_core::Access;
use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::{Id, Kind};
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::history::History;

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
}

impl CarrelError {
    fn failed(error: impl std::fmt::Display) -> Self {
        Self::Failed {
            message: error.to_string(),
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

/// An open archive.
#[derive(Debug, uniffi::Object)]
pub struct ArchiveHandle {
    archive: Archive,
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
        match Archive::find(Path::new(&path)) {
            Ok(archive) => Ok(Arc::new(Self { archive })),
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
        let config = carrel_core::local::config_dir().ok_or_else(|| {
            CarrelError::failed("can't find your home folder; set CARREL_CONFIG_DIR")
        })?;
        create_archive(Path::new(&path), &name, &config)
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

/// Who makes changes from this device: the CLI's `identity.json` in the configuration folder,
/// created as the CLI creates it if it isn't there yet.
#[derive(serde::Serialize, serde::Deserialize)]
struct Identity {
    person: Id,
    device: Id,
}

fn identity(config: &Path) -> Result<Identity, CarrelError> {
    let path = config.join("identity.json");
    let located =
        |e: &dyn std::fmt::Display| CarrelError::failed(format!("{}: {e}", path.display()));
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| located(&e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let identity = Identity {
                person: Id::new_v7(),
                device: Id::new_v7(),
            };
            std::fs::create_dir_all(config).map_err(|e| located(&e))?;
            let text = serde_json::to_string_pretty(&identity).map_err(|e| located(&e))? + "\n";
            std::fs::write(&path, text).map_err(|e| located(&e))?;
            Ok(identity)
        }
        Err(e) => Err(located(&e)),
    }
}

fn create_archive(
    root: &Path,
    name: &str,
    config: &Path,
) -> Result<Arc<ArchiveHandle>, CarrelError> {
    let identity = identity(config)?;
    let archive = Archive::init(root, &InitOptions::new(name)).map_err(CarrelError::failed)?;
    let holder = Holder {
        person: identity.person,
        device: identity.device,
    };
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "create", DEFAULT_LEASE)
        .map_err(CarrelError::failed)?;
    History::init(&archive, &lease).map_err(CarrelError::failed)?;
    lease.release().map_err(CarrelError::failed)?;
    Ok(Arc::new(ArchiveHandle { archive }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let dir = std::env::temp_dir().join(format!("carrel-ffi-{}", Id::new_v7()));
        let config = dir.join("config");
        let root = dir.join("New archive");
        let archive = create_archive(&root, "Letters", &config).unwrap();
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
        assert!(create_archive(&root, "Again", &config).is_err());
        let other = dir.join("Second");
        create_archive(&other, "Second", &config).unwrap();
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
}
