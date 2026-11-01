//! Looking after an archive's embedding index (`docs/design/search.md`), as `carrel index` and
//! the app's Settings do: how far behind it is, rebuilding it, and switching the archive to
//! another embedding model while the current one keeps serving search.
//!
//! The worker keeps the index current in the background after every change
//! ([`crate::worker::Worker::embed`]); these are the by-hand operations. Only the archive's
//! embedding model is used, never another.

use serde_json::Value;

use crate::archive::{Archive, ArchiveError};
use crate::coordinator::{CoordinatorError, WriterLease};
use crate::index::switch::{self, SwitchError};
use crate::index::{Index, IndexError, embed_query, plan};
use crate::providers::{Capability, Registry};
use crate::search::run::{how_to_get, index_failure};
use crate::search::texts::{fill, tidy};

/// Why the index couldn't be read, rebuilt or switched.
#[derive(Debug)]
pub enum ManageError {
    /// The model failed, or the index couldn't be read or written.
    Index {
        /// The embedding model.
        model: String,
        /// What went wrong.
        source: IndexError,
    },
    /// The archive couldn't be read.
    Archive(ArchiveError),
    /// The switch couldn't be started, stopped or finished.
    Switch(SwitchError),
    /// The archive's writer lease couldn't be taken.
    Lease(CoordinatorError),
    /// No provider on this device offers the model, so its index can't be built here.
    NotOffered {
        /// The model asked for.
        model: String,
        /// The embedding models this device can embed with.
        here: Vec<String>,
    },
}

impl std::fmt::Display for ManageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Index { model, source } => f.write_str(&index_failure(source, model)),
            Self::Archive(e) => e.fmt(f),
            Self::Switch(e) => e.fmt(f),
            Self::Lease(e) => e.fmt(f),
            Self::NotOffered { model, here } => {
                let can = if here.is_empty() {
                    "no provider here embeds".to_owned()
                } else {
                    format!("this device can embed with {}", here.join(", "))
                };
                let how = how_to_get(model).map_or(String::new(), |how| format!("; {how}"));
                write!(
                    f,
                    "no provider on this device offers {model}, so it can't build its index; \
                     {can}{how}"
                )
            }
        }
    }
}

impl std::error::Error for ManageError {}

impl From<ArchiveError> for ManageError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<SwitchError> for ManageError {
    fn from(e: SwitchError) -> Self {
        Self::Switch(e)
    }
}

impl From<CoordinatorError> for ManageError {
    fn from(e: CoordinatorError) -> Self {
        Self::Lease(e)
    }
}

/// `source`, from the model `model`.
fn failed(model: &str) -> impl Fn(IndexError) -> ManageError + '_ {
    move |source| ManageError::Index {
        model: model.to_owned(),
        source,
    }
}

/// A switch of embedding model under way, and how far it has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchStatus {
    /// The archive's model until the switch finishes, if it has one.
    pub from: Option<String>,
    /// The model it's switching to.
    pub to: String,
    /// How many items the archive has.
    pub items: usize,
    /// How many are embedded with the new model.
    pub embedded: usize,
    /// How many are still to embed.
    pub behind: usize,
}

/// The index of the archive's embedding model, and a switch under way.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// The archive's embedding model; without one, it's searched by words only.
    pub model: Option<String>,
    /// Whether the model has an index yet.
    pub exists: bool,
    /// How many items the archive has.
    pub items: usize,
    /// How many are embedded and up to date.
    pub embedded: usize,
    /// How many are missing or out of date.
    pub behind: usize,
    /// Items in the index that aren't in the archive any more.
    pub gone: usize,
    /// How many vectors the index holds.
    pub vectors: usize,
    /// How many numbers each vector has.
    pub dimensions: usize,
    /// Whether a rebuild stopped part way; rebuilding again carries on from there.
    pub rebuilding: bool,
    /// The switch to another model under way, if any.
    pub switch: Option<SwitchStatus>,
}

