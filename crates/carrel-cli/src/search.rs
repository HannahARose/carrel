//! `carrel search QUERY`: finds items by the words in their cards and text and, with the
//! embedding index, by meaning, narrowed by filters, with the passage that matched
//! (`docs/design/search.md`).

use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Sensitivity;
use carrel_core::index::{ByMeaning, Index, embed_query, plan};
use carrel_core::providers::Registry;
use carrel_core::search::query::Query;
use carrel_core::search::texts::{Unindexed, fill, tidy};
use carrel_core::search::{Found, Hit, Meaning, Merge, Ranking, Search};
use serde_json::json;

use crate::commands::{locate, plural};
use crate::{Failure, Report};

/// How many results are shown unless `--limit` says otherwise.
pub const DEFAULT_LIMIT: usize = 20;

/// Reads the text of the items `tidy` found missing from the text cache, showing progress on
/// standard error.
fn read_wanted(
    archive: &Archive,
    registry: &Registry,
    wanted: Vec<Unindexed>,
    json: bool,
) -> Result<(), Failure> {
    if wanted.is_empty() {
        return Ok(());
    }
    let mut display = crate::progress::Display::new("Reading text", json);
    fill(archive, registry, wanted, &mut |done, total, item| {
        display.count("items", done, total, item);
    })?;
    display.finish();
    Ok(())
}

/// Reads the text of items not in the text cache yet with `registry`'s providers, showing
/// progress on standard error.
///
/// # Errors
///
/// [`Failure`] if the archive can't be read.
pub fn read_texts(archive: &Archive, registry: &Registry, json: bool) -> Result<(), Failure> {
    let (wanted, _) = tidy(archive)?;
    read_wanted(archive, registry, wanted, json)
}

/// The ranking by meaning for `query` from the archive's embedding `index` for `model`, or why
/// there's none and the search is by words only. Takes `registry`, to embed queries with.
fn by_meaning(
    archive: &Archive,
    model: Option<String>,
    index: Option<Index>,
    registry: Option<Registry>,
    query: &str,
) -> Result<Result<Box<dyn Ranking>, String>, Failure> {
    let Some(model) = model else {
        return Ok(Err(
            "This archive has no embedding model yet, so it was searched by words only; see `carrel index status`."
                .into(),
        ));
    };
    let Some(index) = index else {
        return Ok(Err(format!(
            "There's no meaning index for {model} yet, so this was searched by words only; build it with `carrel index rebuild`."
        )));
    };
    let Some(registry) = registry.filter(|r| crate::index::offered(r, &model)) else {
        let how = crate::index::how_to_get(&model)
            .unwrap_or_else(|| "see `carrel index model` for the models this device has".into());
        return Ok(Err(format!(
            "No provider on this device runs {model}, the archive's embedding model, so this was searched by words only; {how}."
        )));
    };
    let vector = match embed_query(&registry, &model, query) {
        Ok(vector) => vector,
        Err(e) => {
            return Ok(Err(format!(
                "{model} couldn't embed the query ({e}), so this was searched by words only."
            )));
        }
    };
    let behind = plan(archive, Some(&index))
        .map_err(|e| crate::index::failure(e, &model))?
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

/// `carrel search WORDS... [--limit N]`.
///
/// # Errors
///
/// [`Failure`] if there's no archive, the query has a filter that can't be used, or the
/// archive can't be read.
pub fn search(
    archive: Option<&Path>,
    words: &[String],
    limit: usize,
    json: bool,
) -> Result<Report, Failure> {
    let typed = words.join(" ");
    let query = Query::parse(&typed)?;
    if query.text.trim().is_empty() && query.filters.is_empty() {
        return Err(Failure::new(
            "search for some words, a filter such as tag:receipts, or both",
        ));
    }
    let archive = locate(archive)?;
    let (wanted, _) = tidy(&archive)?;
    let model = archive.settings().embedding_model().map(str::to_owned);
    let words = query.text.trim().is_empty();
    let index = match &model {
        Some(model) if !words => Index::open(&archive, model)
            .map_err(|e| crate::index::failure(e, model))?
            .filter(|index| !index.is_empty()),
        _ => None,
    };
    // Providers are only started when there's text to read or a query to embed.
    let registry = (!wanted.is_empty() || index.is_some()).then(|| crate::providers::registry().0);
    if let Some(registry) = &registry {
        read_wanted(&archive, registry, wanted, json)?;
    }
    let mut search = Search::build(&archive)?;
    let mut words_only = None;
    if !words {
        let merge = model
            .as_deref()
            .map_or_else(Merge::default, Merge::for_model);
        match by_meaning(&archive, model, index, registry, &query.text)? {
            Ok(ranking) => search = search.with_meaning(ranking).with_merge(merge),
            Err(why) => words_only = Some(why),
        }
    }
    let results = search.search(&query, limit);
    let marks = if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        ("\x1b[1m", "\x1b[0m")
    } else {
        ("**", "**")
    };
    let mut text = String::new();
    if results.hits.is_empty() {
        writeln!(text, "Nothing found for “{typed}”.").unwrap();
    } else if results.total > results.hits.len() {
        writeln!(
            text,
            "The best {} of {} for “{typed}”:",
            results.hits.len(),
            plural(results.total, "result")
        )
        .unwrap();
    } else {
        writeln!(text, "{} for “{typed}”:", plural(results.total, "result")).unwrap();
    }
    for hit in &results.hits {
        text.push('\n');
        describe(&mut text, hit, marks);
    }
    if let Meaning::Behind { items } = results.meaning {
        writeln!(
            text,
            "\nThe meaning index is behind by {}; they're found by their words until it catches up.",
            plural(items, "item")
        )
        .unwrap();
    }
    if let Some(why) = &words_only {
        writeln!(text, "\n{why}").unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "root": crate::commands::root_of(&archive),
            "query": typed,
            "total": results.total,
            "meaning": results.meaning,
            "words_only": words_only,
            "results": results.hits,
        }),
    ))
}

/// One result: its title and sensitivity mark, where it is, when, and the passage that
/// matched.
fn describe(text: &mut String, hit: &Hit, (open, close): (&str, &str)) {
    match &hit.found {
        Found::Item(item) => {
            let mark = match item.sensitivity {
                Sensitivity::Restricted => "  [restricted]",
                Sensitivity::Sensitive => "  [sensitive]",
                _ => "",
            };
            writeln!(text, "{}{mark}", item.title).unwrap();
            let mut about = vec![item.collection.clone()];
            about.extend(
                item.date
                    .as_ref()
                    .map(|d| d.get(..10).unwrap_or(d).to_owned()),
            );
            about.push(serde_json::to_value(item.kind).map_or_else(
                |_| String::new(),
                |v| v.as_str().unwrap_or_default().to_owned(),
            ));
            if item.checked_out {
                about.push("checked out".into());
            }
            writeln!(text, "  {}", about.join(" · ")).unwrap();
        }
        Found::Collection { path, title } => {
            writeln!(text, "{title}\n  the collection {path}").unwrap();
        }
        Found::Glossary { term } => writeln!(text, "{term}\n  in the glossary").unwrap(),
    }
    if let Some(passage) = &hit.passage {
        writeln!(text, "  {}", passage.marked(open, close)).unwrap();
    }
    let why: Vec<String> = hit.why.iter().map(ToString::to_string).collect();
    writeln!(text, "  matched: {}", why.join(", ")).unwrap();
}
