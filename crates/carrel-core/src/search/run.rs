//! A whole search, as `carrel search` and the apps run it (`docs/design/search.md`): the text
//! cache filled for items not read yet, the word index built, and the meaning ranking merged
//! in when the archive's embedding index and a provider for its model are there. Without
//! them it's by words only, saying why.

use crate::archive::{Archive, ArchiveError};
use crate::index::{ByMeaning, Index, IndexError, embed_query, plan};
use crate::models::{Model, find};
use crate::providers::{ProviderError, Registry};
use crate::search::query::Query;
use crate::search::texts::{fill, tidy};
use crate::search::{Merge, Ranking, Results, Search};

/// What a search found, and why meaning wasn't used when it wasn't.
#[derive(Debug, Clone)]
pub struct Searched {
    /// The results.
    pub results: Results,
    /// Why it was by words only, when the query had words and meaning couldn't be used.
    pub words_only: Option<String>,
    /// When that's because no provider here runs the archive's embedding model, how to get
    /// it.
    pub missing_model: Option<ModelSource>,
}

/// Why a search couldn't run.
#[derive(Debug)]
pub enum SearchError {
    /// The archive couldn't be read.
    Archive(ArchiveError),
    /// The archive's embedding index couldn't be read.
    Index {
        /// The archive's embedding model.
        model: String,
        /// What went wrong.
        error: IndexError,
    },
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(e) => e.fmt(f),
            Self::Index { model, error } => f.write_str(&index_failure(error, model)),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<ArchiveError> for SearchError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

/// How to get an embedding model this device doesn't have, from Carrel's catalog. Each
/// interface offers it its own way: `carrel models download NAME`, a key in the interactive
/// mode, a Download button in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// It's downloaded, as this catalog model.
    Download(&'static Model),
    /// It comes with this provider, which only Apple devices have; there's nothing to
    /// download.
    ComesWith(&'static str),
}

impl std::fmt::Display for ModelSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Download(model) => write!(
                f,
                "download {} ({}) from Carrel's catalog",
                model.name,
                crate::human_size(model.size)
            ),
            Self::ComesWith(provider) => {
                write!(f, "it comes with {provider}, which only Apple devices have")
            }
        }
    }
}

/// How to get `model` on this device, if Carrel's catalog has it: a download, or the provider
/// it comes with.
#[must_use]
pub fn how_to_get(model: &str) -> Option<ModelSource> {
    match find(model)? {
        Model {
            comes_with: Some(provider),
            ..
        } => Some(ModelSource::ComesWith(provider)),
        found => Some(ModelSource::Download(found)),
    }
}

/// `error`, in words, with how to get `model` when no provider here has it.
#[must_use]
pub fn index_failure(error: &IndexError, model: &str) -> String {
    match error {
        IndexError::Provider(e @ ProviderError::Unavailable(_)) => match how_to_get(model) {
            Some(how) => format!("{e}; {how}"),
            None => e.to_string(),
        },
        e => e.to_string(),
    }
}

/// Searches `archive` for `query`, the best `limit` results. `registry` starts the providers;
/// it's only called when there's text to read or a query to embed. `progress` is told how
/// many items' text has been read, of how many, and which, while the text cache is filled.
///
/// # Errors
///
/// [`SearchError`] if the archive or its embedding index can't be read.
pub fn search(
    archive: &Archive,
    query: &Query,
    limit: usize,
    registry: impl FnOnce() -> Registry,
    progress: &mut dyn FnMut(usize, usize, Option<&str>),
) -> Result<Searched, SearchError> {
    let (wanted, _) = tidy(archive)?;
    let model = archive.settings().embedding_model().map(str::to_owned);
    let words = query.text.trim().is_empty();
    let index = match &model {
        Some(model) if !words => Index::open(archive, model)
            .map_err(|error| SearchError::Index {
                model: model.clone(),
                error,
            })?
            .filter(|index| !index.is_empty()),
        _ => None,
    };
    // Providers are only started when there's text to read or a query to embed.
    let registry = (!wanted.is_empty() || index.is_some()).then(registry);
    if let Some(registry) = &registry
        && !wanted.is_empty()
    {
        fill(archive, registry, wanted, progress)?;
    }
    let mut search = Search::build(archive)?;
    let mut words_only = None;
    let mut missing_model = None;
    if !words {
        let merge = model
            .as_deref()
            .map_or_else(Merge::default, Merge::for_model);
        match by_meaning(archive, model, index, registry, &query.text)? {
            Ok(ranking) => search = search.with_meaning(ranking).with_merge(merge),
            Err(WordsOnly { why, missing }) => {
                words_only = Some(why);
                missing_model = missing;
            }
        }
    }
    Ok(Searched {
        results: search.search(query, limit),
        words_only,
        missing_model,
    })
}