/// The switch `archive` is part way through, if any, and how far it has got.
///
/// # Errors
///
/// [`ManageError::Index`] if the new model's index or the archive can't be read.
pub fn switch_status(archive: &Archive) -> Result<Option<SwitchStatus>, ManageError> {
    let Some(switch::Switch { from, to }) = switch::pending(archive) else {
        return Ok(None);
    };
    let index = Index::open(archive, &to).map_err(failed(&to))?;
    let plan = plan(archive, index.as_ref()).map_err(failed(&to))?;
    Ok(Some(SwitchStatus {
        from,
        to,
        items: plan.items,
        embedded: plan.current(),
        behind: plan.pending.len(),
    }))
}

/// The archive's embedding model, how many items are embedded with it and how many are
/// behind, and a switch under way.
///
/// # Errors
///
/// [`ManageError::Index`] if the index or the archive can't be read.
pub fn status(archive: &Archive) -> Result<Status, ManageError> {
    let switch = switch_status(archive)?;
    let Some(model) = archive.settings().embedding_model().map(str::to_owned) else {
        return Ok(Status {
            switch,
            ..Status::default()
        });
    };
    let index = Index::open(archive, &model).map_err(failed(&model))?;
    let plan = plan(archive, index.as_ref()).map_err(failed(&model))?;
    Ok(Status {
        exists: index.is_some(),
        items: plan.items,
        embedded: plan.current(),
        behind: plan.pending.len(),
        gone: plan.gone.len(),
        vectors: index.as_ref().map_or(0, Index::vectors),
        dimensions: index.as_ref().map_or(0, Index::dimensions),
        rebuilding: index.as_ref().is_some_and(Index::rebuilding),
        model: Some(model),
        switch,
    })
}

/// An embedding model a provider on this device offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    /// The model's identifier, as the archive's settings name it.
    pub model: String,
    /// The provider that runs it.
    pub provider: String,
    /// How many numbers its vectors have, if the provider says.
    pub dimensions: Option<u64>,
}

/// The embedding models providers in `registry` offer, best first, each once.
#[must_use]
pub fn offered_models(registry: &Registry) -> Vec<Offered> {
    let mut models: Vec<Offered> = Vec::new();
    for (provider, offer) in registry.ranked(Capability::Embed) {
        if !models.iter().any(|m| m.model == offer.id) {
            models.push(Offered {
                model: offer.id.clone(),
                provider: provider.name().to_owned(),
                dimensions: offer.extra.get("dimensions").and_then(Value::as_u64),
            });
        }
    }
    models
}

/// What catching up is doing, for progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading the text of items not in the text cache yet.
    Reading,
    /// Embedding what's behind.
    Embedding,
}

/// Told the stage, how many are done, of how many, and the item being worked on.
pub type Progress<'a> = dyn FnMut(Stage, usize, usize, Option<&str>) + 'a;

/// Brings `archive`'s `index` up to date: reads the text of items not in the text cache yet,
/// then embeds what's behind, calling `progress` with the stage, how many are done, of how
/// many, and the item. Returns how many items were embedded.
///
/// # Errors
///
/// [`ManageError`] if the archive can't be read, the model fails, or the index can't be
/// written.
pub fn catch_up(
    archive: &Archive,
    registry: &Registry,
    index: &mut Index,
    progress: &mut Progress<'_>,
) -> Result<usize, ManageError> {
    let model = index.model().to_owned();
    let (wanted, _) = tidy(archive)?;
    if !wanted.is_empty() {
        fill(archive, registry, wanted, &mut |done, total, item| {
            progress(Stage::Reading, done, total, item);
        })?;
    }
    let plan = plan(archive, Some(index)).map_err(failed(&model))?;
    let items = plan.pending.len();
    index
        .embed(archive, registry, plan, &mut |done, total, item| {
            progress(Stage::Embedding, done, total, item);
        })
        .map_err(failed(&model))?;
    Ok(items)
}

/// How a rebuild or a switch left the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// How many items were embedded.
    pub embedded: usize,
    /// How many items the index holds.
    pub items: usize,
    /// How many vectors it holds.
    pub vectors: usize,
    /// How many numbers each has.
    pub dimensions: usize,
    /// Whether it carried on from a rebuild or switch that stopped part way.
    pub resumed: bool,
}

