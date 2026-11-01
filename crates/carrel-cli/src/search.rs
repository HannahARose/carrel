//! `carrel search QUERY`: finds items by the words in their cards and text and, with the
//! embedding index, by meaning, narrowed by filters, with the passage that matched
//! (`docs/design/search.md`).

use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Sensitivity;
use carrel_core::providers::Registry;
use carrel_core::search::query::Query;
use carrel_core::search::run::ModelSource;
use carrel_core::search::texts::{Unindexed, fill, tidy};
use carrel_core::search::{Found, Hit, Meaning};
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
    let mut display = None;
    let searched = carrel_core::search::run::search(
        &archive,
        &query,
        limit,
        || crate::providers::registry().0,
        &mut |done, total, item| {
            display
                .get_or_insert_with(|| crate::progress::Display::new("Reading text", json))
                .count("items", done, total, item);
        },
    )
    .map_err(|e| Failure::new(e.to_string()))?;
    if let Some(mut display) = display {
        display.finish();
    }
    let (results, mut words_only) = (searched.results, searched.words_only);
    if let (Some(why), Some(ModelSource::Download(model))) =
        (&mut words_only, searched.missing_model)
    {
        why.push_str(&crate::index::download_command(model.name));
    }
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