/// Why a search was by words only, and how to get the archive's embedding model when that's
/// why.
struct WordsOnly {
    why: String,
    missing: Option<ModelSource>,
}

impl WordsOnly {
    fn because(why: String) -> Self {
        Self { why, missing: None }
    }
}

/// The ranking by meaning for `query` from the archive's embedding `index` for `model`, or why
/// there's none and the search is by words only. Takes `registry`, to embed queries with.
fn by_meaning(
    archive: &Archive,
    model: Option<String>,
    index: Option<Index>,
    registry: Option<Registry>,
    query: &str,
) -> Result<Result<Box<dyn Ranking>, WordsOnly>, SearchError> {
    let Some(model) = model else {
        return Ok(Err(WordsOnly::because(
            "This archive has no embedding model yet, so it was searched by words only; see `carrel index status`."
                .into(),
        )));
    };
    let Some(index) = index else {
        return Ok(Err(WordsOnly::because(format!(
            "There's no meaning index for {model} yet, so this was searched by words only; build it with `carrel index rebuild`."
        ))));
    };
    let Some(registry) = registry.filter(|r| crate::worker::offered(r, &model)) else {
        let missing = how_to_get(&model);
        let how = missing.map_or_else(
            || "Carrel's catalog doesn't have it".to_owned(),
            |how| how.to_string(),
        );
        return Ok(Err(WordsOnly {
            why: format!(
                "No provider on this device runs {model}, the archive's embedding model, so this was searched by words only; {how}."
            ),
            missing,
        }));
    };
    let vector = match embed_query(&registry, &model, query) {
        Ok(vector) => vector,
        Err(e) => {
            return Ok(Err(WordsOnly::because(format!(
                "{model} couldn't embed the query ({e}), so this was searched by words only."
            ))));
        }
    };
    let behind = plan(archive, Some(&index))
        .map_err(|error| SearchError::Index {
            model: model.clone(),
            error,
        })?
        .pending
        .len();
    let asked = query.to_owned();
    let embed = Box::new(move |text: &str| {
        if text == asked {
            Some(vector.clone())
        } else {
            embed_query(&registry, &model, text).ok()
        }
    });
    Ok(Ok(Box::new(ByMeaning::new(
        archive,
        Box::new(index),
        behind,
        embed,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalog_model_is_downloaded_or_comes_with_its_provider() {
        let gemma = find("embeddinggemma-300m").unwrap();
        assert_eq!(
            how_to_get("embeddinggemma-300m"),
            Some(ModelSource::Download(gemma))
        );
        let words = how_to_get("embeddinggemma-300m").unwrap().to_string();
        assert!(
            words.starts_with("download embeddinggemma-300m ("),
            "{words}"
        );
        assert!(!words.contains("carrel"), "no interface's command: {words}");
        assert_eq!(
            how_to_get("apple-nl-sentence-en"),
            Some(ModelSource::ComesWith("carrel-provider-apple"))
        );
        assert_eq!(how_to_get("not-in-the-catalog"), None);
    }
}