impl Built {
    fn of(index: &Index, embedded: usize, resumed: bool) -> Self {
        Self {
            embedded,
            items: index.len(),
            vectors: index.vectors(),
            dimensions: index.dimensions(),
            resumed,
        }
    }
}

/// Embeds every item again with `model`, the archive's embedding model. Interrupted, it
/// carries on where it stopped when run again. The model is asked first, so a model that
/// doesn't answer leaves the old index as it was.
///
/// # Errors
///
/// [`ManageError`] if no provider here runs the model, it fails, or the index can't be
/// written.
pub fn rebuild(
    archive: &Archive,
    registry: &Registry,
    model: &str,
    progress: &mut Progress<'_>,
) -> Result<Built, ManageError> {
    let mut index = Index::open_or_new(archive, model).map_err(failed(model))?;
    let resumed = index.rebuilding();
    if !resumed {
        embed_query(registry, model, "a test").map_err(failed(model))?;
        index.start_rebuild();
        index.save().map_err(failed(model))?;
    }
    let embedded = catch_up(archive, registry, &mut index, progress)?;
    Ok(Built::of(&index, embedded, resumed))
}

/// Takes the archive's writer lease with `lease`, runs `change` holding it, and lets it go.
fn holding<T>(
    lease: &mut dyn FnMut() -> Result<WriterLease, CoordinatorError>,
    change: impl FnOnce(&WriterLease) -> Result<T, SwitchError>,
) -> Result<T, ManageError> {
    let held = lease()?;
    let result = change(&held);
    held.release()?;
    Ok(result?)
}

/// How [`switch_to`] left things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Switched {
    /// The archive already used the model, and wasn't switching.
    Already,
    /// The archive already used the model; the switch to this other one was stopped and its
    /// index removed.
    Stopped(String),
    /// The archive uses the model now, on every device.
    Done {
        /// The model it used before, whose index was removed.
        from: Option<String>,
        /// A switch to another model that was given up to start this one.
        replaced: Option<String>,
        /// The new model's index.
        built: Built,
    },
}

/// Switches `archive` to the embedding model `model`, which a provider in `registry` must
/// offer: builds its index beside the current one while search keeps using the current one,
/// then makes it the archive's model, for every device, and removes the old index. Already
/// the archive's model, it stops a switch to another model, if one is under way. Interrupted,
/// it carries on when run again. `lease` takes the archive's writer lease for each recorded
/// change.
///
/// # Errors
///
/// [`ManageError::NotOffered`] if no provider here offers the model, or [`ManageError`] if it
/// fails, the lease can't be taken, or the index or settings can't be written.
pub fn switch_to(
    archive: &Archive,
    registry: &Registry,
    model: &str,
    lease: &mut dyn FnMut() -> Result<WriterLease, CoordinatorError>,
    progress: &mut Progress<'_>,
) -> Result<Switched, ManageError> {
    let model = model.trim();
    if archive.settings().embedding_model() == Some(model) {
        if switch::pending(archive).is_none() {
            return Ok(Switched::Already);
        }
        let stopped = holding(lease, |held| switch::cancel(archive, held))?;
        return Ok(Switched::Stopped(stopped.unwrap_or_default()));
    }
    if !crate::worker::offered(registry, model) {
        return Err(ManageError::NotOffered {
            model: model.to_owned(),
            here: offered_models(registry)
                .into_iter()
                .map(|m| m.model)
                .collect(),
        });
    }
    // Make sure the model answers before anything is recorded.
    embed_query(registry, model, "a test").map_err(failed(model))?;
    let (mut index, begun) = holding(lease, |held| switch::begin(archive, model, held))?;
    let embedded = catch_up(archive, registry, &mut index, progress)?;
    let finished = holding(lease, |held| switch::finish(archive, model, held))?;
    Ok(Switched::Done {
        from: finished.from,
        replaced: begun.replaced,
        built: Built::of(&index, embedded, begun.resumed),
    })
}
